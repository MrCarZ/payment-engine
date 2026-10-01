use std::{
    error::Error,
    io::{Error as IoError, Result as IoResult, Write},
};

use rstest::rstest;

use super::{OutputError, write};
use crate::{
    adapters::payment::csv::input::Input,
    domain::payment::{Account, ClientId, Money, PositiveAmount},
    manager::payment::{PaymentManager, SourceContext},
};

const HEADER: &str = "client,available,held,total,locked\n";

fn amount(input: &str) -> PositiveAmount {
    input.parse().expect("positive amount")
}

#[rstest]
#[case::zero("0", "0.0000")]
#[case::integer("1", "1.0000")]
#[case::fraction("1.2345", "1.2345")]
#[case::smallest_unit("0.0001", "0.0001")]
fn exact_balances_are_formatted_with_four_places(#[case] deposit: &str, #[case] expected: &str) {
    let mut account = Account::new();
    if deposit != "0" {
        account.deposit(amount(deposit)).unwrap();
    }
    let mut output = Vec::new();
    write(&mut output, [(ClientId::from(1), &account)]).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("{HEADER}1,{expected},0.0000,{expected},false\n")
    );
}

#[test]
fn held_and_negative_balances_are_serialized_without_rounding() {
    let mut account = Account::new();
    account.deposit(amount("10")).unwrap();
    account.withdraw(amount("8")).unwrap();
    account.hold(amount("10")).unwrap();
    let mut output = Vec::new();
    write(&mut output, [(ClientId::from(1), &account)]).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("{HEADER}1,-8.0000,10.0000,2.0000,false\n")
    );
}

#[test]
fn charged_back_account_has_negative_total_and_lowercase_locked_flag() {
    let mut account = Account::new();
    account.deposit(amount("10")).unwrap();
    account.withdraw(amount("8")).unwrap();
    account.hold(amount("10")).unwrap();
    account.chargeback(amount("10")).unwrap();
    let mut output = Vec::new();
    write(&mut output, [(ClientId::from(1), &account)]).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("{HEADER}1,-8.0000,0.0000,-8.0000,true\n")
    );
}

#[test]
fn client_rows_are_sorted_numerically() {
    let account = Account::new();
    let mut output = Vec::new();
    write(
        &mut output,
        [10, u16::MAX, 2, 0].map(|id| (ClientId::from(id), &account)),
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!(
            "{HEADER}0,0.0000,0.0000,0.0000,false\n2,0.0000,0.0000,0.0000,false\n10,0.0000,0.0000,0.0000,false\n65535,0.0000,0.0000,0.0000,false\n"
        )
    );
}

#[test]
fn empty_accounts_still_emit_header() {
    let manager = PaymentManager::new();
    let mut output = Vec::new();
    write(&mut output, manager.accounts()).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), HEADER);
}

#[test]
fn largest_supported_balance_is_preserved() {
    let mut account = Account::new();
    account
        .deposit(PositiveAmount::try_from(Money::from_scaled_units(i128::MAX)).unwrap())
        .unwrap();
    let mut output = Vec::new();
    write(&mut output, [(ClientId::from(1), &account)]).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!(
            "{HEADER}1,17014118346046923173168730371588410.5727,0.0000,17014118346046923173168730371588410.5727,false\n"
        )
    );
}

#[test]
fn input_manager_and_output_integrate_without_cli_wiring() {
    let source = SourceContext {
        run_id: "run-1".into(),
        source_id: "fixture".into(),
        partner_id: None,
    };
    let input = Input::new(
        include_bytes!("../../../../../tests/fixtures/input/payments.csv").as_slice(),
        source,
    )
    .unwrap();
    let mut manager = PaymentManager::new();
    for record in input {
        manager.process(record.unwrap().request).unwrap();
    }
    let before: Vec<_> = manager
        .accounts()
        .map(|(id, account)| (id, account.clone()))
        .collect();
    let mut output = Vec::new();
    write(&mut output, manager.accounts()).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("{HEADER}3,4.0000,0.0000,4.0000,false\n9,4.0000,0.0000,4.0000,false\n")
    );
    for (id, account) in before {
        assert_eq!(manager.account(id), Some(&account));
    }
}

#[derive(Default)]
struct FailingWriter {
    fail_write: bool,
    fail_flush: bool,
    bytes: Vec<u8>,
    flushed: bool,
}

impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
        if self.fail_write {
            return Err(IoError::other("simulated write failure"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> IoResult<()> {
        if self.fail_flush {
            return Err(IoError::other("simulated flush failure"));
        }
        self.flushed = true;
        Ok(())
    }
}

#[rstest]
#[case::write_failure(true, false)]
#[case::flush_failure(false, true)]
fn final_write_and_flush_failures_are_propagated(
    #[case] fail_write: bool,
    #[case] fail_flush: bool,
) {
    let mut writer = FailingWriter {
        fail_write,
        fail_flush,
        ..FailingWriter::default()
    };
    let manager = PaymentManager::new();
    let error = write(&mut writer, manager.accounts()).unwrap_err();
    assert!(matches!(error, OutputError::Io(_)));
    assert!(error.source().is_some());
}

#[test]
fn failure_while_writing_buffered_records_is_propagated() {
    let mut writer = FailingWriter {
        fail_write: true,
        ..FailingWriter::default()
    };
    let account = Account::new();
    let error = write(
        &mut writer,
        (0..1_000).map(|id| (ClientId::from(id), &account)),
    )
    .unwrap_err();
    assert!(matches!(error, OutputError::Csv(_)));
    assert!(error.source().is_some());
}

#[test]
fn successful_output_flushes_the_underlying_writer() {
    let mut writer = FailingWriter::default();
    let manager = PaymentManager::new();
    write(&mut writer, manager.accounts()).unwrap();
    assert!(writer.flushed);
    assert_eq!(writer.bytes, HEADER.as_bytes());
}
