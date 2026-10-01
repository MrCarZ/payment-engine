//! Shared trace delivery contract.

use crate::domain::observability::Event;

mod error;

pub use error::TraceError;

/// Object-safe delivery boundary injected into services by the application.
pub trait TraceService {
    fn emit(&mut self, event: Event) -> Result<(), TraceError>;
    fn flush(&mut self) -> Result<(), TraceError>;
}
