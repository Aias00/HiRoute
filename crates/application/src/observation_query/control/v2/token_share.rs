use crate::control::ApplicationClockPort;
use hiroute_application_api::ValuePeriodV1;
use hiroute_domain::{
    ObservationQueryError as Error, ObservationTokenSharePeriodKindV1 as Kind,
    ObservationTokenShareReadV1, ObservationValueBreakdownV1,
};

pub(super) fn scope(
    clock: &dyn ApplicationClockPort,
    breakdown: Option<ObservationValueBreakdownV1>,
    agent: Option<String>,
    period: ValuePeriodV1,
    from: i64,
    to: i64,
) -> Result<Option<ObservationTokenShareReadV1>, Error> {
    if breakdown.is_none() {
        return if agent.is_none() {
            Ok(None)
        } else {
            Err(Error::InvalidQuery)
        };
    }
    if agent
        .as_ref()
        .is_some_and(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
    {
        return Err(Error::InvalidQuery);
    }
    let kind = match period {
        ValuePeriodV1::Today => Kind::Today,
        ValuePeriodV1::SevenDays => Kind::Last7d,
        ValuePeriodV1::ThirtyDays => return Err(Error::InvalidQuery),
    };
    let offset = |at| {
        clock
            .local_utc_offset_seconds(at)
            .map_err(|_| Error::Unavailable)
    };
    let (start, end) = (offset(from)?, offset(to.saturating_sub(1))?);
    let label = |seconds: i32| {
        format!(
            "UTC{}{:02}:{:02}",
            if seconds < 0 { '-' } else { '+' },
            seconds.unsigned_abs() / 3600,
            seconds.unsigned_abs() % 3600 / 60
        )
    };
    let timezone = if start == end {
        format!("local ({})", label(start))
    } else {
        format!("local ({} → {})", label(start), label(end))
    };
    Ok(Some(ObservationTokenShareReadV1 {
        kind,
        timezone,
        agent_filter: agent,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ControlReadError;
    struct NonUtcClock;
    impl ApplicationClockPort for NonUtcClock {
        fn now_ms(&self) -> Result<i64, ControlReadError> {
            Ok(1_750_032_000_000)
        }
        fn local_day_start_ms(&self, at: i64) -> Result<i64, ControlReadError> {
            Ok((at + 8 * 3_600_000) / 86_400_000 * 86_400_000 - 8 * 3_600_000)
        }
        fn local_utc_offset_seconds(&self, _: i64) -> Result<i32, ControlReadError> {
            Ok(28_800)
        }
    }
    #[test]
    fn token_share_today_retains_the_non_utc_civil_boundary_and_discloses_its_zone() {
        let clock = NonUtcClock;
        let end = clock.now_ms().unwrap();
        let start = clock.local_day_start_ms(end).unwrap();
        assert_ne!(start % 86_400_000, 0);
        let value = scope(
            &clock,
            Some(ObservationValueBreakdownV1::BillingClass),
            Some("codex".into()),
            ValuePeriodV1::Today,
            start,
            end,
        )
        .unwrap()
        .unwrap();
        assert_eq!(value.kind, Kind::Today);
        assert_eq!(value.timezone, "local (UTC+08:00)");
        assert_eq!(value.agent_filter.as_deref(), Some("codex"));
        assert!(
            scope(&clock, None, None, ValuePeriodV1::ThirtyDays, start, end)
                .unwrap()
                .is_none()
        );
        assert!(
            scope(
                &clock,
                None,
                Some("codex".into()),
                ValuePeriodV1::Today,
                start,
                end
            )
            .is_err()
        );
        assert!(
            scope(
                &clock,
                Some(ObservationValueBreakdownV1::BillingClass),
                None,
                ValuePeriodV1::ThirtyDays,
                start,
                end
            )
            .is_err()
        );
    }
}
