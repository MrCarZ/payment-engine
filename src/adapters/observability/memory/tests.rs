use time::UtcOffset;

use super::InMemoryTraceService;
use crate::{
    adapters::observability::tests::{FixedClock, clock, event},
    manager::observability::TraceService,
};

#[test]
fn recorder_preserves_event_order_and_structure() {
    let mut trace = InMemoryTraceService::with_clock(clock());
    let first = event();
    let mut second = event();
    second.component = "another-service".into();
    second.event_name = "another.operation_failed".into();
    trace.emit(first.clone()).unwrap();
    trace.emit(second.clone()).unwrap();
    trace.flush().unwrap();
    assert_eq!(trace.events().len(), 2);
    assert_eq!(trace.events()[0].event, first);
    assert_eq!(trace.events()[1].event, second);
    assert_eq!(trace.events()[0].timestamp, clock().0);
    assert_eq!(trace.events()[1].timestamp, clock().0);
}

#[test]
fn recorder_normalizes_clock_offset() {
    let timestamp = clock().0.to_offset(UtcOffset::from_hms(-3, 0, 0).unwrap());
    let mut trace = InMemoryTraceService::with_clock(FixedClock(timestamp));
    trace.emit(event()).unwrap();
    assert_eq!(trace.events()[0].timestamp.offset(), UtcOffset::UTC);
}

#[test]
fn services_can_use_the_trait_as_a_dynamic_dependency() {
    let mut trace = InMemoryTraceService::default();
    let service: &mut dyn TraceService = &mut trace;
    service.emit(event()).unwrap();
    service.flush().unwrap();
    assert_eq!(trace.events().len(), 1);
}
