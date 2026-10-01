use std::sync::Arc;

/// Shared import metadata; the caller supplies identities rather than deriving
/// a partner identity from a filename or client ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceContext {
    pub run_id: String,
    pub source_id: String,
    pub partner_id: Option<String>,
}

/// Operational metadata kept separate from financial requests and domain state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub source: Arc<SourceContext>,
}
