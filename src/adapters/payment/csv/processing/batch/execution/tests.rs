use std::{
    io::{Error as IoError, Write},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use rstest::rstest;

use crate::{adapters::observability::memory::InMemoryTraceService, domain::observability::Event};

use super::{
    Failure, NonZeroUsize, SourceContext, SourceFailure, State, TraceError, TraceService, run,
    validate,
};

fn context(id: &str) -> SourceContext {
    SourceContext {
        run_id: "batch-1".into(),
        source_id: id.into(),
        partner_id: None,
    }
}

fn inputs() -> [(&'static [u8], SourceContext); 2] {
    [
        (b"type,client,tx,amount\ndeposit,9,1,5\nwithdrawal,9,2,1\ndispute,9,1,\nresolve,9,1,\ndeposit,9,1,5\n", context("a")),
        (b"type,client,tx,amount\nwithdrawal,2,3,1\ndeposit,2,4,3\nresolve,2,99,\ndeposit,7,5,2\n", context("b")),
    ]
}

#[rstest]
#[case(1)]
#[case(2)]
#[case(4)]
fn output_and_outcomes_are_deterministic_across_worker_limits(#[case] workers: usize) {
    let batch = validate(inputs()).unwrap();
    let mut output = Vec::new();
    let report = run(
        batch,
        &mut output,
        NonZeroUsize::new(workers).unwrap(),
        |_| Ok(InMemoryTraceService::new()),
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "client,available,held,total,locked\n2,3.0000,0.0000,3.0000,false\n7,2.0000,0.0000,2.0000,false\n9,4.0000,0.0000,4.0000,false\n"
    );
    assert_eq!(report.summary.applied, 6);
    assert_eq!(report.summary.replayed, 1);
    assert_eq!(report.summary.rejected, 1);
    assert_eq!(report.summary.ignored, 1);
    assert_eq!(report.sources[0].source.source_id, "a");
    assert_eq!(report.sources[1].source.source_id, "b");
    assert!(
        report
            .sources
            .iter()
            .all(|source| source.state == State::Completed && source.error.is_none())
    );
}

#[derive(Default)]
struct Gate {
    entered: Mutex<usize>,
    ready: Condvar,
}

struct GatedTrace {
    gate: Arc<Gate>,
}

impl TraceService for GatedTrace {
    fn emit(&mut self, event: Event) -> Result<(), TraceError> {
        if event.event_name == "payment.request_applied" {
            let mut entered = self.gate.entered.lock().unwrap();
            *entered += 1;
            self.gate.ready.notify_all();
            let (_entered, timeout) = self
                .gate
                .ready
                .wait_timeout_while(entered, Duration::from_secs(5), |entered| *entered < 2)
                .unwrap();
            if timeout.timed_out() {
                return Err(TraceError::new(IoError::other("workers did not overlap")));
            }
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), TraceError> {
        Ok(())
    }
}

#[test]
fn two_sources_process_concurrently_without_timing_assertions() {
    let gate = Arc::new(Gate::default());
    let first = b"type,client,tx,amount\ndeposit,1,1,1\n".as_slice();
    let second = b"type,client,tx,amount\ndeposit,2,2,1\n".as_slice();
    let batch = validate([(first, context("a")), (second, context("b"))]).unwrap();
    let report = run(batch, Vec::new(), NonZeroUsize::new(2).unwrap(), |_| {
        Ok(GatedTrace {
            gate: Arc::clone(&gate),
        })
    })
    .unwrap();
    assert_eq!(report.summary.applied, 2);
    assert_eq!(*gate.entered.lock().unwrap(), 2);
}

struct Trace {
    events: Arc<Mutex<Vec<Event>>>,
    fail_on: Option<&'static str>,
    fail_flush: bool,
    panic_on_request: bool,
}

impl TraceService for Trace {
    fn emit(&mut self, event: Event) -> Result<(), TraceError> {
        let name = event.event_name.clone();
        self.events.lock().unwrap().push(event);
        assert!(
            !(self.panic_on_request && name == "payment.request_applied"),
            "simulated worker panic"
        );
        if self.fail_on == Some(name.as_str()) {
            Err(TraceError::new(IoError::other("delivery failure")))
        } else {
            Ok(())
        }
    }
    fn flush(&mut self) -> Result<(), TraceError> {
        if self.fail_flush {
            Err(TraceError::new(IoError::other("flush failure")))
        } else {
            Ok(())
        }
    }
}

#[rstest]
#[case(1, 0)]
#[case(2, 1)]
fn failed_source_prevents_publication_and_skips_later_groups(
    #[case] workers: usize,
    #[case] peer_applied: u64,
) {
    let a = b"type,client,tx,amount\ndeposit,1,1,17014118346046923173168730371588410.5727\ndeposit,1,2,1\n".as_slice();
    let b = b"type,client,tx,amount\ndeposit,2,3,1\n".as_slice();
    let c = b"type,client,tx,amount\ndeposit,3,4,1\n".as_slice();
    let batch = validate([(a, context("a")), (b, context("b")), (c, context("c"))]).unwrap();
    let mut output = Vec::new();
    let error = run(
        batch,
        &mut output,
        NonZeroUsize::new(workers).unwrap(),
        |_| Ok(InMemoryTraceService::new()),
    )
    .unwrap_err();
    assert!(output.is_empty());
    assert_eq!(error.report.summary.applied, 1 + peer_applied);
    assert_eq!(error.report.summary.processing_errors, 1);
    assert!(matches!(
        error.report.sources[0]
            .error
            .as_ref()
            .unwrap()
            .failure
            .as_ref(),
        SourceFailure::Processing { .. }
    ));
    assert!(matches!(
        error.report.sources[2]
            .error
            .as_ref()
            .unwrap()
            .failure
            .as_ref(),
        SourceFailure::Cancelled
    ));
    assert_eq!(error.report.sources[1].summary.applied, peer_applied);
    assert_eq!(error.report.sources[2].summary.applied, 0);
    assert_eq!(error.report.sources.len(), 3);
    assert_eq!(error.report.sources[0].state, State::Failed);
    assert_eq!(
        error.report.sources[1].state,
        if workers == 1 {
            State::Failed
        } else {
            State::Completed
        }
    );
    assert_eq!(error.report.sources[2].state, State::Failed);
    let cancelled = error
        .report
        .sources
        .iter()
        .filter(|source| {
            matches!(
                source.error.as_ref().map(|error| error.failure.as_ref()),
                Some(SourceFailure::Cancelled)
            )
        })
        .count();
    assert_eq!(cancelled, if workers == 1 { 2 } else { 1 });
    assert_eq!(
        error.report.summary.applied,
        error
            .report
            .sources
            .iter()
            .map(|source| source.summary.applied)
            .sum::<u64>()
    );
}

#[rstest]
#[case(Some("payment.request_applied"), false, false)]
#[case(None, true, false)]
#[case(None, false, true)]
fn delivery_failure_or_panic_keeps_applied_counts_without_retry(
    #[case] fail_on: Option<&'static str>,
    #[case] fail_flush: bool,
    #[case] panic_on_request: bool,
) {
    let a = b"type,client,tx,amount\ndeposit,1,1,1\ndeposit,1,2,1\n".as_slice();
    let b = b"type,client,tx,amount\ndeposit,2,3,1\n".as_slice();
    let batch = validate([(a, context("a")), (b, context("b"))]).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut output = Vec::new();
    let error = run(batch, &mut output, NonZeroUsize::MIN, |_| {
        Ok(Trace {
            events: Arc::clone(&events),
            fail_on,
            fail_flush,
            panic_on_request,
        })
    })
    .unwrap_err();
    assert!(output.is_empty());
    assert_eq!(error.report.summary.applied, if fail_flush { 2 } else { 1 });
    assert_eq!(error.report.sources[1].summary.applied, 0);
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_name == "payment.request_applied")
            .count(),
        if fail_flush { 2 } else { 1 }
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_name == "payment.run_finished")
            .count(),
        2
    );
}

