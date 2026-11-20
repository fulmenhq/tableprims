// SPDX-License-Identifier: MIT OR Apache-2.0

//! Calendar dates and timestamps.
//!
//! Three logical values are supported: a calendar date, a local timestamp
//! with no offset, and a timestamp with a fixed offset from UTC. Values are
//! held as validated fields exactly as written. Nothing here consults a
//! clock, a host time zone or a time-zone database, and no value is
//! converted to another offset. The grammar and the order in which its
//! rules are judged are those of the Tabular Exchange Standard, contract
//! revision `tabular-exchange/0.1.0`, published in fulmenhq/crucible.
//!
//! Grammar:
//!
//! - date: `YYYY-MM-DD`, years `0001` to `9999`, proleptic Gregorian;
//! - local timestamp: date, `T`, `HH:MM:SS`, then a point and exactly the
//!   declared number of fractional digits (no point at precision zero);
//! - offset timestamp: a local timestamp followed by `Z` or `+HH:MM` or
//!   `-HH:MM`.
//!
//! Hours run `00` to `23`; minutes and seconds `00` to `59`. Leap seconds,
//! `24:00`, lowercase designators, a space separator and `-00:00` are
//! refused. `Z` and `+00:00` are kept distinct as written.
//!
//! A timestamp is judged in two parts. First, whether the text is a
//! well-formed timestamp of either type: shape, calendar, clock, a point
//! followed by at least one digit, and an offset that is absent, `Z` or a
//! signed `HH:MM` in range other than `-00:00`. Only a well-formed timestamp
//! is then compared with the declared type: the width of its fraction
//! ([`TemporalError::FractionMismatch`]) and then the presence of its offset
//! ([`TemporalError::OffsetMismatch`]). A text with a malformed offset is
//! therefore never reported as a fraction mismatch, and a well-formed value
//! of the other timestamp type is never reported as malformed.

use core::fmt;

/// Largest number of fractional-second digits.
pub const MAX_FRACTION_DIGITS: u8 = 9;

/// Why a temporal value was refused.
///
/// The messages never include the offending text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TemporalError {
    /// The text does not have the required shape.
    Malformed,
    /// Year, month or day is outside the calendar.
    InvalidDate,
    /// Hour, minute or second is out of range.
    InvalidTime,
    /// The fractional digits do not match the declared precision.
    FractionMismatch,
    /// What follows the seconds and the fraction is not an offset: it is
    /// neither `Z` nor a signed `HH:MM` in range.
    InvalidOffset,
    /// The offset is written `-00:00`.
    NegativeZeroOffset,
    /// A declared precision exceeds [`MAX_FRACTION_DIGITS`].
    InvalidPrecision,
    /// A well-formed timestamp of the other type: a local timestamp was
    /// expected and the value has an offset, or an offset timestamp was
    /// expected and the value has none.
    OffsetMismatch,
}

impl fmt::Display for TemporalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "value does not have the required shape",
            Self::InvalidDate => "date is outside the calendar",
            Self::InvalidTime => "time of day is out of range",
            Self::FractionMismatch => "fractional digits do not match the declared precision",
            Self::InvalidOffset => "offset is malformed or out of range",
            Self::NegativeZeroOffset => "offset of zero must not be negative",
            Self::InvalidPrecision => "precision is out of range",
            Self::OffsetMismatch => {
                "offset is present where none is declared, or absent where one is"
            }
        })
    }
}

impl std::error::Error for TemporalError {}

/// Declared number of fractional-second digits, `0` to
/// [`MAX_FRACTION_DIGITS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FractionPrecision(u8);

impl FractionPrecision {
    /// Creates a precision.
    ///
    /// # Errors
    ///
    /// Refuses more than [`MAX_FRACTION_DIGITS`] digits.
    pub fn new(digits: u8) -> Result<Self, TemporalError> {
        if digits > MAX_FRACTION_DIGITS {
            return Err(TemporalError::InvalidPrecision);
        }
        Ok(Self(digits))
    }

