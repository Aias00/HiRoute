//! Resolve the analysis interval once, through the existing host clock port.
use crate::control::{ApplicationClockPort, ControlReadError};
use hiroute_domain::{
    AnalysisPeriodV1, AnalysisTimeInputV1, AnalysisTimeRangeV1, ObservationQueryError,
};

const DAY: i64 = 86_400_000;

pub(super) fn resolve(
    clock: &dyn ApplicationClockPort,
    input: &AnalysisTimeInputV1,
    now_ms: i64,
) -> Result<AnalysisTimeRangeV1, ObservationQueryError> {
    let absolute = input.from_ms.is_some() || input.to_ms.is_some();
    if u8::from(input.period.is_some()) + u8::from(input.day.is_some()) + u8::from(absolute) > 1
        || input.day.as_deref().is_some_and(|day| !valid_day(day))
    {
        return Err(ObservationQueryError::InvalidQuery);
    }
    let (from, to) = if let Some(day) = input.day.as_deref() {
        let range = clock
            .analysis_local_day(Some(day), 0, now_ms)
            .map_err(map)?;
        (range.from_ms, range.to_ms.min(now_ms))
    } else if absolute {
        let to = input.to_ms.unwrap_or(now_ms);
        (input.from_ms.unwrap_or(to.saturating_sub(7 * DAY)), to)
    } else {
        match input.period.unwrap_or_default() {
            AnalysisPeriodV1::Today | AnalysisPeriodV1::Yesterday => {
                let days_ago = u8::from(input.period == Some(AnalysisPeriodV1::Yesterday));
                let range = clock
                    .analysis_local_day(None, days_ago, now_ms)
                    .map_err(map)?;
                (range.from_ms, range.to_ms.min(now_ms))
            }
            AnalysisPeriodV1::SevenDays => (now_ms.saturating_sub(7 * DAY), now_ms),
            AnalysisPeriodV1::ThirtyDays => (now_ms.saturating_sub(30 * DAY), now_ms),
        }
    };
    if from < 0 || from >= to || to > now_ms {
        return Err(ObservationQueryError::InvalidQuery);
    }
    Ok(AnalysisTimeRangeV1 {
        requested_from_ms: from,
        requested_to_ms: to,
        effective_from_ms: from,
        effective_to_ms: to,
        from_utc_offset_seconds: clock.local_utc_offset_seconds(from).map_err(map)?,
        to_utc_offset_seconds: clock.local_utc_offset_seconds(to).map_err(map)?,
        clipped_for_retention: false,
    })
}

fn map(_: ControlReadError) -> ObservationQueryError {
    ObservationQueryError::Unavailable
}

fn valid_day(day: &str) -> bool {
    let bytes = day.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return false;
    }
    let Ok(year) = day[..4].parse::<u32>() else {
        return false;
    };
    let Ok(month) = day[5..7].parse::<u32>() else {
        return false;
    };
    let Ok(value) = day[8..].parse::<u32>() else {
        return false;
    };
    let days = match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        1..=12 => 31,
        _ => return false,
    };
    year > 0 && value > 0 && value <= days
}

#[cfg(test)]
mod tests {
    use super::*;
    use hiroute_domain::AnalysisLocalDayV1;

    struct Clock;
    impl ApplicationClockPort for Clock {
        fn now_ms(&self) -> Result<i64, ControlReadError> {
            Ok(1_750_000_000_000)
        }
        fn analysis_local_day(
            &self,
            _: Option<&str>,
            days: u8,
            _: i64,
        ) -> Result<AnalysisLocalDayV1, ControlReadError> {
            // A real 23-hour civil day, rather than a rolling-day subtraction.
            let to_ms = 1_741_579_200_000 - i64::from(days) * DAY;
            Ok(AnalysisLocalDayV1 {
                from_ms: to_ms - 23 * 3_600_000,
                to_ms,
            })
        }
        fn local_utc_offset_seconds(&self, at: i64) -> Result<i32, ControlReadError> {
            Ok(if at < 1_741_575_600_000 {
                -18_000
            } else {
                -14_400
            })
        }
    }
    #[test]
    fn civil_days_preserve_distinct_offsets_and_reject_mixed_input() {
        let q = AnalysisTimeInputV1 {
            period: Some(AnalysisPeriodV1::Yesterday),
            ..Default::default()
        };
        let r = resolve(&Clock, &q, 1_750_000_000_000).unwrap();
        assert_eq!(r.requested_to_ms - r.requested_from_ms, 23 * 3_600_000);
        let q = AnalysisTimeInputV1 {
            day: Some("2026-10-16".into()),
            ..q
        };
        assert_eq!(
            resolve(&Clock, &q, 1_750_000_000_000),
            Err(ObservationQueryError::InvalidQuery)
        );
        assert!(valid_day("2024-02-29"));
        assert!(!valid_day("2025-02-29"));
        assert!(!valid_day("2026-04-31"));
        assert!(!valid_day("2026-01-01/extra"));
    }
    #[test]
    fn absolute_range_keeps_each_boundary_offset() {
        let q = AnalysisTimeInputV1 {
            from_ms: Some(1_741_496_400_000),
            to_ms: Some(1_741_579_200_000),
            ..Default::default()
        };
        let r = resolve(&Clock, &q, 1_750_000_000_000).unwrap();
        assert_eq!(
            (r.from_utc_offset_seconds, r.to_utc_offset_seconds),
            (-18_000, -14_400)
        );
        assert_eq!(r.effective_from_ms, r.requested_from_ms);
        assert!(!r.clipped_for_retention);
    }
}
