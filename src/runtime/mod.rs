//! Execution configuration, component wiring, and lifecycle ownership.

use std::{ffi::OsString, path::PathBuf};

mod error;

pub use error::ArgumentError;

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
