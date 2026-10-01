use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    io::Error as IoError,
    path::PathBuf,
};

use serde_json::Error as JsonError;

use crate::bootstrap::payment::{RunError, batch::BatchError};

#[derive(Debug)]
pub enum ArtifactError {
    Io(IoError),
    Json(JsonError),
    Single(Box<RunError>),
    Batch(Box<BatchError>),
    Additional {
        primary: Box<Self>,
        secondary: Box<Self>,
    },
    Run {
        directory: PathBuf,
        error: Box<Self>,
    },
}

impl From<IoError> for ArtifactError {
    fn from(error: IoError) -> Self {
        Self::Io(error)
    }
}
impl From<JsonError> for ArtifactError {
    fn from(error: JsonError) -> Self {
        Self::Json(error)
    }
}
impl From<RunError> for ArtifactError {
    fn from(error: RunError) -> Self {
        Self::Single(Box::new(error))
    }
}
impl From<BatchError> for ArtifactError {
    fn from(error: BatchError) -> Self {
        Self::Batch(Box::new(error))
    }
}

impl Display for ArtifactError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Io(error) => write!(f, "output artifact I/O failed: {error}"),
            Self::Json(error) => write!(f, "run report failed: {error}"),
            Self::Single(error) => error.fmt(f),
            Self::Batch(error) => error.fmt(f),
            Self::Additional { primary, secondary } => {
                write!(f, "{primary}; additionally: {secondary}")
            }
            Self::Run { directory, error } => {
                write!(f, "{error}; output directory: {}", directory.display())
            }
        }
    }
}

impl Error for ArtifactError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Single(error) => Some(error.as_ref()),
            Self::Batch(error) => Some(error.as_ref()),
            Self::Additional { primary, .. } => Some(primary.as_ref()),
            Self::Run { error, .. } => Some(error.as_ref()),
        }
    }
}
