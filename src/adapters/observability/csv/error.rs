use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    io::Error as IoError,
};

use crate::manager::observability::TraceError;

use csv::Error as CsvError;
use serde_json::Error as JsonError;
use time::error::Format;

#[derive(Debug)]
pub enum CsvTraceError {
    Csv(CsvError),
    Io(IoError),
    Json(JsonError),
    Timestamp(Format),
}

impl From<CsvError> for CsvTraceError {
    fn from(error: CsvError) -> Self {
        Self::Csv(error)
    }
}

impl From<IoError> for CsvTraceError {
    fn from(error: IoError) -> Self {
        Self::Io(error)
    }
}

impl From<JsonError> for CsvTraceError {
    fn from(error: JsonError) -> Self {
        Self::Json(error)
    }
}

impl From<Format> for CsvTraceError {
    fn from(error: Format) -> Self {
        Self::Timestamp(error)
    }
}

impl Display for CsvTraceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Csv(error) => write!(f, "trace CSV write failed: {error}"),
            Self::Io(error) => write!(f, "trace flush failed: {error}"),
            Self::Json(error) => write!(f, "trace attribute serialization failed: {error}"),
            Self::Timestamp(error) => write!(f, "trace timestamp formatting failed: {error}"),
        }
    }
}

impl Error for CsvTraceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Csv(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Timestamp(error) => Some(error),
        }
    }
}

impl From<CsvTraceError> for TraceError {
    fn from(error: CsvTraceError) -> Self {
        Self::new(error)
    }
}
