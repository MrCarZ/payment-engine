use std::{
    fs::{File, OpenOptions},
    path::PathBuf,
    process::id,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    adapters::{clock::SystemClock, observability::csv::CsvTraceService},
    bootstrap::config::InputConfig,
    domain::clock::Clock,
    manager::{
        observability::TraceError,
        payment::{SourceContext, run::Summary},
    },
};

use super::{Failure, RunError};

static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) struct Resources {
    pub input: File,
    pub trace: CsvTraceService<File>,
    pub source: SourceContext,
    pub trace_path: PathBuf,
}

/// Constructs resources before processing, preserving non-Unicode paths and
/// exclusively creating the trace sidecar without overwriting existing files.
pub(super) fn prepare(config: InputConfig) -> Result<Resources, RunError> {
    let input = File::open(&config.input_path).map_err(|error| {
        RunError::new(
            Failure::File {
                operation: "open input",
                path: config.input_path.clone(),
                error,
            },
            Summary::default(),
        )
    })?;
    let run_id = format!(
        "{}-{}-{}",
        SystemClock.now().unix_timestamp_nanos(),
        id(),
        RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let mut name = config.input_path.as_os_str().to_os_string();
    name.push(format!(".{run_id}.trace.csv"));
    let trace_path = PathBuf::from(name);
    let trace_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&trace_path)
        .map_err(|error| {
            RunError::new(
                Failure::File {
                    operation: "create trace",
                    path: trace_path.clone(),
                    error,
                },
                Summary::default(),
            )
        })?;
    let trace = CsvTraceService::new(trace_file).map_err(|error| {
        let mut failure = RunError::new(Failure::Trace(TraceError::new(error)), Summary::default());
        failure.trace_path = Some(trace_path.clone());
        failure
    })?;
    let source = SourceContext {
        run_id,
        source_id: config.input_path.to_string_lossy().into_owned(),
        partner_id: None,
    };
    Ok(Resources {
        input,
        trace,
        source,
        trace_path,
    })
}
