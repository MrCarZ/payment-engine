//! Typed external JSON representation of payment execution results.
use super::filesystem::file_name;
use crate::{
    adapters::payment::csv::processing::batch::SourceReport as CsvSourceReport,
    manager::payment::{run::Summary as ManagerSummary, trace::State},
};
use serde::Serialize;
use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    path::PathBuf,
};

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Completed,
    Failed,
}
impl Display for Status {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
        })
    }
}
#[derive(Debug, Serialize)]
pub struct Summary {
    applied: u64,
    ignored: u64,
    rejected: u64,
    replayed: u64,
    input_errors: u64,
    processing_errors: u64,
}
impl From<ManagerSummary> for Summary {
    fn from(summary: ManagerSummary) -> Self {
        Self {
            applied: summary.applied,
            ignored: summary.ignored,
            rejected: summary.rejected,
            replayed: summary.replayed,
            input_errors: summary.input_errors,
            processing_errors: summary.processing_errors,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Source {
    source_id: String,
    status: Status,
    summary: Summary,
    error: Option<String>,
}
impl From<&CsvSourceReport> for Source {
    fn from(source: &CsvSourceReport) -> Self {
        Self {
            source_id: source.source.source_id.clone(),
            status: match source.state {
                State::Completed => Status::Completed,
                State::Failed => Status::Failed,
            },
            summary: source.summary.into(),
            error: source.error.as_ref().map(ToString::to_string),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub run_id: String,
    pub exit_code: u8,
    pub status: Status,
    pub elapsed_seconds: f64,
    pub input_files: Vec<String>,
    pub account_file: Option<String>,
    pub partial_account_file: Option<String>,
    pub trace_files: Vec<String>,
    pub summary: Summary,
    pub sources: Vec<Source>,
    pub error: Option<String>,
}
impl Report {
    pub fn sources(sources: &[CsvSourceReport]) -> Vec<Source> {
        sources.iter().map(Source::from).collect()
    }
    pub fn filenames(paths: &[PathBuf]) -> Vec<String> {
        paths.iter().map(|path| file_name(path)).collect()
    }
}
#[cfg(test)]
mod tests;
