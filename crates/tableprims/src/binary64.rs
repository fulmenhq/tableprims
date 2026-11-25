// SPDX-License-Identifier: MIT OR Apache-2.0

//! Exact numbers as IEEE 754 binary64 values.
//!
//! [`integer_facts`] and [`decimal_facts`] give the nearest binary64 value of
//! an exact number, ties to even, and two facts about it that are
//! independent of each other:
//!
//! - [`Facts::is_exact`]: the binary64 value is the number itself;
//! - [`Facts::round_trips`]: the shortest decimal spelling of the binary64
//!   value denotes the number.
//!
//! The integer 2^60 is exact but does not round-trip; the decimal 0.1 round
//! trips but is not exact. Which facts a destination requires, and any limit
//! on digits, is the destination's rule.
//!
//! [`from_integer`] and [`from_decimal`] apply one such rule, the one of the
//! XLSX write profile `xlsx-write/r1`. An exact number is eligible for a
//! numeric cell when a reader can recover the declared value from it:
//!
//! 1. it has at most [`MAX_SIGNIFICANT_DIGITS`] significant digits, not
//!    counting leading or trailing zeros;
//! 2. it round-trips;
//! 3. for an integer, it is exact.
//!
//! The source value stays the authority: these functions return the
//! binary64 value to store and never a value read back from it. Exactness is
//! decided in integer arithmetic and decimal comparison on digit strings,
//! not through floating point. The rules are provisional and may change in a
//! later release.

use core::fmt;

use crate::exact::{Decimal, Integer};

/// Largest number of significant digits a value eligible under the
/// `xlsx-write/r1` rule may have.
pub const MAX_SIGNIFICANT_DIGITS: u8 = 15;

/// Why an exact number is not eligible for binary64 storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Binary64Error {
    /// The value has more than [`MAX_SIGNIFICANT_DIGITS`] significant digits.
    TooManyDigits,
    /// The shortest spelling of the nearest binary64 value is a different
    /// number.
    NotRoundTrip,
    /// The nearest binary64 value is not the integer itself.
    NotExactInteger,
}

impl Binary64Error {
    /// A stable identifier of the reason: `too-many-digits`,
    /// `not-round-trip` or `not-exact-integer`.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::TooManyDigits => "too-many-digits",
            Self::NotRoundTrip => "not-round-trip",
            Self::NotExactInteger => "not-exact-integer",
        }
    }
}

impl fmt::Display for Binary64Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooManyDigits => "value has too many significant digits for a numeric cell",
            Self::NotRoundTrip => "value does not survive storage as a binary number",
            Self::NotExactInteger => "integer is not exactly representable as a binary number",
        })
    }
}

impl std::error::Error for Binary64Error {}

/// Number of significant digits of a coefficient: its digits without
/// trailing zeros. Zero has one.
fn significant_digits(coefficient: i128) -> u8 {
    let mut rest = coefficient.unsigned_abs();
    if rest == 0 {
        return 1;
    }
    while rest % 10 == 0 {
        rest /= 10;
    }
    let mut digits = 0;
    while rest > 0 {
        rest /= 10;
        digits += 1;
    }
    digits
}

/// Splits a plain decimal spelling into sign, whole digits and fractional
/// digits, with redundant zeros removed. Returns `None` for anything that is
/// not `-?digits(.digits)?`.
fn normalized(text: &str) -> Option<(bool, &str, &str)> {
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = match body.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
        Some(_) => return None,
        None => (body, ""),
    };
    if whole.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let whole = whole.trim_start_matches('0');
    let fraction = fraction.trim_end_matches('0');
    // Zero has no sign.
    let negative = negative && !(whole.is_empty() && fraction.is_empty());
    Some((negative, whole, fraction))
}

