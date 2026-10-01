use super::{
    Failure,
    completion::finalize,
    run,
    workers::{Job, launch},
};
use crate::{
    adapters::observability::memory::InMemoryTraceService,
    domain::{
        observability::Event,
        payment::{Account, ClientId, TransactionId, transaction::Type},
    },
    manager::{
        observability::{TraceError, TraceService},
        payment::{
            Context, Request, SourceContext,
            batch::{Source, ValidatedBatch},
            run::{Output, Record, RunFailure},
            trace::State,
        },
    },
};
use rstest::rstest;
use std::{
    io::Error as IoError,
    num::NonZeroUsize,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
#[derive(Debug)]
struct Envelope {
    request: Request,
    context: Context,
}
impl Record for Envelope {
    fn request(&self) -> Request {
        self.request
    }
    fn context(&self) -> &Context {
        &self.context
    }
}
fn source(id: &str, client: u16, tx: u32, amounts: &[&str]) -> Source<Envelope> {
    let context = SourceContext {
        run_id: "api-batch".into(),
        source_id: id.into(),
        partner_id: None,
    };
    let records = amounts
        .iter()
        .enumerate()
        .map(|(index, amount)| Envelope {
            request: Request::Original {
                client: ClientId::from(client),
                tx: TransactionId::from(tx + index as u32),
                transaction_type: Type::Deposit,
                amount: amount.parse().unwrap(),
            },
            context: Context {
                source: Arc::new(context.clone()),
            },
        })
        .collect();
    Source::new(context, records)
}
fn batch() -> ValidatedBatch<Envelope> {
    ValidatedBatch::try_from(vec![
        source("a", 1, 1, &["1", "2"]),
        source("b", 2, 3, &["4"]),
    ])
    .unwrap()
}
#[derive(Default)]
struct Snapshot {
    accounts: Vec<(u16, String)>,
    calls: usize,
    fail: bool,
}
impl Output for Snapshot {
    type Error = IoError;
    fn publish<'a>(
        &mut self,
        accounts: impl IntoIterator<Item = (ClientId, &'a Account)>,
    ) -> Result<(), Self::Error> {
        self.calls += 1;
        if self.fail {
            return Err(IoError::other("snapshot refused"));
        }
        self.accounts = accounts
            .into_iter()
            .map(|(client, account)| (client.get(), account.available().to_string()))
            .collect();
        self.accounts.sort();
        Ok(())
    }
}
#[rstest]
#[case(1)]
#[case(2)]
#[case(4)]
fn executes_non_csv_sources_with_consistent_results(#[case] workers: usize) {
    let mut output = Snapshot::default();
    let report = run(
        batch(),
        &mut output,
        NonZeroUsize::new(workers).unwrap(),
        |_| Ok(InMemoryTraceService::new()),
    )
    .unwrap();
    assert_eq!(report.summary.applied, 3);
    assert_eq!(output.calls, 1);
    assert_eq!(
        output.accounts,
        [(1, "3.0000".into()), (2, "4.0000".into())]
    );
    assert_eq!(report.sources[0].source.source_id, "a");
    assert_eq!(report.sources[1].source.source_id, "b");
    assert!(
        report
            .sources
            .iter()
            .all(|source| source.state == State::Completed)
    );
}
#[test]
fn failure_suppresses_publication_and_cancels_later_non_csv_sources() {
    let batch = ValidatedBatch::try_from(vec![
        source(
            "a",
            1,
            1,
            &["17014118346046923173168730371588410.5727", "1"],
        ),
        source("b", 2, 3, &["4"]),
    ])
    .unwrap();
    let mut output = Snapshot::default();
    let error = run(batch, &mut output, NonZeroUsize::MIN, |_| {
        Ok(InMemoryTraceService::new())
    })
    .unwrap_err();
    assert_eq!(error.report.summary.applied, 1);
    assert_eq!(error.report.summary.processing_errors, 1);
    assert_eq!(output.calls, 0);
    assert_eq!(error.report.sources.len(), 2);
    assert_eq!(error.report.sources[0].state, State::Failed);
    assert_eq!(error.report.sources[0].summary.applied, 1);
    assert_eq!(error.report.sources[0].summary.processing_errors, 1);
    assert_eq!(error.report.sources[1].state, State::Failed);
    assert_eq!(error.report.sources[1].summary, Default::default());
    assert_eq!(
        error
            .report
            .sources
            .iter()
            .filter(|source| matches!(
                source.error.as_ref().map(|error| error.failure.as_ref()),
                Some(RunFailure::Cancelled)
            ))
            .count(),
        1
    );
    assert!(matches!(
        *error.report.sources[1].error.as_ref().unwrap().failure,
        RunFailure::Cancelled
    ));
}
#[test]
fn output_failure_retains_the_concrete_error_and_failed_source_states() {
    let mut output = Snapshot {
        fail: true,
        ..Snapshot::default()
    };
    let error = run(batch(), &mut output, NonZeroUsize::MIN, |_| {
        Ok(InMemoryTraceService::new())
    })
    .unwrap_err();
    assert!(
        matches!(error.failure.as_ref(), Failure::Output(error) if error.to_string() == "snapshot refused")
    );
    assert_eq!(error.report.summary.applied, 3);
    assert!(
        error
            .report
            .sources
            .iter()
            .all(|source| source.state == State::Failed)
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
fn independent_non_csv_sources_overlap_in_separate_workers() {
    let gate = Arc::new(Gate::default());
    let mut output = Snapshot::default();
    let report = run(batch(), &mut output, NonZeroUsize::new(2).unwrap(), |_| {
        Ok(GatedTrace {
            gate: Arc::clone(&gate),
        })
    })
    .unwrap();
    assert_eq!(report.summary.applied, 3);
    assert_eq!(*gate.entered.lock().unwrap(), 3);
}

struct CleanupTrace {
    source: String,
    attempts: Arc<Mutex<Vec<(String, String)>>>,
    panic_emit: bool,
    panic_flush: bool,
}
impl TraceService for CleanupTrace {
    fn emit(&mut self, event: Event) -> Result<(), TraceError> {
        self.attempts
            .lock()
            .unwrap()
            .push((self.source.clone(), event.event_name));
        assert!(!self.panic_emit, "summary delivery panicked");
        Ok(())
    }
    fn flush(&mut self) -> Result<(), TraceError> {
        self.attempts
            .lock()
            .unwrap()
            .push((self.source.clone(), "flush".into()));
        assert!(!self.panic_flush, "flush panicked");
        Ok(())
    }
}

#[rstest]
#[case(true, false)]
#[case(false, true)]
#[case(true, true)]
fn setup_failure_finalizes_all_prepared_traces_despite_panics(
    #[case] panic_emit: bool,
    #[case] panic_flush: bool,
) {
    let batch = ValidatedBatch::try_from(vec![
        source("a", 1, 1, &["1"]),
        source("b", 2, 2, &["1"]),
        source("c", 3, 3, &["1"]),
    ])
    .unwrap();
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let mut output = Snapshot::default();
    let error = run(batch, &mut output, NonZeroUsize::MIN, |source| {
        if source.source_id == "c" {
            return Err(TraceError::new(IoError::other("primary setup failure")));
        }
        Ok(CleanupTrace {
            source: source.source_id.clone(),
            attempts: Arc::clone(&attempts),
            panic_emit: source.source_id == "a" && panic_emit,
            panic_flush: source.source_id == "a" && panic_flush,
        })
    })
    .unwrap_err();
    assert!(
        matches!(error.failure.as_ref(), Failure::TraceSetup { error, .. } if error.to_string().contains("primary setup failure"))
    );
    assert_eq!(error.report.summary.applied, 0);
    assert_eq!(output.calls, 0);
    assert_eq!(
        error.additional_trace_errors.len(),
        usize::from(panic_emit) + usize::from(panic_flush)
    );
    assert_eq!(
        *attempts.lock().unwrap(),
        vec![
            ("a".into(), "payment.run_finished".into()),
            ("a".into(), "flush".into()),
            ("b".into(), "payment.run_finished".into()),
            ("b".into(), "flush".into()),
        ]
    );
}

#[test]
fn spawn_failure_retains_the_trace_for_finalization() {
    let (source, records) = source("a", 1, 1, &["1"]).into_parts();
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let job = Job {
        index: 0,
        source,
        records,
        trace: CleanupTrace {
            source: "a".into(),
            attempts: Arc::clone(&attempts),
            panic_emit: false,
            panic_flush: false,
        },
    };
    let failure = match launch::<_, _, IoError, ()>(job, |closure_state| {
        drop(closure_state);
        Err(IoError::other("thread creation refused"))
    }) {
        Err(failure) => failure,
        Ok(()) => panic!("expected spawn failure"),
    };
    let report = finalize(vec![*failure], false);
    assert_eq!(report.summary.applied, 0);
    assert!(
        matches!(report.sources[0].error.as_ref().unwrap().failure.as_ref(), RunFailure::WorkerSpawn(error) if error.to_string() == "thread creation refused")
    );
    assert_eq!(
        *attempts.lock().unwrap(),
        vec![
            ("a".into(), "payment.run_finished".into()),
            ("a".into(), "flush".into()),
        ]
    );
}
