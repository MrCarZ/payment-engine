//! CSV representations and streaming I/O for the payment service.

pub mod input;
pub mod output;
pub mod processing;
pub mod row;

mod position;
pub(crate) mod trace;

pub use position::RecordPosition;
