use crate::domain::payment::{
    ClientId, LifecycleAction, PositiveAmount, TransactionId, transaction::Type,
};

/// Validated monetary requests and references to existing originals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Original {
        client: ClientId,
        tx: TransactionId,
        transaction_type: Type,
        amount: PositiveAmount,
    },
    Lifecycle {
        client: ClientId,
        tx: TransactionId,
        action: LifecycleAction,
    },
}
