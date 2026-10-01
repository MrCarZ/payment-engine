use rstest::rstest;

use super::{FieldError, Row};
use crate::{
    domain::payment::{ClientId, LifecycleAction, TransactionId, transaction::Type},
    manager::payment::Request,
};

#[rstest]
#[case::deposit(" deposit ", Type::Deposit)]
#[case::withdrawal(" withdrawal ", Type::Withdrawal)]
fn representation_converts_without_a_reader_or_context(
    #[case] event: &str,
    #[case] transaction_type: Type,
) {
    let row = Row {
        transaction_type: event,
        client: " 7 ",
        tx: " 42 ",
        amount: Some(" 1.2345 "),
    };
    assert_eq!(
        Request::try_from(row).unwrap(),
        Request::Original {
            client: ClientId::from(7),
            tx: TransactionId::from(42),
            transaction_type,
            amount: "1.2345".parse().unwrap(),
        }
    );
}

#[rstest]
#[case::dispute("dispute", LifecycleAction::Dispute)]
#[case::resolve("resolve", LifecycleAction::Resolve)]
#[case::chargeback("chargeback", LifecycleAction::Chargeback)]
fn lifecycle_conversion_does_not_require_amount(
    #[case] event: &str,
    #[case] action: LifecycleAction,
) {
    let row = Row {
        transaction_type: event,
        client: "7",
        tx: "42",
        amount: None,
    };
    assert_eq!(
        Request::try_from(row).unwrap(),
        Request::Lifecycle {
            client: ClientId::from(7),
            tx: TransactionId::from(42),
            action,
        }
    );
}

#[test]
fn conversion_failure_contains_field_without_operational_metadata() {
    let row = Row {
        transaction_type: "deposit",
        client: "7",
        tx: "42",
        amount: None,
    };
    let error = Request::try_from(row).unwrap_err();
    assert_eq!(error.field, "amount");
    assert!(matches!(error.error, FieldError::MissingAmount));
}
