//! CLI invocation composition: source adapters, account output, and run artifacts.

use crate::{
    adapters::{
        artifacts::{
            filesystem::{Tee, file_name, publish, trace_files},
            report::{Report, Status},
            write_diagnostics, write_report,
        },
        cli::config::{Config, Invocation},
        payment::csv::processing::{
            batch::{BatchError, execute_at as execute_batch},
            execute_at,
            setup::create_run_directory,
        },
    },
    manager::payment::run::Summary,
};
use std::{
    io::{Result as IoResult, Write},
    mem::replace,
    path::{Path, PathBuf},
    time::Instant,
};

mod error;
pub use error::ArtifactError;

#[derive(Debug)]
pub struct Execution {
    pub directory: PathBuf,
    pub trace_paths: Vec<PathBuf>,
}

/// Keeps stdout account output while saving a copy and reports under a fresh
/// run directory. Processing failures retain partial output. Later artifact
/// failures preserve already published accounts and return a failed run result.
pub(crate) fn run(
    invocation: Invocation,
    output: impl Write,
    run_id: String,
) -> Result<Execution, ArtifactError> {
    let started = Instant::now();
    let directory =
        create_run_directory(&invocation.output_root, &run_id).map_err(ArtifactError::from)?;
    let partial = directory.join("accounts.partial.csv");
    let mut tee = Tee::new(&partial, output).map_err(ArtifactError::from)?;
    let input_paths: Vec<_> = match &invocation.config {
        Config::Single(config) => vec![config.input_path.clone()],
        Config::Batch(config) => config.input_paths.clone(),
    };
    let (mut result, summary, sources) = match invocation.config {
        Config::Single(config) => match execute_at(config, &mut tee, &directory, &run_id) {
            Ok(execution) => (Ok(()), execution.summary, Vec::new()),
            Err(error) => {
                let summary = error.summary;
                (Err(ArtifactError::from(error)), summary, Vec::new())
            }
        },
        Config::Batch(config) => match execute_batch(config, &mut tee, &directory, &run_id) {
            Ok(execution) => (
                Ok(()),
                execution.report.summary,
                Report::sources(&execution.report.sources),
            ),
            Err(error) => {
                let (summary, sources) = match &error {
                    BatchError::Execution(error) => {
                        (error.report.summary, Report::sources(&error.report.sources))
                    }
                    _ => (Summary::default(), Vec::new()),
                };
                (Err(ArtifactError::from(error)), summary, sources)
            }
        },
    };
    if let Err(error) = tee.flush() {
        result = combine(result, ArtifactError::from(error));
    }
    drop(tee);
    let accounts = directory.join("accounts.csv");
    if result.is_ok()
        && let Err(error) = publish(&partial, &accounts)
    {
        result = Err(ArtifactError::from(error));
    }
    let published = result.is_ok();
    let traces = discover_traces(&directory, &mut result, trace_files);
    let mut report = Report {
        run_id,
        exit_code: if result.is_ok() { 0 } else { 1 },
        status: if result.is_ok() {
            Status::Completed
        } else {
            Status::Failed
        },
        elapsed_seconds: started.elapsed().as_secs_f64(),
        input_files: Report::filenames(&input_paths),
        account_file: if published {
            Some(file_name(&accounts))
        } else {
            None
        },
        partial_account_file: if !published {
            Some(file_name(&partial))
        } else {
            None
        },
        trace_files: Report::filenames(&traces),
        summary: summary.into(),
        sources,
        error: result.as_ref().err().map(ToString::to_string),
    };
    if let Err(error) = write_diagnostics(&directory, &report, &traces).map_err(ArtifactError::from)
    {
        result = combine(result, error);
    }
    report.status = if result.is_ok() {
        Status::Completed
    } else {
        Status::Failed
    };
    report.exit_code = if result.is_ok() { 0 } else { 1 };
    report.error = result.as_ref().err().map(ToString::to_string);
    if let Err(error) = write_report(&directory, &report).map_err(ArtifactError::from) {
        result = combine(result, error);
    }
    result.map_err(|error| ArtifactError::Run {
        directory: directory.clone(),
        error: Box::new(error),
    })?;
    Ok(Execution {
        directory,
        trace_paths: traces,
    })
}

fn discover_traces(
    directory: &Path,
    result: &mut Result<(), ArtifactError>,
    discover: impl FnOnce(&Path) -> IoResult<Vec<PathBuf>>,
) -> Vec<PathBuf> {
    match discover(directory) {
        Ok(paths) => paths,
        Err(error) => {
            let primary = replace(result, Ok(()));
            *result = combine(primary, ArtifactError::from(error));
            Vec::new()
        }
    }
}

fn combine(result: Result<(), ArtifactError>, error: ArtifactError) -> Result<(), ArtifactError> {
    Err(match result {
        Ok(()) => error,
        Err(primary) => ArtifactError::Additional {
            primary: Box::new(primary),
            secondary: Box::new(error),
        },
    })
}

#[cfg(test)]
mod tests;
