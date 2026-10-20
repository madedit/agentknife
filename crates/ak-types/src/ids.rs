//! Identifiers for turns, projects, checkpoints. Native only: `SystemTime::now()`
//! panics on wasm32-unknown-unknown, and the UI never creates ids.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Unique enough within one installation: time + process-wide counter.
pub fn new_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!(
        "{prefix}-{nanos:x}-{}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Seconds since the Unix epoch.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_unique_and_prefixed() {
        let a = super::new_id("turn");
        let b = super::new_id("turn");
        assert!(a.starts_with("turn-"));
        assert_ne!(a, b);
    }
}
