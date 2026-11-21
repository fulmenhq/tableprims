// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"math/big"
	"math/bits"
	"strconv"
	"strings"
)

// Exact integers and decimals.
//
// Grammar:
//
//   - integer: 0 or -?[1-9][0-9]*, at most MaxDigits digits;
//   - decimal at scale zero: the integer grammar;
//   - decimal at scale s > 0: -?(0|[1-9][0-9]*)\.[0-9]{s}.
//
// No plus sign, whitespace, exponent or redundant leading zero is accepted,
// and zero carries no minus sign. Invalid spellings are refused, never
// normalized. The grammar and the MaxDigits bound are judged before any
// declared type.

// The refusals of exact values. Each is located at a row and a column when
// it comes from a row.
var (
	errExactEmpty             = refuse(CategoryInvalidInput, "exact-empty")
	errExactInvalidCharacter  = refuse(CategoryInvalidInput, "exact-invalid-character")
	errExactLeadingZero       = refuse(CategoryInvalidInput, "exact-leading-zero")
	errExactNegativeZero      = refuse(CategoryInvalidInput, "exact-negative-zero")
	errExactMalformed         = refuse(CategoryInvalidInput, "exact-malformed")
	errExactTooManyDigits     = refuse(CategoryResourceLimit, "exact-too-many-digits")
	errExactPointMismatch     = refuse(CategorySchemaMismatch, "exact-point-mismatch")
	errExactScaleMismatch     = refuse(CategorySchemaMismatch, "exact-scale-mismatch")
	errExactPrecisionExceeded = refuse(CategorySchemaMismatch, "exact-precision-exceeded")
	errQuantizeOverflow       = refuse(CategorySchemaMismatch, "quantize-overflow")
	// A type parameter out of range is what a schema document reports for
	// it.
	errTypeInvalid = refuse(CategoryInvalidInput, "schema-member-invalid")
	// A rounding mode that is not one of the five is what a policy document
	// reports for it.
	errRoundingInvalid = refuse(CategoryInvalidInput, "policy-member-invalid")
)

// fresh returns a copy, so that a caller cannot change a shared refusal.
func (r *Refusal) fresh() *Refusal {
	copied := *r
	return &copied
}

// u128 is an unsigned magnitude below 2^128. Every coefficient is below
// 10^38 < 2^127.
type u128 struct{ hi, lo uint64 }

// mul10add returns u*10 + digit. The callers keep the result below 10^38.
func (u u128) mul10add(digit uint64) u128 {
	carry, lo := bits.Mul64(u.lo, 10)
	hi := u.hi*10 + carry
	lo, c := bits.Add64(lo, digit, 0)
	return u128{hi: hi + c, lo: lo}
}

func (u u128) isZero() bool { return u.hi == 0 && u.lo == 0 }

// String is the decimal text of u.
func (u u128) String() string {
	if u.hi == 0 {
		return strconv.FormatUint(u.lo, 10)
	}
	const chunk = 10_000_000_000_000_000_000 // 10^19
	qhi, r := u.hi/chunk, u.hi%chunk
	qlo, low := bits.Div64(r, u.lo, chunk)
	tail := strconv.FormatUint(low, 10)
	return u128{hi: qhi, lo: qlo}.String() + strings.Repeat("0", 19-len(tail)) + tail
}

func (u u128) big() *big.Int {
	b := new(big.Int).SetUint64(u.hi)
	b.Lsh(b, 64)
	return b.Or(b, new(big.Int).SetUint64(u.lo))
}

// signedBig is the signed value of a magnitude.
func signedBig(negative bool, magnitude u128) *big.Int {
	b := magnitude.big()
	if negative {
		b.Neg(b)
	}
	return b
}

// fromBig splits b, whose magnitude is below 2^128, into a sign and a
// magnitude. Zero has no sign.
func fromBig(b *big.Int) (bool, u128) {
	magnitude := new(big.Int).Abs(b)
	lo := new(big.Int).And(magnitude, new(big.Int).SetUint64(^uint64(0))).Uint64()
	hi := magnitude.Rsh(magnitude, 64).Uint64()
	return b.Sign() < 0, u128{hi: hi, lo: lo}
}

