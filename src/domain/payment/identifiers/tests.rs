use rstest::rstest;

use super::{ClientId, TransactionId};

#[rstest]
#[case::zero(0)]
#[case::maximum(u16::MAX)]
fn client_ids_accept_full_u16_range_and_whitespace(#[case] value: u16) {
    let id = ClientId::new(value);
    assert_eq!(ClientId::from(value), id);
    assert_eq!(id.get(), value);
    assert_eq!(format!(" {value} ").parse::<ClientId>(), Ok(id));
    assert_eq!(id.to_string(), value.to_string());
}

#[rstest]
#[case::overflow("65536")]
#[case::negative("-1")]
#[case::empty("")]
#[case::decimal("1.0")]
fn invalid_client_ids_are_rejected(#[case] input: &str) {
    assert!(input.parse::<ClientId>().is_err());
}

#[rstest]
#[case::zero(0)]
#[case::maximum(u32::MAX)]
fn transaction_ids_accept_full_u32_range_and_whitespace(#[case] value: u32) {
    let id = TransactionId::new(value);
    assert_eq!(TransactionId::from(value), id);
    assert_eq!(id.get(), value);
    assert_eq!(format!(" {value} ").parse::<TransactionId>(), Ok(id));
    assert_eq!(id.to_string(), value.to_string());
}

#[rstest]
#[case::overflow("4294967296")]
#[case::negative("-1")]
#[case::empty("")]
#[case::decimal("1.0")]
fn invalid_transaction_ids_are_rejected(#[case] input: &str) {
    assert!(input.parse::<TransactionId>().is_err());
}
