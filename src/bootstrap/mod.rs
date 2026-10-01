//! Composition root for the configured CLI and filesystem adapters.

use std::io::Write;

use crate::adapters::{
    artifacts::identity::new_run_id,
    cli::{config::Invocation, run::run},
};

pub use crate::adapters::cli::run::{ArtifactError, Execution};

/// Starts a CLI invocation with a fresh run identity and its output destination.
/// Managers own financial execution; adapters own representation and delivery.
pub fn execute(invocation: Invocation, output: impl Write) -> Result<Execution, ArtifactError> {
    run(invocation, output, new_run_id())
}
