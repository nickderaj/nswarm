//! Injected clocks for deterministic command and query behavior.

use chrono::{SecondsFormat, Utc};

/// Supplies a v0-compatible ISO-8601 timestamp.
pub trait Clock: Send + Sync {
    /// Returns the current timestamp used for persistence and query cutoffs.
    fn now_iso8601(&self) -> String;
}

/// Production UTC wall clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_iso8601(&self) -> String {
        Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
    }
}

/// Deterministic clock used by tests and parity fixtures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedClock {
    timestamp: String,
}

impl FixedClock {
    /// Creates a clock that always returns `timestamp`.
    #[must_use]
    pub fn new(timestamp: impl Into<String>) -> Self {
        Self {
            timestamp: timestamp.into(),
        }
    }
}

impl Clock for FixedClock {
    fn now_iso8601(&self) -> String {
        self.timestamp.clone()
    }
}
