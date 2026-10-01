use time::OffsetDateTime;

use crate::domain::clock::Clock;

/// System wall clock returning the current UTC time.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
