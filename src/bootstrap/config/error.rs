use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

/// The CLI requires exactly one input-path argument.
#[derive(Debug, PartialEq, Eq)]
pub struct ArgumentError;

impl Display for ArgumentError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str("Expected exactly one input path. Usage: payment-engine <transactions.csv>")
    }
}

impl Error for ArgumentError {}
