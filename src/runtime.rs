//! Execution configuration, component wiring, and lifecycle ownership.

use std::{
    error::Error,
    ffi::OsString,
    fmt::{Display, Formatter, Result as FmtResult},
    path::PathBuf,
};

/// The required input path, retained without requiring Unicode filenames.
#[derive(Debug, PartialEq, Eq)]
pub struct InputConfig {
    pub input_path: PathBuf,
}

impl InputConfig {
    /// Parses arguments excluding the executable name.
    /// File opening and CSV processing belong to later execution phases.
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Result<Self, ArgumentError> {
        let mut args = args.into_iter();
        let input_path = args.next().ok_or(ArgumentError)?;
        if args.next().is_some() {
            return Err(ArgumentError);
        }
        Ok(Self {
            input_path: PathBuf::from(input_path),
        })
    }
}

/// The CLI requires exactly one input-path argument.
#[derive(Debug, PartialEq, Eq)]
pub struct ArgumentError;

impl Display for ArgumentError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str("Expected exactly one input path. Usage: payment-engine <transactions.csv>")
    }
}

impl Error for ArgumentError {}
