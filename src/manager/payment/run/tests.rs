use std::{convert::Infallible, io::Error as IoError, sync::Arc};

use crate::{
    adapters::observability::memory::InMemoryTraceService,
    domain::{
        observability::Event,
        payment::{ClientId, TransactionId, transaction::Type},
    },
    manager::{
        observability::{TraceError, TraceService},
        payment::SourceContext,
    },
};

use super::{Context, Coordinator, Failure, Record, Request};

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

fn envelope(tx: u32, transaction_type: Type, amount: &str) -> Result<Envelope, Infallible> {
    Ok(Envelope {
        request: Request::Original {
            client: ClientId::from(1),
            tx: TransactionId::from(tx),
            transaction_type,
            amount: amount.parse().unwrap(),
        },
        context: Context {
            source: Arc::new(SourceContext {
                run_id: "api-run".into(),
                source_id: "api-source".into(),
                partner_id: None,
            }),
        },
    })
}

#[test]
fn coordinates_non_csv_envelopes_and_classifies_rejected_replays() {
    let records = [
        envelope(1, Type::Withdrawal, "5"),
        envelope(1, Type::Withdrawal, "5"),
        envelope(2, Type::Deposit, "5"),
    ];
    let mut coordinator = Coordinator::new();
    let mut trace = InMemoryTraceService::new();
    coordinator.process(records, &mut trace).unwrap();
    assert_eq!(coordinator.summary().rejected, 1);
    assert_eq!(coordinator.summary().replayed, 1);
    assert_eq!(coordinator.summary().applied, 1);
    assert_eq!(
        coordinator
            .manager()
            .account(ClientId::from(1))
            .unwrap()
            .available()
            .to_string(),
        "5.0000"
    );
    assert_eq!(
        trace.events()[1].event.event_name,
        "payment.request_replayed"
    );
    assert_eq!(trace.events()[1].event.attributes["outcome"], "rejected");
    assert_eq!(
        trace.events()[0].event.attributes["source_id"],
        "api-source"
    );
    assert!(!trace.events()[0].event.attributes.contains_key("line"));
}

#[test]
fn fatal_failure_retains_transport_envelope_and_previous_state() {
    let records = [
        envelope(1, Type::Deposit, "17014118346046923173168730371588410.5727"),
        envelope(2, Type::Deposit, "1"),
        envelope(3, Type::Deposit, "1"),
    ];
    let mut coordinator = Coordinator::new();
    let mut trace = InMemoryTraceService::new();
    let failure = coordinator.process(records, &mut trace).unwrap_err();
    let Failure::Processing {
        record,
        trace_error,
        ..
    } = failure
    else {
        panic!("expected processing failure")
    };
    assert_eq!(record.context.source.source_id, "api-source");
    assert!(trace_error.is_none());
    assert_eq!(coordinator.summary().applied, 1);
    assert_eq!(coordinator.summary().processing_errors, 1);
    assert!(
        coordinator
            .manager()
            .original(TransactionId::from(2))
            .is_none()
    );
    assert!(
        coordinator
            .manager()
            .original(TransactionId::from(3))
            .is_none()
    );
}

struct FailingTrace;

impl TraceService for FailingTrace {
    fn emit(&mut self, _: Event) -> Result<(), TraceError> {
        Err(TraceError::new(IoError::other("delivery failed")))
    }
    fn flush(&mut self) -> Result<(), TraceError> {
        Ok(())
    }
}

#[test]
fn delivery_failure_preserves_applied_state_and_stops_the_sequence() {
    let records = [
        envelope(1, Type::Deposit, "5"),
        envelope(2, Type::Deposit, "5"),
    ];
    let mut coordinator = Coordinator::new();
    assert!(matches!(
        coordinator.process(records, &mut FailingTrace),
        Err(Failure::Trace(_))
    ));
    assert_eq!(coordinator.summary().applied, 1);
    assert_eq!(
        coordinator
            .manager()
            .account(ClientId::from(1))
            .unwrap()
            .available()
            .to_string(),
        "5.0000"
    );
    assert!(
        coordinator
            .manager()
            .original(TransactionId::from(2))
            .is_none()
    );
}
