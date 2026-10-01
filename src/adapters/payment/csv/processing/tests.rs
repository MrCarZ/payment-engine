use std::io::{Error as IoError, Write};

use rstest::rstest;

use crate::{domain::observability::Event, manager::observability::TraceError};

use super::{Failure, SourceContext, Summary, TraceService, run};

#[derive(Default)]
struct Trace {
    events: Vec<Event>,
    fail_on: Option<&'static str>,
    fail_flush: bool,
    fail_flush_on: Option<usize>,
    flushes: usize,
}

impl TraceService for Trace {
    fn emit(&mut self, event: Event) -> Result<(), TraceError> {
        let fail = self.fail_on == Some(event.event_name.as_str());
        self.events.push(event);
        if fail {
            Err(TraceError::new(IoError::other("emit failure")))
        } else {
            Ok(())
        }
    }

    fn flush(&mut self) -> Result<(), TraceError> {
        self.flushes += 1;
        if self.fail_flush || self.fail_flush_on == Some(self.flushes) {
            Err(TraceError::new(IoError::other("flush failure")))
        } else {
            Ok(())
        }
    }
}

fn source() -> SourceContext {
    SourceContext {
        run_id: "run-1".into(),
        source_id: "csv-1".into(),
        partner_id: Some("partner-1".into()),
    }
}

#[test]
fn composes_input_processing_output_and_trace_in_order() {
    let csv = "type,client,tx,amount\ndeposit,2,1,5\ndeposit,2,1,5\nwithdrawal,1,2,1\nresolve,3,99,\ndispute,2,1,\nchargeback,2,1,\ndeposit,2,3,1\n";
    let mut output = Vec::new();
    let mut trace = Trace::default();
    let summary = run(csv.as_bytes(), &mut output, source(), &mut trace).unwrap();
    assert_eq!(
        summary,
        Summary {
            applied: 3,
            ignored: 1,
            rejected: 2,
            replayed: 1,
            input_errors: 0,
            processing_errors: 0
        }
    );
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "client,available,held,total,locked\n1,0.0000,0.0000,0.0000,false\n2,0.0000,0.0000,0.0000,true\n3,0.0000,0.0000,0.0000,false\n"
    );
    let names: Vec<_> = trace
        .events
        .iter()
        .map(|event| event.event_name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "payment.request_applied",
            "payment.request_replayed",
            "payment.request_rejected",
            "payment.request_ignored",
            "payment.request_applied",
            "payment.request_applied",
            "payment.request_rejected",
            "payment.run_finished"
        ]
    );
    let summary_event = trace.events.last().unwrap();
    assert_eq!(summary_event.attributes["status"], "completed");
    assert_eq!(summary_event.attributes["applied"], 3);
    assert_eq!(summary_event.attributes["replayed"], 1);
    assert_eq!(trace.events[0].attributes["line"], 2);
    assert_eq!(trace.flushes, 2);
}

#[rstest]
#[case("", 0)]
#[case("client,tx,amount\n", 0)]
#[case(
    "type,client,tx,amount\ndeposit,1,1,1\ndeposit,1,2,invalid\ndeposit,1,3,1\n",
    1
)]
fn input_failure_stops_without_publishing_accounts(#[case] csv: &str, #[case] applied: u64) {
    let mut output = Vec::new();
    let mut trace = Trace::default();
    let error = run(csv.as_bytes(), &mut output, source(), &mut trace).unwrap_err();
    assert!(matches!(*error.failure, Failure::Input(_)));
    assert_eq!(error.summary.applied, applied);
    assert_eq!(error.summary.input_errors, 1);
    assert!(output.is_empty());
    assert_eq!(
        trace.events[trace.events.len() - 2].event_name,
        "payment.input_failed"
    );
    assert_eq!(trace.events.last().unwrap().attributes["status"], "failed");
    assert_eq!(trace.flushes, 1);
}

#[test]
fn arithmetic_failure_is_traced_with_context_and_stops_following_rows() {
    let csv = "type,client,tx,amount\ndeposit,1,1,17014118346046923173168730371588410.5727\ndeposit,1,2,1\ndeposit,2,3,1\n";
    let mut output = Vec::new();
    let mut trace = Trace::default();
    let error = run(csv.as_bytes(), &mut output, source(), &mut trace).unwrap_err();
    assert!(matches!(*error.failure, Failure::Processing { .. }));
    assert_eq!(error.summary.applied, 1);
    assert_eq!(error.summary.processing_errors, 1);
    assert!(output.is_empty());
    assert_eq!(trace.events.len(), 3);
    assert_eq!(
        trace.events[1].attributes["reason_code"],
        "arithmetic_error"
    );
    assert_eq!(trace.events[1].attributes["line"], 3);
    assert!(error.to_string().contains("record 2"));
}

