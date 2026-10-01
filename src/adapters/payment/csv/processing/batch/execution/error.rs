use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

use crate::{
    adapters::payment::csv::output::OutputError,
    manager::{observability::TraceError, payment::SourceContext},
};

use super::Report;

#[derive(Debug)]
pub enum Failure {
    TraceSetup {
        source: SourceContext,
        error: TraceError,
    },
    SourcesFailed,
    Output(OutputError),
}

#[derive(Debug)]
pub struct ExecutionError {
    pub failure: Box<Failure>,
    pub report: Report,
    pub additional_trace_errors: Vec<TraceError>,
}

impl Display for ExecutionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self.failure.as_ref() {
            Failure::TraceSetup { source, error } => write!(
                f,
                "trace setup for source {} failed: {error}",
                source.source_id
            )?,
            Failure::SourcesFailed => f.write_str("batch source processing failed")?,
            Failure::Output(error) => write!(f, "batch account output failed: {error}")?,
        }
        for source in &self.report.sources {
            if let Some(error) = &source.error {
                write!(f, "; source {}: {error}", source.source.source_id)?;
            }
        }
        for error in &self.additional_trace_errors {
            write!(f, "; additionally: {error}")?;
        }
        Ok(())
    }
}

impl Error for ExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self.failure.as_ref() {
            Failure::TraceSetup { error, .. } => Some(error),
            Failure::Output(error) => Some(error),
            Failure::SourcesFailed => self.report.sources.iter().find_map(|source| {
                source
                    .error
                    .as_ref()
                    .map(|error| error as &(dyn Error + 'static))
            }),
        }
    }
}
