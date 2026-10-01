use rstest::rstest;

use super::{Outcome, PaymentManager, ProcessingError, Reason, Report, Request};
use crate::domain::payment::{
    Account, ClientId, LifecycleAction, Money, MoneyError, TransactionId,
    transaction::{State, Type},
};

fn processed(outcome: Outcome) -> Report {
    Report {
        outcome,
        replayed: false,
    }
}

fn original(client: u16, tx: u32, transaction_type: Type, amount: &str) -> Request {
    Request::Original {
        client: ClientId::from(client),
        tx: TransactionId::from(tx),
        transaction_type,
        amount: amount.parse().expect("positive amount"),
    }
}

fn lifecycle(client: u16, tx: u32, action: LifecycleAction) -> Request {
    Request::Lifecycle {
        client: ClientId::from(client),
        tx: TransactionId::from(tx),
        action,
    }
}

fn apply(manager: &mut PaymentManager, request: Request) {
    assert_eq!(manager.process(request), Ok(processed(Outcome::Applied)));
}

fn assert_balances(
    manager: &PaymentManager,
    client: u16,
    available: &str,
    held: &str,
    total: &str,
    locked: bool,
) {
    let account = manager
        .account(ClientId::from(client))
        .expect("client exists");
    assert_eq!(account.available(), available.parse::<Money>().unwrap());
    assert_eq!(account.held(), held.parse::<Money>().unwrap());
    assert_eq!(account.total(), Ok(total.parse::<Money>().unwrap()));
    assert_eq!(account.is_locked(), locked);
}

#[test]
fn interleaved_clients_and_unordered_ids_match_example_balances() {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(2, 20, Type::Deposit, "2"));
    apply(&mut manager, original(1, 10, Type::Deposit, "1"));
    apply(&mut manager, original(1, 3, Type::Deposit, "2"));
    apply(&mut manager, original(1, 4, Type::Withdrawal, "1.5"));
    assert_eq!(
        manager.process(original(2, 5, Type::Withdrawal, "3")),
        Ok(processed(Outcome::Rejected(
            Reason::InsufficientAvailableFunds
        )))
    );
    assert_balances(&manager, 1, "1.5", "0", "1.5", false);
    assert_balances(&manager, 2, "2", "0", "2", false);
    assert_eq!(manager.accounts().count(), 2);
}

#[test]
fn rejected_first_withdrawal_creates_account_and_retains_original() {
    let mut manager = PaymentManager::new();
    let request = original(7, 42, Type::Withdrawal, "1");
    let outcome = Outcome::Rejected(Reason::InsufficientAvailableFunds);
    assert_eq!(manager.process(request), Ok(processed(outcome)));
    assert_balances(&manager, 7, "0", "0", "0", false);
    let record = manager.original(TransactionId::from(42)).unwrap();
    assert_eq!(record.request(), request);
    assert_eq!(record.outcome(), outcome);
    assert!(record.transaction().is_none());
}

#[rstest]
#[case::dispute(LifecycleAction::Dispute)]
#[case::resolve(LifecycleAction::Resolve)]
#[case::chargeback(LifecycleAction::Chargeback)]
fn unknown_references_are_ignored_and_create_client(#[case] action: LifecycleAction) {
    let mut manager = PaymentManager::new();
    assert_eq!(
        manager.process(lifecycle(1, 99, action)),
        Ok(processed(Outcome::Ignored(Reason::UnknownTransaction)))
    );
    assert_balances(&manager, 1, "0", "0", "0", false);
    assert!(manager.original(TransactionId::from(99)).is_none());
}

#[rstest]
#[case::dispute(LifecycleAction::Dispute)]
#[case::resolve(LifecycleAction::Resolve)]
#[case::chargeback(LifecycleAction::Chargeback)]
fn client_mismatch_preserves_owner_and_transaction(#[case] action: LifecycleAction) {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 1, Type::Deposit, "10"));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Dispute));
    let before = manager.original(TransactionId::from(1)).unwrap().clone();
    assert_eq!(
        manager.process(lifecycle(2, 1, action)),
        Ok(processed(Outcome::Rejected(Reason::ClientMismatch)))
    );
    assert_balances(&manager, 1, "0", "10", "10", false);
    assert_balances(&manager, 2, "0", "0", "0", false);
    assert_eq!(manager.original(TransactionId::from(1)), Some(&before));
}

