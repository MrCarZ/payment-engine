use std::{
    cell::Cell,
    io::{Cursor, Error as IoError, Read, Result as IoResult},
    rc::Rc,
    sync::Arc,
};

use rstest::rstest;

use super::{Input, InputError, Type as ErrorType};
use crate::{
    adapters::payment::csv::RecordPosition,
    domain::payment::{ClientId, LifecycleAction, TransactionId, transaction::Type},
    manager::payment::{Outcome, PaymentManager, Request, SourceContext},
};

fn source() -> SourceContext {
    SourceContext {
        run_id: "run-1".into(),
        source_id: "source-1".into(),
        partner_id: Some("partner-1".into()),
    }
}

struct ChunkedReader {
    input: Cursor<Vec<u8>>,
    chunk_size: usize,
}

impl Read for ChunkedReader {
    fn read(&mut self, buffer: &mut [u8]) -> IoResult<usize> {
        let length = buffer.len().min(self.chunk_size);
        self.input.read(&mut buffer[..length])
    }
}

#[rstest]
fn record_positions_preserve_source_bytes_and_physical_lines(
    #[values("\n", "\r\n")] newline: &str,
    #[values(1, 8192)] chunk_size: usize,
) {
    let header = format!("type,client,tx,amount{newline}");
    let first = format!("deposit,1,1,2{newline}");
    let second = format!("dispute,1,1,\"ignored{newline}amount\"{newline}");
    let third = format!("resolve,1,1,{newline}");
    let data = format!("{header}{first}{second}{third}");
    let reader = ChunkedReader {
        input: Cursor::new(data.into_bytes()),
        chunk_size,
    };
    let input = Input::new(reader, source()).unwrap();
    let positions: Vec<_> = input.map(|row| row.unwrap().position).collect();
    assert_eq!(
        positions,
        vec![
            RecordPosition {
                record: 1,
                line: 2,
                byte: header.len() as u64
            },
            RecordPosition {
                record: 2,
                line: 3,
                byte: (header.len() + first.len()) as u64
            },
            RecordPosition {
                record: 3,
                line: 5,
                byte: (header.len() + first.len() + second.len()) as u64
            },
        ]
    );
}

#[rstest]
#[case::invalid_field(b"deposit,1,2,invalid", false)]
#[case::invalid_utf8(b"deposit,1,2,\xff", true)]
fn input_errors_preserve_positions_and_remain_terminal(
    #[case] invalid: &[u8],
    #[case] csv_error: bool,
    #[values("\n", "\r\n")] newline: &str,
    #[values(1, 8192)] chunk_size: usize,
) {
    let prefix = format!("type,client,tx,amount{newline}deposit,1,1,2{newline}");
    let mut bytes = prefix.as_bytes().to_vec();
    bytes.extend_from_slice(invalid);
    bytes.extend_from_slice(format!("{newline}deposit,1,3,10{newline}").as_bytes());
    let reader = ChunkedReader {
        input: Cursor::new(bytes),
        chunk_size,
    };
    let mut input = Input::new(reader, source()).unwrap();
    assert!(input.next().unwrap().is_ok());
    let error = input.next().unwrap().unwrap_err();
    assert_eq!(
        error.position,
        RecordPosition {
            record: 2,
            line: 3,
            byte: prefix.len() as u64
        }
    );
    assert_eq!(*error.context.source, source());
    if csv_error {
        assert!(matches!(error.error_type, ErrorType::Csv(_)));
    } else {
        assert!(matches!(
            error.error_type,
            ErrorType::InvalidField {
                field: "amount",
                ..
            }
        ));
    }
    assert!(input.next().is_none());
    assert!(input.next().is_none());
}

fn first(input: &str) -> Result<Request, InputError> {
    let mut input = Input::new(input.as_bytes(), source())?;
    input
        .next()
        .expect("input contains a row")
        .map(|record| record.request)
}

