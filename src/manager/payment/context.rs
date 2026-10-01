use std::sync::Arc;

/// Shared import metadata; the caller supplies identities rather than deriving
/// a partner identity from a filename or client ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceContext {
    pub run_id: String,
    pub source_id: String,
    pub partner_id: Option<String>,
}

/// CSV record index (header is zero), one-based line, and zero-based byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordPosition {
    pub record: u64,
    pub line: u64,
    pub byte: u64,
}

/// Operational metadata kept separate from financial requests and domain state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub source: Arc<SourceContext>,
    pub position: RecordPosition,
}
