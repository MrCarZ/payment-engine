use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

use crate::{adapters::payment::csv::input::InputError, manager::payment::batch::ValidationError};

#[derive(Debug)]
pub enum BatchError {
    Input(InputError),
    Contract(ValidationError),
}

impl Display for BatchError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Input(error) => write!(f, "batch input failed: {error}"),
            Self::Contract(error) => write!(f, "batch contract failed: {error}"),
        }
    }
}

impl Error for BatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Contract(error) => Some(error),
        }
    }
}
