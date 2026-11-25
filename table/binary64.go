// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"math/big"
	"strconv"
	"strings"
)

// Binary64 is the nearest IEEE 754 binary64 value of an exact number, ties
// to even, and two facts about it that are independent of each other: the
// integer 2^60 is exact but does not round-trip, and the decimal 0.1 round
// trips but is not exact. Which facts a destination requires, and any limit
// on digits, is the destination's rule. [XLSXWriteR1Binary64FromInteger]
// and [XLSXWriteR1Binary64FromDecimal] apply the rule of the XLSX write
// profile xlsx-write/r1; other destinations may use the facts directly.
//
// Exactness is decided in integer arithmetic. The round-trip fact uses
// strconv's correctly rounded parsing and shortest formatting, and the
// comparison with the number is made on digit strings.
type Binary64 struct {
	value      float64
	exact      bool
	roundTrips bool
}

// Value returns the nearest binary64 value. It is finite, and zero is
// positive.
func (b Binary64) Value() float64 { return b.value }

// Exact reports whether the binary64 value is the number itself.
func (b Binary64) Exact() bool { return b.exact }

// RoundTrips reports whether the shortest decimal spelling of the binary64
// value denotes the number. The shortest spelling has the fewest significant
// digits that parse back to the value and, among those, is the closest to
// it, ties to an even last digit, as strconv.FormatFloat prints it.
func (b Binary64) RoundTrips() bool { return b.roundTrips }

// Binary64 returns the nearest binary64 value of the integer and its facts.
func (i Integer) Binary64() Binary64 { return binary64Of(i.String(), i.magnitude, 0) }

// Binary64 returns the nearest binary64 value of the decimal and its facts.
// The declared scale is not carried by the value.
func (d Decimal) Binary64() Binary64 { return binary64Of(d.String(), d.magnitude, d.scale) }

func binary64Of(canonical string, magnitude u128, scale uint8) Binary64 {
	// ParseFloat rounds decimal text to the nearest binary64 value, ties to
	// even, and FormatFloat with precision -1 prints the shortest digits that
	// parse back to the same value. Every exact number is far inside the
	// binary64 range, so the parse cannot fail.
	value, err := strconv.ParseFloat(canonical, 64)
	if err != nil {
		panic("table: canonical exact-number text does not parse as binary64: " + err.Error())
	}
	if value == 0 {
		value = 0 // zero is positive
	}
	return Binary64{
		value:      value,
		exact:      exactInBinary64(magnitude.big(), scale),
		roundTrips: sameDecimalValue(strconv.FormatFloat(value, 'f', -1, 64), canonical),
	}
}

// exactInBinary64 reports whether magnitude / 10^scale is a binary64 value.
// It is when it is a binary fraction, that is when 5^scale divides the
// magnitude, whose odd part then fits in 53 bits. Every number of the exact
// domain is far inside the binary64 exponent range, so no other condition
// applies. The argument is consumed.
func exactInBinary64(magnitude *big.Int, scale uint8) bool {
	if magnitude.Sign() == 0 {
		return true
	}
	five := big.NewInt(5)
	quotient, remainder := new(big.Int), new(big.Int)
	for range scale {
		quotient.QuoRem(magnitude, five, remainder)
		if remainder.Sign() != 0 {
			return false
		}
		magnitude.Set(quotient)
	}
	magnitude.Rsh(magnitude, magnitude.TrailingZeroBits())
	return magnitude.BitLen() <= 53
}

// normalizedDecimal splits a plain decimal spelling, -?digits(.digits)?, into
// its sign, whole digits and fractional digits without redundant zeros.
func normalizedDecimal(text string) (negative bool, whole, fraction string, ok bool) {
	body, negative := strings.CutPrefix(text, "-")
	whole, fraction, hasPoint := strings.Cut(body, ".")
	if whole == "" || (hasPoint && fraction == "") {
		return false, "", "", false
	}
	for _, b := range []byte(whole + fraction) {
		if b < '0' || b > '9' {
			return false, "", "", false
		}
	}
	whole = strings.TrimLeft(whole, "0")
	fraction = strings.TrimRight(fraction, "0")
	// Zero has no sign.
	negative = negative && (whole != "" || fraction != "")
	return negative, whole, fraction, true
}