#[rstest]
#[case::dispute(LifecycleAction::Dispute)]
#[case::resolve(LifecycleAction::Resolve)]
#[case::chargeback(LifecycleAction::Chargeback)]
fn rejected_originals_are_not_disputable(#[case] action: LifecycleAction) {
    let mut manager = PaymentManager::new();
    assert_eq!(
        manager.process(original(1, 1, Type::Withdrawal, "1")),
        Ok(processed(Outcome::Rejected(
            Reason::InsufficientAvailableFunds
        )))
    );
    assert_eq!(
        manager.process(lifecycle(1, 1, action)),
        Ok(processed(Outcome::Ignored(Reason::TransactionNotAccepted)))
    );
    assert_balances(&manager, 1, "0", "0", "0", false);
}

#[rstest]
#[case::dispute(LifecycleAction::Dispute)]
#[case::resolve(LifecycleAction::Resolve)]
#[case::chargeback(LifecycleAction::Chargeback)]
fn accepted_withdrawal_references_are_rejected(#[case] action: LifecycleAction) {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 1, Type::Deposit, "2"));
    apply(&mut manager, original(1, 2, Type::Withdrawal, "1"));
    let before = manager.original(TransactionId::from(2)).unwrap().clone();
    assert_eq!(
        manager.process(lifecycle(1, 2, action)),
        Ok(processed(Outcome::Rejected(Reason::NotDisputable)))
    );
    assert_balances(&manager, 1, "1", "0", "1", false);
    assert_eq!(manager.original(TransactionId::from(2)), Some(&before));
}

#[test]
fn dispute_before_deposit_is_not_buffered() {
    let mut manager = PaymentManager::new();
    assert_eq!(
        manager.process(lifecycle(1, 1, LifecycleAction::Dispute)),
        Ok(processed(Outcome::Ignored(Reason::UnknownTransaction)))
    );
    apply(&mut manager, original(1, 1, Type::Deposit, "2"));
    assert_balances(&manager, 1, "2", "0", "2", false);
}

#[test]
fn spent_deposit_can_be_disputed_resolved_redisputed_and_charged_back() {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 1, Type::Deposit, "10"));
    apply(&mut manager, original(1, 2, Type::Withdrawal, "8"));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Dispute));
    assert_balances(&manager, 1, "-8", "10", "2", false);
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Resolve));
    assert_balances(&manager, 1, "2", "0", "2", false);
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Dispute));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Chargeback));
    assert_balances(&manager, 1, "-8", "0", "-8", true);
    let record = manager.original(TransactionId::from(1)).unwrap();
    assert_eq!(record.outcome(), Outcome::Applied);
    assert_eq!(record.transaction().unwrap().state(), State::ChargedBack);
}

#[rstest]
#[case::resolve_without_dispute(&[], LifecycleAction::Resolve, Reason::NotDisputed)]
#[case::chargeback_without_dispute(&[], LifecycleAction::Chargeback, Reason::NotDisputed)]
#[case::repeated_dispute(&[LifecycleAction::Dispute], LifecycleAction::Dispute, Reason::AlreadyDisputed)]
#[case::repeated_resolve(&[LifecycleAction::Dispute, LifecycleAction::Resolve], LifecycleAction::Resolve, Reason::NotDisputed)]
#[case::repeated_chargeback(&[LifecycleAction::Dispute, LifecycleAction::Chargeback], LifecycleAction::Chargeback, Reason::AlreadyChargedBack)]
#[case::dispute_after_chargeback(&[LifecycleAction::Dispute, LifecycleAction::Chargeback], LifecycleAction::Dispute, Reason::AlreadyChargedBack)]
fn inapplicable_lifecycle_actions_preserve_all_state(
    #[case] preceding: &[LifecycleAction],
    #[case] action: LifecycleAction,
    #[case] reason: Reason,
) {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 1, Type::Deposit, "10"));
    for action in preceding {
        apply(&mut manager, lifecycle(1, 1, *action));
    }
    let accounts = manager.accounts.clone();
    let originals = manager.originals.clone();
    assert_eq!(
        manager.process(lifecycle(1, 1, action)),
        Ok(processed(Outcome::Ignored(reason)))
    );
    assert_eq!(manager.accounts, accounts);
    assert_eq!(manager.originals, originals);
}

