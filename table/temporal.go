// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"strconv"
	"strings"
)

// Calendar dates and timestamps.
//
// Grammar:
//
//   - date: YYYY-MM-DD, years 0001 to 9999, proleptic Gregorian;
//   - local timestamp: date, T, HH:MM:SS, then a point and exactly the
//     declared number of fractional digits (no point at precision zero);
//   - offset timestamp: a local timestamp followed by Z or +HH:MM or -HH:MM.
//
// Hours run 00 to 23; minutes and seconds 00 to 59. Leap seconds, 24:00,
// lowercase designators, a space separator and -00:00 are refused. Z and
// +00:00 are kept distinct as written.
//
// A timestamp is judged in two parts. First, whether the text is a
// well-formed timestamp of either type: shape, calendar, clock, a point
// followed by at least one digit, and an offset that is absent, Z or a
// signed HH:MM in range other than -00:00. Only a well-formed timestamp is
// then compared with the declared type: the width of its fraction and then
// the presence of its offset.

// The refusals of temporal values.
var (
	errTemporalMalformed          = refuse(CategoryInvalidInput, "temporal-malformed")
	errTemporalInvalidDate        = refuse(CategoryInvalidInput, "temporal-invalid-date")
	errTemporalInvalidTime        = refuse(CategoryInvalidInput, "temporal-invalid-time")
	errTemporalInvalidOffset      = refuse(CategoryInvalidInput, "temporal-invalid-offset")
	errTemporalNegativeZeroOffset = refuse(CategoryInvalidInput, "temporal-negative-zero-offset")
	errTemporalFractionMismatch   = refuse(CategorySchemaMismatch, "temporal-fraction-mismatch")
	errTemporalOffsetMismatch     = refuse(CategorySchemaMismatch, "temporal-offset-mismatch")
)

// FractionPrecision is a declared number of fractional-second digits, 0 to
// MaxFractionDigits. The zero value is zero digits.
type FractionPrecision struct{ digits uint8 }

// NewFractionPrecision creates a precision. It refuses, with the rule
// schema-member-invalid, a number of digits outside 0 to MaxFractionDigits.
func NewFractionPrecision(digits int) (FractionPrecision, error) {
	if digits < 0 || digits > MaxFractionDigits {
		return FractionPrecision{}, errTypeInvalid.fresh()
	}
	return FractionPrecision{digits: uint8(digits)}, nil
}

// Digits is the number of fractional digits.
func (p FractionPrecision) Digits() int { return int(p.digits) }

// digits reads text, which is all ASCII digits, as a number.
func digits(text string) (uint32, bool) {
	var value uint32
	for i := 0; i < len(text); i++ {
		b := text[i]
		if b < '0' || b > '9' {
			return 0, false
		}
		// Callers pass at most nine digits, which fits a uint32.
		value = value*10 + uint32(b-'0')
	}
	return value, true
}

func isLeapYear(year uint32) bool {
	return (year%4 == 0 && year%100 != 0) || year%400 == 0
}

func daysInMonth(year, month uint32) uint32 {
	switch month {
	case 4, 6, 9, 11:
		return 30
	case 2:
		if isLeapYear(year) {
			return 29
		}
		return 28
	}
	return 31
}

func pad(value uint32, width int) string {
	text := strconv.FormatUint(uint64(value), 10)
	if len(text) < width {
		return strings.Repeat("0", width-len(text)) + text
	}
	return text
}

// Date is a calendar date in the proleptic Gregorian calendar.
//
// Values compare equal with == when they name the same day.
type Date struct {
	year  uint16
	month uint8
	day   uint8
}

// ParseDate parses YYYY-MM-DD. It refuses text of the wrong shape and dates
// that do not exist, with a *Refusal naming the rule.
func ParseDate(text string) (Date, error) {
	date, err := parseDate(text)
	if err != nil {
		return Date{}, err.fresh()
	}
	return date, nil
}

func parseDate(text string) (Date, *Refusal) {
	if len(text) != 10 || text[4] != '-' || text[7] != '-' {
		return Date{}, errTemporalMalformed
	}
	year, okYear := digits(text[0:4])
	month, okMonth := digits(text[5:7])
	day, okDay := digits(text[8:10])
	if !okYear || !okMonth || !okDay {
		return Date{}, errTemporalMalformed
	}
	if year == 0 || month < 1 || month > 12 || day == 0 || day > daysInMonth(year, month) {
		return Date{}, errTemporalInvalidDate
	}
	return Date{year: uint16(year), month: uint8(month), day: uint8(day)}, nil
}

