use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

/// Lifecycle failures are translated into processing outcomes by the manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    NotDisputable,
    AlreadyDisputed,
    NotDisputed,
    AlreadyChargedBack,
}

impl Display for TransitionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(match self {
            Self::NotDisputable => "only accepted deposits can be disputed",
            Self::AlreadyDisputed => "transaction is already disputed",
            Self::NotDisputed => "transaction is not under dispute",
            Self::AlreadyChargedBack => "transaction has already been charged back",
        })
    }
}

impl Error for TransitionError {}
