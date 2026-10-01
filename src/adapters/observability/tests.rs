use serde_json::{Map, Value};
use time::OffsetDateTime;

use crate::{
    domain::clock::Clock,
    domain::observability::{Event, Severity},
};

pub(super) struct FixedClock(pub OffsetDateTime);

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.0
    }
}

pub(super) fn clock() -> FixedClock {
    FixedClock(OffsetDateTime::from_unix_timestamp(1_600_000_000).unwrap())
}

pub(super) fn event() -> Event {
    Event {
        severity: Severity::Info,
        component: "example-service".into(),
        event_name: "example.operation_completed".into(),
        correlation_id: Some("request-42".into()),
        message: "Operation completed".into(),
        attributes: Map::from_iter([("count".into(), Value::from(2))]),
    }
}