#[test]
fn locked_account_rejects_originals_but_settles_existing_disputes() {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 1, Type::Deposit, "2"));
    apply(&mut manager, original(1, 2, Type::Deposit, "3"));
    apply(&mut manager, original(1, 3, Type::Deposit, "4"));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Dispute));
    apply(&mut manager, lifecycle(1, 2, LifecycleAction::Dispute));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Chargeback));
    for (tx, transaction_type) in [(4, Type::Deposit), (5, Type::Withdrawal)] {
        assert_eq!(
            manager.process(original(1, tx, transaction_type, "1")),
            Ok(processed(Outcome::Rejected(Reason::AccountLocked)))
        );
        assert!(
            manager
                .original(TransactionId::from(tx))
                .unwrap()
                .transaction()
                .is_none()
        );
    }
    apply(&mut manager, lifecycle(1, 2, LifecycleAction::Resolve));
    apply(&mut manager, lifecycle(1, 3, LifecycleAction::Dispute));
    apply(&mut manager, lifecycle(1, 3, LifecycleAction::Chargeback));
    assert_balances(&manager, 1, "3", "0", "3", true);
}

#[test]
fn original_overflow_changes_neither_accounts_nor_records() {
    let mut manager = PaymentManager::new();
    let max = Money::from_scaled_units(i128::MAX).to_string();
    apply(&mut manager, original(1, 1, Type::Deposit, &max));
    let accounts = manager.accounts.clone();
    let originals = manager.originals.clone();
    assert_eq!(
        manager.process(original(1, 2, Type::Deposit, "0.0001")),
        Err(ProcessingError::Arithmetic(MoneyError::Overflow))
    );
    assert_eq!(manager.accounts, accounts);
    assert_eq!(manager.originals, originals);
}

#[test]
fn dispute_overflow_does_not_commit_balance_or_transaction_candidate() {
    let mut manager = PaymentManager::new();
    let max = Money::from_scaled_units(i128::MAX).to_string();
    apply(&mut manager, original(1, 1, Type::Deposit, &max));
    apply(&mut manager, original(1, 2, Type::Withdrawal, "0.0001"));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Dispute));
    apply(&mut manager, original(1, 3, Type::Deposit, "0.0001"));
    let accounts = manager.accounts.clone();
    let originals = manager.originals.clone();
    assert_eq!(
        manager.process(lifecycle(1, 3, LifecycleAction::Dispute)),
        Err(ProcessingError::Arithmetic(MoneyError::Overflow))
    );
    assert_eq!(manager.accounts, accounts);
    assert_eq!(manager.originals, originals);
    assert_eq!(
        manager
            .original(TransactionId::from(3))
            .unwrap()
            .transaction()
            .unwrap()
            .state(),
        State::Posted
    );
}

#[test]
fn inconsistent_held_funds_are_fatal_without_partial_commit() {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 1, Type::Deposit, "2"));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Dispute));
    // Simulate corrupted state to verify invariant failures are not business rejections.
    manager.accounts.insert(ClientId::from(1), Account::new());
    let accounts = manager.accounts.clone();
    let originals = manager.originals.clone();
    assert_eq!(
        manager.process(lifecycle(1, 1, LifecycleAction::Chargeback)),
        Err(ProcessingError::InconsistentState)
    );
    assert_eq!(manager.accounts, accounts);
    assert_eq!(manager.originals, originals);
}

#[rstest]
#[case::deposit(Type::Deposit)]
#[case::withdrawal(Type::Withdrawal)]
fn normalized_original_replays_return_stored_outcome_without_mutation(
    #[case] transaction_type: Type,
) {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 100, Type::Deposit, "10"));
    apply(&mut manager, original(1, 1, transaction_type, "2.0"));
    let accounts = manager.accounts.clone();
    let originals = manager.originals.clone();
    for _ in 0..3 {
        let report = manager
            .process(original(1, 1, transaction_type, " +002.0000 "))
            .unwrap();
        assert_eq!(report.outcome(), Outcome::Applied);
        assert!(report.is_replay());
        assert_eq!(manager.accounts, accounts);
        assert_eq!(manager.originals, originals);
    }
}