#[rstest]
#[case::deposit("deposit", Type::Deposit)]
#[case::withdrawal("withdrawal", Type::Withdrawal)]
fn original_rows_accept_whitespace_and_reordered_headers(
    #[case] event: &str,
    #[case] transaction_type: Type,
) {
    let input = format!(" amount , tx , type , client \n 1.2345 , 4294967295 , {event} , 65535 \n");
    assert_eq!(
        first(&input).unwrap(),
        Request::Original {
            client: ClientId::from(u16::MAX),
            tx: TransactionId::from(u32::MAX),
            transaction_type,
            amount: "1.2345".parse().unwrap(),
        }
    );
}

#[rstest]
#[case::integer("1", 10_000)]
#[case::four_places("1.2345", 12_345)]
#[case::smallest_unit("0.0001", 1)]
fn amounts_preserve_exact_scaled_units(#[case] amount: &str, #[case] units: i128) {
    let input = format!("type,client,tx,amount\ndeposit,1,1,{amount}\n");
    let Request::Original { amount: parsed, .. } = first(&input).unwrap() else {
        panic!("expected original");
    };
    assert_eq!(parsed.money().scaled_units(), units);
}

#[rstest]
#[case::dispute("dispute", LifecycleAction::Dispute)]
#[case::resolve("resolve", LifecycleAction::Resolve)]
#[case::chargeback("chargeback", LifecycleAction::Chargeback)]
fn lifecycle_rows_accept_empty_omitted_and_ignored_amounts(
    #[case] event: &str,
    #[case] action: LifecycleAction,
    #[values("", ",", ",this is ignored")] suffix: &str,
) {
    let input = format!("type,client,tx,amount\n{event},1,42{suffix}\n");
    assert_eq!(
        first(&input).unwrap(),
        Request::Lifecycle {
            client: ClientId::from(1),
            tx: TransactionId::from(42),
            action,
        }
    );
}

#[rstest]
#[case::empty("")]
#[case::missing("type,client,tx\n")]
#[case::duplicate("type,client,tx,tx\n")]
#[case::extra("type,client,tx,amount,other\n")]
#[case::wrong_case("Type,client,tx,amount\n")]
fn invalid_headers_are_rejected(#[case] input: &str) {
    let error = match Input::new(input.as_bytes(), source()) {
        Ok(_) => panic!("expected header error"),
        Err(error) => error,
    };
    assert!(matches!(error.error_type, ErrorType::InvalidHeaders));
    assert_eq!(error.position.record, 0);
}

#[rstest]
#[case::missing_amount("deposit,1,1,", "amount")]
#[case::omitted_amount("withdrawal,1,1", "amount")]
#[case::zero_amount("deposit,1,1,0", "amount")]
#[case::negative_amount("deposit,1,1,-1", "amount")]
#[case::excess_precision("deposit,1,1,1.00001", "amount")]
#[case::invalid_amount("deposit,1,1,invalid", "amount")]
#[case::client_overflow("deposit,65536,1,1", "client")]
#[case::negative_client("deposit,-1,1,1", "client")]
#[case::missing_client("deposit,,1,1", "client")]
#[case::tx_overflow("deposit,1,4294967296,1", "tx")]
#[case::negative_tx("deposit,1,-1,1", "tx")]
#[case::missing_tx("deposit,1,,1", "tx")]
#[case::unknown_type("transfer,1,1,1", "type")]
fn invalid_fields_identify_the_field_and_record(#[case] row: &str, #[case] field: &str) {
    let error = first(&format!("type,client,tx,amount\n{row}\n")).unwrap_err();
    assert!(
        matches!(error.error_type, ErrorType::InvalidField { field: actual, .. } if actual == field)
    );
    assert_eq!(error.position.record, 1);
    assert_eq!(error.position.line, 2);
    assert!(
        error
            .to_string()
            .contains("source source-1 at record 1, line 2")
    );
}

#[rstest]
#[case::too_short("dispute,1")]
#[case::too_long("deposit,1,1,1,extra")]
fn malformed_record_lengths_are_rejected(#[case] row: &str) {
    let error = first(&format!("type,client,tx,amount\n{row}\n")).unwrap_err();
    assert!(matches!(error.error_type, ErrorType::InvalidRecordLength));
}

