//! Artifact storage and serialized reporting; execution policy belongs to managers.
use serde_json::to_writer_pretty;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
mod error;
pub use error::Error;
pub mod filesystem;
pub mod identity;
pub mod report;
use filesystem::{create_file, publish};
use report::Report;

/// Publish only a fully written report; failed writes retain a partial artifact.
pub fn write_report(directory: &Path, report: &Report) -> Result<(), Error> {
    let partial = directory.join("report.partial.json");
    let mut file = create_file(&partial)?;
    to_writer_pretty(&mut file, report)?;
    file.write_all(b"\n")?;
    file.flush()?;
    drop(file);
    publish(&partial, &directory.join("report.json"))?;
    Ok(())
}

pub fn write_diagnostics(
    directory: &Path,
    report: &Report,
    traces: &[PathBuf],
) -> Result<(), Error> {
    let mut diagnostics = create_file(&directory.join("diagnostics.log"))?;
    writeln!(diagnostics, "Status: {}", report.status)?;
    for path in traces {
        writeln!(diagnostics, "Trace log: {}", path.display())?;
    }
    if let Some(error) = &report.error {
        writeln!(diagnostics, "{error}")?;
    }
    diagnostics.flush()?;
    Ok(())
}