    /// Number of fractional digits.
    #[must_use]
    pub fn digits(self) -> u8 {
        self.0
    }
}

/// Reads exactly `bytes.len()` ASCII digits as a number.
fn digits(bytes: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        // Callers pass at most nine digits, which fits in u32.
        value = value * 10 + u32::from(byte - b'0');
    }
    Some(value)
}

fn is_leap_year(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 31,
    }
}

/// A calendar date in the proleptic Gregorian calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// Parses `YYYY-MM-DD`.
    ///
    /// # Errors
    ///
    /// Refuses text of the wrong shape and dates that do not exist.
    pub fn parse(text: &str) -> Result<Self, TemporalError> {
        Self::parse_bytes(text.as_bytes())
    }

    fn parse_bytes(bytes: &[u8]) -> Result<Self, TemporalError> {
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(TemporalError::Malformed);
        }
        let year = digits(&bytes[0..4]).ok_or(TemporalError::Malformed)?;
        let month = digits(&bytes[5..7]).ok_or(TemporalError::Malformed)?;
        let day = digits(&bytes[8..10]).ok_or(TemporalError::Malformed)?;
        // Four, two and two digits fit their fields.
        let (year, month, day) = (year as u16, month as u8, day as u8);
        if year == 0 || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return Err(TemporalError::InvalidDate);
        }
        Ok(Self { year, month, day })
    }

    /// Year, `1` to `9999`.
    #[must_use]
    pub fn year(self) -> u16 {
        self.year
    }

    /// Month, `1` to `12`.
    #[must_use]
    pub fn month(self) -> u8 {
        self.month
    }

    /// Day of the month, starting at `1`.
    #[must_use]
    pub fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// The parts of a well-formed timestamp of either type, before it is
/// compared with a declared type.
struct Parts<'a> {
    date: Date,
    hour: u8,
    minute: u8,
    second: u8,
    /// The digits after the point, at least one; `None` without a point.
    fraction: Option<&'a [u8]>,
    /// The offset; `None` when nothing follows the seconds and the fraction.
    offset: Option<Offset>,
}

impl<'a> Parts<'a> {
    /// Judges whether `bytes` is a well-formed timestamp, with or without
    /// an offset and with a fraction of any width.
    fn parse(bytes: &'a [u8]) -> Result<Self, TemporalError> {
        // Date, 'T' and HH:MM:SS occupy the first nineteen bytes.
        if bytes.len() < 19 || bytes[10] != b'T' || bytes[13] != b':' || bytes[16] != b':' {
            return Err(TemporalError::Malformed);
        }
        let date = Date::parse_bytes(&bytes[0..10])?;
        let hour = digits(&bytes[11..13]).ok_or(TemporalError::Malformed)?;
        let minute = digits(&bytes[14..16]).ok_or(TemporalError::Malformed)?;
        let second = digits(&bytes[17..19]).ok_or(TemporalError::Malformed)?;
        if hour > 23 || minute > 59 || second > 59 {
            return Err(TemporalError::InvalidTime);
        }
        let tail = &bytes[19..];
        // The fraction is a point and the longest run of digits after it.
        let (fraction, rest) = match tail.strip_prefix(b".") {
            Some(after) => {
                let run = after.iter().take_while(|b| b.is_ascii_digit()).count();
                if run == 0 {
                    return Err(TemporalError::Malformed);
                }
                let (fraction, rest) = after.split_at(run);
                (Some(fraction), rest)
            }
            None => (None, tail),
        };
        let offset = if rest.is_empty() {
            None
        } else {
            Some(Offset::parse_bytes(rest)?)
        };
        Ok(Self {
            date,
            // Two digits each, checked against their ranges above.
            hour: hour as u8,
            minute: minute as u8,
            second: second as u8,
            fraction,
            offset,
        })
    }

