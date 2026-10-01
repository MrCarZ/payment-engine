use std::{
    io::{Error as IoError, Write},
    sync::Arc,
};

use rstest::rstest;
use serde_json::Value;

use crate::{
    adapters::observability::{csv::CsvTraceService, memory::InMemoryTraceService},
    domain::{
        observability::Severity,
        payment::{ClientId, MoneyError, PositiveAmount, TransactionId},
    },
    manager::{observability::TraceService, payment::PaymentManager},
};

use super::{
    Context, LifecycleAction, Outcome, ProcessingError, Reason, Report, Request, SourceContext,
    State, Summary, Type, processing_failed, request_processed, run_finished, source_attributes,
};

fn context() -> Context {
    Context {
        source: Arc::new(SourceContext {
            run_id: "run-1".into(),
            source_id: "source-2".into(),
            partner_id: Some("partner-3".into()),
        }),
    }
}

fn deposit() -> Request {
    Request::Original {
        client: ClientId::from(7),
        tx: TransactionId::from(42),
        transaction_type: Type::Deposit,
        amount: "1.0000".parse::<PositiveAmount>().unwrap(),
    }
}

#[rstest]
#[case(Outcome::Applied, "payment.request_applied", None)]
#[case(
    Outcome::Ignored(Reason::UnknownTransaction),
    "payment.request_ignored",
    Some("unknown_transaction")
)]
#[case(
    Outcome::Ignored(Reason::TransactionNotAccepted),
    "payment.request_ignored",
    Some("transaction_not_accepted")
)]
#[case(
    Outcome::Rejected(Reason::ClientMismatch),
    "payment.request_rejected",
    Some("client_mismatch")
)]
#[case(
    Outcome::Rejected(Reason::NotDisputable),
    "payment.request_rejected",
    Some("not_disputable")
)]
#[case(
    Outcome::Ignored(Reason::AlreadyDisputed),
    "payment.request_ignored",
    Some("already_disputed")
)]
#[case(
    Outcome::Ignored(Reason::NotDisputed),
    "payment.request_ignored",
    Some("not_disputed")
)]
#[case(
    Outcome::Ignored(Reason::AlreadyChargedBack),
    "payment.request_ignored",
    Some("already_charged_back")
)]
#[case(
    Outcome::Rejected(Reason::AccountLocked),
    "payment.request_rejected",
    Some("account_locked")
)]
#[case(
    Outcome::Rejected(Reason::InsufficientAvailableFunds),
    "payment.request_rejected",
    Some("insufficient_available_funds")
)]
#[case(
    Outcome::Rejected(Reason::ConflictingTransactionId),
    "payment.request_rejected",
    Some("conflicting_transaction_id")
)]
fn outcomes_have_stable_codes(
    #[case] outcome: Outcome,
    #[case] name: &str,
    #[case] reason: Option<&str>,
) {
    let event = request_processed(
        &context(),
        deposit(),
        Report {
            outcome,
            replayed: false,
        },
    );
    assert_eq!(event.event_name, name);
    assert_eq!(
        event.attributes.get("reason_code").and_then(Value::as_str),
        reason
    );
    assert_eq!(
        event.severity,
        if outcome == Outcome::Applied {
            Severity::Info
        } else {
            Severity::Warn
        }
    );
    assert_eq!(event.attributes["replayed"], false);
}

#[rstest]
#[case(Type::Deposit, "deposit")]
#[case(Type::Withdrawal, "withdrawal")]
fn original_types_are_classified(#[case] transaction_type: Type, #[case] expected: &str) {
    let Request::Original {
        client, tx, amount, ..
    } = deposit()
    else {
        unreachable!()
    };
    let event = request_processed(
        &context(),
        Request::Original {
            client,
            tx,
            amount,
            transaction_type,
        },
        Report {
            outcome: Outcome::Applied,
            replayed: false,
        },
    );
    assert_eq!(event.attributes["request_type"], expected);
}

