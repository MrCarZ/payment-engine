//! Per-invocation output storage, independent of payment business rules.

use std::{
    fs::{File, OpenOptions, read_dir, rename},
    io::{BufWriter, Result as IoResult, Write},
    path::PathBuf,
    time::Instant,
};

use serde_json::{Value, json, to_writer_pretty};

use crate::{
    bootstrap::{
        config::{Config, Invocation},
        payment::{
            batch::{BatchError, SourceReport, execute_at as execute_batch},
            execute_at,
            setup::{create_run_directory, new_run_id},
        },
    },
    manager::payment::{run::Summary, trace::State},
};

mod error;
pub use error::ArtifactError;

#[derive(Debug)]
pub struct Execution {
    pub directory: PathBuf,
    pub trace_paths: Vec<PathBuf>,
}

struct Tee<W> {
    file: BufWriter<File>,
    output: W,
}

impl<W: Write> Write for Tee<W> {
    fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
        self.file.write_all(bytes)?;
        self.output.write_all(bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> IoResult<()> {
        self.file.flush()?;
        self.output.flush()
    }
}

/// Keeps stdout account output while saving a copy and reports under a fresh
/// run directory. Failed runs retain partial output without publishing accounts.csv.
pub fn execute(invocation: Invocation, output: impl Write) -> Result<Execution, ArtifactError> {
    let started = Instant::now();
    let run_id = new_run_id();
    let directory =
        create_run_directory(&invocation.output_root, &run_id).map_err(ArtifactError::from)?;
    let partial = directory.join("accounts.partial.csv");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(ArtifactError::from)?;
    let mut tee = Tee {
        file: BufWriter::new(file),
        output,
    };
    let input_paths: Vec<_> = match &invocation.config {
        Config::Single(config) => vec![config.input_path.clone()],
        Config::Batch(config) => config.input_paths.clone(),
    };
    let (mut result, summary, sources) = match invocation.config {
        Config::Single(config) => match execute_at(config, &mut tee, &directory, &run_id) {
            Ok(execution) => (Ok(()), execution.summary, json!([])),
            Err(error) => {
                let summary = error.summary;
                (Err(ArtifactError::from(error)), summary, json!([]))
            }
        },
        Config::Batch(config) => match execute_batch(config, &mut tee, &directory, &run_id) {
            Ok(execution) => (
                Ok(()),
                execution.report.summary,
                source_reports(&execution.report.sources),
            ),
            Err(error) => {
                let (summary, sources) = match &error {
                    BatchError::Execution(error) => {
                        (error.report.summary, source_reports(&error.report.sources))
                    }
                    _ => (Summary::default(), json!([])),
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
        && let Err(error) = rename(&partial, &accounts)
    {
        result = Err(ArtifactError::from(error));
    }
    let mut traces: Vec<_> = read_dir(&directory)
        .map_err(ArtifactError::from)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(ArtifactError::from)?;
    traces.retain(|path| path.to_string_lossy().ends_with(".trace.csv"));
    traces.sort();
    let report = json!({
        "run_id": run_id,
        "exit_code": if result.is_ok() { 0 } else { 1 },
        "status": if result.is_ok() { "completed" } else { "failed" },
        "elapsed_seconds": started.elapsed().as_secs_f64(),
        "input_paths": input_paths.iter().map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "accounts_path": if result.is_ok() { Some(accounts.to_string_lossy().into_owned()) } else { None },
        "partial_accounts_path": if result.is_err() { Some(partial.to_string_lossy().into_owned()) } else { None },
        "trace_paths": traces.iter().map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "summary": summary_value(summary),
        "sources": sources,
        "error": result.as_ref().err().map(ToString::to_string),
    });
    let persist = (|| -> Result<(), ArtifactError> {
        let mut report_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("report.json"))?;
        to_writer_pretty(&mut report_file, &report)?;
        report_file.write_all(b"\n")?;
        report_file.flush()?;
        let mut diagnostics = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("diagnostics.log"))?;
        writeln!(
            diagnostics,
            "Status: {}",
            report["status"].as_str().unwrap_or("failed")
        )?;
        for path in &traces {
            writeln!(diagnostics, "Trace log: {}", path.display())?;
        }
        if let Some(error) = result.as_ref().err() {
            writeln!(diagnostics, "{error}")?;
        }
        diagnostics.flush()?;
        Ok(())
    })();
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

fn source_reports(sources: &[SourceReport]) -> Value {
    Value::Array(sources.iter().map(|source| json!({
        "source_id": source.source.source_id,
        "status": match source.state { State::Completed => "completed", State::Failed => "failed" },
        "summary": summary_value(source.summary),
        "error": source.error.as_ref().map(ToString::to_string),
    })).collect())
}

fn summary_value(summary: Summary) -> Value {
    json!({ "applied": summary.applied, "ignored": summary.ignored, "rejected": summary.rejected, "replayed": summary.replayed, "input_errors": summary.input_errors, "processing_errors": summary.processing_errors })
}
