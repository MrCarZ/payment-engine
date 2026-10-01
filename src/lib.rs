//! Payment processing library with explicit component boundaries.
//!
//! Adapters translate external formats into application requests. The application
//! coordinates domain operations. The runtime wires components together; shared
//! observability handles event delivery without depending on payment rules.

pub mod adapters;
pub mod application;
pub mod domain;
pub mod observability;
pub mod runtime;