// sameDecimalValue reports whether two plain decimal spellings denote the
// same number. A spelling in any other form, including one with an
// exponent, equals nothing.
func sameDecimalValue(left, right string) bool {
	ln, lw, lf, lok := normalizedDecimal(left)
	rn, rw, rf, rok := normalizedDecimal(right)
	return lok && rok && ln == rn && lw == rw && lf == rf
}

// xlsxWriteR1MaxSignificantDigits is the largest number of significant
// digits, not counting leading or trailing zeros, of a value eligible under
// xlsx-write/r1. It differs from Decimal.SignificantDigits, which counts
// trailing zeros of the coefficient.
const xlsxWriteR1MaxSignificantDigits = 15

// Reasons a value is not eligible under a binary64 eligibility rule, as
// returned by [Binary64Error.Reason].
const (
	// Binary64TooManyDigits is a value with more significant digits than
	// the rule allows.
	Binary64TooManyDigits = "too-many-digits"
	// Binary64NotRoundTrip is a value that does not round-trip.
	Binary64NotRoundTrip = "not-round-trip"
	// Binary64NotExactInteger is an integer that is not exact.
	Binary64NotExactInteger = "not-exact-integer"
)

// Binary64Error is the error [XLSXWriteR1Binary64FromInteger] and
// [XLSXWriteR1Binary64FromDecimal] return for a value the rule does not
// admit. It concerns one value and is neither a [CapabilityError] nor a
// [Refusal]; a destination maps it to its own rule and location.
type Binary64Error struct {
	reason string
}

// Reason returns Binary64TooManyDigits, Binary64NotRoundTrip or
// Binary64NotExactInteger.
func (e *Binary64Error) Reason() string { return e.reason }

// Error describes the reason.
func (e *Binary64Error) Error() string {
	switch e.reason {
	case Binary64TooManyDigits:
		return "value has too many significant digits for a numeric cell"
	case Binary64NotRoundTrip:
		return "value does not survive storage as a binary number"
	case Binary64NotExactInteger:
		return "integer is not exactly representable as a binary number"
	default:
		return "value is not admitted as a binary number"
	}
}

// significantDigits counts the digits of a magnitude without its trailing
// zeros; zero has one.
func significantDigits(magnitude u128) int {
	digits := strings.TrimRight(magnitude.String(), "0")
	return max(len(digits), 1)
}

// XLSXWriteR1Binary64FromInteger returns the binary64 value to store for an
// integer under the rule of the XLSX write profile xlsx-write/r1: at most 15
// significant digits, not counting trailing zeros, then round-trips, then
// exact. Otherwise it returns a *Binary64Error with the first reason that
// applies, in that order. It grants no workbook capability.
func XLSXWriteR1Binary64FromInteger(i Integer) (float64, error) {
	if significantDigits(i.magnitude) > xlsxWriteR1MaxSignificantDigits {
		return 0, &Binary64Error{reason: Binary64TooManyDigits}
	}
	b := i.Binary64()
	if !b.roundTrips {
		return 0, &Binary64Error{reason: Binary64NotRoundTrip}
	}
	if !b.exact {
		return 0, &Binary64Error{reason: Binary64NotExactInteger}
	}
	return b.value, nil
}

// XLSXWriteR1Binary64FromDecimal returns the binary64 value to store for a
// decimal under the rule of the XLSX write profile xlsx-write/r1: at most 15
// significant digits, not counting trailing zeros, then round-trips. A
// decimal need not be exact. Otherwise it returns a *Binary64Error with the
// first reason that applies, in that order. The declared scale is not
// carried by the value. It grants no workbook capability.
func XLSXWriteR1Binary64FromDecimal(d Decimal) (float64, error) {
	if significantDigits(d.magnitude) > xlsxWriteR1MaxSignificantDigits {
		return 0, &Binary64Error{reason: Binary64TooManyDigits}
	}
	b := d.Binary64()
	if !b.roundTrips {
		return 0, &Binary64Error{reason: Binary64NotRoundTrip}
	}
	return b.value, nil
}
