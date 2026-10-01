//! Per-invocation output storage, independent of payment business rules.

use crate::{
    adapters::{
        artifacts::{
            filesystem::{Tee, file_name, publish, trace_files},
            persist,
            report::{Report, Status},
        },
        cli::config::{Config, Invocation},
        payment::csv::processing::{
            batch::{BatchError, execute_at as execute_batch},
            execute_at,
            setup::{create_run_directory, new_run_id},
        },
    },
    manager::payment::run::Summary,
};
use std::{io::Write, path::PathBuf, time::Instant};

mod error;
pub use error::ArtifactError;

#[derive(Debug)]
pub struct Execution {
    pub directory: PathBuf,
    pub trace_paths: Vec<PathBuf>,
}

/// Keeps stdout account output while saving a copy and reports under a fresh
/// run directory. Failed runs retain partial output without publishing accounts.csv.
pub fn execute(invocation: Invocation, output: impl Write) -> Result<Execution, ArtifactError> {
    let started = Instant::now();
    let run_id = new_run_id();
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
    let traces = trace_files(&directory).map_err(ArtifactError::from)?;
    let report = Report {
        run_id,
        exit_code: if result.is_ok() { 0 } else { 1 },
        status: if result.is_ok() {
            Status::Completed
        } else {
            Status::Failed
        },
        elapsed_seconds: started.elapsed().as_secs_f64(),
        input_files: Report::filenames(&input_paths),
        account_file: if result.is_ok() {
            Some(file_name(&accounts))
        } else {
            None
        },
        partial_account_file: if result.is_err() {
            Some(file_name(&partial))
        } else {
            None
        },
        trace_files: Report::filenames(&traces),
        summary: summary.into(),
        sources,
        error: result.as_ref().err().map(ToString::to_string),
    };
    let persist = persist(&directory, &report, &traces).map_err(ArtifactError::from);
    if let Err(error) = persist {
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

fn combine(result: Result<(), ArtifactError>, error: ArtifactError) -> Result<(), ArtifactError> {
    Err(match result {
        Ok(()) => error,
        Err(primary) => ArtifactError::Additional {
            primary: Box::new(primary),
            secondary: Box::new(error),
        },
    })
}
