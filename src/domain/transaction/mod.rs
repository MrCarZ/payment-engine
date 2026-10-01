mod error;

pub use error::TransitionError;

use super::{ClientId, PositiveAmount, TransactionId};

/// The type of an original monetary transaction, distinct from reference actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Deposit,
    Withdrawal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Posted,
    Disputed,
    ChargedBack,
}

/// An action referencing an existing transaction and its stored amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
    Dispute,
    Resolve,
    Chargeback,
}

/// An accepted original transaction. Rejected original requests are tracked by
/// the payment manager and must not be represented as posted transactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    id: TransactionId,
    client: ClientId,
    transaction_type: Type,
    amount: PositiveAmount,
    state: State,
}

impl Transaction {
    /// Records an original whose account operation has succeeded. This does not
    /// apply a deposit or withdrawal; the manager coordinates that operation.
    pub const fn posted(
        id: TransactionId,
        client: ClientId,
        transaction_type: Type,
        amount: PositiveAmount,
    ) -> Self {
        Self {
            id,
            client,
            transaction_type,
            amount,
            state: State::Posted,
        }
    }

    pub const fn id(&self) -> TransactionId {
        self.id
    }

    pub const fn client(&self) -> ClientId {
        self.client
    }

    pub const fn transaction_type(&self) -> Type {
        self.transaction_type
    }

    pub const fn amount(&self) -> PositiveAmount {
        self.amount
    }

    pub const fn state(&self) -> State {
        self.state
    }

    /// Validates a lifecycle action and returns a candidate record. The original
    /// is unchanged on success or failure. The manager must validate reference
    /// ownership and commit the candidate together with the account update.
    pub fn transition(&self, action: LifecycleAction) -> Result<Self, TransitionError> {
        if self.transaction_type != Type::Deposit {
            return Err(TransitionError::NotDisputable);
        }
        let state = match (self.state, action) {
            (State::ChargedBack, _) => return Err(TransitionError::AlreadyChargedBack),
            (State::Posted, LifecycleAction::Dispute) => State::Disputed,
            (State::Disputed, LifecycleAction::Resolve) => State::Posted,
            (State::Disputed, LifecycleAction::Chargeback) => State::ChargedBack,
            (State::Disputed, LifecycleAction::Dispute) => {
                return Err(TransitionError::AlreadyDisputed);
            }
            (State::Posted, _) => return Err(TransitionError::NotDisputed),
        };
        Ok(Self {
            state,
            ..self.clone()
        })
    }
}

#[cfg(test)]
mod tests;
