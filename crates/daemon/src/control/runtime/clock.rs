//! The existing host-clock authority resolves calendar days and each boundary's
//! UTC offset. This does not create an observation or configurable timezone.
use super::{ApplicationClockPort, ControlReadError, LocalControlAdapter};
use hiroute_domain::AnalysisLocalDayV1;
use rusqlite::{Connection, params};
use std::time::{SystemTime, UNIX_EPOCH};

impl ApplicationClockPort for LocalControlAdapter {
    fn now_ms(&self) -> Result<i64, ControlReadError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ControlReadError::Corrupt)?
            .as_millis()
            .try_into()
            .map_err(|_| ControlReadError::Corrupt)
    }

    fn local_day_start_ms(&self, at_ms: i64) -> Result<i64, ControlReadError> {
        Ok(local_day(None, 0, at_ms)?.from_ms)
    }

    fn analysis_local_day(
        &self,
        day: Option<&str>,
        days_ago: u8,
        at_ms: i64,
    ) -> Result<AnalysisLocalDayV1, ControlReadError> {
        local_day(day, days_ago, at_ms)
    }

    fn local_utc_offset_seconds(&self, at_ms: i64) -> Result<i32, ControlReadError> {
        utc_offset(at_ms)
    }
}

fn utc_offset(at_ms: i64) -> Result<i32, ControlReadError> {
    if at_ms < 0 {
        return Err(ControlReadError::Corrupt);
    }
    let clock = Connection::open_in_memory().map_err(|_| ControlReadError::Unavailable)?;
    clock
        .query_row(
            "SELECT CAST(strftime('%s',?1,'unixepoch','localtime') AS INTEGER)-?1",
            [at_ms / 1000],
            |r| r.get(0),
        )
        .map_err(|_| ControlReadError::Unavailable)
}

fn local_day(
    day: Option<&str>,
    days_ago: u8,
    at_ms: i64,
) -> Result<AnalysisLocalDayV1, ControlReadError> {
    if at_ms < 0 || days_ago > 1 || (day.is_some() && days_ago != 0) {
        return Err(ControlReadError::Corrupt);
    }
    let clock = Connection::open_in_memory().map_err(|_| ControlReadError::Unavailable)?;
    let civil: String = clock
        .query_row(
            "SELECT date(COALESCE(?1,date(?2,'unixepoch','localtime')),?3)",
            params![day, at_ms / 1000, format!("-{days_ago} days")],
            |r| r.get(0),
        )
        .map_err(|_| ControlReadError::Unavailable)?;
    // Apply the next civil day before converting either endpoint to UTC.
    clock.query_row(
        "SELECT CAST(strftime('%s',?1,'utc') AS INTEGER)*1000,CAST(strftime('%s',?1,'+1 day','utc') AS INTEGER)*1000",
        [civil], |r| Ok(AnalysisLocalDayV1 { from_ms: r.get(0)?, to_ms: r.get(1)? }),
    ).map_err(|_| ControlReadError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn civil_day_boundaries_follow_both_dst_transitions() {
        if std::env::var_os("HIROUTE_CLOCK_DST_CHILD").is_none() {
            let output=std::process::Command::new(std::env::current_exe().unwrap())
                .env("TZ","America/New_York").env("HIROUTE_CLOCK_DST_CHILD","1")
                .args(["--exact","control::runtime::clock::tests::civil_day_boundaries_follow_both_dst_transitions","--nocapture"])
                .output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            return;
        }
        for (day, hours, from_offset, to_offset) in [
            ("2026-03-07", 24, -18000, -18000),
            ("2026-03-08", 23, -18000, -14400),
            ("2026-11-01", 25, -14400, -18000),
        ] {
            let range = local_day(Some(day), 0, 1).unwrap();
            assert_eq!(range.to_ms - range.from_ms, hours * 3_600_000, "{day}");
            assert_eq!(utc_offset(range.from_ms).unwrap(), from_offset);
            assert_eq!(utc_offset(range.to_ms).unwrap(), to_offset);
        }
    }
}
