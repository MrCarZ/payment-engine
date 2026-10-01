use super::{ExecutionError, Failure, Report, SourceReport, ValidatedBatch, workers::Job};
use crate::manager::{
    observability::{TraceError, TraceService},
    payment::{
        SourceContext,
        run::{RunError, RunFailure as SourceFailure, Summary, finish_as},
        trace::State,
    },
};
use std::{collections::VecDeque, convert::Infallible};

pub(super) fn prepare<R, T: TraceService, O>(
    batch: ValidatedBatch<R>,
    mut trace_factory: impl FnMut(&SourceContext) -> Result<T, TraceError>,
) -> Result<VecDeque<Job<R, T>>, ExecutionError<R, O>> {
    let mut pending: VecDeque<Job<R, T>> = VecDeque::new();
    for (index, source) in batch.into_sources().into_iter().enumerate() {
        let (source, records) = source.into_parts();
        let trace = match trace_factory(&source) {
            Ok(trace) => trace,
            Err(error) => {
                return Err(cancel_prepared(pending, source, error));
            }
        };
        pending.push_back(Job {
            index,
            source,
            records,
            trace,
        });
    }
    Ok(pending)
}

fn cancel_prepared<R, T: TraceService, O>(
    pending: VecDeque<Job<R, T>>,
    source: SourceContext,
    error: TraceError,
) -> ExecutionError<R, O> {
    let mut report = Report::default();
    let mut additional_trace_errors = Vec::new();
    for mut job in pending {
        let failure: RunError<R, Infallible, O> = finish_as(
            &job.source,
            Summary::default(),
            State::Failed,
            Err(SourceFailure::Cancelled),
            &mut job.trace,
        )
        .unwrap_err();
        additional_trace_errors.extend(failure.additional_trace_errors);
        report.sources.push(SourceReport {
            source: job.source,
            summary: Summary::default(),
            state: State::Failed,
            error: Some(RunError::new(SourceFailure::Cancelled, Summary::default())),
        });
    }
    ExecutionError {
        failure: Box::new(Failure::TraceSetup { source, error }),
        report,
        additional_trace_errors,
    }
}
