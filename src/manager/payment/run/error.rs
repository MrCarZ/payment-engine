use std::{
    error::Error,
    fmt::{Debug, Display, Formatter, Result as FmtResult},
};

use crate::manager::{observability::TraceError, payment::ProcessingError};

/// Preserves adapter-owned input errors and envelopes without depending on
/// their concrete transport. Trace errors never replace the primary failure.
#[derive(Debug)]
pub enum Failure<R, E> {
    Input {
        error: E,
        trace_error: Option<TraceError>,
    },
    Processing {
        record: R,
        error: ProcessingError,
        trace_error: Option<TraceError>,
    },
    Trace(TraceError),
}

impl<R, E: Display> Display for Failure<R, E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let trace_error = match self {
            Self::Input { error, trace_error } => {
                write!(f, "input failed: {error}")?;
                trace_error
            }
            Self::Processing {
                error, trace_error, ..
            } => {
                write!(f, "processing failed: {error}")?;
                trace_error
            }
            Self::Trace(error) => return write!(f, "{error}"),
        };
        if let Some(error) = trace_error {
            write!(f, "; additionally: {error}")?;
        }
        Ok(())
    }
}

impl<R: Debug, E: Error + 'static> Error for Failure<R, E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input { error, .. } => Some(error),
            Self::Processing { error, .. } => Some(error),
            Self::Trace(error) => Some(error),
        }
    }
}
