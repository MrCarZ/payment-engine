use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

use crate::{
    adapters::payment::csv::input::InputError, adapters::payment::csv::processing::RunError,
    manager::payment::batch::ValidationError,
};

use super::ExecutionError;

#[derive(Debug)]
pub enum BatchError {
    Setup(Box<RunError>),
    Execution(Box<ExecutionError>),
    Input(InputError),
    Contract(ValidationError),
}

impl Display for BatchError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Setup(error) => write!(f, "batch setup failed: {error}"),
            Self::Execution(error) => error.fmt(f),
            Self::Input(error) => write!(f, "batch input failed: {error}"),
            Self::Contract(error) => write!(f, "batch contract failed: {error}"),
        }
    }
}

impl Error for BatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Setup(error) => Some(error.as_ref()),
            Self::Execution(error) => Some(error.as_ref()),
            Self::Input(error) => Some(error),
            Self::Contract(error) => Some(error),
        }
    }
}
