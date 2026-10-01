use super::{ExecutionError, Failure, Report, SourceReport, workers::Completed};
use crate::manager::{
    observability::TraceService,
    payment::{
        run::{Output, RunError, RunFailure as SourceFailure, finish_as},
        trace::State,
    },
};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(super) fn publish<R, T, O: Output>(
    completed: &[Completed<R, T, O::Error>],
    output: &mut O,
) -> Result<(), O::Error> {
    let processing_failed = completed.iter().any(|source| source.result.is_err());
    if processing_failed {
        Ok(())
    } else {
        output.publish(
            completed
                .iter()
                .flat_map(|source| source.coordinator.manager().accounts()),
        )
    }
}

pub(super) fn finalize<R, T: TraceService, O>(
    completed: Vec<Completed<R, T, O>>,
    publication_failed: bool,
) -> Report<R, O> {
    let mut report = Report::default();
    for source in completed {
        report.push(finalize_source(source, publication_failed));
    }
    report
}

pub(super) fn complete<R, O>(
    report: Report<R, O>,
    output_result: Result<(), O>,
) -> Result<Report<R, O>, ExecutionError<R, O>> {
    let failure = match output_result {
        Err(error) => Some(Failure::Output(error)),
        Ok(()) if report.has_failures() => Some(Failure::SourcesFailed),
        Ok(()) => None,
    };
    match failure {
        Some(failure) => Err(ExecutionError {
            failure: Box::new(failure),
            report,
            additional_trace_errors: Vec::new(),
        }),
        None => Ok(report),
    }
}

fn finalize_source<R, T: TraceService, O>(
    mut source: Completed<R, T, O>,
    publication_failed: bool,
) -> SourceReport<R, O> {
    let summary = source.coordinator.summary();
    let mut state = if source.result.is_ok() && !publication_failed {
        State::Completed
    } else {
        State::Failed
    };
    let result = match &mut source.trace {
        Some(trace) => match catch_unwind(AssertUnwindSafe(|| {
            finish_as(&source.source, summary, state, source.result, trace)
        })) {
            Ok(result) => result,
            Err(_) => Err(RunError::new(SourceFailure::WorkerPanicked, summary)),
        },
        None => Err(RunError::new(source.result.unwrap_err(), summary)),
    };
    if result.is_err() {
        state = State::Failed;
    }
    SourceReport {
        source: source.source,
        summary,
        state,
        error: result.err(),
    }
}
