use std::{
    cell::RefCell,
    error::Error,
    io::{Error as IoError, Result as IoResult, Write},
    rc::Rc,
};

use csv::Reader;
use rstest::rstest;
use serde_json::{Value, from_str, json};
use time::{Date, Month, UtcOffset};

use super::{CsvTraceError, CsvTraceService};
use crate::{
    adapters::observability::tests::{FixedClock, clock, event},
    domain::observability::Severity,
    manager::observability::{TraceError, TraceService},
};

fn csv_error(error: &TraceError) -> &CsvTraceError {
    error
        .source()
        .unwrap()
        .downcast_ref()
        .expect("CSV error cause")
}

const HEADER: &str = "timestamp,severity,component,event_name,correlation_id,message,attributes\n";

#[rstest]
#[case::debug(Severity::Debug, "debug")]
#[case::info(Severity::Info, "info")]
#[case::warn(Severity::Warn, "warn")]
#[case::error(Severity::Error, "error")]
fn structured_events_preserve_fields_and_severity(#[case] severity: Severity, #[case] label: &str) {
    let mut bytes = Vec::new();
    let mut trace = CsvTraceService::with_clock(&mut bytes, clock()).unwrap();
    let mut event = event();
    event.severity = severity;
    trace.emit(event.clone()).unwrap();
    trace.flush().unwrap();
    drop(trace);

    let mut reader = Reader::from_reader(bytes.as_slice());
    assert_eq!(
        reader.headers().unwrap().iter().collect::<Vec<_>>(),
        [
            "timestamp",
            "severity",
            "component",
            "event_name",
            "correlation_id",
            "message",
            "attributes"
        ]
    );
    let row = reader.records().next().unwrap().unwrap();
    assert_eq!(&row[0], "2020-09-13T12:26:40Z");
    assert_eq!(&row[1], label);
    assert_eq!(&row[2], event.component);
    assert_eq!(&row[3], event.event_name);
    assert_eq!(&row[4], event.correlation_id.unwrap());
    assert_eq!(&row[5], event.message);
    assert_eq!(
        from_str::<Value>(&row[6]).unwrap(),
        Value::Object(event.attributes)
    );
}

#[rstest]
#[case::comma("message,with,commas")]
#[case::quotes("a \"quoted\" message")]
#[case::newline("first line\nsecond line")]
fn csv_escaping_round_trips_messages_and_nested_attributes(#[case] message: &str) {
    let mut bytes = Vec::new();
    let mut trace = CsvTraceService::with_clock(&mut bytes, clock()).unwrap();
    let mut event = event();
    event.message = message.into();
    event
        .attributes
        .insert("details".into(), json!({"items": [1, true, null, message]}));
    trace.emit(event.clone()).unwrap();
    trace.flush().unwrap();
    drop(trace);
    let row = Reader::from_reader(bytes.as_slice())
        .records()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(&row[5], message);
    assert_eq!(
        from_str::<Value>(&row[6]).unwrap(),
        Value::Object(event.attributes)
    );
}

#[test]
fn absent_correlation_and_attributes_have_stable_representation() {
    let mut bytes = Vec::new();
    let mut trace = CsvTraceService::with_clock(&mut bytes, clock()).unwrap();
    let mut event = event();
    event.correlation_id = None;
    event.attributes.clear();
    trace.emit(event).unwrap();
    trace.flush().unwrap();
    drop(trace);
    let row = Reader::from_reader(bytes.as_slice())
        .records()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(&row[4], "");
    assert_eq!(&row[6], "{}");
}

#[test]
fn clock_offsets_are_normalized_to_utc() {
    let timestamp = clock().0.to_offset(UtcOffset::from_hms(3, 0, 0).unwrap());
    let mut bytes = Vec::new();
    let mut trace = CsvTraceService::with_clock(&mut bytes, FixedClock(timestamp)).unwrap();
    trace.emit(event()).unwrap();
    trace.flush().unwrap();
    drop(trace);
    let row = Reader::from_reader(bytes.as_slice())
        .records()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(&row[0], "2020-09-13T12:26:40Z");
}

#[test]
fn empty_sink_still_writes_one_header_when_flushed() {
    let mut bytes = Vec::new();
    let mut trace = CsvTraceService::new(&mut bytes).unwrap();
    trace.flush().unwrap();
    trace.flush().unwrap();
    drop(trace);
    assert_eq!(String::from_utf8(bytes).unwrap(), HEADER);
}

#[derive(Default)]
struct WriterState {
    bytes: Vec<u8>,
    fail_write: bool,
    fail_flush: bool,
    flushes: usize,
}

struct ProbeWriter(Rc<RefCell<WriterState>>);

impl Write for ProbeWriter {
    fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
        let mut state = self.0.borrow_mut();
        if state.fail_write {
            return Err(IoError::other("simulated trace write failure"));
        }
        state.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> IoResult<()> {
        let mut state = self.0.borrow_mut();
        state.flushes += 1;
        if state.fail_flush {
            return Err(IoError::other("simulated trace flush failure"));
        }
        Ok(())
    }
}

#[test]
fn emit_buffers_small_events_until_explicit_flush() {
    let state = Rc::new(RefCell::new(WriterState::default()));
    let mut trace = CsvTraceService::with_clock(ProbeWriter(Rc::clone(&state)), clock()).unwrap();
    trace.emit(event()).unwrap();
    assert!(state.borrow().bytes.is_empty());
    trace.flush().unwrap();
    assert!(!state.borrow().bytes.is_empty());
    assert_eq!(state.borrow().flushes, 1);
}

#[rstest]
#[case::write_failure(true, false)]
#[case::flush_failure(false, true)]
fn explicit_flush_propagates_write_and_flush_failures(
    #[case] fail_write: bool,
    #[case] fail_flush: bool,
) {
    let state = Rc::new(RefCell::new(WriterState {
        fail_write,
        fail_flush,
        ..WriterState::default()
    }));
    let mut trace = CsvTraceService::with_clock(ProbeWriter(state), clock()).unwrap();
    trace.emit(event()).unwrap();
    let error = trace.flush().unwrap_err();
    assert!(matches!(csv_error(&error), CsvTraceError::Io(_)));
    assert!(error.source().is_some());
}

#[test]
fn emit_propagates_io_failures_when_buffer_fills() {
    let state = Rc::new(RefCell::new(WriterState {
        fail_write: true,
        ..WriterState::default()
    }));
    let mut trace = CsvTraceService::with_clock(ProbeWriter(state), clock()).unwrap();
    let mut event = event();
    event.message = "x".repeat(16_384);
    let error = trace.emit(event).unwrap_err();
    assert!(matches!(csv_error(&error), CsvTraceError::Csv(_)));
    assert!(error.source().is_some());
}

#[test]
fn unformattable_timestamp_returns_error_without_event_record() {
    let timestamp = Date::from_calendar_date(-1, Month::January, 1)
        .unwrap()
        .midnight()
        .assume_utc();
    let mut bytes = Vec::new();
    let mut trace = CsvTraceService::with_clock(&mut bytes, FixedClock(timestamp)).unwrap();
    let error = trace.emit(event()).unwrap_err();
    assert!(matches!(csv_error(&error), CsvTraceError::Timestamp(_)));
    trace.flush().unwrap();
    drop(trace);
    assert_eq!(String::from_utf8(bytes).unwrap(), HEADER);
}
