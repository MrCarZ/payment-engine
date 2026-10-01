use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    io::Error as IoError,
    path::PathBuf,
};

use crate::{
    adapters::payment::csv::{
        RecordPosition,
        input::{InputError, Record},
        output::OutputError,
    },
    manager::{
        observability::TraceError,
        payment::{
            Context, ProcessingError,
            run::{Failure as CoordinatorFailure, Summary},
        },
    },
};

#[derive(Debug)]
pub enum Failure {
    File {
        operation: &'static str,
        path: PathBuf,
        error: IoError,
    },
    Input(InputError),
    Processing {
        context: Context,
        position: RecordPosition,
        error: ProcessingError,
    },
    Output(OutputError),
    Trace(TraceError),
    WithTrace {
        failure: Box<Failure>,
        trace_error: TraceError,
    },
}

impl From<CoordinatorFailure<Record, InputError>> for Failure {
    fn from(failure: CoordinatorFailure<Record, InputError>) -> Self {
        let (failure, trace_error) = match failure {
            CoordinatorFailure::Input { error, trace_error } => (Self::Input(error), trace_error),
            CoordinatorFailure::Processing {
                record,
                error,
                trace_error,
            } => (
                Self::Processing {
                    context: record.context,
                    position: record.position,
                    error,
                },
                trace_error,
            ),
            CoordinatorFailure::Trace(error) => return Self::Trace(error),
        };
        match trace_error {
            Some(trace_error) => Self::WithTrace {
                failure: Box::new(failure),
                trace_error,
            },
            None => failure,
        }
    }
}

impl Display for Failure {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::File {
                operation,
                path,
                error,
            } => write!(f, "cannot {operation} {}: {error}", path.display()),
            Self::Input(error) => write!(f, "input failed: {error}"),
            Self::Processing {
                context,
                position,
                error,
            } => write!(
                f,
                "processing source {} at record {}, line {}, byte {} failed: {error}",
                context.source.source_id, position.record, position.line, position.byte
            ),
            Self::Output(error) => write!(f, "account output failed: {error}"),
            Self::Trace(error) => error.fmt(f),
            Self::WithTrace {
                failure,
                trace_error,
            } => write!(f, "{failure}; additionally: {trace_error}"),
        }
    }
}

impl Error for Failure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::File { error, .. } => Some(error),
            Self::Input(error) => Some(error),
            Self::Processing { error, .. } => Some(error),
            Self::Output(error) => Some(error),
            Self::Trace(error) => Some(error),
            Self::WithTrace { failure, .. } => Some(failure.as_ref()),
        }
    }
}

/// Preserves the primary failure, completed work counts, and final trace errors.
#[derive(Debug)]
pub struct RunError {
    pub failure: Box<Failure>,
    pub summary: Summary,
    pub additional_trace_errors: Vec<TraceError>,
    pub trace_path: Option<PathBuf>,
}

impl RunError {
    pub(super) fn new(failure: Failure, summary: Summary) -> Self {
        Self {
            failure: Box::new(failure),
            summary,
            additional_trace_errors: Vec::new(),
            trace_path: None,
        }
    }
}

impl Display for RunError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.failure.fmt(f)?;
        for error in &self.additional_trace_errors {
            write!(f, "; additionally: {error}")?;
        }
        if let Some(path) = &self.trace_path {
            write!(f, "; trace log: {}", path.display())?;
        }
        Ok(())
    }
}

impl Error for RunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.failure.as_ref())
    }
}
