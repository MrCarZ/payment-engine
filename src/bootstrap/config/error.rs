use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

/// Invalid input-path arguments.
#[derive(Debug, PartialEq, Eq)]
pub struct ArgumentError;

impl Display for ArgumentError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(
            "Expected input path(s). Usage: payment-engine <transactions.csv> [more.csv ...]",
        )
    }
}

impl Error for ArgumentError {}
