use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    iter::repeat_n,
    str::FromStr,
};

mod error;

pub use error::{AmountError, MoneyError};

/// Exact signed money in units of 0.0001.
///
/// All i128 scaled values are supported. Arithmetic is checked explicitly;
/// no unchecked arithmetic operator implementations are exposed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Money(i128);

impl Money {
    pub const SCALE: i128 = 10_000;
    pub const ZERO: Self = Self(0);

    /// Constructs a balance from already-scaled units, not whole currency units.
    pub const fn from_scaled_units(units: i128) -> Self {
        Self(units)
    }

    pub const fn scaled_units(self) -> i128 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(MoneyError::Overflow)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, MoneyError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(MoneyError::Overflow)
    }
}

impl FromStr for Money {
    type Err = MoneyError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let (negative, digits) = match input.as_bytes().first() {
            Some(b'-') => (true, &input[1..]),
            Some(b'+') => (false, &input[1..]),
            _ => (false, input),
        };
        let (whole, fraction) = match digits.split_once('.') {
            Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
            Some(_) => return Err(MoneyError::InvalidFormat),
            None => (digits, ""),
        };
        if whole.is_empty()
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(MoneyError::InvalidFormat);
        }
        if fraction.len() > 4 {
            return Err(MoneyError::ExcessPrecision);
        }

        // Accumulating negative inputs negatively also accepts i128::MIN,
        // whose absolute magnitude cannot be represented by a positive i128.
        let mut units = 0_i128;
        for digit in whole
            .bytes()
            .chain(fraction.bytes())
            .chain(repeat_n(b'0', 4 - fraction.len()))
        {
            let value = i128::from(digit - b'0');
            units = units.checked_mul(10).ok_or(MoneyError::Overflow)?;
            units = if negative {
                units.checked_sub(value)
            } else {
                units.checked_add(value)
            }
            .ok_or(MoneyError::Overflow)?;
        }
        Ok(Self(units))
    }
}

impl Display for Money {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let magnitude = self.0.unsigned_abs();
        let scale = Self::SCALE as u128;
        if self.0 < 0 {
            f.write_str("-")?;
        }
        write!(f, "{}.{:04}", magnitude / scale, magnitude % scale)
    }
}

/// A strictly positive deposit or withdrawal amount, distinct from a balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PositiveAmount(Money);

impl PositiveAmount {
    pub const fn money(self) -> Money {
        self.0
    }
}

impl TryFrom<Money> for PositiveAmount {
    type Error = AmountError;

    fn try_from(money: Money) -> Result<Self, Self::Error> {
        if money > Money::ZERO {
            Ok(Self(money))
        } else {
            Err(AmountError::NonPositive)
        }
    }
}

impl FromStr for PositiveAmount {
    type Err = AmountError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let money = input.parse::<Money>().map_err(AmountError::InvalidMoney)?;
        Self::try_from(money)
    }
}

impl Display for PositiveAmount {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests;
