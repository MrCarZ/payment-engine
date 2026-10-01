use crate::{
    adapters::clock::SystemClock,
    domain::clock::Clock,
    domain::observability::{Event, RecordedEvent},
    manager::observability::{TraceError, TraceService},
};

/// Recorder for consumer tests, retaining complete structured events in order.
pub struct InMemoryTraceService<C: Clock = SystemClock> {
    events: Vec<RecordedEvent>,
    clock: C,
}

impl InMemoryTraceService {
    pub fn new() -> Self {
        Self::with_clock(SystemClock)
    }
}

impl Default for InMemoryTraceService {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Clock> InMemoryTraceService<C> {
    pub fn with_clock(clock: C) -> Self {
        Self {
            events: Vec::new(),
            clock,
        }
    }

    pub fn events(&self) -> &[RecordedEvent] {
        &self.events
    }
}

impl<C: Clock> TraceService for InMemoryTraceService<C> {
    fn emit(&mut self, event: Event) -> Result<(), TraceError> {
        self.events
            .push(RecordedEvent::new(event, self.clock.now()));
        Ok(())
    }

    fn flush(&mut self) -> Result<(), TraceError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
