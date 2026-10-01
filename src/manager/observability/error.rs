use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

/// Transport-independent failure retaining the adapter's underlying error.
#[derive(Debug)]
pub struct TraceError {
    cause: Box<dyn Error + Send + Sync>,
}

impl TraceError {
    pub fn new(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            cause: Box::new(error),
        }
    }
}

impl Display for TraceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "trace delivery failed: {}", self.cause)
    }
}

impl Error for TraceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.cause.as_ref())
    }
}
