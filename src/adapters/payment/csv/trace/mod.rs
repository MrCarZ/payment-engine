//! CSV-specific input failure mapping; raw rows and parser messages are omitted.

use serde_json::Value;

use crate::{
    domain::{
        observability::{Event, Severity},
        payment::{AmountError, MoneyError},
    },
    manager::payment::{
        ProcessingError, Report,
        trace::{
            processing_failed as payment_processing_failed,
            request_processed as payment_request_processed, source_attributes,
        },
    },
};

use super::RecordPosition;
use super::input::{FieldError, InputError, Record, Type};

/// Adds CSV provenance to the transport-independent payment outcome mapping.
pub fn request_processed(record: &Record, report: Report) -> Event {
    let mut event = payment_request_processed(&record.context, record.request, report);
    add_position(&mut event, record.position);
    event
}

pub fn processing_failed(record: &Record, error: ProcessingError) -> Event {
    let mut event = payment_processing_failed(&record.context, record.request, error);
    add_position(&mut event, record.position);
    event
}

fn add_position(event: &mut Event, position: RecordPosition) {
    event
        .attributes
        .insert("record".into(), Value::from(position.record));
    event
        .attributes
        .insert("line".into(), Value::from(position.line));
    event
        .attributes
        .insert("byte".into(), Value::from(position.byte));
}

pub fn input_failed(error: &InputError) -> Event {
    let mut attributes = source_attributes(&error.context.source);
    attributes.insert("record".into(), Value::from(error.position.record));
    attributes.insert("line".into(), Value::from(error.position.line));
    attributes.insert("byte".into(), Value::from(error.position.byte));
    let code = match &error.error_type {
        Type::InvalidHeaders => "invalid_headers",
        Type::InvalidRecordLength => "invalid_record_length",
        Type::InvalidField { field, error } => {
            attributes.insert("field".into(), Value::from(*field));
            match error {
                FieldError::Identifier(_) => "invalid_identifier",
                FieldError::MissingAmount => "missing_amount",
                FieldError::UnknownType => "unknown_type",
                FieldError::Amount(AmountError::NonPositive) => "non_positive_amount",
                FieldError::Amount(AmountError::InvalidMoney(error)) => match error {
                    MoneyError::InvalidFormat => "invalid_amount_format",
                    MoneyError::ExcessPrecision => "excess_amount_precision",
                    MoneyError::Overflow => "amount_overflow",
                },
            }
        }
        Type::Csv(_) => "csv_read_error",
    };
    attributes.insert("reason_code".into(), Value::from(code));
    Event {
        severity: Severity::Error,
        component: "payment".into(),
        event_name: "payment.input_failed".into(),
        correlation_id: Some(error.context.source.run_id.clone()),
        message: "Payment CSV input failed".into(),
        attributes,
    }
}

#[cfg(test)]
mod tests;
