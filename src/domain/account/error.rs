use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

use crate::domain::MoneyError;

/// Business restrictions are separate from financial arithmetic failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountError {
    Locked,
    InsufficientAvailableFunds,
    InsufficientHeldFunds,
    Arithmetic(MoneyError),
}

impl From<MoneyError> for AccountError {
    fn from(error: MoneyError) -> Self {
        Self::Arithmetic(error)
    }
}

impl Display for AccountError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Locked => f.write_str("account is locked"),
            Self::InsufficientAvailableFunds => f.write_str("insufficient available funds"),
            Self::InsufficientHeldFunds => f.write_str("insufficient held funds"),
            Self::Arithmetic(error) => error.fmt(f),
        }
    }
}

impl Error for AccountError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Arithmetic(error) => Some(error),
            _ => None,
        }
    }
}
