use std::{
    fs::{File, OpenOptions, create_dir, create_dir_all},
    path::{Path, PathBuf},
    process::id,
    sync::atomic::{AtomicU64, Ordering},
};

use uuid::Uuid;

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
/// exclusively creating the trace file without overwriting existing files.
pub(super) fn prepare(
    config: InputConfig,
    directory: &Path,
    run_id: &str,
) -> Result<Resources, RunError> {
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
    let canonical = config.input_path.canonicalize().map_err(|error| {
        RunError::new(
            Failure::File {
                operation: "identify input",
                path: config.input_path.clone(),
                error,
            },
            Summary::default(),
        )
    })?;
    let (trace, trace_path) = create_trace(directory, 1)?;
    let source = SourceContext {
        run_id: run_id.into(),
        source_id: source_id(&canonical),
        partner_id: None,
    };
    Ok(Resources {
        input,
        trace,
        source,
        trace_path,
    })
}

/// Derives a stable identity from a canonical native path without publishing it.
/// Native encoding preserves non-Unicode paths; identities are platform-local.
pub(crate) fn source_id(canonical: &Path) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        canonical.as_os_str().as_encoded_bytes(),
    )
    .to_string()
}

/// Creates an execution identity for source traces and output directories.
pub(crate) fn new_run_id() -> String {
    format!(
        "{}-{}-{}",
        SystemClock.now().unix_timestamp_nanos(),
        id(),
        RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

pub(super) fn create_trace(
    directory: &Path,
    source_index: usize,
) -> Result<(CsvTraceService<File>, PathBuf), RunError> {
    let trace_path = directory.join(format!("source-{source_index:04}.trace.csv"));
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
    Ok((trace, trace_path))
}

pub(crate) fn create_run_directory(root: &Path, run_id: &str) -> Result<PathBuf, RunError> {
    let directory = root.join(run_id);
    create_dir_all(root)
        .and_then(|()| create_dir(&directory))
        .map_err(|error| {
            RunError::new(
                Failure::File {
                    operation: "create run directory",
                    path: directory.clone(),
                    error,
                },
                Summary::default(),
            )
        })?;
    Ok(directory)
}