#[rstest]
#[case::posted(&[])]
#[case::disputed(&[LifecycleAction::Dispute])]
#[case::resolved(&[LifecycleAction::Dispute, LifecycleAction::Resolve])]
#[case::charged_back(&[LifecycleAction::Dispute, LifecycleAction::Chargeback])]
fn deposit_replay_preserves_current_lifecycle_and_account_state(
    #[case] actions: &[LifecycleAction],
) {
    let mut manager = PaymentManager::new();
    let request = original(1, 1, Type::Deposit, "2");
    apply(&mut manager, request);
    for action in actions {
        apply(&mut manager, lifecycle(1, 1, *action));
    }
    let accounts = manager.accounts.clone();
    let originals = manager.originals.clone();
    let report = manager.process(request).unwrap();
    assert_eq!(report.outcome(), Outcome::Applied);
    assert!(report.is_replay());
    assert_eq!(manager.accounts, accounts);
    assert_eq!(manager.originals, originals);
}

#[test]
fn rejected_withdrawal_replay_does_not_succeed_after_later_deposit() {
    let mut manager = PaymentManager::new();
    let request = original(1, 1, Type::Withdrawal, "2");
    let rejected = Outcome::Rejected(Reason::InsufficientAvailableFunds);
    assert_eq!(manager.process(request), Ok(processed(rejected)));
    apply(&mut manager, original(1, 2, Type::Deposit, "10"));
    let accounts = manager.accounts.clone();
    let originals = manager.originals.clone();
    let report = manager.process(request).unwrap();
    assert_eq!(report.outcome(), rejected);
    assert!(report.is_replay());
    assert_eq!(manager.accounts, accounts);
    assert_eq!(manager.originals, originals);
    assert!(
        manager
            .original(TransactionId::from(1))
            .unwrap()
            .transaction()
            .is_none()
    );
}

#[rstest]
#[case::deposit(Type::Deposit)]
#[case::withdrawal(Type::Withdrawal)]
fn account_locked_rejections_are_replayed(#[case] transaction_type: Type) {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 1, Type::Deposit, "2"));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Dispute));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Chargeback));
    let request = original(1, 2, transaction_type, "1");
    let rejected = Outcome::Rejected(Reason::AccountLocked);
    assert_eq!(manager.process(request), Ok(processed(rejected)));
    let accounts = manager.accounts.clone();
    let originals = manager.originals.clone();
    let report = manager.process(request).unwrap();
    assert_eq!(report.outcome(), rejected);
    assert!(report.is_replay());
    assert_eq!(manager.accounts, accounts);
    assert_eq!(manager.originals, originals);
}

#[rstest]
#[case::different_amount(1, Type::Deposit, "3")]
#[case::different_type(1, Type::Withdrawal, "2")]
#[case::different_client(2, Type::Deposit, "2")]
fn conflicting_reuse_is_rejected_without_overwriting_original(
    #[case] client: u16,
    #[case] transaction_type: Type,
    #[case] amount: &str,
) {
    let mut manager = PaymentManager::new();
    let request = original(1, 1, Type::Deposit, "2");
    apply(&mut manager, request);
    let originals = manager.originals.clone();
    let report = manager
        .process(original(client, 1, transaction_type, amount))
        .unwrap();
    assert_eq!(
        report.outcome(),
        Outcome::Rejected(Reason::ConflictingTransactionId)
    );
    assert!(!report.is_replay());
    assert_eq!(manager.originals, originals);
    assert_balances(&manager, 1, "2", "0", "2", false);
    if client == 2 {
        assert_balances(&manager, 2, "0", "0", "0", false);
    }
    let replay = manager.process(request).unwrap();
    assert_eq!(replay.outcome(), Outcome::Applied);
    assert!(replay.is_replay());
}

#[test]
fn rejected_original_reserves_id_against_conflicting_reuse() {
    let mut manager = PaymentManager::new();
    let request = original(1, 1, Type::Withdrawal, "2");
    let rejected = Outcome::Rejected(Reason::InsufficientAvailableFunds);
    assert_eq!(manager.process(request), Ok(processed(rejected)));
    let originals = manager.originals.clone();
    let report = manager.process(original(1, 1, Type::Deposit, "2")).unwrap();
    assert_eq!(
        report.outcome(),
        Outcome::Rejected(Reason::ConflictingTransactionId)
    );
    assert!(!report.is_replay());
    assert_eq!(manager.originals, originals);
    assert_balances(&manager, 1, "0", "0", "0", false);
}

