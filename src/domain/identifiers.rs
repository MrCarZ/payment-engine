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
mod tests {
    use super::*;

    #[test]
    fn client_ids_accept_full_u16_range_and_whitespace() {
        for value in [0, u16::MAX] {
            let id = ClientId::new(value);
            assert_eq!(ClientId::from(value), id);
            assert_eq!(id.get(), value);
            assert_eq!(format!(" {value} ").parse::<ClientId>(), Ok(id));
            assert_eq!(id.to_string(), value.to_string());
        }
        for invalid in ["65536", "-1", "", "1.0"] {
            assert!(invalid.parse::<ClientId>().is_err());
        }
    }

    #[test]
    fn transaction_ids_accept_full_u32_range_and_whitespace() {
        for value in [0, u32::MAX] {
            let id = TransactionId::new(value);
            assert_eq!(TransactionId::from(value), id);
            assert_eq!(id.get(), value);
            assert_eq!(format!(" {value} ").parse::<TransactionId>(), Ok(id));
            assert_eq!(id.to_string(), value.to_string());
        }
        for invalid in ["4294967296", "-1", "", "1.0"] {
            assert!(invalid.parse::<TransactionId>().is_err());
        }
    }
}
