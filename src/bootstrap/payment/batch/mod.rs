//! Parse CSV sources into immutable snapshots, then validate before execution.

use std::io::Read;

use crate::{
    adapters::payment::csv::input::{Input, Record},
    manager::payment::{
        SourceContext,
        batch::{Source, ValidatedBatch},
    },
};

mod error;
pub use error::BatchError;

/// Every source is fully parsed and validated before returning. Input errors
/// stop preflight; no accounts, output, trace files, or worker threads are created.
pub fn validate<R: Read>(
    inputs: impl IntoIterator<Item = (R, SourceContext)>,
) -> Result<ValidatedBatch<Record>, BatchError> {
    let mut sources = Vec::new();
    for (reader, context) in inputs {
        let input = Input::new(reader, context.clone()).map_err(BatchError::Input)?;
        let records = input
            .collect::<Result<Vec<_>, _>>()
            .map_err(BatchError::Input)?;
        sources.push(Source::new(context, records));
    }
    ValidatedBatch::try_from(sources).map_err(BatchError::Contract)
}

#[cfg(test)]
mod tests;
