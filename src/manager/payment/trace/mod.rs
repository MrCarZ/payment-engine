//! Pure payment-to-event mapping. Delivery and run accounting belong to callers.

use serde_json::{Map, Value};

use crate::domain::{
    observability::{Event, Severity},
    payment::{LifecycleAction, transaction::Type},
};

use super::{Context, Outcome, ProcessingError, Reason, Report, Request, SourceContext};

/// Counts supplied by the run coordinator. Applied/ignored/rejected exclude
/// replays; replayed counts all original retries regardless of stored outcome.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub applied: u64,
    pub ignored: u64,
    pub rejected: u64,
    pub replayed: u64,
    pub input_errors: u64,
    pub processing_errors: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Completed,
    Failed,
}

/// Common source attributes, also usable by transport-specific event mappings.
pub fn source_attributes(source: &SourceContext) -> Map<String, Value> {
    let mut attributes = Map::from_iter([
        ("run_id".into(), Value::from(source.run_id.clone())),
        ("source_id".into(), Value::from(source.source_id.clone())),
    ]);
    if let Some(partner) = &source.partner_id {
        attributes.insert("partner_id".into(), Value::from(partner.clone()));
    }
    attributes
}

/// A replay has its own event name so an earlier Applied result cannot be
/// mistaken for another balance movement.
pub fn request_processed(context: &Context, request: Request, report: Report) -> Event {
    let (outcome, reason, severity) = match report.outcome() {
        Outcome::Applied => ("applied", None, Severity::Info),
        Outcome::Ignored(reason) => ("ignored", Some(reason), Severity::Warn),
        Outcome::Rejected(reason) => ("rejected", Some(reason), Severity::Warn),
    };
    let mut attributes = request_attributes(context, request);
    attributes.insert("outcome".into(), Value::from(outcome));
    attributes.insert("replayed".into(), Value::from(report.is_replay()));
    if let Some(reason) = reason {
        attributes.insert("reason_code".into(), Value::from(reason_code(reason)));
    }
    let (name, message) = if report.is_replay() {
        ("payment.request_replayed", "Original request replayed")
    } else {
        match report.outcome() {
            Outcome::Applied => ("payment.request_applied", "Payment request applied"),
            Outcome::Ignored(_) => ("payment.request_ignored", "Payment request ignored"),
            Outcome::Rejected(_) => ("payment.request_rejected", "Payment request rejected"),
        }
    };
    event(&context.source, name, message, severity, attributes)
}

pub fn processing_failed(context: &Context, request: Request, error: ProcessingError) -> Event {
    let mut attributes = request_attributes(context, request);
    let code = match error {
        ProcessingError::Arithmetic(_) => "arithmetic_error",
        ProcessingError::InconsistentState => "inconsistent_state",
    };
    attributes.insert("reason_code".into(), Value::from(code));
    event(
        &context.source,
        "payment.processing_failed",
        "Payment processing failed",
        Severity::Error,
        attributes,
    )
}

pub fn run_finished(source: &SourceContext, summary: Summary, state: State) -> Event {
    let mut attributes = source_attributes(source);
    for (name, count) in [
        ("applied", summary.applied),
        ("ignored", summary.ignored),
        ("rejected", summary.rejected),
        ("replayed", summary.replayed),
        ("input_errors", summary.input_errors),
        ("processing_errors", summary.processing_errors),
    ] {
        attributes.insert(name.into(), Value::from(count));
    }
    let (status, severity) = match state {
        State::Completed => ("completed", Severity::Info),
        State::Failed => ("failed", Severity::Error),
    };
    attributes.insert("status".into(), Value::from(status));
    event(
        source,
        "payment.run_finished",
        "Payment run finished",
        severity,
        attributes,
    )
}

fn request_attributes(context: &Context, request: Request) -> Map<String, Value> {
    let (client, tx, request_type) = match request {
        Request::Original {
            client,
            tx,
            transaction_type,
            ..
        } => {
            let name = match transaction_type {
                Type::Deposit => "deposit",
                Type::Withdrawal => "withdrawal",
            };
            (client, tx, name)
        }
        Request::Lifecycle { client, tx, action } => {
            let name = match action {
                LifecycleAction::Dispute => "dispute",
                LifecycleAction::Resolve => "resolve",
                LifecycleAction::Chargeback => "chargeback",
            };
            (client, tx, name)
        }
    };
    let mut attributes = source_attributes(&context.source);
    attributes.insert("client_id".into(), Value::from(client.get()));
    attributes.insert("transaction_id".into(), Value::from(tx.get()));
    attributes.insert("request_type".into(), Value::from(request_type));
    attributes
}

fn reason_code(reason: Reason) -> &'static str {
    match reason {
        Reason::UnknownTransaction => "unknown_transaction",
        Reason::TransactionNotAccepted => "transaction_not_accepted",
        Reason::ClientMismatch => "client_mismatch",
        Reason::NotDisputable => "not_disputable",
        Reason::AlreadyDisputed => "already_disputed",
        Reason::NotDisputed => "not_disputed",
        Reason::AlreadyChargedBack => "already_charged_back",
        Reason::AccountLocked => "account_locked",
        Reason::InsufficientAvailableFunds => "insufficient_available_funds",
        Reason::ConflictingTransactionId => "conflicting_transaction_id",
    }
}

fn event(
    source: &SourceContext,
    name: &str,
    message: &str,
    severity: Severity,
    attributes: Map<String, Value>,
) -> Event {
    Event {
        severity,
        component: "payment".into(),
        event_name: name.into(),
        correlation_id: Some(source.run_id.clone()),
        message: message.into(),
        attributes,
    }
}

#[cfg(test)]
mod tests;
