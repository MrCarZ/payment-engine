use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    io::Error as IoError,
};

use csv::Error as CsvError;

use crate::domain::payment::MoneyError;

#[derive(Debug)]
pub enum OutputError {
    Csv(CsvError),
    Io(IoError),
    Arithmetic(MoneyError),
}

impl From<CsvError> for OutputError {
    fn from(error: CsvError) -> Self {
        Self::Csv(error)
    }
}

impl From<IoError> for OutputError {
    fn from(error: IoError) -> Self {
        Self::Io(error)
    }
}

impl From<MoneyError> for OutputError {
    fn from(error: MoneyError) -> Self {
        Self::Arithmetic(error)
    }
}

impl Display for OutputError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Csv(error) => write!(f, "account CSV output failed: {error}"),
            Self::Io(error) => write!(f, "account output flush failed: {error}"),
            Self::Arithmetic(error) => write!(f, "account total calculation failed: {error}"),
        }
    }
}

impl Error for OutputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Csv(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Arithmetic(error) => Some(error),
        }
    }
}
