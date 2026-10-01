//! Artifact storage and serialized reporting; execution policy belongs to managers.
use serde_json::to_writer_pretty;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
mod error;
pub use error::Error;
pub mod filesystem;
pub mod report;
use filesystem::create_file;
use report::Report;

pub fn persist(directory: &Path, report: &Report, traces: &[PathBuf]) -> Result<(), Error> {
    let mut file = create_file(&directory.join("report.json"))?;
    to_writer_pretty(&mut file, report)?;
    file.write_all(b"\n")?;
    file.flush()?;
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
