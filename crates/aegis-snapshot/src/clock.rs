//! Injectable clock primitives for deterministic retention tests.

use time::OffsetDateTime;

/// Injectable clock for deterministic retention tests.
pub trait Clock: Send + Sync {
    /// Return the current time.
    fn now(&self) -> OffsetDateTime;
}

/// Clock that returns the current system time.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

/// Clock that returns a fixed timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock(OffsetDateTime);

impl FixedClock {
    /// Create a clock that always returns `timestamp`.
    pub fn new(timestamp: OffsetDateTime) -> Self {
        Self(timestamp)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.0
    }
}
