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
            run::{Output, Record},
        },
    },
};
use rstest::rstest;
use std::{convert::Infallible, error::Error, io::Error as IoError, sync::Arc};

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
fn source() -> SourceContext {
    SourceContext {
        run_id: "api-run".into(),
        source_id: "api-source".into(),
        partner_id: None,
    }
}
fn records() -> [Result<Envelope, Infallible>; 2] {
    [1, 2].map(|tx| {
        Ok(Envelope {
            request: Request::Original {
                client: ClientId::from(1),
                tx: TransactionId::from(tx),
                transaction_type: Type::Deposit,
                amount: "2".parse().unwrap(),
            },
            context: Context {
                source: Arc::new(source()),
            },
        })
    })
}
#[derive(Default)]
struct Snapshot {
    calls: usize,
    available: Option<String>,
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
        self.available = accounts
            .into_iter()
            .next()
            .map(|(_, account)| account.available().to_string());
        Ok(())
    }
}
#[test]
fn executes_non_csv_records_and_publishes_through_an_injected_output() {
    let mut output = Snapshot::default();
    let mut trace = InMemoryTraceService::new();
    let summary = run(records(), &source(), &mut output, &mut trace).unwrap();
    assert_eq!(summary.applied, 2);
    assert_eq!(output.calls, 1);
    assert_eq!(output.available.as_deref(), Some("4.0000"));
    let finished = &trace.events().last().unwrap().event;
    assert_eq!(finished.attributes["status"], "completed");
    assert!(!finished.attributes.contains_key("line"));
}
#[test]
fn preserves_the_output_error_and_finalizes_a_failed_run() {
    let mut output = Snapshot {
        fail: true,
        ..Snapshot::default()
    };
    let mut trace = InMemoryTraceService::new();
    let error = run(records(), &source(), &mut output, &mut trace).unwrap_err();
    assert!(matches!(*error.failure, Failure::Output(_)));
    assert_eq!(error.summary.applied, 2);
    assert_eq!(
        error.source().unwrap().source().unwrap().to_string(),
        "snapshot refused"
    );
    assert_eq!(
        trace.events().last().unwrap().event.attributes["status"],
        "failed"
    );
}
struct Trace {
    events: Vec<Event>,
    fail_event: Option<&'static str>,
    fail_flush: bool,
}
impl TraceService for Trace {
    fn emit(&mut self, event: Event) -> Result<(), TraceError> {
        let failed = self.fail_event == Some(event.event_name.as_str());
        self.events.push(event);
        if failed {
            Err(TraceError::new(IoError::other("delivery failed")))
        } else {
            Ok(())
        }
    }
    fn flush(&mut self) -> Result<(), TraceError> {
        if self.fail_flush {
            Err(TraceError::new(IoError::other("flush failed")))
        } else {
            Ok(())
        }
    }
}
#[rstest]
#[case(Some("payment.request_applied"), false, 1, 0)]
#[case(None, true, 2, 0)]
#[case(Some("payment.run_finished"), false, 2, 1)]
fn trace_failures_preserve_counts_and_publication_order(
    #[case] fail_event: Option<&'static str>,
    #[case] fail_flush: bool,
    #[case] applied: u64,
    #[case] publications: usize,
) {
    let mut output = Snapshot::default();
    let mut trace = Trace {
        events: Vec::new(),
        fail_event,
        fail_flush,
    };
    let error = run(records(), &source(), &mut output, &mut trace).unwrap_err();
    assert_eq!(error.summary.applied, applied);
    assert_eq!(output.calls, publications);
    assert_eq!(
        trace
            .events
            .iter()
            .filter(|event| event.event_name == "payment.request_applied")
            .count(),
        applied as usize
    );
    assert_eq!(
        trace.events.last().unwrap().event_name,
        "payment.run_finished"
    );
}
