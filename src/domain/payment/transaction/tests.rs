use rstest::rstest;

use super::{LifecycleAction, State, Transaction, TransitionError, Type};
use crate::domain::payment::{ClientId, TransactionId};

fn posted(transaction_type: Type) -> Transaction {
    Transaction::posted(
        TransactionId::from(42),
        ClientId::from(7),
        transaction_type,
        "10.1234".parse().expect("positive amount"),
    )
}

#[rstest]
#[case::deposit(Type::Deposit)]
#[case::withdrawal(Type::Withdrawal)]
fn accepted_original_preserves_metadata_and_starts_posted(#[case] transaction_type: Type) {
    let transaction = posted(transaction_type);
    assert_eq!(transaction.id(), TransactionId::from(42));
    assert_eq!(transaction.client(), ClientId::from(7));
    assert_eq!(transaction.transaction_type(), transaction_type);
    assert_eq!(transaction.amount().to_string(), "10.1234");
    assert_eq!(transaction.state(), State::Posted);
}

#[rstest]
#[case::dispute(State::Posted, LifecycleAction::Dispute, Ok(State::Disputed))]
#[case::resolve_without_dispute(
    State::Posted,
    LifecycleAction::Resolve,
    Err(TransitionError::NotDisputed)
)]
#[case::chargeback_without_dispute(
    State::Posted,
    LifecycleAction::Chargeback,
    Err(TransitionError::NotDisputed)
)]
#[case::repeated_dispute(
    State::Disputed,
    LifecycleAction::Dispute,
    Err(TransitionError::AlreadyDisputed)
)]
#[case::resolve(State::Disputed, LifecycleAction::Resolve, Ok(State::Posted))]
#[case::chargeback(State::Disputed, LifecycleAction::Chargeback, Ok(State::ChargedBack))]
#[case::dispute_after_chargeback(
    State::ChargedBack,
    LifecycleAction::Dispute,
    Err(TransitionError::AlreadyChargedBack)
)]
#[case::resolve_after_chargeback(
    State::ChargedBack,
    LifecycleAction::Resolve,
    Err(TransitionError::AlreadyChargedBack)
)]
#[case::repeated_chargeback(
    State::ChargedBack,
    LifecycleAction::Chargeback,
    Err(TransitionError::AlreadyChargedBack)
)]
fn deposit_transition_matrix(
    #[case] state: State,
    #[case] action: LifecycleAction,
    #[case] expected: Result<State, TransitionError>,
) {
    let mut transaction = posted(Type::Deposit);
    transaction.state = state;
    let before = transaction.clone();
    let result = transaction.transition(action);

    assert_eq!(
        result
            .as_ref()
            .map(Transaction::state)
            .map_err(|error| *error),
        expected
    );
    assert_eq!(transaction, before);
    if let Ok(candidate) = result {
        assert_eq!(candidate.id(), before.id());
        assert_eq!(candidate.client(), before.client());
        assert_eq!(candidate.transaction_type(), before.transaction_type());
        assert_eq!(candidate.amount(), before.amount());
    }
}

#[rstest]
#[case::dispute(LifecycleAction::Dispute)]
#[case::resolve(LifecycleAction::Resolve)]
#[case::chargeback(LifecycleAction::Chargeback)]
fn withdrawals_reject_all_lifecycle_actions(#[case] action: LifecycleAction) {
    let transaction = posted(Type::Withdrawal);
    let before = transaction.clone();
    assert_eq!(
        transaction.transition(action),
        Err(TransitionError::NotDisputable)
    );
    assert_eq!(transaction, before);
}

#[test]
fn resolved_deposit_can_be_disputed_again() {
    let transaction = posted(Type::Deposit);
    let disputed = transaction.transition(LifecycleAction::Dispute).unwrap();
    let resolved = disputed.transition(LifecycleAction::Resolve).unwrap();
    assert_eq!(resolved, transaction);
    let redisputed = resolved.transition(LifecycleAction::Dispute).unwrap();
    assert_eq!(redisputed, disputed);
}