#[rstest]
#[case(LifecycleAction::Dispute, "dispute")]
#[case(LifecycleAction::Resolve, "resolve")]
#[case(LifecycleAction::Chargeback, "chargeback")]
fn lifecycle_types_are_classified(#[case] action: LifecycleAction, #[case] expected: &str) {
    let event = request_processed(
        &context(),
        Request::Lifecycle {
            client: ClientId::from(7),
            tx: TransactionId::from(42),
            action,
        },
        Report {
            outcome: Outcome::Applied,
            replayed: false,
        },
    );
    assert_eq!(event.attributes["request_type"], expected);
}

#[rstest]
#[case(Type::Deposit, "applied")]
#[case(Type::Withdrawal, "rejected")]
fn actual_replays_are_distinct_and_deliverable(
    #[case] transaction_type: Type,
    #[case] outcome: &str,
) {
    let mut manager = PaymentManager::new();
    let Request::Original {
        client, tx, amount, ..
    } = deposit()
    else {
        unreachable!()
    };
    let request = Request::Original {
        client,
        tx,
        amount,
        transaction_type,
    };
    let initial = manager.process(request).unwrap();
    let replay = manager.process(request).unwrap();
    let mut trace = InMemoryTraceService::new();
    trace
        .emit(request_processed(&context(), request, initial))
        .unwrap();
    trace
        .emit(request_processed(&context(), request, replay))
        .unwrap();
    let event = &trace.events()[1].event;
    assert_eq!(event.event_name, "payment.request_replayed");
    assert_eq!(event.attributes["outcome"], outcome);
    assert_eq!(event.attributes["replayed"], true);
    assert_eq!(event.attributes["client_id"], 7);
    assert_eq!(event.attributes["transaction_id"], 42);
    assert_eq!(event.attributes["source_id"], "source-2");
    assert_eq!(event.attributes["partner_id"], "partner-3");
    assert_eq!(event.correlation_id.as_deref(), Some("run-1"));
    assert!(!event.attributes.contains_key("amount"));
    let expected = if transaction_type == Type::Deposit {
        "1.0000"
    } else {
        "0.0000"
    };
    assert_eq!(
        manager.account(client).unwrap().available().to_string(),
        expected
    );
}

#[rstest]
#[case(ProcessingError::Arithmetic(MoneyError::Overflow), "arithmetic_error")]
#[case(ProcessingError::InconsistentState, "inconsistent_state")]
fn fatal_failures_are_errors(#[case] error: ProcessingError, #[case] expected: &str) {
    let event = processing_failed(&context(), deposit(), error);
    assert_eq!(event.event_name, "payment.processing_failed");
    assert_eq!(event.severity, Severity::Error);
    assert_eq!(event.attributes["reason_code"], expected);
}

#[rstest]
#[case(State::Completed, "completed", Severity::Info)]
#[case(State::Failed, "failed", Severity::Error)]
fn summaries_preserve_all_counts(
    #[case] state: State,
    #[case] status: &str,
    #[case] severity: Severity,
) {
    let summary = Summary {
        applied: 1,
        ignored: 2,
        rejected: 3,
        replayed: 4,
        input_errors: 5,
        processing_errors: 6,
    };
    let event = run_finished(&context().source, summary, state);
    assert_eq!(event.event_name, "payment.run_finished");
    assert_eq!(event.severity, severity);
    assert_eq!(event.attributes["status"], status);
    for (key, expected) in [
        ("applied", 1),
        ("ignored", 2),
        ("rejected", 3),
        ("replayed", 4),
        ("input_errors", 5),
        ("processing_errors", 6),
    ] {
        assert_eq!(event.attributes[key], expected);
    }
}

#[test]
fn absent_partner_is_omitted() {
    let mut source = (*context().source).clone();
    source.partner_id = None;
    assert!(!source_attributes(&source).contains_key("partner_id"));
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> Result<usize, IoError> {
        Err(IoError::other("delivery failed"))
    }
    fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}

#[test]
fn delivery_failure_does_not_undo_payment() {
    let mut manager = PaymentManager::new();
    let request = deposit();
    let report = manager.process(request).unwrap();
    let mut trace = CsvTraceService::new(FailingWriter).unwrap();
    trace
        .emit(request_processed(&context(), request, report))
        .unwrap();
    assert!(trace.flush().is_err());
    assert_eq!(
        manager
            .account(ClientId::from(7))
            .unwrap()
            .available()
            .to_string(),
        "1.0000"
    );
    assert!(manager.process(request).unwrap().is_replay());
}
