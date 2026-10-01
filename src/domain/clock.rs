use time::OffsetDateTime;

/// Injectable wall-clock timestamp source for services across the codebase.
pub trait Clock {
    fn now(&self) -> OffsetDateTime;
}
