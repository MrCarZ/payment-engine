use super::super::{Failure as CoordinatorFailure, Summary};
use crate::manager::observability::TraceError;
use std::{
    error::Error,
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    io::Error as IoError,
};

#[derive(Debug)]
pub enum Failure<R, E, O> {
    Coordinator(CoordinatorFailure<R, E>),
    Output(O),
    Trace(TraceError),
    Cancelled,
    WorkerPanicked,
    WorkerSpawn(IoError),
}

#[derive(Debug)]
pub struct RunError<R, E, O> {
    pub failure: Box<Failure<R, E, O>>,
    pub summary: Summary,
    pub additional_trace_errors: Vec<TraceError>,
}
impl<R, E, O> RunError<R, E, O> {
    pub(crate) fn new(failure: Failure<R, E, O>, summary: Summary) -> Self {
        Self {
            failure: Box::new(failure),
            summary,
            additional_trace_errors: Vec::new(),
        }
    }
}
impl<R, E: Display, O: Display> Display for Failure<R, E, O> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Coordinator(error) => error.fmt(f),
            Self::Output(error) => write!(f, "account output failed: {error}"),
            Self::Trace(error) => write!(f, "{error}"),
            Self::Cancelled => f.write_str("source skipped after batch failure"),
            Self::WorkerPanicked => f.write_str("payment worker panicked"),
            Self::WorkerSpawn(error) => write!(f, "cannot start payment worker: {error}"),
        }
    }
}
impl<R: Debug + 'static, E: Error + 'static, O: Error + 'static> Error for Failure<R, E, O> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Coordinator(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Trace(error) => Some(error),
            Self::WorkerSpawn(error) => Some(error),
            Self::Cancelled | Self::WorkerPanicked => None,
        }
    }
}
impl<R, E: Display, O: Display> Display for RunError<R, E, O> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.failure.fmt(f)?;
        for error in &self.additional_trace_errors {
            write!(f, "; additionally: {error}")?;
        }
        Ok(())
    }
}
impl<R: Debug + 'static, E: Error + 'static, O: Error + 'static> Error for RunError<R, E, O> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.failure.as_ref())
    }
}
