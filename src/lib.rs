//! Payment processing library with explicit component boundaries.
//!
//! Adapters translate external formats into manager requests. Each manager
//! coordinates domain operations. The bootstrap wires components together; shared
//! observability handles event delivery without depending on payment rules.

pub mod adapters;
pub mod bootstrap;
pub mod domain;
pub mod manager;
