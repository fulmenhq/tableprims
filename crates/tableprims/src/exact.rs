// SPDX-License-Identifier: MIT OR Apache-2.0

//! Exact integers and decimals.
//!
//! Values are held as a signed integer coefficient and, for decimals, a
//! declared scale. Nothing in this module converts through binary floating
//! point. The textual grammar and bounds implemented here are provisional
//! and may change before the first release.
//!
//! Grammar:
//!
//! - integer: `0` or `-?[1-9][0-9]*`, at most [`MAX_DIGITS`] digits;
//! - decimal at scale zero: the integer grammar;
//! - decimal at scale `s > 0`: `-?(0|[1-9][0-9]*)\.[0-9]{s}`.
//!
//! No plus sign, whitespace, exponent or redundant leading zero is accepted,
//! and zero carries no minus sign. Invalid spellings are refused, never
//! normalized.

use core::fmt;

/// Largest number of decimal digits in a coefficient.
pub const MAX_DIGITS: u8 = 38;

/// Why an exact value was refused.
///
/// The messages never include the offending text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExactError {
    /// The text is empty.
    Empty,
    /// A character other than a digit, a leading `-` or the decimal point.
    InvalidCharacter,
    /// A redundant leading zero, such as `007` or `00.5`.
    LeadingZero,
    /// Zero written with a minus sign.
    NegativeZero,
    /// A decimal point with no digit before it or after it, or more than one
    /// decimal point.
    Malformed,
    /// More than [`MAX_DIGITS`] significant digits, whatever the declared
    /// type.
    DigitLimitExceeded,
    /// A decimal point in an integer or where the declared scale is zero, or
    /// no decimal point where the declared scale is positive.
    PointMismatch,
    /// The number of fractional digits differs from the declared scale.
    ScaleMismatch,
    /// More significant digits than the declared precision.
    PrecisionExceeded,
    /// A precision outside `1..=MAX_DIGITS` or a scale above the precision.
    InvalidType,
    /// Decimals with different scales were combined.
    ScaleDiffers,
}

impl fmt::Display for ExactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "empty exact value",
            Self::InvalidCharacter => "invalid character in exact value",
            Self::LeadingZero => "redundant leading zero in exact value",
            Self::NegativeZero => "zero written with a minus sign",
            Self::Malformed => "malformed exact value",
            Self::DigitLimitExceeded => "exact value has too many digits",
            Self::PointMismatch => "decimal point does not match the declared scale",
            Self::ScaleMismatch => "fractional digits differ from the declared scale",
            Self::PrecisionExceeded => "exact value exceeds the declared precision",
            Self::InvalidType => "invalid decimal precision or scale",
            Self::ScaleDiffers => "decimals have different scales",
        })
    }
}

impl std::error::Error for ExactError {}

/// An exact value split into its parts, before any declared type applies.
struct Lexed<'a> {
    negative: bool,
    /// The digits after the decimal point, when the text has one.
    fraction: Option<&'a str>,
    /// The digits as a magnitude.
    magnitude: i128,
    /// Significant digits; zero has one.
    significant: u8,
}

/// Checks the grammar `-?(0|[1-9][0-9]*)(\.[0-9]+)?`, refuses negative zero
/// and more than [`MAX_DIGITS`] significant digits. These rules hold for
/// every declared type, so they are checked before any of them.
fn lex(text: &str) -> Result<Lexed<'_>, ExactError> {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if unsigned.is_empty() {
        return Err(ExactError::Empty);
    }
    if !unsigned.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(ExactError::InvalidCharacter);
    }
    let (whole, fraction) = match unsigned.split_once('.') {
        None => (unsigned, None),
        Some((whole, fraction)) => {
            if whole.is_empty() || fraction.is_empty() || fraction.contains('.') {
                return Err(ExactError::Malformed);
            }
            (whole, Some(fraction))
        }
    };
    if whole.len() > 1 && whole.starts_with('0') {
        return Err(ExactError::LeadingZero);
    }
    let mut magnitude: i128 = 0;
    let mut significant: u8 = 0;
    for byte in whole.bytes().chain(fraction.unwrap_or("").bytes()) {
        if magnitude != 0 || byte != b'0' {
            significant += 1;
            if significant > MAX_DIGITS {
                return Err(ExactError::DigitLimitExceeded);
            }
        }
        // At most MAX_DIGITS significant digits reach here, and
        // 10^38 - 1 < i128::MAX, so this cannot overflow.
        magnitude = magnitude * 10 + i128::from(byte - b'0');
    }
    if negative && magnitude == 0 {
        return Err(ExactError::NegativeZero);
    }
    Ok(Lexed {
        negative,
        fraction,
        magnitude,
        significant: significant.max(1),
    })
}

