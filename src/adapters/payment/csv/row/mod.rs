use crate::{
    domain::payment::{LifecycleAction, transaction::Type},
    manager::payment::Request,
};

mod error;

pub use error::{ConversionError, FieldError};

/// A borrowed external representation, independent of the CSV reader and
/// operational metadata. Conversion produces an owned, validated request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row<'a> {
    pub transaction_type: &'a str,
    pub client: &'a str,
    pub tx: &'a str,
    pub amount: Option<&'a str>,
}

impl TryFrom<Row<'_>> for Request {
    type Error = ConversionError;

    fn try_from(row: Row<'_>) -> Result<Self, Self::Error> {
        let client = row.client.parse().map_err(|error| ConversionError {
            field: "client",
            error: FieldError::Identifier(error),
        })?;
        let tx = row.tx.parse().map_err(|error| ConversionError {
            field: "tx",
            error: FieldError::Identifier(error),
        })?;
        match row.transaction_type.trim() {
            event @ ("deposit" | "withdrawal") => {
                let amount = row.amount.unwrap_or("").trim();
                if amount.is_empty() {
                    return Err(ConversionError {
                        field: "amount",
                        error: FieldError::MissingAmount,
                    });
                }
                let amount = amount.parse().map_err(|error| ConversionError {
                    field: "amount",
                    error: FieldError::Amount(error),
                })?;
                let transaction_type = if event == "deposit" {
                    Type::Deposit
                } else {
                    Type::Withdrawal
                };
                Ok(Self::Original {
                    client,
                    tx,
                    transaction_type,
                    amount,
                })
            }
            event @ ("dispute" | "resolve" | "chargeback") => {
                let action = match event {
                    "dispute" => LifecycleAction::Dispute,
                    "resolve" => LifecycleAction::Resolve,
                    _ => LifecycleAction::Chargeback,
                };
                Ok(Self::Lifecycle { client, tx, action })
            }
            _ => Err(ConversionError {
                field: "type",
                error: FieldError::UnknownType,
            }),
        }
    }
}

#[cfg(test)]
mod tests;
