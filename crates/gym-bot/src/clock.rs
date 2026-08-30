//! Injected clocks for deterministic command and query behavior.

use jiff::{Timestamp, Zoned, tz::TimeZone};

/// Supplies a v0-compatible ISO-8601 timestamp.
pub trait Clock: Send + Sync {
    /// Returns the current timestamp used for persistence and query cutoffs.
    fn now_iso8601(&self) -> String;
}

/// Production wall clock formatted in the configured IANA time zone like v0.
#[derive(Clone, Debug)]
pub struct SystemClock {
    time_zone: TimeZone,
}

impl SystemClock {
    /// Creates a production clock for an IANA time-zone name.
    ///
    /// # Errors
    ///
    /// Returns [`jiff::Error`] if the configured time zone is unavailable.
    pub fn new(time_zone: &str) -> Result<Self, jiff::Error> {
        Ok(Self {
            time_zone: TimeZone::get(time_zone)?,
        })
    }
}

impl Clock for SystemClock {
    fn now_iso8601(&self) -> String {
        format_v0_iso8601(&Timestamp::now().to_zoned(self.time_zone.clone()))
    }
}

/// Converts one absolute timestamp to v0's configured-zone storage format.
///
/// # Errors
///
/// Returns [`jiff::Error`] if the timestamp or IANA time-zone name is invalid.
pub fn timestamp_in_timezone(timestamp: &str, time_zone: &str) -> Result<String, jiff::Error> {
    let timestamp: Timestamp = timestamp.parse()?;
    Ok(format_v0_iso8601(
        &timestamp.to_zoned(TimeZone::get(time_zone)?),
    ))
}

fn format_v0_iso8601(timestamp: &Zoned) -> String {
    if timestamp.microsecond() == 0 {
        timestamp.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string()
    } else {
        timestamp.strftime("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string()
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