/// Number of decimal digits in a coefficient; zero has one.
fn digit_count(coefficient: i128) -> u8 {
    let mut rest = coefficient.unsigned_abs();
    let mut digits: u8 = 1;
    while rest >= 10 {
        rest /= 10;
        digits += 1;
    }
    digits
}

/// `10^exponent` for `exponent <= MAX_DIGITS`.
fn pow10(exponent: u8) -> i128 {
    debug_assert!(exponent <= MAX_DIGITS);
    10_i128.pow(u32::from(exponent))
}

/// An exact integer of at most [`MAX_DIGITS`] digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Integer(i128);

impl Integer {
    /// Parses the canonical integer grammar.
    ///
    /// # Errors
    ///
    /// Returns an [`ExactError`] when the text is not canonical, has more
    /// than [`MAX_DIGITS`] digits or has a decimal point.
    pub fn parse(text: &str) -> Result<Self, ExactError> {
        let lexed = lex(text)?;
        if lexed.fraction.is_some() {
            return Err(ExactError::PointMismatch);
        }
        Ok(Self(if lexed.negative {
            -lexed.magnitude
        } else {
            lexed.magnitude
        }))
    }

    /// The value as a signed integer.
    #[must_use]
    pub fn value(self) -> i128 {
        self.0
    }
}

impl fmt::Display for Integer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A decimal column type: total digits and digits after the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecimalType {
    precision: u8,
    scale: u8,
}

impl DecimalType {
    /// Declares a decimal type.
    ///
    /// # Errors
    ///
    /// Returns [`ExactError::InvalidType`] unless
    /// `1 <= precision <= MAX_DIGITS` and `scale <= precision`.
    pub fn new(precision: u8, scale: u8) -> Result<Self, ExactError> {
        if precision == 0 || precision > MAX_DIGITS || scale > precision {
            return Err(ExactError::InvalidType);
        }
        Ok(Self { precision, scale })
    }

    /// Total number of significant digits allowed.
    #[must_use]
    pub fn precision(self) -> u8 {
        self.precision
    }

    /// Number of digits after the decimal point.
    #[must_use]
    pub fn scale(self) -> u8 {
        self.scale
    }
}

/// How a discarded remainder changes the retained digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RoundingMode {
    /// Ties go to the neighbour whose last digit is even.
    HalfEven,
    /// Ties go to the neighbour farther from zero.
    HalfAwayFromZero,
    /// Discard the remainder.
    TowardZero,
    /// Round up to the next value toward positive infinity.
    TowardPositive,
    /// Round down to the next value toward negative infinity.
    TowardNegative,
}

/// The result of [`Decimal::quantize`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quantized {
    /// The value at the target scale.
    pub value: Decimal,
    /// Whether the mathematical value changed.
    pub value_changed: bool,
    /// Whether the scale changed, even if the value did not.
    pub scale_changed: bool,
}

/// An exact decimal: `coefficient * 10^-scale`.
///
/// Equality compares coefficient and scale, so `1.0` and `1.00` are distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Decimal {
    coefficient: i128,
    scale: u8,
}

