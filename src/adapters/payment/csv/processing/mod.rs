//! Payment composition, account publication, and trace lifecycle.

use std::{
    io::{Read, Write},
    iter::once,
    path::{Path, PathBuf},
};

use crate::{
    adapters::cli::config::InputConfig,
    adapters::payment::csv::{
        input::{Input, Record},
        output::Output as CsvOutput,
    },
    manager::{
        observability::TraceService,
        payment::{
            SourceContext,
            run::{Summary, run as run_records},
        },
    },
};

pub mod batch;
mod error;
pub(crate) mod setup;

pub use error::{Failure, RunError};
use setup::{create_run_directory, new_run_id, prepare};

#[derive(Debug)]
pub struct Execution {
    pub summary: Summary,
    pub trace_path: PathBuf,
}

/// Constructs resources, then connects the CSV adapters and payment coordinator.
pub fn execute(config: InputConfig, output: impl Write) -> Result<Execution, RunError> {
    let run_id = new_run_id();
    let directory = create_run_directory(Path::new("output"), &run_id)?;
    execute_at(config, output, &directory, &run_id)
}

pub(crate) fn execute_at(
    config: InputConfig,
    output: impl Write,
    directory: &Path,
    run_id: &str,
) -> Result<Execution, RunError> {
    let resources = prepare(config, directory, run_id)?;
    let mut trace = resources.trace;
    let summary =
        run(resources.input, output, resources.source, &mut trace).map_err(|mut error| {
            error.trace_path = Some(resources.trace_path.clone());
            error
        })?;
    Ok(Execution {
        summary,
        trace_path: resources.trace_path,
    })
}

/// Publishes accounts only after successful processing and request trace flush.
/// Final summary delivery and flushing are attempted on success and failure.
pub fn run(
    input: impl Read,
    output: impl Write,
    source: SourceContext,
    trace: &mut dyn TraceService,
) -> Result<Summary, RunError> {
    let mut output = CsvOutput::new(output);
    match Input::new(input, source.clone()) {
        Ok(input) => run_records(input, &source, &mut output, trace),
        Err(error) => run_records(once(Err::<Record, _>(error)), &source, &mut output, trace),
    }
    .map_err(|error| RunError::from_execution(error, |error| error))
}

#[cfg(test)]
mod tests;
