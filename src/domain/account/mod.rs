mod error;

pub use error::AccountError;

use super::{Money, MoneyError, PositiveAmount};

/// A single asset account. Transaction ownership and lifecycle are checked by
/// the caller; these operations enforce balance and account restrictions.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Account {
    available: Money,
    held: Money,
    locked: bool,
}

impl Account {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn available(&self) -> Money {
        self.available
    }

    pub const fn held(&self) -> Money {
        self.held
    }

    pub const fn is_locked(&self) -> bool {
        self.locked
    }

    /// Derives total using checked arithmetic. Every successful mutation also
    /// verifies that this sum remains representable.
    pub fn total(&self) -> Result<Money, MoneyError> {
        self.available.checked_add(self.held)
    }

    pub fn deposit(&mut self, amount: PositiveAmount) -> Result<(), AccountError> {
        self.ensure_unlocked()?;
        let available = self.available.checked_add(amount.money())?;
        self.commit_balances(available, self.held)
    }

    pub fn withdraw(&mut self, amount: PositiveAmount) -> Result<(), AccountError> {
        self.ensure_unlocked()?;
        if self.available < amount.money() {
            return Err(AccountError::InsufficientAvailableFunds);
        }
        let available = self.available.checked_sub(amount.money())?;
        self.commit_balances(available, self.held)
    }

    /// Holds the full amount even if the account's available balance is smaller.
    /// Lifecycle operations remain permitted on locked accounts.
    pub fn hold(&mut self, amount: PositiveAmount) -> Result<(), AccountError> {
        let available = self.available.checked_sub(amount.money())?;
        let held = self.held.checked_add(amount.money())?;
        self.commit_balances(available, held)
    }

    pub fn release(&mut self, amount: PositiveAmount) -> Result<(), AccountError> {
        self.ensure_held(amount)?;
        let available = self.available.checked_add(amount.money())?;
        let held = self.held.checked_sub(amount.money())?;
        self.commit_balances(available, held)
    }

    /// Removes held funds and permanently locks the account. The lock changes
    /// only after all balance validation succeeds.
    pub fn chargeback(&mut self, amount: PositiveAmount) -> Result<(), AccountError> {
        self.ensure_held(amount)?;
        let held = self.held.checked_sub(amount.money())?;
        self.commit_balances(self.available, held)?;
        self.locked = true;
        Ok(())
    }

    fn ensure_unlocked(&self) -> Result<(), AccountError> {
        if self.locked {
            Err(AccountError::Locked)
        } else {
            Ok(())
        }
    }

    fn ensure_held(&self, amount: PositiveAmount) -> Result<(), AccountError> {
        if self.held < amount.money() {
            Err(AccountError::InsufficientHeldFunds)
        } else {
            Ok(())
        }
    }

    fn commit_balances(&mut self, available: Money, held: Money) -> Result<(), AccountError> {
        available.checked_add(held)?;
        self.available = available;
        self.held = held;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
