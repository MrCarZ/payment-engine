use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    num::ParseIntError,
};

use crate::domain::payment::AmountError;

#[derive(Debug)]
pub enum FieldError {
    Identifier(ParseIntError),
    Amount(AmountError),
    MissingAmount,
    UnknownType,
}

impl Display for FieldError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Identifier(error) => error.fmt(f),
            Self::Amount(error) => error.fmt(f),
            Self::MissingAmount => f.write_str("original transactions require an amount"),
            Self::UnknownType => f.write_str("unsupported transaction type"),
        }
    }
}

impl Error for FieldError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Identifier(error) => Some(error),
            Self::Amount(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct ConversionError {
    pub field: &'static str,
    pub error: FieldError,
}

impl Display for ConversionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "invalid {}: {}", self.field, self.error)
    }
}

impl Error for ConversionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}
