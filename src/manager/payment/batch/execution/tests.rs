use super::{Failure, run};
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