#[test]
fn trace_emit_failure_never_retries_an_applied_payment() {
    let csv = "type,client,tx,amount\ndeposit,1,1,5\ndeposit,1,2,5\n";
    let mut output = Vec::new();
    let mut trace = Trace {
        fail_on: Some("payment.request_applied"),
        ..Trace::default()
    };
    let error = run(csv.as_bytes(), &mut output, source(), &mut trace).unwrap_err();
    assert!(matches!(*error.failure, Failure::Trace(_)));
    assert_eq!(error.summary.applied, 1);
    assert_eq!(trace.events.len(), 2);
    assert_eq!(trace.events[1].event_name, "payment.run_finished");
    assert_eq!(trace.events[1].attributes["status"], "failed");
    assert!(output.is_empty());
    assert_eq!(trace.flushes, 1);
}

#[rstest]
#[case("payment.input_failed", false, 0)]
#[case("payment.run_finished", true, 2)]
fn original_input_failure_survives_trace_failures(
    #[case] fail_on: &'static str,
    #[case] fail_flush: bool,
    #[case] additional: usize,
) {
    let mut trace = Trace {
        fail_on: Some(fail_on),
        fail_flush,
        ..Trace::default()
    };
    let error = run("".as_bytes(), Vec::new(), source(), &mut trace).unwrap_err();
    if fail_on == "payment.input_failed" {
        assert!(matches!(*error.failure, Failure::WithTrace { .. }));
    } else {
        assert!(matches!(*error.failure, Failure::Input(_)));
    }
    assert_eq!(error.additional_trace_errors.len(), additional);
    assert!(error.to_string().contains("input failed"));
    assert_eq!(trace.flushes, 1);
}

#[test]
fn buffered_trace_failure_prevents_account_publication() {
    let mut output = Vec::new();
    let mut trace = Trace {
        fail_flush: true,
        ..Trace::default()
    };
    let error = run(
        "type,client,tx,amount\ndeposit,1,1,5\n".as_bytes(),
        &mut output,
        source(),
        &mut trace,
    )
    .unwrap_err();
    assert!(matches!(*error.failure, Failure::Trace(_)));
    assert_eq!(error.summary.applied, 1);
    assert_eq!(error.additional_trace_errors.len(), 1);
    assert_eq!(trace.events.last().unwrap().attributes["status"], "failed");
    assert!(output.is_empty());
    assert_eq!(trace.flushes, 2);
}

struct FailingWriter {
    fail_flush: bool,
}

impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, IoError> {
        if self.fail_flush {
            Ok(bytes.len())
        } else {
            Err(IoError::other("write failure"))
        }
    }
    fn flush(&mut self) -> Result<(), IoError> {
        Err(IoError::other("flush failure"))
    }
}

#[rstest]
#[case(false)]
#[case(true)]
fn output_failure_finishes_as_failed(#[case] fail_flush: bool) {
    let mut trace = Trace::default();
    let error = run(
        "type,client,tx,amount\n".as_bytes(),
        FailingWriter { fail_flush },
        source(),
        &mut trace,
    )
    .unwrap_err();
    assert!(matches!(*error.failure, Failure::Output(_)));
    assert_eq!(trace.events.last().unwrap().attributes["status"], "failed");
    assert_eq!(trace.flushes, 2);
}

#[test]
fn summary_delivery_failure_reports_failure_even_after_output() {
    let mut output = Vec::new();
    let mut trace = Trace {
        fail_on: Some("payment.run_finished"),
        ..Trace::default()
    };
    let error = run(
        "type,client,tx,amount\n".as_bytes(),
        &mut output,
        source(),
        &mut trace,
    )
    .unwrap_err();
    assert!(matches!(*error.failure, Failure::Trace(_)));
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "client,available,held,total,locked\n"
    );
    assert_eq!(trace.flushes, 2);
}

#[test]
fn final_flush_failure_is_returned_after_complete_account_output() {
    let mut output = Vec::new();
    let mut trace = Trace {
        fail_flush_on: Some(2),
        ..Trace::default()
    };
    let error = run(
        "type,client,tx,amount\ndeposit,1,1,5\n".as_bytes(),
        &mut output,
        source(),
        &mut trace,
    )
    .unwrap_err();
    assert!(matches!(*error.failure, Failure::Trace(_)));
    assert_eq!(error.summary.applied, 1);
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "client,available,held,total,locked\n1,5.0000,0.0000,5.0000,false\n"
    );
    assert_eq!(
        trace.events.last().unwrap().attributes["status"],
        "completed"
    );
    assert_eq!(trace.flushes, 2);
}
