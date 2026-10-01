use std::{ffi::OsString, num::NonZeroUsize, path::PathBuf, thread::available_parallelism};

mod error;
pub use error::ArgumentError;

#[derive(Debug, PartialEq, Eq)]
pub struct BatchConfig {
    pub input_paths: Vec<PathBuf>,
    pub workers: NonZeroUsize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Config {
    Single(InputConfig),
    Batch(BatchConfig),
}

impl Config {
    /// One path retains streaming execution; multiple paths use batch preflight.
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Result<Self, ArgumentError> {
        let mut paths: Vec<_> = args.into_iter().map(PathBuf::from).collect();
        match paths.len() {
            0 => Err(ArgumentError),
            1 => Ok(Self::Single(InputConfig {
                input_path: paths.remove(0),
            })),
            _ => Ok(Self::Batch(BatchConfig {
                input_paths: paths,
                workers: available_parallelism().unwrap_or(NonZeroUsize::MIN),
            })),
        }
    }
}

/// The required input path, retained without requiring Unicode filenames.
#[derive(Debug, PartialEq, Eq)]
pub struct InputConfig {
    pub input_path: PathBuf,
}

impl InputConfig {
    /// Parses arguments excluding the executable name.
    /// File opening and processing are handled by execute.
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

#[cfg(test)]
mod tests;
