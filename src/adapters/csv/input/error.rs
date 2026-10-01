use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    num::ParseIntError,
};

use csv::Error as CsvError;

use crate::{domain::payment::AmountError, manager::payment::Context};

#[derive(Debug)]
pub struct InputError {
    pub context: Context,
    pub error_type: Type,
}

#[derive(Debug)]
pub enum Type {
    InvalidHeaders,
    InvalidRecordLength,
    InvalidField {
        field: &'static str,
        error: FieldError,
    },
    Csv(CsvError),
}

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

impl Display for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::InvalidHeaders => {
                f.write_str("expected exactly the headers type, client, tx, amount, each once")
            }
            Self::InvalidRecordLength => f.write_str("invalid number of CSV fields"),
            Self::InvalidField { field, error } => write!(f, "invalid {field}: {error}"),
            Self::Csv(error) => error.fmt(f),
        }
    }
}

impl Error for Type {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidField { error, .. } => Some(error),
            Self::Csv(error) => Some(error),
            _ => None,
        }
    }
}

impl Display for InputError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "source {} at record {}, line {}, byte {}: {}",
            self.context.source.source_id,
            self.context.position.record,
            self.context.position.line,
            self.context.position.byte,
            self.error_type
        )
    }
}

impl Error for InputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error_type)
    }
}
