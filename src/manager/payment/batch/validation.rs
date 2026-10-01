use super::{Location, Source, ValidationError};
use crate::{
    domain::payment::{ClientId, TransactionId},
    manager::payment::{Request, SourceContext, run::Record},
};
use std::collections::{HashMap, HashSet};

struct Owner {
    source_index: usize,
    location: Location,
}
struct Reference {
    tx: TransactionId,
    source_index: usize,
    location: Location,
}
struct Validator<'a> {
    run_id: &'a str,
    source_ids: HashSet<&'a str>,
    clients: HashMap<ClientId, Owner>,
    originals: HashMap<TransactionId, Owner>,
    references: Vec<Reference>,
}

pub(super) fn validate<R: Record>(sources: &[Source<R>]) -> Result<(), ValidationError> {
    let first = sources.first().ok_or(ValidationError::EmptyBatch)?;
    let mut validator = Validator::new(&first.context().run_id);
    for (source_index, source) in sources.iter().enumerate() {
        validator.validate_source(source_index, source)?;
    }
    validator.validate_references()
}
impl<'a> Validator<'a> {
    fn new(run_id: &'a str) -> Self {
        Self {
            run_id,
            source_ids: HashSet::new(),
            clients: HashMap::new(),
            originals: HashMap::new(),
            references: Vec::new(),
        }
    }
    fn validate_source<R: Record>(
        &mut self,
        source_index: usize,
        source: &'a Source<R>,
    ) -> Result<(), ValidationError> {
        let context = source.context();
        self.validate_context(context)?;
        for (index, record) in source.records().iter().enumerate() {
            self.validate_record(source_index, index + 1, context, record)?;
        }
        Ok(())
    }
    fn validate_context(&mut self, context: &'a SourceContext) -> Result<(), ValidationError> {
        if context.run_id != self.run_id {
            return Err(ValidationError::RunMismatch {
                source_id: context.source_id.clone(),
                expected: self.run_id.into(),
                actual: context.run_id.clone(),
            });
        }
        if !self.source_ids.insert(&context.source_id) {
            return Err(ValidationError::DuplicateSource {
                source_id: context.source_id.clone(),
            });
        }
        Ok(())
    }
    fn validate_record<R: Record>(
        &mut self,
        source_index: usize,
        record_index: usize,
        context: &SourceContext,
        record: &R,
    ) -> Result<(), ValidationError> {
        let location = Location {
            source_id: context.source_id.clone(),
            record: record_index,
        };
        if record.context().source.as_ref() != context {
            return Err(ValidationError::ContextMismatch { location });
        }
        match record.request() {
            Request::Original { client, tx, .. } => {
                self.register_client(client, source_index, &location)?;
                self.register_original(tx, source_index, location)?;
            }
            Request::Lifecycle { client, tx, .. } => {
                self.register_client(client, source_index, &location)?;
                self.references.push(Reference {
                    tx,
                    source_index,
                    location,
                });
            }
        }
        Ok(())
    }
    fn register_client(
        &mut self,
        client: ClientId,
        source_index: usize,
        location: &Location,
    ) -> Result<(), ValidationError> {
        if let Some(owner) = self.clients.get(&client) {
            if owner.source_index != source_index {
                return Err(ValidationError::ClientOverlap {
                    client,
                    first: owner.location.clone(),
                    conflicting: location.clone(),
                });
            }
        } else {
            self.clients.insert(
                client,
                Owner {
                    source_index,
                    location: location.clone(),
                },
            );
        }
        Ok(())
    }
    fn register_original(
        &mut self,
        tx: TransactionId,
        source_index: usize,
        location: Location,
    ) -> Result<(), ValidationError> {
        if let Some(owner) = self.originals.get(&tx) {
            if owner.source_index != source_index {
                return Err(ValidationError::TransactionOverlap {
                    tx,
                    first: owner.location.clone(),
                    conflicting: location,
                });
            }
        } else {
            self.originals.insert(
                tx,
                Owner {
                    source_index,
                    location,
                },
            );
        }
        Ok(())
    }
    fn validate_references(self) -> Result<(), ValidationError> {
        // Collect all originals first so cross-source forward references behave
        // identically regardless of source order. Globally unknown IDs remain valid.
        for reference in self.references {
            if let Some(owner) = self.originals.get(&reference.tx)
                && owner.source_index != reference.source_index
            {
                return Err(ValidationError::CrossSourceReference {
                    tx: reference.tx,
                    original: owner.location.clone(),
                    reference: reference.location,
                });
            }
        }
        Ok(())
    }
}
