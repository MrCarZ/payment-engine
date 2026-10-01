//! Processing, account publication, and final trace delivery policy.
use super::{Coordinator, InputFailure, Output, Record, Summary};
use crate::manager::{
    observability::{TraceError, TraceService},
    payment::{
        SourceContext,
        trace::{State, run_finished},
    },
};
use std::{
    io::Error as IoError,
    panic::{AssertUnwindSafe, catch_unwind},
};
mod error;
pub use error::{Failure, RunError};

/// Processes ordered requests, flushes their traces, publishes accounts, then
/// finalizes summary delivery. Financial operations are never retried for I/O.
pub fn run<R: Record, E: InputFailure, O: Output>(
    records: impl IntoIterator<Item = Result<R, E>>,
    source: &SourceContext,
    output: &mut O,
    trace: &mut dyn TraceService,
) -> Result<Summary, RunError<R, E, O::Error>> {
    let mut coordinator = Coordinator::new();
    let result = coordinator
        .process(records, trace)
        .map_err(Failure::Coordinator)
        .and_then(|()| trace.flush().map_err(Failure::Trace))
        .and_then(|()| {
            output
                .publish(coordinator.manager().accounts())
                .map_err(Failure::Output)
        });
    let state = if result.is_ok() {
        State::Completed
    } else {
        State::Failed
    };
    finish_as(source, coordinator.summary(), state, result, trace)
}

pub(crate) fn finish_as<R, E, O>(
    source: &SourceContext,
    summary: Summary,
    state: State,
    result: Result<(), Failure<R, E, O>>,
    trace: &mut dyn TraceService,
) -> Result<Summary, RunError<R, E, O>> {
    let mut failure = result.err().map(|failure| RunError::new(failure, summary));
    for result in [
        deliver(|| trace.emit(run_finished(source, summary, state))),
        deliver(|| trace.flush()),
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

fn deliver(operation: impl FnOnce() -> Result<(), TraceError>) -> Result<(), TraceError> {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or_else(|_| {
        Err(TraceError::new(IoError::other(
            "trace finalization panicked",
        )))
    })
}

#[cfg(test)]
mod tests;
