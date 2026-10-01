use rstest::rstest;

use super::{Account, AccountError};
use crate::domain::{Money, MoneyError, PositiveAmount};

fn amount(value: &str) -> PositiveAmount {
    value.parse().expect("positive amount")
}

fn money(value: &str) -> Money {
    value.parse().expect("valid balance")
}

fn assert_balances(account: &Account, available: &str, held: &str, total: &str) {
    assert_eq!(account.available(), money(available));
    assert_eq!(account.held(), money(held));
    assert_eq!(account.total(), Ok(money(total)));
    assert_eq!(
        account.available().checked_add(account.held()),
        account.total()
    );
    assert!(account.held() >= Money::ZERO);
}

#[test]
fn new_account_has_zero_balances_and_is_unlocked() {
    let account = Account::new();
    assert_balances(&account, "0", "0", "0");
    assert!(!account.is_locked());
}

#[test]
fn deposits_and_withdrawals_preserve_exact_precision() {
    let mut account = Account::new();
    account.deposit(amount("1.2345")).unwrap();
    account.deposit(amount("0.0001")).unwrap();
    account.withdraw(amount("0.2345")).unwrap();
    assert_balances(&account, "1.0001", "0", "1.0001");
}

#[test]
fn exact_available_balance_can_be_withdrawn() {
    let mut account = Account::new();
    account.deposit(amount("2")).unwrap();
    account.withdraw(amount("2")).unwrap();
    assert_balances(&account, "0", "0", "0");
}

#[test]
fn held_funds_cannot_be_withdrawn() {
    let mut account = Account::new();
    account.deposit(amount("10")).unwrap();
    account.hold(amount("8")).unwrap();
    let before = account.clone();
    assert_eq!(
        account.withdraw(amount("3")),
        Err(AccountError::InsufficientAvailableFunds)
    );
    assert_eq!(account, before);
}

#[test]
fn dispute_after_spending_can_be_released_or_charged_back() {
    let mut account = Account::new();
    account.deposit(amount("10")).unwrap();
    account.withdraw(amount("8")).unwrap();
    account.hold(amount("10")).unwrap();
    assert_balances(&account, "-8", "10", "2");

    let mut resolved = account.clone();
    resolved.release(amount("10")).unwrap();
    assert_balances(&resolved, "2", "0", "2");
    assert!(!resolved.is_locked());

    account.chargeback(amount("10")).unwrap();
    assert_balances(&account, "-8", "0", "-8");
    assert!(account.is_locked());
}

#[test]
fn negative_available_balance_rejects_withdrawals() {
    let mut account = Account::new();
    account.deposit(amount("1")).unwrap();
    account.withdraw(amount("1")).unwrap();
    account.hold(amount("1")).unwrap();
    let before = account.clone();
    assert_eq!(
        account.withdraw(amount("0.0001")),
        Err(AccountError::InsufficientAvailableFunds)
    );
    assert_eq!(account, before);
}

#[test]
fn insufficient_held_funds_leave_balances_and_lock_unchanged() {
    let mut account = Account::new();
    account.deposit(amount("2")).unwrap();
    account.hold(amount("1")).unwrap();
    let before = account.clone();
    assert_eq!(
        account.release(amount("2")),
        Err(AccountError::InsufficientHeldFunds)
    );
    assert_eq!(account, before);
    assert_eq!(
        account.chargeback(amount("2")),
        Err(AccountError::InsufficientHeldFunds)
    );
    assert_eq!(account, before);
}

#[test]
fn chargeback_locks_new_payments_but_allows_remaining_lifecycle_operations() {
    let mut account = Account::new();
    account.deposit(amount("10")).unwrap();
    account.hold(amount("2")).unwrap();
    account.hold(amount("3")).unwrap();
    account.chargeback(amount("2")).unwrap();
    assert_balances(&account, "5", "3", "8");
    let before = account.clone();
    assert_eq!(account.deposit(amount("1")), Err(AccountError::Locked));
    assert_eq!(account.withdraw(amount("1")), Err(AccountError::Locked));
    assert_eq!(account, before);
    account.release(amount("3")).unwrap();
    account.hold(amount("1")).unwrap();
    account.chargeback(amount("1")).unwrap();
    assert_balances(&account, "7", "0", "7");
    assert!(account.is_locked());
}

#[test]
fn deposit_overflow_does_not_change_account() {
    let mut account = Account::new();
    let max = PositiveAmount::try_from(Money::from_scaled_units(i128::MAX)).unwrap();
    account.deposit(max).unwrap();
    let before = account.clone();
    assert_eq!(
        account.deposit(amount("0.0001")),
        Err(AccountError::Arithmetic(MoneyError::Overflow))
    );
    assert_eq!(account, before);
}

#[test]
fn deposit_checks_total_even_when_available_fits() {
    let mut account = Account::new();
    let max = PositiveAmount::try_from(Money::from_scaled_units(i128::MAX)).unwrap();
    account.deposit(max).unwrap();
    account.hold(max).unwrap();
    let before = account.clone();
    assert_eq!(
        account.deposit(amount("0.0001")),
        Err(AccountError::Arithmetic(MoneyError::Overflow))
    );
    assert_eq!(account, before);
}

#[rstest]
#[case::available_underflow(i128::MIN, 0)]
#[case::held_overflow(-1, i128::MAX)]
fn hold_overflow_never_commits_the_first_balance_change(
    #[case] available: i128,
    #[case] held: i128,
) {
    // Initial states satisfy nonnegative held funds and representable total.
    let mut account = Account {
        available: Money::from_scaled_units(available),
        held: Money::from_scaled_units(held),
        locked: false,
    };
    assert!(account.total().is_ok());
    let before = account.clone();
    assert_eq!(
        account.hold(amount("0.0001")),
        Err(AccountError::Arithmetic(MoneyError::Overflow))
    );
    assert_eq!(account, before);
}
