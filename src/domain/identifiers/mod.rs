use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    num::ParseIntError,
    str::FromStr,
};

/// A client identifier, distinct from a transaction identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(u16);

impl ClientId {
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

impl From<u16> for ClientId {
    fn from(value: u16) -> Self {
        Self::new(value)
    }
}

impl FromStr for ClientId {
    type Err = ParseIntError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.trim().parse().map(Self)
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.0.fmt(f)
    }
}

/// A globally unique original transaction identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransactionId(u32);

impl TransactionId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl From<u32> for TransactionId {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

impl FromStr for TransactionId {
    type Err = ParseIntError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.trim().parse().map(Self)
    }
}

impl Display for TransactionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests;