// powers holds 10^0 to 10^MaxDigits. They are read and never changed.
var powers = func() []*big.Int {
	out := make([]*big.Int, MaxDigits+1)
	ten := big.NewInt(10)
	out[0] = big.NewInt(1)
	for i := 1; i <= MaxDigits; i++ {
		out[i] = new(big.Int).Mul(out[i-1], ten)
	}
	return out
}()

// digitCount is the number of decimal digits of b; zero has one.
func digitCount(b *big.Int) int {
	if b.Sign() == 0 {
		return 1
	}
	return len(new(big.Int).Abs(b).Text(10))
}

// lexed is an exact value split into its parts, before any declared type
// applies.
type lexed struct {
	negative bool
	hasPoint bool
	fraction string
	// magnitude is the digits as a number.
	magnitude u128
	// significant is the number of significant digits; zero has one.
	significant int
}

// lex checks the grammar -?(0|[1-9][0-9]*)(\.[0-9]+)?, refuses negative
// zero and more than MaxDigits significant digits. These rules hold for
// every declared type, so they are judged before any of them.
func lex(text string) (lexed, *Refusal) {
	unsigned, negative := strings.CutPrefix(text, "-")
	if unsigned == "" {
		return lexed{}, errExactEmpty
	}
	for i := 0; i < len(unsigned); i++ {
		if b := unsigned[i]; (b < '0' || b > '9') && b != '.' {
			return lexed{}, errExactInvalidCharacter
		}
	}
	whole, fraction, hasPoint := strings.Cut(unsigned, ".")
	if hasPoint && (whole == "" || fraction == "" || strings.Contains(fraction, ".")) {
		return lexed{}, errExactMalformed
	}
	if len(whole) > 1 && whole[0] == '0' {
		return lexed{}, errExactLeadingZero
	}
	var magnitude u128
	significant := 0
	for _, part := range [2]string{whole, fraction} {
		for i := 0; i < len(part); i++ {
			b := part[i]
			if !magnitude.isZero() || b != '0' {
				significant++
				if significant > MaxDigits {
					return lexed{}, errExactTooManyDigits
				}
			}
			// At most MaxDigits significant digits reach here, so the
			// magnitude stays below 10^38.
			magnitude = magnitude.mul10add(uint64(b - '0'))
		}
	}
	if negative && magnitude.isZero() {
		return lexed{}, errExactNegativeZero
	}
	return lexed{
		negative:    negative,
		hasPoint:    hasPoint,
		fraction:    fraction,
		magnitude:   magnitude,
		significant: max(significant, 1),
	}, nil
}

// Integer is an exact integer of at most MaxDigits digits.
//
// The zero value is 0. Values compare equal with == when they are equal.
type Integer struct {
	negative  bool
	magnitude u128
}

// ParseInteger parses the canonical integer grammar.
//
// It refuses text that is not canonical, has more than MaxDigits digits or
// has a decimal point, with a *Refusal naming the rule.
func ParseInteger(text string) (Integer, error) {
	l, err := lex(text)
	if err != nil {
		return Integer{}, err.fresh()
	}
	if l.hasPoint {
		return Integer{}, errExactPointMismatch.fresh()
	}
	return Integer{negative: l.negative, magnitude: l.magnitude}, nil
}

// Sign returns -1, 0 or +1.
func (i Integer) Sign() int {
	switch {
	case i.magnitude.isZero():
		return 0
	case i.negative:
		return -1
	}
	return 1
}

// Big returns the value as a new big.Int.
func (i Integer) Big() *big.Int { return signedBig(i.negative, i.magnitude) }

// Int64 returns the value and whether it fits an int64.
func (i Integer) Int64() (int64, bool) {
	if i.magnitude.hi != 0 {
		return 0, false
	}
	if i.negative {
		if i.magnitude.lo > 1<<63 {
			return 0, false
		}
		return -int64(i.magnitude.lo-1) - 1, true
	}
	if i.magnitude.lo > 1<<63-1 {
		return 0, false
	}
	return int64(i.magnitude.lo), true
}

