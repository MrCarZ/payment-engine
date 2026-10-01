//! Bounded execution of validated transport-independent source snapshots.
use super::ValidatedBatch;
use crate::manager::{
    observability::{TraceError, TraceService},
    payment::{
        SourceContext,
        run::{
            Coordinator, Output, Record, RunError, RunFailure as SourceFailure, Summary, finish_as,
        },
        trace::State,
    },
};
use std::{
    collections::VecDeque,
    convert::Infallible,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    thread::{Builder, scope},
};
mod error;
pub use error::{ExecutionError, Failure};

#[derive(Debug)]
pub struct SourceReport<R, O> {
    pub source: SourceContext,
    pub summary: Summary,
    pub state: State,
    pub error: Option<RunError<R, Infallible, O>>,
}
#[derive(Debug)]
pub struct Report<R, O> {
    pub summary: Summary,
    pub sources: Vec<SourceReport<R, O>>,
}
impl<R, O> Default for Report<R, O> {
    fn default() -> Self {
        Self {
            summary: Summary::default(),
            sources: Vec::new(),
        }
    }
}
struct Job<R, T> {
    index: usize,
    source: SourceContext,
    records: Vec<R>,
    trace: T,
}
struct Completed<R, T, O> {
    index: usize,
    source: SourceContext,
    coordinator: Coordinator,
    trace: Option<T>,
    result: Result<(), SourceFailure<R, Infallible, O>>,
}
/// Constructs one sink per source before starting bounded groups of workers.
/// A failed group prevents later groups from starting. In-flight peers finish.
pub fn run<R: Record + Send, T: TraceService + Send, O: Output>(
    batch: ValidatedBatch<R>,
    output: &mut O,
    workers: NonZeroUsize,
    mut trace_factory: impl FnMut(&SourceContext) -> Result<T, TraceError>,
) -> Result<Report<R, O::Error>, ExecutionError<R, O::Error>>
where
    O::Error: Send,
{
    let mut pending: VecDeque<Job<R, T>> = VecDeque::new();
    for (index, source) in batch.into_sources().into_iter().enumerate() {
        let (source, records) = source.into_parts();
        let trace = match trace_factory(&source) {
            Ok(trace) => trace,
            Err(error) => {
                let mut report = Report::default();
                let mut additional_trace_errors = Vec::new();
                for mut job in pending {
                    let failure: RunError<R, Infallible, O::Error> = finish_as(
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
                return Err(ExecutionError {
                    failure: Box::new(Failure::TraceSetup { source, error }),
                    report,
                    additional_trace_errors,
                });
            }
        };
        pending.push_back(Job {
            index,
            source,
            records,
            trace,
        });
    }
    let mut completed = Vec::new();
    while !pending.is_empty() {
        let mut group = scope(|scope| {
            let mut handles = Vec::new();
            let mut results = Vec::new();
            for _ in 0..workers.get() {
                let Some(job) = pending.pop_front() else {
                    break;
                };
                let index = job.index;
                let source = job.source.clone();
                match Builder::new()
                    .name(format!("payment-source-{index}"))
                    .spawn_scoped(scope, move || process(job))
                {
                    Ok(handle) => handles.push((index, source, handle)),
                    Err(error) => {
                        results.push(Completed {
                            index,
                            source,
                            coordinator: Coordinator::new(),
                            trace: None,
                            result: Err(SourceFailure::WorkerSpawn(error)),
                        });
                        break;
                    }
                }
            }
            for (index, source, handle) in handles {
                results.push(match handle.join() {
                    Ok(result) => result,
                    Err(_) => Completed {
                        index,
                        source,
                        coordinator: Coordinator::new(),
                        trace: None,
                        result: Err(SourceFailure::WorkerPanicked),
                    },
                });
            }
            results
        });
        let failed = group.iter().any(|source| source.result.is_err());
        completed.append(&mut group);
        if failed {
            break;
        }
    }
    for job in pending {
        completed.push(Completed {
            index: job.index,
            source: job.source,
            coordinator: Coordinator::new(),
            trace: Some(job.trace),
            result: Err(SourceFailure::Cancelled),
        });
    }
    completed.sort_by_key(|source| source.index);
    let processing_failed = completed.iter().any(|source| source.result.is_err());
    let output_result = if processing_failed {
        Ok(())
    } else {
        output.publish(
            completed
                .iter()
                .flat_map(|source| source.coordinator.manager().accounts()),
        )
    };
    let publication_failed = output_result.is_err();
    let mut report = Report::default();
    for mut source in completed {
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
        add_summary(&mut report.summary, summary);
        report.sources.push(SourceReport {
            source: source.source,
            summary,
            state,
            error: result.err(),
        });
    }
    let failure = match output_result {
        Err(error) => Some(Failure::Output(error)),
        Ok(()) if report.sources.iter().any(|source| source.error.is_some()) => {
            Some(Failure::SourcesFailed)
        }
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

fn process<R: Record, T: TraceService, O>(mut job: Job<R, T>) -> Completed<R, T, O> {
    let mut coordinator = Coordinator::new();
    let result = match catch_unwind(AssertUnwindSafe(|| {
        coordinator
            .process(
                job.records.into_iter().map(Ok::<_, Infallible>),
                &mut job.trace,
            )
            .map_err(SourceFailure::Coordinator)
            .and_then(|()| job.trace.flush().map_err(SourceFailure::Trace))
    })) {
        Ok(result) => result,
        Err(_) => Err(SourceFailure::WorkerPanicked),
    };
    Completed {
        index: job.index,
        source: job.source,
        coordinator,
        trace: Some(job.trace),
        result,
    }
}

fn add_summary(total: &mut Summary, source: Summary) {
    total.applied += source.applied;
    total.ignored += source.ignored;
    total.rejected += source.rejected;
    total.replayed += source.replayed;
    total.input_errors += source.input_errors;
    total.processing_errors += source.processing_errors;
}

#[cfg(test)]
mod tests;