impl Decimal {
    /// Parses the canonical decimal grammar for a declared type.
    ///
    /// # Errors
    ///
    /// Returns an [`ExactError`] when the text is not canonical, its
    /// fractional digits differ from the declared scale, or its coefficient
    /// has more significant digits than the declared precision. The grammar
    /// and the [`MAX_DIGITS`] bound are checked before the declared type.
    pub fn parse(text: &str, ty: DecimalType) -> Result<Self, ExactError> {
        let lexed = lex(text)?;
        let fraction = lexed.fraction.unwrap_or("");
        if lexed.fraction.is_some() != (ty.scale > 0) {
            return Err(ExactError::PointMismatch);
        }
        if fraction.len() != usize::from(ty.scale) {
            return Err(ExactError::ScaleMismatch);
        }
        if lexed.significant > ty.precision {
            return Err(ExactError::PrecisionExceeded);
        }
        Ok(Self {
            coefficient: if lexed.negative {
                -lexed.magnitude
            } else {
                lexed.magnitude
            },
            scale: ty.scale,
        })
    }

    /// The signed coefficient.
    #[must_use]
    pub fn coefficient(self) -> i128 {
        self.coefficient
    }

    /// Number of digits after the decimal point.
    #[must_use]
    pub fn scale(self) -> u8 {
        self.scale
    }

    /// Number of significant digits in the coefficient; zero has one.
    #[must_use]
    pub fn significant_digits(self) -> u8 {
        digit_count(self.coefficient)
    }

    /// Adds two decimals of the same scale.
    ///
    /// # Errors
    ///
    /// Returns [`ExactError::ScaleDiffers`] for different scales and
    /// [`ExactError::PrecisionExceeded`] when the sum needs more than
    /// [`MAX_DIGITS`] digits.
    pub fn checked_add(self, other: Self) -> Result<Self, ExactError> {
        if self.scale != other.scale {
            return Err(ExactError::ScaleDiffers);
        }
        // Two 38-digit magnitudes can exceed i128, so the addition is checked
        // before the digit bound is applied.
        let coefficient = self
            .coefficient
            .checked_add(other.coefficient)
            .filter(|sum| digit_count(*sum) <= MAX_DIGITS)
            .ok_or(ExactError::PrecisionExceeded)?;
        Ok(Self {
            coefficient,
            scale: self.scale,
        })
    }

    /// Re-expresses the value in the target type.
    ///
    /// Raising the scale pads exactly. Lowering it applies `mode` and reports
    /// whether the value changed.
    ///
    /// # Errors
    ///
    /// Returns [`ExactError::PrecisionExceeded`] when the result has more
    /// significant digits than the target precision. Nothing is clipped.
    pub fn quantize(
        self,
        target: DecimalType,
        mode: RoundingMode,
    ) -> Result<Quantized, ExactError> {
        let scale_changed = target.scale != self.scale;
        let (coefficient, value_changed) = if target.scale >= self.scale {
            let factor = pow10(target.scale - self.scale);
            let padded = self
                .coefficient
                .checked_mul(factor)
                .ok_or(ExactError::PrecisionExceeded)?;
            (padded, false)
        } else {
            let divisor = pow10(self.scale - target.scale);
            let quotient = self.coefficient / divisor;
            let remainder = self.coefficient % divisor;
            (round(quotient, remainder, divisor, mode), remainder != 0)
        };
        if digit_count(coefficient) > target.precision {
            return Err(ExactError::PrecisionExceeded);
        }
        Ok(Quantized {
            value: Self {
                coefficient,
                scale: target.scale,
            },
            value_changed,
            scale_changed,
        })
    }
}

