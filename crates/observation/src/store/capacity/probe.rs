//! Opt-in test-only attribution; absent from production builds and ordinary tests.
use std::{
    sync::{Mutex, OnceLock},
    time::Instant,
};

#[derive(Clone, serde::Serialize)]
pub(crate) struct Cost {
    at_ms: i64,
    wall_us: u128,
    cpu_ticks: Option<u64>,
    reason: String,
}
static COSTS: OnceLock<Mutex<Vec<Cost>>> = OnceLock::new();

pub(crate) fn enable() {
    COSTS.get_or_init(|| Mutex::new(Vec::new()));
}
pub(crate) fn samples() -> Vec<Cost> {
    COSTS
        .get()
        .map(|costs| costs.lock().unwrap().clone())
        .unwrap_or_default()
}
pub(crate) fn begin() -> Option<(Instant, Option<u64>)> {
    COSTS.get().map(|_| (Instant::now(), thread_ticks()))
}
pub(crate) fn record(start: Option<(Instant, Option<u64>)>, at_ms: i64, reason: &str) {
    if let Some((wall, cpu)) = start {
        let end = thread_ticks();
        COSTS.get().unwrap().lock().unwrap().push(Cost {
            at_ms,
            wall_us: wall.elapsed().as_micros(),
            cpu_ticks: cpu.zip(end).map(|(a, b)| b.saturating_sub(a)),
            reason: reason.into(),
        });
    }
}
fn thread_ticks() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string("/proc/thread-self/stat").ok()?;
        let (_, fields) = stat.rsplit_once(") ")?;
        let mut fields = fields.split_whitespace();
        let user: u64 = fields.nth(11)?.parse().ok()?;
        let system: u64 = fields.next()?.parse().ok()?;
        user.checked_add(system)
    }
    #[cfg(not(target_os = "linux"))]
    None
}