#[test]
fn first_error_is_terminal_and_later_rows_are_not_processed() {
    let bytes = b"type,client,tx,amount\ndeposit,1,1,2\ndeposit,1,2,invalid\ndeposit,1,3,10\n";
    let mut input = Input::new(bytes.as_slice(), source()).unwrap();
    let mut manager = PaymentManager::new();
    let first = input.next().unwrap().unwrap();
    manager.process(first.request).unwrap();
    let error = input.next().unwrap().unwrap_err();
    assert_eq!(error.position.record, 2);
    assert_eq!(error.position.line, 3);
    assert!(input.next().is_none());
    assert!(input.next().is_none());
    assert_eq!(
        manager
            .account(ClientId::from(1))
            .unwrap()
            .available()
            .to_string(),
        "2.0000"
    );
    assert!(manager.original(TransactionId::from(2)).is_none());
    assert!(manager.original(TransactionId::from(3)).is_none());
}

#[test]
fn fixture_preserves_order_context_and_integrates_with_manager() {
    let fixture = include_str!("../../../../../tests/fixtures/input/payments.csv");
    let input = Input::new(fixture.as_bytes(), source()).unwrap();
    let mut manager = PaymentManager::new();
    let mut previous_source = None;
    for (index, record) in input.enumerate() {
        let record = record.unwrap();
        assert_eq!(record.position.record, index as u64 + 1);
        assert_eq!(record.position.line, index as u64 + 2);
        assert_eq!(*record.context.source, source());
        if let Some(previous) = previous_source {
            assert!(Arc::ptr_eq(&previous, &record.context.source));
        }
        previous_source = Some(Arc::clone(&record.context.source));
        let report = manager.process(record.request).unwrap();
        assert_eq!(report.outcome(), Outcome::Applied);
    }
    assert_eq!(
        manager
            .account(ClientId::from(9))
            .unwrap()
            .available()
            .to_string(),
        "4.0000"
    );
    assert_eq!(
        manager
            .account(ClientId::from(3))
            .unwrap()
            .available()
            .to_string(),
        "4.0000"
    );
}

#[test]
fn header_only_input_is_empty() {
    let mut input = Input::new(b"type,client,tx,amount\n".as_slice(), source()).unwrap();
    assert!(input.next().is_none());
    assert!(input.next().is_none());
}

struct TrackingReader {
    input: Cursor<Vec<u8>>,
    bytes_read: Rc<Cell<usize>>,
}

impl Read for TrackingReader {
    fn read(&mut self, buffer: &mut [u8]) -> IoResult<usize> {
        let count = self.input.read(buffer)?;
        self.bytes_read.set(self.bytes_read.get() + count);
        Ok(count)
    }
}

#[test]
fn reading_first_row_does_not_load_entire_source() {
    let data = format!(
        "type,client,tx,amount\n{}",
        "deposit,1,1,1\n".repeat(100_000)
    )
    .into_bytes();
    let length = data.len();
    let count = Rc::new(Cell::new(0));
    let reader = TrackingReader {
        input: Cursor::new(data),
        bytes_read: Rc::clone(&count),
    };
    let mut input = Input::new(reader, source()).unwrap();
    assert!(input.next().unwrap().is_ok());
    assert!(count.get() < length);
}

struct FailingReader {
    prefix: Cursor<&'static [u8]>,
}

impl Read for FailingReader {
    fn read(&mut self, buffer: &mut [u8]) -> IoResult<usize> {
        let count = self.prefix.read(buffer)?;
        if count == 0 {
            Err(IoError::other("simulated read failure"))
        } else {
            Ok(count)
        }
    }
}

#[test]
fn io_failure_is_reported_once_after_previous_complete_records() {
    let reader = FailingReader {
        prefix: Cursor::new(b"type,client,tx,amount\ndeposit,1,1,1\n"),
    };
    let mut input = Input::new(reader, source()).unwrap();
    assert!(input.next().unwrap().is_ok());
    assert!(matches!(
        input.next().unwrap().unwrap_err().error_type,
        ErrorType::Csv(_)
    ));
    assert!(input.next().is_none());
}
