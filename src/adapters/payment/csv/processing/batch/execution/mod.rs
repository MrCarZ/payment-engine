//! Bounded concurrent source execution with isolated state and trace sinks.

use std::{
    collections::HashMap,
    fs::File,
    io::Write,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

use crate::{
    adapters::payment::csv::{
        input::Record,
        output::{Output as CsvOutput, OutputError},
    },
    adapters::{
        artifacts::identity::new_run_id,
        cli::config::BatchConfig,
        payment::csv::processing::{
            Failure as SourceFailure, RunError,
            setup::{create_run_directory, create_trace, source_id},
        },
    },
    manager::{
        observability::{TraceError, TraceService},
        payment::{
            SourceContext,
            batch::{
                ExecutionError as ManagerExecutionError, Failure as ManagerFailure,
                Report as ManagerReport, ValidatedBatch, run as run_batch,
            },
            run::Summary,
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

/// CSV composition around the manager-owned batch lifecycle.
pub fn run<T: TraceService + Send>(
    batch: ValidatedBatch<Record>,
    output: impl Write,
    workers: NonZeroUsize,
    trace_factory: impl FnMut(&SourceContext) -> Result<T, TraceError>,
) -> Result<Report, ExecutionError> {
    run_batch(batch, &mut CsvOutput::new(output), workers, trace_factory)
        .map(report_from_manager)
        .map_err(error_from_manager)
}

fn report_from_manager(report: ManagerReport<Record, OutputError>) -> Report {
    Report {
        summary: report.summary,
        sources: report
            .sources
            .into_iter()
            .map(|source| SourceReport {
                source: source.source,
                summary: source.summary,
                state: source.state,
                error: source
                    .error
                    .map(|error| RunError::from_execution(error, |never| match never {})),
            })
            .collect(),
    }
}

fn error_from_manager(error: ManagerExecutionError<Record, OutputError>) -> ExecutionError {
    let failure = match *error.failure {
        ManagerFailure::TraceSetup { source, error } => Failure::TraceSetup { source, error },
        ManagerFailure::SourcesFailed => Failure::SourcesFailed,
        ManagerFailure::Output(error) => Failure::Output(error),
    };
    ExecutionError {
        failure: Box::new(failure),
        report: report_from_manager(error.report),
        additional_trace_errors: error.additional_trace_errors,
    }
}

#[cfg(test)]
mod tests;