    /// The date, time and fraction as a value of the declared precision.
    fn local(&self, precision: FractionPrecision) -> Result<LocalTimestamp, TemporalError> {
        let written = self.fraction.unwrap_or_default();
        if written.len() != usize::from(precision.0) {
            return Err(TemporalError::FractionMismatch);
        }
        // At most nine digits, checked above.
        let fraction = digits(written).ok_or(TemporalError::Malformed)?;
        Ok(LocalTimestamp {
            date: self.date,
            hour: self.hour,
            minute: self.minute,
            second: self.second,
            fraction,
            precision,
        })
    }
}

/// A date and time of day with no offset.
///
/// Equality compares the fields and the declared precision as written. No
/// ordering is defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalTimestamp {
    date: Date,
    hour: u8,
    minute: u8,
    second: u8,
    fraction: u32,
    precision: FractionPrecision,
}

impl LocalTimestamp {
    /// Parses `YYYY-MM-DDTHH:MM:SS` followed by a point and exactly
    /// `precision` fractional digits, or by nothing at precision zero.
    ///
    /// # Errors
    ///
    /// Refuses, in this order: text that is not a well-formed timestamp
    /// (wrong shape, a date that does not exist, a time out of range, a
    /// point with no digit after it, a malformed or negative-zero offset); a
    /// fraction that does not match the declared precision; and a
    /// well-formed timestamp that carries an offset.
    pub fn parse(text: &str, precision: FractionPrecision) -> Result<Self, TemporalError> {
        let parts = Parts::parse(text.as_bytes())?;
        let value = parts.local(precision)?;
        match parts.offset {
            None => Ok(value),
            Some(_) => Err(TemporalError::OffsetMismatch),
        }
    }

    /// Calendar date.
    #[must_use]
    pub fn date(self) -> Date {
        self.date
    }

    /// Hour, `0` to `23`.
    #[must_use]
    pub fn hour(self) -> u8 {
        self.hour
    }

    /// Minute, `0` to `59`.
    #[must_use]
    pub fn minute(self) -> u8 {
        self.minute
    }

    /// Second, `0` to `59`.
    #[must_use]
    pub fn second(self) -> u8 {
        self.second
    }

    /// Fractional second as an integer count of units of
    /// `10^-precision` seconds.
    #[must_use]
    pub fn fraction(self) -> u32 {
        self.fraction
    }

    /// Declared fractional precision.
    #[must_use]
    pub fn precision(self) -> FractionPrecision {
        self.precision
    }
}

impl fmt::Display for LocalTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}T{:02}:{:02}:{:02}",
            self.date, self.hour, self.minute, self.second
        )?;
        if self.precision.0 > 0 {
            write!(
                f,
                ".{:0width$}",
                self.fraction,
                width = usize::from(self.precision.0)
            )?;
        }
        Ok(())
    }
}

/// A fixed offset from UTC, kept as written.
///
/// `Z` and `+00:00` are different values that both denote zero minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Offset {
    designator: bool,
    negative: bool,
    hours: u8,
    minutes: u8,
}

impl Offset {
    fn parse_bytes(bytes: &[u8]) -> Result<Self, TemporalError> {
        if bytes == b"Z" {
            return Ok(Self {
                designator: true,
                negative: false,
                hours: 0,
                minutes: 0,
            });
        }
        if bytes.len() != 6 || bytes[3] != b':' {
            return Err(TemporalError::InvalidOffset);
        }
        let negative = match bytes[0] {
            b'+' => false,
            b'-' => true,
            _ => return Err(TemporalError::InvalidOffset),
        };
        let hours = digits(&bytes[1..3]).ok_or(TemporalError::InvalidOffset)?;
        let minutes = digits(&bytes[4..6]).ok_or(TemporalError::InvalidOffset)?;
        if hours > 23 || minutes > 59 {
            return Err(TemporalError::InvalidOffset);
        }
        if negative && hours == 0 && minutes == 0 {
            return Err(TemporalError::NegativeZeroOffset);
        }
        Ok(Self {
            designator: false,
            negative,
            hours: hours as u8,
            minutes: minutes as u8,
        })
    }

