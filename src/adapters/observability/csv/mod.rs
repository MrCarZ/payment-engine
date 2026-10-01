use std::io::Write;

use csv::{Writer, WriterBuilder};
use serde::Serialize;
use serde_json::to_string;
use time::format_description::well_known::Rfc3339;

use crate::{
    adapters::clock::SystemClock,
    domain::clock::Clock,
    domain::observability::{Event, RecordedEvent},
    manager::observability::{TraceError, TraceService},
};

mod error;

pub use error::CsvTraceError;

#[derive(Serialize)]
struct Row<'a> {
    timestamp: String,
    severity: &'a str,
    component: &'a str,
    event_name: &'a str,
    correlation_id: Option<&'a str>,
    message: &'a str,
    attributes: String,
}

/// Buffered generic CSV logging. Call flush explicitly to observe delivery
/// failures; successful emit may only have buffered an event.
pub struct CsvTraceService<W: Write, C: Clock = SystemClock> {
    writer: Writer<W>,
    clock: C,
}

impl<W: Write> CsvTraceService<W> {
    pub fn new(output: W) -> Result<Self, CsvTraceError> {
        Self::with_clock(output, SystemClock)
    }
}

impl<W: Write, C: Clock> CsvTraceService<W, C> {
    pub fn with_clock(output: W, clock: C) -> Result<Self, CsvTraceError> {
        let mut writer = WriterBuilder::new().has_headers(false).from_writer(output);
        writer.write_record([
            "timestamp",
            "severity",
            "component",
            "event_name",
            "correlation_id",
            "message",
            "attributes",
        ])?;
        Ok(Self { writer, clock })
    }

    fn write_event(&mut self, event: Event) -> Result<(), CsvTraceError> {
        let recorded = RecordedEvent::new(event, self.clock.now());
        let event = &recorded.event;
        let row = Row {
            timestamp: recorded.timestamp.format(&Rfc3339)?,
            severity: event.severity.as_str(),
            component: &event.component,
            event_name: &event.event_name,
            correlation_id: event.correlation_id.as_deref(),
            message: &event.message,
            attributes: to_string(&event.attributes)?,
        };
        self.writer.serialize(row)?;
        Ok(())
    }
}

impl<W: Write, C: Clock> TraceService for CsvTraceService<W, C> {
    fn emit(&mut self, event: Event) -> Result<(), TraceError> {
        self.write_event(event).map_err(TraceError::from)
    }

    fn flush(&mut self) -> Result<(), TraceError> {
        self.writer
            .flush()
            .map_err(CsvTraceError::from)
            .map_err(TraceError::from)
    }
}

#[cfg(test)]
mod tests;