/// Whether two plain decimal spellings denote the same number.
///
/// Each spelling is an optional minus sign, digits, and optionally a point
/// followed by digits. Leading zeros, trailing fractional zeros and the sign
/// of zero do not affect the result. A spelling in any other form, including
/// one with an exponent, equals nothing.
#[must_use]
pub fn same_decimal_value(left: &str, right: &str) -> bool {
    match (normalized(left), normalized(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// The nearest binary64 value of an exact number and two facts about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facts {
    value: f64,
    exact: bool,
    round_trips: bool,
}

impl Facts {
    /// The nearest binary64 value, ties to even. It is finite, and zero is
    /// positive.
    #[must_use]
    pub fn value(self) -> f64 {
        self.value
    }

    /// Whether the binary64 value is the number itself.
    #[must_use]
    pub fn is_exact(self) -> bool {
        self.exact
    }

    /// Whether the shortest decimal spelling of the binary64 value denotes
    /// the number. The shortest spelling has the fewest significant digits
    /// that parse back to the value and, among those, is the closest to it,
    /// ties to an even last digit.
    #[must_use]
    pub fn round_trips(self) -> bool {
        self.round_trips
    }
}

/// Whether `coefficient / 10^scale` is a binary64 value. It is when it is a
/// binary fraction, that is when 5^scale divides the coefficient, whose odd
/// part then fits in 53 bits. Every number of the exact domain is far inside
/// the binary64 exponent range, so no other condition applies.
fn exact_in_binary64(coefficient: i128, scale: u8) -> bool {
    let mut rest = coefficient.unsigned_abs();
    if rest == 0 {
        return true;
    }
    for _ in 0..scale {
        if rest % 5 != 0 {
            return false;
        }
        rest /= 5;
    }
    rest >>= rest.trailing_zeros();
    rest < 1 << 53
}

/// Plain decimal text of `digits * 10^exponent`, without an exponent.
fn plain(digits: u128, exponent: i32) -> String {
    let text = digits.to_string();
    if exponent >= 0 {
        return text + &"0".repeat(exponent.unsigned_abs() as usize);
    }
    let point = exponent.unsigned_abs() as usize;
    let text = format!("{text:0>width$}", width = point + 1);
    let (whole, fraction) = text.split_at(text.len() - point);
    format!("{whole}.{fraction}")
}

/// Whether `digits * 10^exponent` is exactly the binary64 value `target`.
fn is_exactly(digits: u128, exponent: i32, target: f64) -> bool {
    let Ok(scale) = u8::try_from((-exponent).max(0)) else {
        return false;
    };
    let coefficient = if exponent >= 0 {
        let Some(c) = 10u128
            .checked_pow(exponent.unsigned_abs())
            .and_then(|p| digits.checked_mul(p))
        else {
            return false;
        };
        c
    } else {
        digits
    };
    let Ok(coefficient) = i128::try_from(coefficient) else {
        return false;
    };
    exact_in_binary64(coefficient, if exponent >= 0 { 0 } else { scale })
        && plain(digits, exponent).parse::<f64>() == Ok(target)
}

/// The shortest decimal spelling of a finite binary64 value: the fewest
/// significant digits that parse back to the value and, among those, the
/// spelling closest to it, ties to an even last digit. This is what Go's
/// `strconv` and Python's `repr` print. The standard library's `Display`
/// prints a shortest closest spelling but breaks an exact tie away from
/// zero, so a tie is detected here, in exact arithmetic, and resolved.
fn shortest_spelling(value: f64) -> String {
    let magnitude = value.abs();
    let text = magnitude.to_string();
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let all = format!("{whole}{fraction}");
    let significant = all.trim_start_matches('0');
    let trimmed = significant.trim_end_matches('0');
    let sign = if value.is_sign_negative() && magnitude != 0.0 {
        "-"
    } else {
        ""
    };
    let Ok(digits) = trimmed.parse::<u128>() else {
        return text; // zero
    };
    if digits % 2 == 0 {
        return format!("{sign}{text}");
    }
    // magnitude is about digits * 10^exponent; a neighbour one unit in the
    // last place away is as close when the value is exactly their midpoint.
    let exponent = (significant.len() - trimmed.len()) as i32 - fraction.len() as i32;
    for neighbour in [digits - 1, digits + 1] {
        let midpoint = digits + neighbour; // twice the midpoint, in units
        if is_exactly(midpoint * 5, exponent - 1, magnitude)
            && plain(neighbour, exponent).parse::<f64>() == Ok(magnitude)
        {
            return format!("{sign}{}", plain(neighbour, exponent));
        }
    }
    format!("{sign}{text}")
}

/// The facts of an exact number, from its canonical text, coefficient and
/// scale.
fn facts(canonical: &str, coefficient: i128, scale: u8) -> Facts {
    // The standard library parses decimal text to the nearest binary64
    // value, ties to even, and prints the shortest digits that parse back to
    // the same value, without an exponent. Every exact number parses to a
    // finite value.
    let mut value: f64 = canonical
        .parse()
        .expect("canonical exact-number text parses as a binary64 value");
    if value == 0.0 {
        value = 0.0;
    }
    Facts {
        value,
        exact: exact_in_binary64(coefficient, scale),
        round_trips: same_decimal_value(&shortest_spelling(value), canonical),
    }
}

/// The nearest binary64 value of an integer and its facts.
#[must_use]
pub fn integer_facts(value: Integer) -> Facts {
    facts(&value.to_string(), value.value(), 0)
}

/// The nearest binary64 value of a decimal and its facts. The declared
/// scale is not carried by the value.
#[must_use]
pub fn decimal_facts(value: Decimal) -> Facts {
    facts(&value.to_string(), value.coefficient(), value.scale())
}

/// Returns the binary64 value to store for a decimal, if it is eligible
/// under the rule of the XLSX write profile `xlsx-write/r1`.
///
/// The declared scale is not carried by the result and must travel
/// separately.
///
/// # Errors
///
/// Refuses a value with too many significant digits and a value whose
/// nearest binary64 value has a different shortest spelling.
pub fn from_decimal(value: Decimal) -> Result<f64, Binary64Error> {
    if significant_digits(value.coefficient()) > MAX_SIGNIFICANT_DIGITS {
        return Err(Binary64Error::TooManyDigits);
    }
    let facts = decimal_facts(value);
    if !facts.round_trips {
        return Err(Binary64Error::NotRoundTrip);
    }
    Ok(facts.value)
}

/// Returns the binary64 value to store for an integer, if it is eligible
/// under the rule of the XLSX write profile `xlsx-write/r1`.
///
/// # Errors
///
/// Refuses everything [`from_decimal`] refuses, and an integer whose nearest
/// binary64 value is a different number.
pub fn from_integer(value: Integer) -> Result<f64, Binary64Error> {
    if significant_digits(value.value()) > MAX_SIGNIFICANT_DIGITS {
        return Err(Binary64Error::TooManyDigits);
    }
    let facts = integer_facts(value);
    if !facts.round_trips {
        return Err(Binary64Error::NotRoundTrip);
    }
    if !facts.exact {
        return Err(Binary64Error::NotExactInteger);
    }
    Ok(facts.value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exact::DecimalType;

    fn dec(text: &str, precision: u8, scale: u8) -> Decimal {
        Decimal::parse(text, DecimalType::new(precision, scale).unwrap()).unwrap()
    }

    fn int(text: &str) -> Integer {
        Integer::parse(text).unwrap()
    }

    #[test]
    fn ordinary_decimals_are_eligible() {
        for (text, precision, scale) in [
            ("12.34", 4, 2),
            ("123.4500", 7, 4),
            ("1.245", 4, 3),
            ("-12.50", 4, 2),
            ("0.0001", 5, 4),
            ("100.0001", 7, 4),
            ("0.5", 2, 1),
        ] {
            let stored = from_decimal(dec(text, precision, scale)).unwrap();
            assert!(same_decimal_value(&stored.to_string(), text), "{text}");
        }
        assert_eq!(from_decimal(dec("-12.50", 4, 2)).unwrap(), -12.5);
    }

    #[test]
    fn zero_is_eligible_at_any_scale_and_positive() {
        for (text, scale) in [("0", 0), ("0.00", 2)] {
            let stored = from_decimal(dec(text, 3, scale)).unwrap();
            assert_eq!(stored, 0.0);
            assert!(stored.is_sign_positive());
        }
        assert_eq!(from_integer(int("0")).unwrap(), 0.0);
    }

    #[test]
    fn fifteen_significant_digits_is_the_limit() {
        assert!(from_decimal(dec("999999999999999", 15, 0)).is_ok());
        assert!(from_decimal(dec("0.999999999999999", 16, 15)).is_ok());
        assert!(from_decimal(dec("9.99999999999999", 15, 14)).is_ok());
        assert_eq!(
            from_decimal(dec("1000000000000000.3", 17, 1)),
            Err(Binary64Error::TooManyDigits)
        );
        assert_eq!(
            from_decimal(dec("1.000000000000001", 16, 15)),
            Err(Binary64Error::TooManyDigits)
        );
        assert_eq!(
            from_integer(int("9999999999999999")),
            Err(Binary64Error::TooManyDigits)
        );
    }

    #[test]
    fn trailing_zeros_do_not_count_as_significant() {
        // Fifteen significant digits padded to scale 20.
        assert!(from_decimal(dec("1.23456789012345000000", 21, 20)).is_ok());
        assert!(from_integer(int("123456789012344000")).is_ok());
        // Fifteen significant digits, but an odd multiple of eight beyond
        // 2^53 has no exact binary64 value.
        assert_eq!(
            from_integer(int("123456789012345000")),
            Err(Binary64Error::NotExactInteger)
        );
    }

    #[test]
    fn integers_near_two_to_the_53_are_outside() {
        for text in ["9007199254740991", "9007199254740992", "9007199254740993"] {
            assert_eq!(
                from_integer(int(text)),
                Err(Binary64Error::TooManyDigits),
                "{text}"
            );
        }
    }

    #[test]
    fn integers_must_be_exact_in_binary() {
        // 10^22 is the largest power of ten that binary64 holds exactly.
        assert_eq!(from_integer(int("10000000000000000000000")).unwrap(), 1e22);
        assert_eq!(
            from_integer(int("100000000000000000000000")),
            Err(Binary64Error::NotExactInteger)
        );
        let ten_to_37 = "10000000000000000000000000000000000000";
        assert_eq!(
            from_integer(int(ten_to_37)),
            Err(Binary64Error::NotExactInteger)
        );
        assert_eq!(
            from_integer(int(&format!("-{ten_to_37}"))),
            Err(Binary64Error::NotExactInteger)
        );
        // The same value in a decimal column round-trips as a decimal.
        assert!(from_decimal(dec(ten_to_37, 38, 0)).is_ok());
        assert_eq!(
            from_integer(int("-9007199254740000")).unwrap(),
            -9007199254740000.0
        );
    }

    #[test]
    fn exponent_edges_are_eligible() {
        // Smallest and largest magnitudes of the 38-digit domain.
        assert!(from_decimal(dec("0.00000000000000000000000000000000000001", 38, 38)).is_ok());
        assert!(from_decimal(dec("-0.00000000000000000000000000000000000001", 38, 38)).is_ok());
        assert!(from_decimal(dec("90000000000000000000000000000000000000", 38, 0)).is_ok());
        assert!(from_decimal(dec("0.00000000000000000000000123456789012345", 38, 38)).is_ok());
    }

    #[test]
    fn decimal_spellings_compare_by_value() {
        assert!(same_decimal_value("123.45", "123.4500"));
        assert!(same_decimal_value("0", "0.00"));
        assert!(same_decimal_value("-0", "0"));
        assert!(same_decimal_value("0012", "12"));
        assert!(same_decimal_value("-12.5", "-12.50"));
        assert!(!same_decimal_value("12.5", "-12.5"));
        assert!(!same_decimal_value("123.45", "123.46"));
        assert!(!same_decimal_value("9007199254740992", "9007199254740993"));
        assert!(!same_decimal_value("1.0", "10"));
        for bad in [
            "", "-", ".5", "5.", "1e5", "1E5", "+1", " 1", "1 ", "NaN", "inf", "1.2.3",
        ] {
            assert!(!same_decimal_value(bad, bad), "{bad:?}");
        }
    }

    #[test]
    fn stored_value_spells_back_to_the_source_for_many_values() {
        // Every two-decimal amount from -50.00 to 50.00.
        let ty = DecimalType::new(4, 2).unwrap();
        for cents in -5000i32..=5000 {
            let sign = if cents < 0 { "-" } else { "" };
            let text = format!("{sign}{}.{:02}", cents.abs() / 100, cents.abs() % 100);
            let stored = from_decimal(Decimal::parse(&text, ty).unwrap()).unwrap();
            assert!(same_decimal_value(&stored.to_string(), &text), "{text}");
        }
    }
}
