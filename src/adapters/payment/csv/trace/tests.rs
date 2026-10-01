use std::io::{Error as IoError, Read};

use rstest::rstest;

use crate::{
    domain::observability::Severity,
    manager::payment::{PaymentManager, ProcessingError, SourceContext},
};

use super::{super::input::Input, input_failed, processing_failed, request_processed};

fn source() -> SourceContext {
    SourceContext {
        run_id: "run-1".into(),
        source_id: "csv-1".into(),
        partner_id: None,
    }
}

#[rstest]
#[case("type,client,tx\n", "invalid_headers", None)]
#[case(
    "type,client,tx,amount\ndeposit,1,1,1,extra\n",
    "invalid_record_length",
    None
)]
#[case(
    "type,client,tx,amount\ndeposit,secret,1,1\n",
    "invalid_identifier",
    Some("client")
)]
#[case(
    "type,client,tx,amount\ndeposit,1,secret,1\n",
    "invalid_identifier",
    Some("tx")
)]
#[case(
    "type,client,tx,amount\ndeposit,1,1,\n",
    "missing_amount",
    Some("amount")
)]
#[case("type,client,tx,amount\nsecret,1,1,1\n", "unknown_type", Some("type"))]
#[case(
    "type,client,tx,amount\ndeposit,1,1,-1\n",
    "non_positive_amount",
    Some("amount")
)]
#[case(
    "type,client,tx,amount\ndeposit,1,1,secret\n",
    "invalid_amount_format",
    Some("amount")
)]
#[case(
    "type,client,tx,amount\ndeposit,1,1,1.12345\n",
    "excess_amount_precision",
    Some("amount")
)]
#[case(
    "type,client,tx,amount\ndeposit,1,1,999999999999999999999999999999999999999999999\n",
    "amount_overflow",
    Some("amount")
)]
fn malformed_input_has_safe_classification(
    #[case] csv: &str,
    #[case] code: &str,
    #[case] field: Option<&str>,
    #[values("\n", "\r\n")] newline: &str,
) {
    let csv = csv.replace('\n', newline);
    let error = match Input::new(csv.as_bytes(), source()) {
        Err(error) => error,
        Ok(mut input) => input.next().unwrap().unwrap_err(),
    };
    let event = input_failed(&error);
    assert_eq!(event.event_name, "payment.input_failed");
    assert_eq!(event.severity, Severity::Error);
    assert_eq!(event.attributes["reason_code"], code);
    assert_eq!(
        event
            .attributes
            .get("field")
            .and_then(|value| value.as_str()),
        field
    );
    assert_eq!(event.attributes["record"], error.position.record);
    assert_eq!(event.attributes["line"], error.position.line);
    assert_eq!(event.attributes["byte"], error.position.byte);
    let header_error = code == "invalid_headers";
    assert_eq!(event.attributes["record"], if header_error { 0 } else { 1 });
    assert_eq!(event.attributes["line"], if header_error { 1 } else { 2 });
    let byte = if header_error {
        0
    } else {
        csv.find('\n').unwrap() + 1
    };
    assert_eq!(event.attributes["byte"], byte);
    assert_eq!(event.attributes["source_id"], "csv-1");
    assert_eq!(event.correlation_id.as_deref(), Some("run-1"));
    assert!(!format!("{event:?}").contains("secret"));
}

struct FailingReader;

#[rstest]
fn csv_outcomes_and_processing_failures_preserve_record_provenance(
    #[values("\n", "\r\n")] newline: &str,
) {
    let header = format!("type,client,tx,amount{newline}");
    let csv = format!("{header}deposit,1,42,5{newline}");
    let record = Input::new(csv.as_bytes(), source())
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let report = PaymentManager::new().process(record.request).unwrap();
    let applied = request_processed(&record, report);
    let failed = processing_failed(&record, ProcessingError::InconsistentState);
    assert_eq!(applied.event_name, "payment.request_applied");
    assert_eq!(failed.event_name, "payment.processing_failed");
    for event in [applied, failed] {
        assert_eq!(event.attributes["record"], 1);
        assert_eq!(event.attributes["line"], 2);
        assert_eq!(event.attributes["byte"], header.len());
        assert_eq!(event.attributes["transaction_id"], 42);
    }
}

impl Read for FailingReader {
    fn read(&mut self, _: &mut [u8]) -> Result<usize, IoError> {
        Err(IoError::other("sensitive infrastructure details"))
    }
}

#[test]
fn read_error_is_classified_without_exposing_infrastructure_details() {
    let error = match Input::new(FailingReader, source()) {
        Err(error) => error,
        Ok(_) => panic!("reader must fail"),
    };
    let event = input_failed(&error);
    assert_eq!(event.attributes["reason_code"], "csv_read_error");
    assert!(!format!("{event:?}").contains("sensitive"));
}
