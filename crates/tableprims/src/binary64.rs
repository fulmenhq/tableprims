// SPDX-License-Identifier: MIT OR Apache-2.0

//! Eligibility of exact numbers for storage as IEEE 754 binary64.
//!
//! Some destinations can hold a number only as a binary64 value. An exact
//! number is eligible for such a cell when a reader can recover the declared
//! value from it:
//!
//! 1. it has at most [`MAX_SIGNIFICANT_DIGITS`] significant digits, not
//!    counting leading or trailing zeros;
//! 2. the shortest decimal spelling of the nearest binary64 value equals it
//!    mathematically;
//! 3. for an integer, the nearest binary64 value is the integer itself.
//!
//! The source value stays the authority: these functions return the
//! binary64 value to store and never a value read back from it. Decimal
//! comparison is done on digit strings, not through floating point. The
//! rules are provisional and may change in a later release.

use core::fmt;

use crate::exact::{Decimal, Integer};

/// Largest number of significant digits an eligible value may have.
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

/// Converts canonical exact-number text to the nearest binary64 value and
/// checks that its shortest spelling denotes the same number.
fn nearest_round_trip(canonical: &str) -> Result<f64, Binary64Error> {
    // The standard library parses decimal text to the nearest binary64
    // value, ties to even, and prints the shortest digits that parse back to
    // the same value, without an exponent.
    let value: f64 = canonical.parse().map_err(|_| Binary64Error::NotRoundTrip)?;
    if !value.is_finite() || !same_decimal_value(&value.to_string(), canonical) {
        return Err(Binary64Error::NotRoundTrip);
    }
    Ok(value)
}

/// Returns the binary64 value to store for a decimal, if it is eligible.
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
    nearest_round_trip(&value.to_string())
}

/// Returns the binary64 value to store for an integer, if it is eligible.
///
/// # Errors
///
/// Refuses everything [`from_decimal`] refuses, and an integer whose nearest
/// binary64 value is a different number.
pub fn from_integer(value: Integer) -> Result<f64, Binary64Error> {
    if significant_digits(value.value()) > MAX_SIGNIFICANT_DIGITS {
        return Err(Binary64Error::TooManyDigits);
    }
    let stored = nearest_round_trip(&value.to_string())?;
    // An exact integer has at most 38 digits, so the stored value is below
    // 2^127 in magnitude and converts without saturating. A binary64 value
    // of that size with a fractional part cannot be nearest to an integer.
    if stored.fract() != 0.0 || stored as i128 != value.value() {
        return Err(Binary64Error::NotExactInteger);
    }
    Ok(stored)
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
