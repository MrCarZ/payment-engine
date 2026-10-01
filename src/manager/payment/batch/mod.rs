//! Preflight validation for independently processed sources with disjoint clients.

use std::collections::HashMap;

use crate::domain::payment::{ClientId, TransactionId};

use super::{Request, SourceContext, run::Record};

mod error;
mod execution;
pub use error::{Location, ValidationError};
pub use execution::{ExecutionError, Failure, Report, SourceReport, run};

/// A complete, ordered source snapshot. Construction alone does not validate it.
#[derive(Debug)]
pub struct Source<R> {
    context: SourceContext,
    records: Vec<R>,
}

impl<R> Source<R> {
    pub fn new(context: SourceContext, records: Vec<R>) -> Self {
        Self { context, records }
    }
    pub fn context(&self) -> &SourceContext {
        &self.context
    }
    pub fn records(&self) -> &[R] {
        &self.records
    }
    pub fn into_parts(self) -> (SourceContext, Vec<R>) {
        (self.context, self.records)
    }
}

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
        let first = sources.first().ok_or(ValidationError::EmptyBatch)?;
        let run_id = &first.context.run_id;
        let mut source_ids = HashMap::new();
        let mut clients: HashMap<ClientId, (usize, Location)> = HashMap::new();
        let mut originals: HashMap<TransactionId, (usize, Location)> = HashMap::new();
        let mut references = Vec::new();
        for (source_index, source) in sources.iter().enumerate() {
            let context = &source.context;
            if context.run_id != *run_id {
                return Err(ValidationError::RunMismatch {
                    source_id: context.source_id.clone(),
                    expected: run_id.clone(),
                    actual: context.run_id.clone(),
                });
            }
            if source_ids
                .insert(&context.source_id, source_index)
                .is_some()
            {
                return Err(ValidationError::DuplicateSource {
                    source_id: context.source_id.clone(),
                });
            }
            for (index, record) in source.records.iter().enumerate() {
                let location = Location {
                    source_id: context.source_id.clone(),
                    record: index + 1,
                };
                if record.context().source.as_ref() != context {
                    return Err(ValidationError::ContextMismatch { location });
                }
                let (client, tx, original) = match record.request() {
                    Request::Original { client, tx, .. } => (client, tx, true),
                    Request::Lifecycle { client, tx, .. } => (client, tx, false),
                };
                if let Some((owner, first)) = clients.get(&client) {
                    if *owner != source_index {
                        return Err(ValidationError::ClientOverlap {
                            client,
                            first: first.clone(),
                            conflicting: location,
                        });
                    }
                } else {
                    clients.insert(client, (source_index, location.clone()));
                }
                if original {
                    if let Some((owner, first)) = originals.get(&tx) {
                        if *owner != source_index {
                            return Err(ValidationError::TransactionOverlap {
                                tx,
                                first: first.clone(),
                                conflicting: location,
                            });
                        }
                    } else {
                        originals.insert(tx, (source_index, location));
                    }
                } else {
                    references.push((tx, source_index, location));
                }
            }
        }
        // Check after scanning all originals, including forward references and
        // sources that appear later in the manifest. Unknown IDs stay unknown.
        for (tx, source_index, reference) in references {
            if let Some((owner, original)) = originals.get(&tx)
                && *owner != source_index
            {
                return Err(ValidationError::CrossSourceReference {
                    tx,
                    original: original.clone(),
                    reference,
                });
            }
        }
        Ok(Self { sources })
    }
}

#[cfg(test)]
mod tests;