// Year is the year, 1 to 9999.
func (d Date) Year() int { return int(d.year) }

// Month is the month, 1 to 12.
func (d Date) Month() int { return int(d.month) }

// Day is the day of the month, starting at 1.
func (d Date) Day() int { return int(d.day) }

// String is the text YYYY-MM-DD.
func (d Date) String() string {
	return pad(uint32(d.year), 4) + "-" + pad(uint32(d.month), 2) + "-" + pad(uint32(d.day), 2)
}

// parts are the parts of a well-formed timestamp of either type, before it
// is compared with a declared type.
type parts struct {
	date                 Date
	hour, minute, second uint8
	// fraction is the digits after the point, at least one, when hasPoint.
	fraction string
	hasPoint bool
	// offset is the offset, when hasOffset.
	offset    Offset
	hasOffset bool
}

// parseParts judges whether text is a well-formed timestamp, with or
// without an offset and with a fraction of any width.
func parseParts(text string) (parts, *Refusal) {
	// Date, T and HH:MM:SS occupy the first nineteen bytes.
	if len(text) < 19 || text[10] != 'T' || text[13] != ':' || text[16] != ':' {
		return parts{}, errTemporalMalformed
	}
	date, err := parseDate(text[0:10])
	if err != nil {
		return parts{}, err
	}
	hour, okHour := digits(text[11:13])
	minute, okMinute := digits(text[14:16])
	second, okSecond := digits(text[17:19])
	if !okHour || !okMinute || !okSecond {
		return parts{}, errTemporalMalformed
	}
	if hour > 23 || minute > 59 || second > 59 {
		return parts{}, errTemporalInvalidTime
	}
	p := parts{date: date, hour: uint8(hour), minute: uint8(minute), second: uint8(second)}
	rest := text[19:]
	// The fraction is a point and the longest run of digits after it.
	if after, ok := strings.CutPrefix(rest, "."); ok {
		run := 0
		for run < len(after) && after[run] >= '0' && after[run] <= '9' {
			run++
		}
		if run == 0 {
			return parts{}, errTemporalMalformed
		}
		p.fraction, p.hasPoint = after[:run], true
		rest = after[run:]
	}
	if rest != "" {
		offset, err := parseOffset(rest)
		if err != nil {
			return parts{}, err
		}
		p.offset, p.hasOffset = offset, true
	}
	return p, nil
}

// local is the date, time and fraction as a value of the declared
// precision.
func (p parts) local(precision FractionPrecision) (LocalTimestamp, *Refusal) {
	if len(p.fraction) != int(precision.digits) {
		return LocalTimestamp{}, errTemporalFractionMismatch
	}
	// At most nine digits, checked above.
	fraction, _ := digits(p.fraction)
	return LocalTimestamp{
		date:      p.date,
		hour:      p.hour,
		minute:    p.minute,
		second:    p.second,
		fraction:  fraction,
		precision: precision,
	}, nil
}

// LocalTimestamp is a date and time of day with no offset.
//
// Equality with == compares the fields and the declared precision as
// written. No ordering is defined.
type LocalTimestamp struct {
	date                 Date
	hour, minute, second uint8
	fraction             uint32
	precision            FractionPrecision
}

// ParseLocalTimestamp parses YYYY-MM-DDTHH:MM:SS followed by a point and
// exactly precision fractional digits, or by nothing at precision zero.
//
// It refuses, in this order: text that is not a well-formed timestamp (wrong
// shape, a date that does not exist, a time out of range, a point with no
// digit after it, a malformed or negative-zero offset); a fraction that does
// not match the declared precision; and a well-formed timestamp that carries
// an offset.
func ParseLocalTimestamp(text string, precision FractionPrecision) (LocalTimestamp, error) {
	value, err := parseLocalTimestamp(text, precision)
	if err != nil {
		return LocalTimestamp{}, err.fresh()
	}
	return value, nil
}

func parseLocalTimestamp(text string, precision FractionPrecision) (LocalTimestamp, *Refusal) {
	p, err := parseParts(text)
	if err != nil {
		return LocalTimestamp{}, err
	}
	value, err := p.local(precision)
	if err != nil {
		return LocalTimestamp{}, err
	}
	if p.hasOffset {
		return LocalTimestamp{}, errTemporalOffsetMismatch
	}
	return value, nil
}

// Date is the calendar date.
func (t LocalTimestamp) Date() Date { return t.date }

