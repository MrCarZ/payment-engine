//! Bounded concurrent source execution with isolated state and trace sinks.

use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    io::Write,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    thread::{Builder, scope},
};

use crate::{
    adapters::payment::csv::{
        input::{InputError, Record},
        output::write as write_accounts,
    },
    bootstrap::{
        config::BatchConfig,
        payment::{
            Failure as SourceFailure, RunError, finish_as,
            setup::{create_run_directory, create_trace, new_run_id, source_id},
        },
    },
    manager::{
        observability::{TraceError, TraceService},
        payment::{
            SourceContext,
            batch::ValidatedBatch,
            run::{Coordinator, Summary},
            trace::State,
        },
    },
};

use super::{BatchError, validate};

mod error;
pub use error::{ExecutionError, Failure};

#[derive(Debug)]
pub struct SourceReport {
    pub source: SourceContext,
    pub summary: Summary,
    pub state: State,
    pub error: Option<RunError>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub summary: Summary,
    pub sources: Vec<SourceReport>,
}

#[derive(Debug)]
pub struct Execution {
    pub report: Report,
    pub trace_paths: Vec<PathBuf>,
}

struct Job<T> {
    index: usize,
    source: SourceContext,
    records: Vec<Record>,
    trace: T,
}

struct Completed<T> {
    index: usize,
    source: SourceContext,
    coordinator: Coordinator,
    trace: Option<T>,
    result: Result<(), SourceFailure>,
}

/// Validates all inputs before creating trace files or processing payments.
pub fn execute(config: BatchConfig, output: impl Write) -> Result<Execution, BatchError> {
    let run_id = new_run_id();
    let directory = create_run_directory(Path::new("output"), &run_id)
        .map_err(|error| BatchError::Setup(Box::new(error)))?;
    execute_at(config, output, &directory, &run_id)
}

pub(crate) fn execute_at(
    config: BatchConfig,
    output: impl Write,
    directory: &Path,
    run_id: &str,
) -> Result<Execution, BatchError> {
    let mut inputs = Vec::new();
    let mut paths = HashMap::new();
    for path in &config.input_paths {
        let input = File::open(path).map_err(|error| {
            BatchError::Setup(Box::new(RunError::new(
                SourceFailure::File {
                    operation: "open input",
                    path: path.clone(),
                    error,
                },
                Summary::default(),
            )))
        })?;
        // Canonical source identities reject alternate paths to the same file.
        let canonical = path.canonicalize().map_err(|error| {
            BatchError::Setup(Box::new(RunError::new(
                SourceFailure::File {
                    operation: "identify input",
                    path: path.clone(),
                    error,
                },
                Summary::default(),
            )))
        })?;
        let source_id = source_id(&canonical);
        paths.insert(source_id.clone(), inputs.len() + 1);
        inputs.push((
            input,
            SourceContext {
                run_id: run_id.into(),
                source_id,
                partner_id: None,
            },
        ));
    }
    let batch = validate(inputs)?;
    let mut trace_paths = Vec::new();
    let report = run(batch, output, config.workers, |source| {
        let path = paths
            .get(&source.source_id)
            .expect("validated source has its native path");
        let (trace, path) = create_trace(directory, *path).map_err(TraceError::new)?;
        trace_paths.push(path);
        Ok(trace)
    })
    .map_err(|mut error| {
        for source in &mut error.report.sources {
            if let Some(failure) = &mut source.error {
                let index = paths
                    .get(&source.source.source_id)
                    .expect("source has its trace index");
                failure.trace_path = Some(directory.join(format!("source-{index:04}.trace.csv")));
            }
        }
        BatchError::Execution(Box::new(error))
    })?;
    Ok(Execution {
        report,
        trace_paths,
    })
}

/// Constructs one sink per source before starting bounded groups of workers.
/// A failed group prevents later groups from starting. In-flight peers finish.
pub fn run<T: TraceService + Send>(
    batch: ValidatedBatch<Record>,
    output: impl Write,
    workers: NonZeroUsize,
    mut trace_factory: impl FnMut(&SourceContext) -> Result<T, TraceError>,
) -> Result<Report, ExecutionError> {
    let mut pending: VecDeque<Job<T>> = VecDeque::new();
    for (index, source) in batch.into_sources().into_iter().enumerate() {
        let (source, records) = source.into_parts();
        let trace = match trace_factory(&source) {
            Ok(trace) => trace,
            Err(error) => {
                let mut report = Report::default();
                let mut additional_trace_errors = Vec::new();
                for mut job in pending {
                    let failure = finish_as(
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
        write_accounts(
            output,
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

fn process<T: TraceService>(mut job: Job<T>) -> Completed<T> {
    let mut coordinator = Coordinator::new();
    let result = match catch_unwind(AssertUnwindSafe(|| {
        coordinator
            .process(
                job.records.into_iter().map(Ok::<_, InputError>),
                &mut job.trace,
            )
            .map_err(SourceFailure::from)
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