// String is the canonical text of the value.
func (i Integer) String() string {
	if i.negative {
		return "-" + i.magnitude.String()
	}
	return i.magnitude.String()
}

// DecimalType is a decimal column type: the total number of significant
// digits (the precision) and the number of digits after the point (the
// scale).
//
// The zero value is not a valid type; build one with NewDecimalType.
type DecimalType struct {
	precision uint8
	scale     uint8
}

// NewDecimalType declares a decimal type. It refuses, with the rule
// schema-member-invalid, a precision outside 1 to MaxDigits or a scale
// outside 0 to the precision.
func NewDecimalType(precision, scale int) (DecimalType, error) {
	if precision < 1 || precision > MaxDigits || scale < 0 || scale > precision {
		return DecimalType{}, errTypeInvalid.fresh()
	}
	return DecimalType{precision: uint8(precision), scale: uint8(scale)}, nil
}

// Precision is the total number of significant digits allowed.
func (t DecimalType) Precision() int { return int(t.precision) }

// Scale is the number of digits after the decimal point.
func (t DecimalType) Scale() int { return int(t.scale) }

func (t DecimalType) valid() bool { return t.precision >= 1 }

// RoundingMode says how a discarded remainder changes the retained digits.
type RoundingMode uint8

// The rounding modes of the standard.
const (
	// HalfEven sends ties to the neighbour whose last digit is even.
	HalfEven RoundingMode = iota
	// HalfAwayFromZero sends ties to the neighbour farther from zero.
	HalfAwayFromZero
	// TowardZero discards the remainder.
	TowardZero
	// TowardPositive rounds to the next value toward positive infinity.
	TowardPositive
	// TowardNegative rounds to the next value toward negative infinity.
	TowardNegative
)

var roundingNames = [...]string{
	HalfEven:         "half-even",
	HalfAwayFromZero: "half-away-from-zero",
	TowardZero:       "toward-zero",
	TowardPositive:   "toward-positive",
	TowardNegative:   "toward-negative",
}

// String is the name of the mode in a policy document, such as "half-even".
func (m RoundingMode) String() string {
	if int(m) < len(roundingNames) {
		return roundingNames[m]
	}
	return "rounding(" + strconv.Itoa(int(m)) + ")"
}

func roundingByName(name string) (RoundingMode, bool) {
	for mode, known := range roundingNames {
		if known == name {
			return RoundingMode(mode), true
		}
	}
	return 0, false
}

// Decimal is an exact decimal: a coefficient times 10^-scale.
//
// Equality with == compares coefficient and scale, so 1.0 and 1.00 are
// distinct. The zero value is 0 at scale zero.
type Decimal struct {
	negative  bool
	magnitude u128
	scale     uint8
}

// ParseDecimal parses the canonical decimal grammar for a declared type.
//
// It refuses text that is not canonical, whose fractional digits differ from
// the declared scale, or whose coefficient has more significant digits than
// the declared precision, with a *Refusal naming the rule. The grammar and
// the MaxDigits bound are judged before the declared type.
func ParseDecimal(text string, t DecimalType) (Decimal, error) {
	if !t.valid() {
		return Decimal{}, errTypeInvalid.fresh()
	}
	l, err := lex(text)
	if err != nil {
		return Decimal{}, err.fresh()
	}
	if l.hasPoint != (t.scale > 0) {
		return Decimal{}, errExactPointMismatch.fresh()
	}
	if len(l.fraction) != int(t.scale) {
		return Decimal{}, errExactScaleMismatch.fresh()
	}
	if l.significant > int(t.precision) {
		return Decimal{}, errExactPrecisionExceeded.fresh()
	}
	return Decimal{negative: l.negative, magnitude: l.magnitude, scale: t.scale}, nil
}