#[test]
fn trace_setup_failure_happens_before_any_payment_is_processed() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let batch = validate(inputs()).unwrap();
    let mut output = Vec::new();
    let error = run(
        batch,
        &mut output,
        NonZeroUsize::new(2).unwrap(),
        |source| {
            if source.source_id == "b" {
                return Err(TraceError::new(IoError::other("setup failure")));
            }
            Ok(Trace {
                events: Arc::clone(&events),
                fail_on: None,
                fail_flush: false,
                panic_on_request: false,
            })
        },
    )
    .unwrap_err();
    assert!(matches!(error.failure.as_ref(), Failure::TraceSetup { .. }));
    assert_eq!(error.report.summary.applied, 0);
    assert!(output.is_empty());
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .all(|event| event.event_name == "payment.run_finished")
    );
}

struct FailingOutput;

impl Write for FailingOutput {
    fn write(&mut self, _: &[u8]) -> Result<usize, IoError> {
        Err(IoError::other("output failed"))
    }
    fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}

#[test]
fn output_failure_is_preserved_and_all_source_summaries_fail() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let batch = validate(inputs()).unwrap();
    let error = run(batch, FailingOutput, NonZeroUsize::new(2).unwrap(), |_| {
        Ok(Trace {
            events: Arc::clone(&events),
            fail_on: None,
            fail_flush: false,
            panic_on_request: false,
        })
    })
    .unwrap_err();
    assert!(matches!(error.failure.as_ref(), Failure::Output(_)));
    assert_eq!(error.report.summary.applied, 6);
    for event in events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| event.event_name == "payment.run_finished")
    {
        assert_eq!(event.attributes["status"], "failed");
    }
}

#[test]
fn final_trace_failure_can_fail_batch_after_complete_account_output() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let batch = validate(inputs()).unwrap();
    let mut output = Vec::new();
    let error = run(batch, &mut output, NonZeroUsize::new(2).unwrap(), |_| {
        Ok(Trace {
            events: Arc::clone(&events),
            fail_on: Some("payment.run_finished"),
            fail_flush: false,
            panic_on_request: false,
        })
    })
    .unwrap_err();
    assert!(!output.is_empty());
    assert_eq!(error.report.summary.applied, 6);
    assert!(
        error
            .report
            .sources
            .iter()
            .all(|source| source.error.is_some())
    );
}
