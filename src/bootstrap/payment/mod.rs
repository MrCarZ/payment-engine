//! Payment composition, account publication, and trace lifecycle.

use std::{
    io::{Read, Write},
    iter::once,
    path::PathBuf,
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
mod setup;

pub use error::{Failure, RunError};
use setup::prepare;

#[derive(Debug)]
pub struct Execution {
    pub summary: Summary,
    pub trace_path: PathBuf,
}

/// Constructs resources, then connects the CSV adapters and payment coordinator.
pub fn execute(config: InputConfig, output: impl Write) -> Result<Execution, RunError> {
    let resources = prepare(config)?;
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
