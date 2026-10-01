use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

use crate::domain::payment::{ClientId, TransactionId};

/// One-based request ordinal within a source; not a physical text line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub source_id: String,
    pub record: usize,
}

impl Display for Location {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "source {} at record {}", self.source_id, self.record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    EmptyBatch,
    DuplicateSource {
        source_id: String,
    },
    RunMismatch {
        source_id: String,
        expected: String,
        actual: String,
    },
    ContextMismatch {
        location: Location,
    },
    ClientOverlap {
        client: ClientId,
        first: Location,
        conflicting: Location,
    },
    TransactionOverlap {
        tx: TransactionId,
        first: Location,
        conflicting: Location,
    },
    CrossSourceReference {
        tx: TransactionId,
        original: Location,
        reference: Location,
    },
}

impl Display for ValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::EmptyBatch => f.write_str("batch must contain at least one source"),
            Self::DuplicateSource { source_id } => write!(f, "duplicate batch source {source_id}"),
            Self::RunMismatch {
                source_id,
                expected,
                actual,
            } => write!(
                f,
                "source {source_id} belongs to run {actual}, expected {expected}"
            ),
            Self::ContextMismatch { location } => {
                write!(f, "request context does not match {location}")
            }
            Self::ClientOverlap {
                client,
                first,
                conflicting,
            } => write!(
                f,
                "client {client} occurs in both {first} and {conflicting}"
            ),
            Self::TransactionOverlap {
                tx,
                first,
                conflicting,
            } => write!(
                f,
                "original transaction {tx} occurs in both {first} and {conflicting}"
            ),
            Self::CrossSourceReference {
                tx,
                original,
                reference,
            } => write!(
                f,
                "{reference} references transaction {tx} owned by {original}"
            ),
        }
    }
}

impl Error for ValidationError {}
