//! Validation and execution of isolated payment source snapshots.
use super::run::Record;

mod error;
mod execution;
mod source;
mod validation;
pub use error::{Location, ValidationError};
pub use execution::{ExecutionError, Failure, Report, SourceReport, run};
pub use source::Source;
use validation::validate;

/// Immutable snapshots which passed the batch contract. No payment state has
/// been changed. Consume the batch to hand its sources to execution workers.
#[derive(Debug)]
pub struct ValidatedBatch<R> {
    sources: Vec<Source<R>>,
}

impl<R> ValidatedBatch<R> {
    pub fn sources(&self) -> &[Source<R>] {
        &self.sources
    }
    pub fn into_sources(self) -> Vec<Source<R>> {
        self.sources
    }
}

impl<R: Record> TryFrom<Vec<Source<R>>> for ValidatedBatch<R> {
    type Error = ValidationError;
    fn try_from(sources: Vec<Source<R>>) -> Result<Self, Self::Error> {
        validate(&sources)?;
        Ok(Self { sources })
    }
}

#[cfg(test)]
mod tests;
