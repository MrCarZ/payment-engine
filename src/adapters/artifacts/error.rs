use serde_json::Error as JsonError;
use std::{
    error::Error as StdError,
    fmt::{Display, Formatter, Result as FmtResult},
    io::Error as IoError,
};
#[derive(Debug)]
pub enum Error {
    Io(IoError),
    Json(JsonError),
}
impl From<IoError> for Error {
    fn from(error: IoError) -> Self {
        Self::Io(error)
    }
}
impl From<JsonError> for Error {
    fn from(error: JsonError) -> Self {
        Self::Json(error)
    }
}
impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Io(error) => write!(f, "artifact I/O failed: {error}"),
            Self::Json(error) => write!(f, "report serialization failed: {error}"),
        }
    }
}
impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
        }
    }
}