    /// Whether the offset is written `Z` rather than as hours and minutes.
    #[must_use]
    pub fn is_utc_designator(self) -> bool {
        self.designator
    }

    /// Offset east of UTC in minutes; negative values are west.
    #[must_use]
    pub fn total_minutes(self) -> i16 {
        let magnitude = i16::from(self.hours) * 60 + i16::from(self.minutes);
        if self.negative {
            -magnitude
        } else {
            magnitude
        }
    }
}

impl fmt::Display for Offset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.designator {
            return f.write_str("Z");
        }
        write!(
            f,
            "{}{:02}:{:02}",
            if self.negative { '-' } else { '+' },
            self.hours,
            self.minutes
        )
    }
}

/// A date and time of day with an explicit fixed offset from UTC.
///
/// Equality compares the fields as written: two values naming the same
/// instant at different offsets are not equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OffsetTimestamp {
    local: LocalTimestamp,
    offset: Offset,
}

impl OffsetTimestamp {
    /// Parses a local timestamp followed by `Z`, `+HH:MM` or `-HH:MM`.
    ///
    /// # Errors
    ///
    /// Refuses, in this order: text that is not a well-formed timestamp,
    /// as [`LocalTimestamp::parse`] does; a fraction that does not match the
    /// declared precision; and a well-formed timestamp that carries no
    /// offset.
    pub fn parse(text: &str, precision: FractionPrecision) -> Result<Self, TemporalError> {
        let parts = Parts::parse(text.as_bytes())?;
        let local = parts.local(precision)?;
        match parts.offset {
            Some(offset) => Ok(Self { local, offset }),
            None => Err(TemporalError::OffsetMismatch),
        }
    }

    /// Date and time of day as written, without the offset.
    #[must_use]
    pub fn local(self) -> LocalTimestamp {
        self.local
    }

    /// Offset as written.
    #[must_use]
    pub fn offset(self) -> Offset {
        self.offset
    }
}

