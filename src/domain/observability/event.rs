use serde_json::{Map, Value};
use time::{OffsetDateTime, UtcOffset};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Debug,
    Info,
    Warn,
    Error,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Caller-supplied semantics, independent of any domain or delivery technology.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub severity: Severity,
    pub component: String,
    pub event_name: String,
    pub correlation_id: Option<String>,
    pub message: String,
    pub attributes: Map<String, Value>,
}

/// Timestamped event retained by recorders and delivered by sinks.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedEvent {
    pub timestamp: OffsetDateTime,
    pub event: Event,
}

impl RecordedEvent {
    pub fn new(event: Event, timestamp: OffsetDateTime) -> Self {
        Self {
            timestamp: timestamp.to_offset(UtcOffset::UTC),
            event,
        }
    }
}
