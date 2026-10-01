use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

use crate::domain::{AccountError, MoneyError};

/// Fatal processing failures, distinct from normal business outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingError {
    Arithmetic(MoneyError),
    InconsistentState,
}

impl From<AccountError> for ProcessingError {
    fn from(error: AccountError) -> Self {
        match error {
            AccountError::Arithmetic(error) => Self::Arithmetic(error),
            // Ordinary original-request rejections are handled before conversion.
            // Lifecycle operations cannot legitimately lack their recorded hold.
            _ => Self::InconsistentState,
        }
    }
}

impl Display for ProcessingError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Arithmetic(error) => error.fmt(f),
            Self::InconsistentState => {
                f.write_str("account and transaction state are inconsistent")
            }
        }
    }
}

impl Error for ProcessingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Arithmetic(error) => Some(error),
            Self::InconsistentState => None,
        }
    }
}