#[test]
fn arithmetic_failure_does_not_reserve_original_id() {
    let mut manager = PaymentManager::new();
    let max = Money::from_scaled_units(i128::MAX).to_string();
    apply(&mut manager, original(1, 1, Type::Deposit, &max));
    let request = original(1, 2, Type::Deposit, "0.0001");
    assert_eq!(
        manager.process(request),
        Err(ProcessingError::Arithmetic(MoneyError::Overflow))
    );
    assert!(manager.original(TransactionId::from(2)).is_none());
    apply(&mut manager, original(1, 3, Type::Withdrawal, "0.0001"));
    let report = manager.process(request).unwrap();
    assert_eq!(report.outcome(), Outcome::Applied);
    assert!(!report.is_replay());
}

#[test]
fn lifecycle_references_are_not_original_replays() {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 1, Type::Deposit, "2"));
    let request = lifecycle(1, 1, LifecycleAction::Dispute);
    let applied = manager.process(request).unwrap();
    assert_eq!(applied.outcome(), Outcome::Applied);
    assert!(!applied.is_replay());
    let ignored = manager.process(request).unwrap();
    assert_eq!(ignored.outcome(), Outcome::Ignored(Reason::AlreadyDisputed));
    assert!(!ignored.is_replay());
    assert_balances(&manager, 1, "0", "2", "2", false);
}

#[test]
fn held_funds_rejection_is_retained_and_replayed_after_resolution() {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 1, Type::Deposit, "10"));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Dispute));
    let withdrawal = original(1, 2, Type::Withdrawal, "1");
    let rejected = Outcome::Rejected(Reason::InsufficientAvailableFunds);
    let accounts = manager.accounts.clone();
    let deposit = manager.original(TransactionId::from(1)).unwrap().clone();
    assert_eq!(manager.process(withdrawal), Ok(processed(rejected)));
    assert_eq!(manager.accounts, accounts);
    assert_eq!(manager.original(TransactionId::from(1)), Some(&deposit));
    let record = manager.original(TransactionId::from(2)).unwrap();
    assert_eq!(record.request(), withdrawal);
    assert_eq!(record.outcome(), rejected);
    assert!(record.transaction().is_none());
    assert_balances(&manager, 1, "0", "10", "10", false);

    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Resolve));
    let accounts = manager.accounts.clone();
    let originals = manager.originals.clone();
    let replay = manager.process(withdrawal).unwrap();
    assert_eq!(replay.outcome(), rejected);
    assert!(replay.is_replay());
    assert_eq!(manager.accounts, accounts);
    assert_eq!(manager.originals, originals);
    assert_balances(&manager, 1, "10", "0", "10", false);
    apply(&mut manager, original(1, 3, Type::Withdrawal, "1"));
    assert_balances(&manager, 1, "9", "0", "9", false);
}

#[test]
fn later_deposit_recovers_negative_available_without_releasing_held_funds() {
    let mut manager = PaymentManager::new();
    apply(&mut manager, original(1, 1, Type::Deposit, "10"));
    assert_balances(&manager, 1, "10", "0", "10", false);
    apply(&mut manager, original(1, 2, Type::Withdrawal, "8"));
    assert_balances(&manager, 1, "2", "0", "2", false);
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Dispute));
    assert_balances(&manager, 1, "-8", "10", "2", false);
    let disputed = manager.original(TransactionId::from(1)).unwrap().clone();
    apply(&mut manager, original(1, 3, Type::Deposit, "9"));
    assert_balances(&manager, 1, "1", "10", "11", false);
    assert_eq!(manager.original(TransactionId::from(1)), Some(&disputed));
    apply(&mut manager, original(1, 4, Type::Withdrawal, "1"));
    assert_balances(&manager, 1, "0", "10", "10", false);
    assert_eq!(manager.original(TransactionId::from(1)), Some(&disputed));
    apply(&mut manager, lifecycle(1, 1, LifecycleAction::Resolve));
    assert_balances(&manager, 1, "10", "0", "10", false);
    assert_eq!(
        manager
            .original(TransactionId::from(1))
            .unwrap()
            .transaction()
            .unwrap()
            .state(),
        State::Posted
    );
}
