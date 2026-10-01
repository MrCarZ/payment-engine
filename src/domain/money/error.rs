use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

/// Invalid decimal input or a value outside the supported scaled range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneyError {
    InvalidFormat,
    ExcessPrecision,
    Overflow,
}

impl Display for MoneyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(match self {
            Self::InvalidFormat => {
                "expected a decimal number with digits before and after any decimal point"
            }
            Self::ExcessPrecision => "money supports at most four fractional digits",
            Self::Overflow => "money exceeds the supported signed 128-bit scaled range",
        })
    }
}

impl Error for MoneyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountError {
    InvalidMoney(MoneyError),
    NonPositive,
}

impl Display for AmountError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::InvalidMoney(error) => error.fmt(f),
            Self::NonPositive => f.write_str("transaction amount must be greater than zero"),
        }
    }
}

impl Error for AmountError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMoney(error) => Some(error),
            Self::NonPositive => None,
        }
    }
}