// Coefficient returns the signed coefficient as a new big.Int.
func (d Decimal) Coefficient() *big.Int { return signedBig(d.negative, d.magnitude) }

// Scale is the number of digits after the decimal point.
func (d Decimal) Scale() int { return int(d.scale) }

// Sign returns -1, 0 or +1.
func (d Decimal) Sign() int {
	switch {
	case d.magnitude.isZero():
		return 0
	case d.negative:
		return -1
	}
	return 1
}

// SignificantDigits is the number of digits in the coefficient; zero has
// one.
func (d Decimal) SignificantDigits() int { return len(d.magnitude.String()) }

// String is the canonical text of the value, with exactly Scale digits after
// the point.
func (d Decimal) String() string {
	digits := d.magnitude.String()
	sign := ""
	if d.negative {
		sign = "-"
	}
	if d.scale == 0 {
		return sign + digits
	}
	width := int(d.scale) + 1
	if len(digits) < width {
		digits = strings.Repeat("0", width-len(digits)) + digits
	}
	cut := len(digits) - int(d.scale)
	return sign + digits[:cut] + "." + digits[cut:]
}

// Quantized is the result of Decimal.Quantize.
type Quantized struct {
	// Value is the value at the target scale.
	Value Decimal
	// ValueChanged says whether the mathematical value changed.
	ValueChanged bool
	// ScaleChanged says whether the scale changed, even if the value did
	// not.
	ScaleChanged bool
}

// Quantize re-expresses the value at the scale of the target type.
//
// Raising the scale pads exactly. Lowering it applies mode and reports
// whether the value changed. A result with more significant digits than the
// target precision is refused with the rule quantize-overflow; nothing is
// clipped.
func (d Decimal) Quantize(target DecimalType, mode RoundingMode) (Quantized, error) {
	if !target.valid() {
		return Quantized{}, errTypeInvalid.fresh()
	}
	if int(mode) >= len(roundingNames) {
		return Quantized{}, errRoundingInvalid.fresh()
	}
	scaleChanged := target.scale != d.scale
	coefficient := d.Coefficient()
	valueChanged := false
	if target.scale >= d.scale {
		coefficient.Mul(coefficient, powers[target.scale-d.scale])
	} else {
		divisor := powers[d.scale-target.scale]
		quotient, remainder := new(big.Int).QuoRem(coefficient, divisor, new(big.Int))
		coefficient = round(quotient, remainder, divisor, mode)
		valueChanged = remainder.Sign() != 0
	}
	if digitCount(coefficient) > int(target.precision) {
		return Quantized{}, errQuantizeOverflow.fresh()
	}
	negative, magnitude := fromBig(coefficient)
	return Quantized{
		Value:        Decimal{negative: negative, magnitude: magnitude, scale: target.scale},
		ValueChanged: valueChanged,
		ScaleChanged: scaleChanged,
	}, nil
}

// round applies a rounding mode to a truncated quotient. remainder has the
// sign of the dividend and a magnitude below divisor.
func round(quotient, remainder, divisor *big.Int, mode RoundingMode) *big.Int {
	if remainder.Sign() == 0 {
		return quotient
	}
	away := new(big.Int)
	if remainder.Sign() > 0 {
		away.Add(quotient, big.NewInt(1))
	} else {
		away.Sub(quotient, big.NewInt(1))
	}
	magnitude := new(big.Int).Abs(remainder)
	// The remainder against half the divisor, without halving.
	againstHalf := magnitude.Cmp(new(big.Int).Sub(divisor, magnitude))
	switch mode {
	case TowardZero:
		return quotient
	case TowardPositive:
		if remainder.Sign() > 0 {
			return away
		}
		return quotient
	case TowardNegative:
		if remainder.Sign() < 0 {
			return away
		}
		return quotient
	case HalfAwayFromZero:
		if againstHalf < 0 {
			return quotient
		}
		return away
	default: // HalfEven
		switch {
		case againstHalf < 0:
			return quotient
		case againstHalf > 0:
			return away
		case quotient.Bit(0) == 0:
			return quotient
		}
		return away
	}
}
