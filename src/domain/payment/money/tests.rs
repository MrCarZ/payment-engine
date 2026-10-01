use rstest::rstest;

use super::{AmountError, Money, MoneyError, PositiveAmount};

fn money(input: &str) -> Money {
    input.parse().expect("valid money")
}

#[rstest]
#[case::zero("0", 0, "0.0000")]
#[case::integer("1", 10_000, "1.0000")]
#[case::sign_and_whitespace("  +001.2  ", 12_000, "1.2000")]
#[case::two_places("1.23", 12_300, "1.2300")]
#[case::three_places("1.234", 12_340, "1.2340")]
#[case::four_places("1.2345", 12_345, "1.2345")]
#[case::smallest_positive("0.0001", 1, "0.0001")]
#[case::smallest_negative("-0.0001", -1, "-0.0001")]
#[case::negative_balance("-8", -80_000, "-8.0000")]
#[case::negative_zero("-0.0000", 0, "0.0000")]
fn decimals_parse_exactly_and_format_with_four_places(
    #[case] input: &str,
    #[case] units: i128,
    #[case] output: &str,
) {
    let parsed = money(input);
    assert_eq!(parsed.scaled_units(), units);
    assert_eq!(parsed.to_string(), output);
}

#[rstest]
#[case::empty("")]
#[case::whitespace(" ")]
#[case::plus_only("+")]
#[case::minus_only("-")]
#[case::missing_integer(".1")]
#[case::missing_fraction("1.")]
#[case::multiple_points("1.2.3")]
#[case::scientific_notation("1e2")]
#[case::nan("NaN")]
#[case::infinity("inf")]
#[case::grouping("1,000")]
#[case::internal_whitespace("1 0")]
#[case::multiple_signs("--1")]
#[case::non_ascii_digits("１２")]
fn malformed_inputs_are_rejected(#[case] input: &str) {
    assert_eq!(input.parse::<Money>(), Err(MoneyError::InvalidFormat));
}

#[rstest]
#[case::extra_digit("1.23456")]
#[case::trailing_zero("1.00000")]
#[case::negative_fraction("-0.00001")]
fn precision_is_never_silently_rounded(#[case] input: &str) {
    assert_eq!(input.parse::<Money>(), Err(MoneyError::ExcessPrecision));
}

#[rstest]
#[case::minimum(i128::MIN)]
#[case::above_minimum(i128::MIN + 1)]
#[case::negative_unit(-1)]
#[case::zero(0)]
#[case::positive_unit(1)]
#[case::below_maximum(i128::MAX - 1)]
#[case::maximum(i128::MAX)]
fn full_scaled_range_round_trips(#[case] units: i128) {
    let original = Money::from_scaled_units(units);
    assert_eq!(money(&original.to_string()), original);
}

#[rstest]
#[case::maximum("17014118346046923173168730371588410.5727", i128::MAX)]
#[case::minimum("-17014118346046923173168730371588410.5728", i128::MIN)]
fn decimal_limits_map_to_scaled_limits(#[case] input: &str, #[case] units: i128) {
    assert_eq!(money(input).scaled_units(), units);
}

#[rstest]
#[case::above_maximum("17014118346046923173168730371588410.5728")]
#[case::below_minimum("-17014118346046923173168730371588410.5729")]
#[case::unscaled_maximum("170141183460469231731687303715884105727")]
fn values_outside_scaled_range_are_rejected(#[case] input: &str) {
    assert_eq!(input.parse::<Money>(), Err(MoneyError::Overflow));
}

#[rstest]
#[case::decimal_addition("0.1", "0.2", "0.3")]
#[case::recover_negative_balance("-8", "10", "2")]
fn addition_is_exact(#[case] left: &str, #[case] right: &str, #[case] expected: &str) {
    assert_eq!(money(left).checked_add(money(right)), Ok(money(expected)));
}

#[test]
fn subtraction_supports_negative_balances() {
    assert_eq!(money("2").checked_sub(money("10")), Ok(money("-8")));
}

#[rstest]
#[case::addition_above_maximum(i128::MAX, 1, false)]
#[case::subtraction_below_minimum(i128::MIN, 1, true)]
#[case::addition_below_minimum(i128::MIN, -1, false)]
#[case::subtraction_above_maximum(i128::MAX, -1, true)]
fn arithmetic_overflow_returns_errors_without_changing_values(
    #[case] units: i128,
    #[case] delta: i128,
    #[case] subtract: bool,
) {
    let value = Money::from_scaled_units(units);
    let other = Money::from_scaled_units(delta);
    let result = if subtract {
        value.checked_sub(other)
    } else {
        value.checked_add(other)
    };
    assert_eq!(result, Err(MoneyError::Overflow));
    assert_eq!(value.scaled_units(), units);
}

#[rstest]
#[case::zero("0")]
#[case::negative_zero("-0")]
#[case::negative_fraction("-0.0001")]
#[case::negative_integer("-10")]
fn nonpositive_transaction_amounts_are_rejected(#[case] input: &str) {
    assert_eq!(
        input.parse::<PositiveAmount>(),
        Err(AmountError::NonPositive)
    );
}

#[test]
fn positive_transaction_amount_preserves_money_and_precision_validation() {
    let amount = "0.0001".parse::<PositiveAmount>().unwrap();
    assert_eq!(amount.money(), money("0.0001"));
    assert_eq!(amount.to_string(), "0.0001");
    assert_eq!(
        "1.00001".parse::<PositiveAmount>(),
        Err(AmountError::InvalidMoney(MoneyError::ExcessPrecision))
    );
}
