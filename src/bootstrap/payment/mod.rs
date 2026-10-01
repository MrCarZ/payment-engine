//! Payment composition, account publication, and trace lifecycle.

use std::{
    io::{Read, Write},
    iter::once,
    path::{Path, PathBuf},
};

use crate::{
    adapters::payment::csv::{
        input::{Input, Record},
        output::write as write_accounts,
    },
    bootstrap::config::InputConfig,
    manager::{
        observability::TraceService,
        payment::{
            SourceContext,
            run::{Coordinator, Summary},
            trace::{State, run_finished},
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
    let mut coordinator = Coordinator::new();
    let result = match Input::new(input, source.clone()) {
        Ok(input) => coordinator.process(input, trace),
        Err(error) => coordinator.process(once(Err::<Record, _>(error)), trace),
    }
    .map_err(Failure::from)
    .and_then(|()| trace.flush().map_err(Failure::Trace))
    .and_then(|()| {
        write_accounts(output, coordinator.manager().accounts()).map_err(Failure::Output)
    });
    finish(&source, coordinator.summary(), result, trace)
}

fn finish(
    source: &SourceContext,
    summary: Summary,
    result: Result<(), Failure>,
    trace: &mut dyn TraceService,
) -> Result<Summary, RunError> {
    let state = if result.is_ok() {
        State::Completed
    } else {
        State::Failed
    };
    finish_as(source, summary, state, result, trace)
}

fn finish_as(
    source: &SourceContext,
    summary: Summary,
    state: State,
    result: Result<(), Failure>,
    trace: &mut dyn TraceService,
) -> Result<Summary, RunError> {
    let mut failure = result.err().map(|failure| RunError::new(failure, summary));
    // Always attempt summary delivery and explicit flush, preserving every error.
    for result in [
        trace.emit(run_finished(source, summary, state)),
        trace.flush(),
    ] {
        if let Err(error) = result {
            match &mut failure {
                Some(failure) => failure.additional_trace_errors.push(error),
                None => failure = Some(RunError::new(Failure::Trace(error), summary)),
            }
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(summary),
    }
}

#[cfg(test)]
mod tests;
