use std::collections::HashMap;

use crate::domain::payment::{
    Account, AccountError, ClientId, LifecycleAction, PositiveAmount, Transaction, TransactionId,
    TransitionError, transaction::Type,
};

pub mod batch;
mod context;
mod error;
mod request;
pub mod run;
pub mod trace;

pub use context::{Context, SourceContext};
pub use error::ProcessingError;
pub use request::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    Ignored(Reason),
    Rejected(Reason),
}

/// The business outcome and whether it was returned from an original replay.
/// A replayed Applied outcome does not mean funds were moved again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    outcome: Outcome,
    replayed: bool,
}

impl Report {
    pub const fn outcome(&self) -> Outcome {
        self.outcome
    }

    pub const fn is_replay(&self) -> bool {
        self.replayed
    }
}

/// Stable classifications for future trace and partner reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    UnknownTransaction,
    TransactionNotAccepted,
    ClientMismatch,
    NotDisputable,
    AlreadyDisputed,
    NotDisputed,
    AlreadyChargedBack,
    AccountLocked,
    InsufficientAvailableFunds,
    ConflictingTransactionId,
}

/// Retains the original request and outcome even when the request was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalRecord {
    request: Request,
    outcome: Outcome,
    transaction: Option<Transaction>,
}

impl OriginalRecord {
    pub const fn request(&self) -> Request {
        self.request
    }

    pub const fn outcome(&self) -> Outcome {
        self.outcome
    }

    pub fn transaction(&self) -> Option<&Transaction> {
        self.transaction.as_ref()
    }
}

/// Synchronous in-memory coordination; no CSV, file, or tracing dependencies.
#[derive(Debug, Default)]
pub struct PaymentManager {
    accounts: HashMap<ClientId, Account>,
    originals: HashMap<TransactionId, OriginalRecord>,
}

impl PaymentManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn account(&self, client: ClientId) -> Option<&Account> {
        self.accounts.get(&client)
    }

    pub fn original(&self, tx: TransactionId) -> Option<&OriginalRecord> {
        self.originals.get(&tx)
    }

    pub fn accounts(&self) -> impl Iterator<Item = (ClientId, &Account)> {
        self.accounts
            .iter()
            .map(|(client, account)| (*client, account))
    }

    pub fn process(&mut self, request: Request) -> Result<Report, ProcessingError> {
        if let Request::Original { tx, .. } = request
            && let Some(original) = self.originals.get(&tx)
            && original.request == request
        {
            return Ok(Report {
                outcome: original.outcome,
                replayed: true,
            });
        }
        let outcome = match request {
            Request::Original {
                client,
                tx,
                transaction_type,
                amount,
            } => self.process_original(request, client, tx, transaction_type, amount),
            Request::Lifecycle { client, tx, action } => self.process_lifecycle(client, tx, action),
        }?;
        Ok(Report {
            outcome,
            replayed: false,
        })
    }

    fn process_original(
        &mut self,
        request: Request,
        client: ClientId,
        tx: TransactionId,
        transaction_type: Type,
        amount: PositiveAmount,
    ) -> Result<Outcome, ProcessingError> {
        if self.originals.contains_key(&tx) {
            self.accounts.entry(client).or_default();
            return Ok(Outcome::Rejected(Reason::ConflictingTransactionId));
        }
        let mut account = self.accounts.get(&client).cloned().unwrap_or_default();
        let result = match transaction_type {
            Type::Deposit => account.deposit(amount),
            Type::Withdrawal => account.withdraw(amount),
        };
        let outcome = match result {
            Ok(()) => Outcome::Applied,
            Err(AccountError::Locked) => Outcome::Rejected(Reason::AccountLocked),
            Err(AccountError::InsufficientAvailableFunds) => {
                Outcome::Rejected(Reason::InsufficientAvailableFunds)
            }
            Err(error) => return Err(error.into()),
        };
        let transaction = if outcome == Outcome::Applied {
            Some(Transaction::posted(tx, client, transaction_type, amount))
        } else {
            None
        };
        // All domain checks precede committing either record.
        self.accounts.insert(client, account);
        self.originals.insert(
            tx,
            OriginalRecord {
                request,
                outcome,
                transaction,
            },
        );
        Ok(outcome)
    }

    fn process_lifecycle(
        &mut self,
        client: ClientId,
        tx: TransactionId,
        action: LifecycleAction,
    ) -> Result<Outcome, ProcessingError> {
        let Some(mut original) = self.originals.get(&tx).cloned() else {
            self.accounts.entry(client).or_default();
            return Ok(Outcome::Ignored(Reason::UnknownTransaction));
        };
        let Request::Original { client: owner, .. } = original.request else {
            return Err(ProcessingError::InconsistentState);
        };
        if owner != client {
            self.accounts.entry(client).or_default();
            return Ok(Outcome::Rejected(Reason::ClientMismatch));
        }
        let Some(transaction) = original.transaction() else {
            return Ok(Outcome::Ignored(Reason::TransactionNotAccepted));
        };
        let candidate = match transaction.transition(action) {
            Ok(candidate) => candidate,
            Err(TransitionError::NotDisputable) => {
                return Ok(Outcome::Rejected(Reason::NotDisputable));
            }
            Err(TransitionError::AlreadyDisputed) => {
                return Ok(Outcome::Ignored(Reason::AlreadyDisputed));
            }
            Err(TransitionError::NotDisputed) => return Ok(Outcome::Ignored(Reason::NotDisputed)),
            Err(TransitionError::AlreadyChargedBack) => {
                return Ok(Outcome::Ignored(Reason::AlreadyChargedBack));
            }
        };
        let mut account = self
            .accounts
            .get(&client)
            .cloned()
            .ok_or(ProcessingError::InconsistentState)?;
        match action {
            LifecycleAction::Dispute => account.hold(candidate.amount()),
            LifecycleAction::Resolve => account.release(candidate.amount()),
            LifecycleAction::Chargeback => account.chargeback(candidate.amount()),
        }
        .map_err(ProcessingError::from)?;
        original.transaction = Some(candidate);
        self.accounts.insert(client, account);
        self.originals.insert(tx, original);
        Ok(Outcome::Applied)
    }
}

#[cfg(test)]
mod tests;