// Hour is the hour, 0 to 23.
func (t LocalTimestamp) Hour() int { return int(t.hour) }

// Minute is the minute, 0 to 59.
func (t LocalTimestamp) Minute() int { return int(t.minute) }

// Second is the second, 0 to 59.
func (t LocalTimestamp) Second() int { return int(t.second) }

// Fraction is the fractional second as a count of units of 10^-Precision
// seconds.
func (t LocalTimestamp) Fraction() uint32 { return t.fraction }

// Precision is the declared fractional precision.
func (t LocalTimestamp) Precision() FractionPrecision { return t.precision }

// String is the text of the timestamp at its declared precision.
func (t LocalTimestamp) String() string {
	text := t.date.String() + "T" + pad(uint32(t.hour), 2) + ":" + pad(uint32(t.minute), 2) +
		":" + pad(uint32(t.second), 2)
	if t.precision.digits > 0 {
		text += "." + pad(t.fraction, int(t.precision.digits))
	}
	return text
}

// Offset is a fixed offset from UTC, kept as written. Z and +00:00 are
// different values that both denote zero minutes.
type Offset struct {
	designator bool
	negative   bool
	hours      uint8
	minutes    uint8
}

func parseOffset(text string) (Offset, *Refusal) {
	if text == "Z" {
		return Offset{designator: true}, nil
	}
	if len(text) != 6 || text[3] != ':' {
		return Offset{}, errTemporalInvalidOffset
	}
	var negative bool
	switch text[0] {
	case '+':
	case '-':
		negative = true
	default:
		return Offset{}, errTemporalInvalidOffset
	}
	hours, okHours := digits(text[1:3])
	minutes, okMinutes := digits(text[4:6])
	if !okHours || !okMinutes || hours > 23 || minutes > 59 {
		return Offset{}, errTemporalInvalidOffset
	}
	if negative && hours == 0 && minutes == 0 {
		return Offset{}, errTemporalNegativeZeroOffset
	}
	return Offset{negative: negative, hours: uint8(hours), minutes: uint8(minutes)}, nil
}

// UTCDesignator reports whether the offset is written Z rather than as
// hours and minutes.
func (o Offset) UTCDesignator() bool { return o.designator }

// Minutes is the offset east of UTC in minutes; negative values are west.
func (o Offset) Minutes() int {
	total := int(o.hours)*60 + int(o.minutes)
	if o.negative {
		return -total
	}
	return total
}

// String is the offset as written: Z, +HH:MM or -HH:MM.
func (o Offset) String() string {
	if o.designator {
		return "Z"
	}
	sign := "+"
	if o.negative {
		sign = "-"
	}
	return sign + pad(uint32(o.hours), 2) + ":" + pad(uint32(o.minutes), 2)
}

// OffsetTimestamp is a date and time of day with an explicit fixed offset
// from UTC.
//
// Equality with == compares the fields as written: two values naming the
// same instant at different offsets are not equal.
type OffsetTimestamp struct {
	local  LocalTimestamp
	offset Offset
}

// ParseOffsetTimestamp parses a local timestamp followed by Z, +HH:MM or
// -HH:MM.
//
// It refuses, in this order: text that is not a well-formed timestamp, as
// ParseLocalTimestamp does; a fraction that does not match the declared
// precision; and a well-formed timestamp that carries no offset.
func ParseOffsetTimestamp(text string, precision FractionPrecision) (OffsetTimestamp, error) {
	value, err := parseOffsetTimestamp(text, precision)
	if err != nil {
		return OffsetTimestamp{}, err.fresh()
	}
	return value, nil
}

func parseOffsetTimestamp(text string, precision FractionPrecision) (OffsetTimestamp, *Refusal) {
	p, err := parseParts(text)
	if err != nil {
		return OffsetTimestamp{}, err
	}
	local, err := p.local(precision)
	if err != nil {
		return OffsetTimestamp{}, err
	}
	if !p.hasOffset {
		return OffsetTimestamp{}, errTemporalOffsetMismatch
	}
	return OffsetTimestamp{local: local, offset: p.offset}, nil
}

// Local is the date and time of day as written, without the offset.
func (t OffsetTimestamp) Local() LocalTimestamp { return t.local }

// Offset is the offset as written.
func (t OffsetTimestamp) Offset() Offset { return t.offset }

// String is the text of the timestamp with its offset as written.
func (t OffsetTimestamp) String() string { return t.local.String() + t.offset.String() }
