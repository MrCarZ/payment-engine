//! CSV representations and streaming I/O for the payment service.

pub mod input;
pub mod row;

/// CSV record index (header is zero), one-based line, and zero-based byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordPosition {
    pub record: u64,
    pub line: u64,
    pub byte: u64,
}