/// Applies a rounding mode to a truncated quotient.
///
/// `remainder` has the sign of the dividend and a magnitude below `divisor`.
fn round(quotient: i128, remainder: i128, divisor: i128, mode: RoundingMode) -> i128 {
    if remainder == 0 {
        return quotient;
    }
    let away = if remainder > 0 {
        quotient + 1
    } else {
        quotient - 1
    };
    let magnitude = remainder.abs();
    // Compare the remainder with half the divisor without doubling it, which
    // could overflow near the top of the range.
    let against_half = magnitude.cmp(&(divisor - magnitude));
    match mode {
        RoundingMode::TowardZero => quotient,
        RoundingMode::TowardPositive => {
            if remainder > 0 {
                away
            } else {
                quotient
            }
        }
        RoundingMode::TowardNegative => {
            if remainder < 0 {
                away
            } else {
                quotient
            }
        }
        RoundingMode::HalfAwayFromZero => {
            if against_half.is_lt() {
                quotient
            } else {
                away
            }
        }
        RoundingMode::HalfEven => match against_half {
            core::cmp::Ordering::Less => quotient,
            core::cmp::Ordering::Greater => away,
            core::cmp::Ordering::Equal => {
                if quotient % 2 == 0 {
                    quotient
                } else {
                    away
                }
            }
        },
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scale == 0 {
            return write!(f, "{}", self.coefficient);
        }
        let divisor = pow10(self.scale).unsigned_abs();
        let magnitude = self.coefficient.unsigned_abs();
        let sign = if self.coefficient < 0 { "-" } else { "" };
        write!(
            f,
            "{sign}{}.{:0width$}",
            magnitude / divisor,
            magnitude % divisor,
            width = usize::from(self.scale)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(precision: u8, scale: u8) -> DecimalType {
        DecimalType::new(precision, scale).expect("valid decimal type")
    }

    fn dec(text: &str, precision: u8, scale: u8) -> Decimal {
        Decimal::parse(text, ty(precision, scale)).expect("valid decimal")
    }

    #[test]
    fn integers_round_trip_canonical_text() {
        for text in [
            "0",
            "7",
            "-7",
            "9007199254740991",
            "9007199254740992",
            "9007199254740993",
            "12345678901234567890",
            "99999999999999999999999999999999999999",
            "-99999999999999999999999999999999999999",
        ] {
            let value = Integer::parse(text).expect("canonical integer");
            assert_eq!(value.to_string(), text);
        }
    }

    #[test]
    fn adjacent_large_integers_stay_distinct() {
        let low = Integer::parse("9007199254740992").expect("integer");
        let high = Integer::parse("9007199254740993").expect("integer");
        assert_eq!(high.value() - low.value(), 1);
    }

    #[test]
    fn noncanonical_integers_are_refused() {
        let cases = [
            ("", ExactError::Empty),
            ("-", ExactError::Empty),
            ("000123", ExactError::LeadingZero),
            ("00", ExactError::LeadingZero),
            ("-0", ExactError::NegativeZero),
            ("+1", ExactError::InvalidCharacter),
            (" 1", ExactError::InvalidCharacter),
            ("1 ", ExactError::InvalidCharacter),
            ("1e3", ExactError::InvalidCharacter),
            ("1.0", ExactError::PointMismatch),
            (".5", ExactError::Malformed),
            ("1.", ExactError::Malformed),
            ("1..2", ExactError::Malformed),
            ("-0.0", ExactError::NegativeZero),
            ("--1", ExactError::InvalidCharacter),
            ("١٢٣", ExactError::InvalidCharacter),
            (
                "100000000000000000000000000000000000000",
                ExactError::DigitLimitExceeded,
            ),
            (
                "1000000000000000000000000000000000000.0",
                ExactError::PointMismatch,
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(Integer::parse(text), Err(expected), "text: {text:?}");
        }
    }

    #[test]
    fn decimal_types_are_bounded() {
        assert!(DecimalType::new(1, 0).is_ok());
        assert!(DecimalType::new(38, 38).is_ok());
        assert_eq!(DecimalType::new(0, 0), Err(ExactError::InvalidType));
        assert_eq!(DecimalType::new(39, 0), Err(ExactError::InvalidType));
        assert_eq!(DecimalType::new(4, 5), Err(ExactError::InvalidType));
    }

    #[test]
    fn decimals_round_trip_with_declared_scale() {
        let cases = [
            ("123.4500", 7, 4),
            ("0.01", 2, 2),
            ("0.00", 2, 2),
            ("-1234.56", 6, 2),
            ("-0.05", 3, 2),
            ("42", 2, 0),
            ("0", 1, 0),
            ("0.99999999999999999999999999999999999999", 38, 38),
        ];
        for (text, precision, scale) in cases {
            let value = dec(text, precision, scale);
            assert_eq!(value.to_string(), text);
            assert_eq!(value.scale(), scale);
        }
        assert_eq!(dec("123.4500", 7, 4).coefficient(), 1_234_500);
        assert_eq!(dec("0.00", 2, 2).significant_digits(), 1);
    }

    #[test]
    fn noncanonical_or_out_of_type_decimals_are_refused() {
        let cases = [
            ("123.4500", 6, 4, ExactError::PrecisionExceeded),
            ("1.00", 2, 2, ExactError::PrecisionExceeded),
            ("123.45", 7, 4, ExactError::ScaleMismatch),
            ("123.45000", 9, 4, ExactError::ScaleMismatch),
            ("123", 7, 4, ExactError::PointMismatch),
            ("123.", 7, 0, ExactError::Malformed),
            ("1.5", 3, 0, ExactError::PointMismatch),
            (".5", 2, 1, ExactError::Malformed),
            ("-.5", 2, 1, ExactError::Malformed),
            ("1.", 2, 1, ExactError::Malformed),
            ("1..2", 2, 1, ExactError::Malformed),
            (".", 2, 1, ExactError::Malformed),
            ("01.5", 3, 1, ExactError::LeadingZero),
            ("-0.00", 2, 2, ExactError::NegativeZero),
            ("-0", 1, 0, ExactError::NegativeZero),
            ("+1.5", 2, 1, ExactError::InvalidCharacter),
            ("1.5e2", 4, 3, ExactError::InvalidCharacter),
            ("1,5", 2, 1, ExactError::InvalidCharacter),
            ("1.2.3", 3, 1, ExactError::Malformed),
            // Grammar and the global digit bound come before the declared type.
            ("01.50", 3, 1, ExactError::LeadingZero),
            ("-0.0", 2, 2, ExactError::NegativeZero),
            (
                "1234567890123456789012345678901234567.89",
                5,
                2,
                ExactError::DigitLimitExceeded,
            ),
            ("123.45", 4, 2, ExactError::PrecisionExceeded),
            ("1.0", 3, 2, ExactError::ScaleMismatch),
            ("", 2, 1, ExactError::Empty),
        ];
        for (text, precision, scale, expected) in cases {
            assert_eq!(
                Decimal::parse(text, ty(precision, scale)),
                Err(expected),
                "text: {text:?} as decimal({precision},{scale})"
            );
        }
    }

    #[test]
    fn scale_is_part_of_identity() {
        assert_ne!(dec("1.0", 3, 1), dec("1.00", 3, 2));
    }

    #[test]
    fn sums_are_exact() {
        // 0.1 + 0.2 is 0.3 exactly; binary floating point gives 0.30000000000000004.
        let sum = dec("0.1", 2, 1)
            .checked_add(dec("0.2", 2, 1))
            .expect("same scale");
        assert_eq!(sum.to_string(), "0.3");

        let mut total = dec("0.0000", 12, 4);
        for text in ["123.4500", "-0.0001", "99999999.9999", "-123.4500"] {
            total = total.checked_add(dec(text, 12, 4)).expect("same scale");
        }
        assert_eq!(total.to_string(), "99999999.9998");

        assert_eq!(
            dec("1.0", 2, 1).checked_add(dec("1.00", 3, 2)),
            Err(ExactError::ScaleDiffers)
        );
        let top = dec("99999999999999999999999999999999999999", 38, 0);
        assert_eq!(top.checked_add(top), Err(ExactError::PrecisionExceeded));
    }

    /// Rounds `text` at scale 1 to an integer and returns the result.
    fn to_whole(text: &str, mode: RoundingMode) -> String {
        dec(text, 3, 1)
            .quantize(ty(3, 0), mode)
            .expect("fits")
            .value
            .to_string()
    }

    #[test]
    fn rounding_modes_follow_their_mathematical_direction() {
        use RoundingMode::{
            HalfAwayFromZero, HalfEven, TowardNegative, TowardPositive, TowardZero,
        };
        // (input, half-even, half-away, toward-zero, toward-positive, toward-negative)
        let table = [
            ("2.5", "2", "3", "2", "3", "2"),
            ("3.5", "4", "4", "3", "4", "3"),
            ("-2.5", "-2", "-3", "-2", "-2", "-3"),
            ("-3.5", "-4", "-4", "-3", "-3", "-4"),
            ("2.4", "2", "2", "2", "3", "2"),
            ("2.6", "3", "3", "2", "3", "2"),
            ("-2.4", "-2", "-2", "-2", "-2", "-3"),
            ("-2.6", "-3", "-3", "-2", "-2", "-3"),
            ("0.5", "0", "1", "0", "1", "0"),
            ("-0.5", "0", "-1", "0", "0", "-1"),
            ("2.0", "2", "2", "2", "2", "2"),
        ];
        for (input, even, away, zero, positive, negative) in table {
            assert_eq!(to_whole(input, HalfEven), even, "half-even {input}");
            assert_eq!(to_whole(input, HalfAwayFromZero), away, "half-away {input}");
            assert_eq!(to_whole(input, TowardZero), zero, "toward-zero {input}");
            assert_eq!(
                to_whole(input, TowardPositive),
                positive,
                "toward-positive {input}"
            );
            assert_eq!(
                to_whole(input, TowardNegative),
                negative,
                "toward-negative {input}"
            );
        }
    }

    #[test]
    fn halfway_thousandths_round_to_hundredths() {
        let cases = [
            ("1.245", RoundingMode::HalfEven, "1.24"),
            ("-1.245", RoundingMode::HalfEven, "-1.24"),
            ("1.245", RoundingMode::HalfAwayFromZero, "1.25"),
            ("-1.245", RoundingMode::HalfAwayFromZero, "-1.25"),
            ("1.255", RoundingMode::HalfEven, "1.26"),
        ];
        for (input, mode, expected) in cases {
            let result = dec(input, 4, 3).quantize(ty(3, 2), mode).expect("fits");
            assert_eq!(result.value.to_string(), expected, "{input} {mode:?}");
            assert!(result.value_changed);
            assert!(result.scale_changed);
        }
    }

    #[test]
    fn quantize_reports_value_and_scale_changes_separately() {
        let same = dec("1.50", 3, 2)
            .quantize(ty(3, 2), RoundingMode::HalfEven)
            .expect("fits");
        assert!(!same.value_changed && !same.scale_changed);

        let trimmed = dec("1.50", 3, 2)
            .quantize(ty(3, 1), RoundingMode::HalfEven)
            .expect("fits");
        assert_eq!(trimmed.value.to_string(), "1.5");
        assert!(!trimmed.value_changed && trimmed.scale_changed);

        let padded = dec("1.5", 2, 1)
            .quantize(ty(5, 4), RoundingMode::TowardZero)
            .expect("fits");
        assert_eq!(padded.value.to_string(), "1.5000");
        assert!(!padded.value_changed && padded.scale_changed);
    }

    #[test]
    fn quantize_refuses_instead_of_clipping() {
        // Padding 12.5 to scale 3 needs five digits.
        assert_eq!(
            dec("12.5", 3, 1).quantize(ty(4, 3), RoundingMode::HalfEven),
            Err(ExactError::PrecisionExceeded)
        );
        // Rounding 9.5 up to 10 needs two digits.
        assert_eq!(
            dec("9.5", 2, 1).quantize(ty(1, 0), RoundingMode::HalfAwayFromZero),
            Err(ExactError::PrecisionExceeded)
        );
        // Padding the widest coefficient overflows the bound.
        assert_eq!(
            dec("99999999999999999999999999999999999999", 38, 0)
                .quantize(ty(38, 1), RoundingMode::HalfEven),
            Err(ExactError::PrecisionExceeded)
        );
    }

    #[test]
    fn rounding_at_the_widest_scale_does_not_overflow() {
        let value = dec("0.99999999999999999999999999999999999999", 38, 38);
        let rounded = value
            .quantize(ty(1, 0), RoundingMode::HalfEven)
            .expect("fits");
        assert_eq!(rounded.value.to_string(), "1");
        let half = dec("0.50000000000000000000000000000000000000", 38, 38);
        let even = half
            .quantize(ty(1, 0), RoundingMode::HalfEven)
            .expect("fits");
        assert_eq!(even.value.to_string(), "0");
    }

    #[test]
    fn rounding_a_small_negative_to_zero_has_no_sign() {
        let result = dec("-0.004", 3, 3)
            .quantize(ty(3, 2), RoundingMode::HalfEven)
            .expect("fits");
        assert_eq!(result.value.to_string(), "0.00");
        assert!(result.value_changed);
    }
}