impl fmt::Display for OffsetTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.local, self.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(digits: u8) -> FractionPrecision {
        FractionPrecision::new(digits).unwrap()
    }

    #[test]
    fn precision_is_bounded() {
        assert!(FractionPrecision::new(9).is_ok());
        assert_eq!(
            FractionPrecision::new(10),
            Err(TemporalError::InvalidPrecision)
        );
    }

    #[test]
    fn dates_round_trip() {
        for text in ["0001-01-01", "2024-02-29", "2000-02-29", "9999-12-31"] {
            assert_eq!(Date::parse(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn impossible_dates_are_refused() {
        for text in [
            "2023-02-29",
            "1900-02-29",
            "2024-04-31",
            "2024-00-10",
            "2024-13-01",
            "2024-01-00",
            "2024-01-32",
            "0000-01-01",
        ] {
            assert_eq!(Date::parse(text), Err(TemporalError::InvalidDate), "{text}");
        }
    }

    #[test]
    fn misshapen_dates_are_refused() {
        for text in [
            "",
            "2024-1-01",
            "24-01-01",
            "2024/01/01",
            "2024-01-01 ",
            " 2024-01-01",
            "+2024-01-01",
            "2024-W01-1",
            "2024-001",
            "20240101",
            "2024-01-0१",
        ] {
            assert_eq!(Date::parse(text), Err(TemporalError::Malformed), "{text:?}");
        }
    }

    #[test]
    fn local_timestamps_round_trip_at_declared_precision() {
        let cases = [
            ("2024-02-29T00:00:00", 0),
            ("2024-02-29T23:59:59", 0),
            ("2024-02-29T12:34:56.7", 1),
            ("2024-02-29T12:34:56.000", 3),
            ("2024-02-29T12:34:56.007", 3),
            ("2024-02-29T12:34:56.123456789", 9),
        ];
        for (text, digits) in cases {
            let value = LocalTimestamp::parse(text, p(digits)).unwrap();
            assert_eq!(value.to_string(), text);
            assert_eq!(value.precision().digits(), digits);
        }
        let value = LocalTimestamp::parse("2024-02-29T12:34:56.007", p(3)).unwrap();
        assert_eq!(value.fraction(), 7);
        assert_eq!((value.hour(), value.minute(), value.second()), (12, 34, 56));
    }

    #[test]
    fn fraction_must_match_declared_precision() {
        let cases = [
            ("2024-01-01T00:00:00.0", 0),
            ("2024-01-01T00:00:00", 3),
            ("2024-01-01T00:00:00.12", 3),
            ("2024-01-01T00:00:00.1234", 3),
            // More digits than any column declares is still well formed.
            ("2024-01-01T00:00:00.1234567890", 9),
            ("2024-01-01T00:00:00.12345678901234567890", 0),
        ];
        for (text, digits) in cases {
            assert_eq!(
                LocalTimestamp::parse(text, p(digits)),
                Err(TemporalError::FractionMismatch),
                "{text}"
            );
        }
    }

    #[test]
    fn a_point_needs_a_digit_whatever_the_declared_precision() {
        for digits in [0, 3] {
            for text in ["2026-11-19T08:30:00.", "2026-11-19T08:30:00.Z"] {
                assert_eq!(
                    LocalTimestamp::parse(text, p(digits)),
                    Err(TemporalError::Malformed),
                    "{text} at {digits}"
                );
                assert_eq!(
                    OffsetTimestamp::parse(text, p(digits)),
                    Err(TemporalError::Malformed),
                    "{text} at {digits}"
                );
            }
        }
    }

    #[test]
    fn a_well_formed_timestamp_of_the_other_type_is_an_offset_mismatch() {
        for (text, digits) in [
            ("2026-11-19T08:30:00Z", 0),
            ("2026-11-19T08:30:00+01:00", 0),
            ("2026-11-19T08:30:00+00:00", 0),
            ("2026-11-19T08:30:00.000Z", 3),
            ("2026-11-19T08:30:00.123456789+05:30", 9),
        ] {
            assert_eq!(
                LocalTimestamp::parse(text, p(digits)),
                Err(TemporalError::OffsetMismatch),
                "{text}"
            );
        }
        for (text, digits) in [
            ("2026-11-19T08:30:00", 0),
            ("2026-11-19T08:30:00.123", 3),
            ("2026-11-19T08:30:00.123456789", 9),
        ] {
            assert_eq!(
                OffsetTimestamp::parse(text, p(digits)),
                Err(TemporalError::OffsetMismatch),
                "{text}"
            );
        }
    }

    #[test]
    fn well_formedness_is_judged_before_the_declared_type() {
        use TemporalError::{InvalidOffset, NegativeZeroOffset};
        // The fraction width does not fit either; the offset decides.
        for (text, digits, expected) in [
            ("2026-11-19T08:30:00,000", 3, InvalidOffset),
            ("2026-11-19T08:30:00.12+24:00", 3, InvalidOffset),
            ("2026-11-19T08:30:00-00:00", 3, NegativeZeroOffset),
            ("2026-11-19T08:30:00.5+24:00", 0, InvalidOffset),
        ] {
            assert_eq!(
                LocalTimestamp::parse(text, p(digits)),
                Err(expected),
                "{text}"
            );
        }
        for (text, digits, expected) in [
            ("2026-11-19T08:30:00.12+99:99", 3, InvalidOffset),
            ("2026-11-19T08:30:00+99:99", 3, InvalidOffset),
            ("2026-11-19T08:30:00.12-00:00", 3, NegativeZeroOffset),
            ("2026-11-19T08:30:00.5+24:00", 0, InvalidOffset),
        ] {
            assert_eq!(
                OffsetTimestamp::parse(text, p(digits)),
                Err(expected),
                "{text}"
            );
        }
        // The fraction fits; a local timestamp still names the offset rule.
        for (text, digits, expected) in [
            ("2026-11-19T08:30:00 ", 0, InvalidOffset),
            ("2026-11-19T08:30:00+24:00", 0, InvalidOffset),
            ("2026-11-19T08:30:00+05:60", 0, InvalidOffset),
            ("2026-11-19T08:30:00z", 0, InvalidOffset),
            ("2026-11-19T08:30:00-00:00", 0, NegativeZeroOffset),
            ("2026-11-19T08:30:00.123-00:00", 3, NegativeZeroOffset),
        ] {
            assert_eq!(
                LocalTimestamp::parse(text, p(digits)),
                Err(expected),
                "{text}"
            );
        }
        // Calendar and clock come before the offset and the declared type.
        assert_eq!(
            LocalTimestamp::parse("2026-02-30T08:30:00+24:00", p(3)),
            Err(TemporalError::InvalidDate)
        );
        assert_eq!(
            OffsetTimestamp::parse("2026-11-19T24:00:00", p(3)),
            Err(TemporalError::InvalidTime)
        );
        // A fraction mismatch is reported before an offset mismatch.
        assert_eq!(
            LocalTimestamp::parse("2026-11-19T08:30:00.12Z", p(3)),
            Err(TemporalError::FractionMismatch)
        );
        assert_eq!(
            OffsetTimestamp::parse("2026-11-19T08:30:00", p(3)),
            Err(TemporalError::FractionMismatch)
        );
    }

    #[test]
    fn times_out_of_range_are_refused() {
        for text in [
            "2024-01-01T24:00:00",
            "2024-01-01T23:60:00",
            "2024-06-30T23:59:60",
        ] {
            assert_eq!(
                LocalTimestamp::parse(text, p(0)),
                Err(TemporalError::InvalidTime),
                "{text}"
            );
        }
    }

    #[test]
    fn misshapen_timestamps_are_refused() {
        for text in [
            "2024-01-01",
            "2024-01-01 00:00:00",
            "2024-01-01t00:00:00",
            "2024-01-01T00:00",
            "2024-01-01T0:00:000",
            "2024-01-01T00-00-00",
        ] {
            assert_eq!(
                LocalTimestamp::parse(text, p(0)),
                Err(TemporalError::Malformed),
                "{text}"
            );
        }
        // Text after the fraction that is not an offset.
        assert_eq!(
            LocalTimestamp::parse("2024-01-01T00:00:00.12x", p(3)),
            Err(TemporalError::InvalidOffset)
        );
        assert_eq!(
            LocalTimestamp::parse("2024-01-01T00:00:00.123x", p(3)),
            Err(TemporalError::InvalidOffset)
        );
        assert_eq!(
            LocalTimestamp::parse("2024-01-01T00:00:00,123", p(3)),
            Err(TemporalError::InvalidOffset)
        );
        // A local timestamp carries no offset.
        assert_eq!(
            LocalTimestamp::parse("2024-01-01T00:00:00Z", p(0)),
            Err(TemporalError::OffsetMismatch)
        );
    }

    #[test]
    fn offset_timestamps_round_trip_as_written() {
        let cases = [
            ("2024-02-29T12:34:56Z", 0),
            ("2024-02-29T12:34:56+00:00", 0),
            ("2024-02-29T12:34:56+05:30", 0),
            ("2024-02-29T12:34:56-08:00", 0),
            ("2024-02-29T12:34:56.123Z", 3),
            ("2024-02-29T12:34:56.123456789-00:01", 9),
            ("2024-02-29T12:34:56.5+23:59", 1),
        ];
        for (text, digits) in cases {
            let value = OffsetTimestamp::parse(text, p(digits)).unwrap();
            assert_eq!(value.to_string(), text);
        }
    }

    #[test]
    fn utc_designator_and_zero_offset_stay_distinct() {
        let z = OffsetTimestamp::parse("2024-01-01T00:00:00Z", p(0)).unwrap();
        let zero = OffsetTimestamp::parse("2024-01-01T00:00:00+00:00", p(0)).unwrap();
        assert_ne!(z, zero);
        assert!(z.offset().is_utc_designator());
        assert!(!zero.offset().is_utc_designator());
        assert_eq!(z.offset().total_minutes(), 0);
        assert_eq!(zero.offset().total_minutes(), 0);
        assert_eq!(z.local(), zero.local());
    }

    #[test]
    fn timestamps_of_different_precision_are_not_equal() {
        let tenths = LocalTimestamp::parse("2026-01-01T00:00:00.1", p(1)).unwrap();
        let hundredths = LocalTimestamp::parse("2026-01-01T00:00:00.10", p(2)).unwrap();
        assert_ne!(tenths, hundredths);
    }

    #[test]
    fn offset_minutes_are_signed() {
        let east = OffsetTimestamp::parse("2024-01-01T00:00:00+05:30", p(0)).unwrap();
        let west = OffsetTimestamp::parse("2024-01-01T00:00:00-08:00", p(0)).unwrap();
        assert_eq!(east.offset().total_minutes(), 330);
        assert_eq!(west.offset().total_minutes(), -480);
    }

    #[test]
    fn bad_offsets_are_refused() {
        for text in [
            "2024-01-01T00:00:00z",
            "2024-01-01T00:00:00+0530",
            "2024-01-01T00:00:00+05",
            "2024-01-01T00:00:00+5:30",
            "2024-01-01T00:00:00+24:00",
            "2024-01-01T00:00:00+05:60",
            "2024-01-01T00:00:00 +05:30",
            "2024-01-01T00:00:00+05:30 ",
            "2024-01-01T00:00:00ZZ",
            "2024-01-01T00:00:00[UTC]",
        ] {
            assert_eq!(
                OffsetTimestamp::parse(text, p(0)),
                Err(TemporalError::InvalidOffset),
                "{text:?}"
            );
        }
        assert_eq!(
            OffsetTimestamp::parse("2024-01-01T00:00:00-00:00", p(0)),
            Err(TemporalError::NegativeZeroOffset)
        );
        // A missing offset is a value of the other type, not a bad offset.
        assert_eq!(
            OffsetTimestamp::parse("2024-01-01T00:00:00", p(0)),
            Err(TemporalError::OffsetMismatch)
        );
    }

    #[test]
    fn offset_timestamp_fraction_errors_are_classified() {
        let cases = [
            ("2024-01-01T00:00:00.1Z", 0),
            ("2024-01-01T00:00:00Z", 3),
            ("2024-01-01T00:00:00.12Z", 3),
            ("2024-01-01T00:00:00.1234Z", 3),
            ("2024-01-01T00:00:00.123", 3),
        ];
        for (text, digits) in cases {
            let err = OffsetTimestamp::parse(text, p(digits)).unwrap_err();
            let expected = if text.ends_with('Z') {
                TemporalError::FractionMismatch
            } else {
                TemporalError::OffsetMismatch
            };
            assert_eq!(err, expected, "{text}");
        }
    }

    #[test]
    fn offset_timestamp_reports_date_and_time_errors() {
        assert_eq!(
            OffsetTimestamp::parse("2023-02-29T00:00:00Z", p(0)),
            Err(TemporalError::InvalidDate)
        );
        assert_eq!(
            OffsetTimestamp::parse("2024-01-01T24:00:00Z", p(0)),
            Err(TemporalError::InvalidTime)
        );
        assert_eq!(
            OffsetTimestamp::parse("2024-01-01", p(0)),
            Err(TemporalError::Malformed)
        );
    }
}
