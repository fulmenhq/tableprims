// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"errors"
	"math/big"
	"strings"
	"testing"
)

// rule is the rule of a refusal, or "" for no error.
func rule(t *testing.T, err error) string {
	t.Helper()
	if err == nil {
		return ""
	}
	var refusal *Refusal
	if !errors.As(err, &refusal) {
		t.Fatalf("not a refusal: %v", err)
	}
	return refusal.Rule()
}

func decimalType(t *testing.T, precision, scale int) DecimalType {
	t.Helper()
	ty, err := NewDecimalType(precision, scale)
	if err != nil {
		t.Fatalf("decimal(%d,%d): %v", precision, scale, err)
	}
	return ty
}

func dec(t *testing.T, text string, precision, scale int) Decimal {
	t.Helper()
	d, err := ParseDecimal(text, decimalType(t, precision, scale))
	if err != nil {
		t.Fatalf("%s as decimal(%d,%d): %v", text, precision, scale, err)
	}
	return d
}

func TestIntegersRoundTripCanonicalText(t *testing.T) {
	for _, text := range []string{
		"0", "7", "-7", "9007199254740991", "9007199254740992", "9007199254740993",
		"12345678901234567890", "18446744073709551616", "10000000000000000000000000000000000000",
		"99999999999999999999999999999999999999", "-99999999999999999999999999999999999999",
	} {
		value, err := ParseInteger(text)
		if err != nil || value.String() != text || value.Big().String() != text {
			t.Errorf("%s: %v %v", text, value, err)
		}
	}
	low, _ := ParseInteger("9007199254740992")
	high, _ := ParseInteger("9007199254740993")
	if new(big.Int).Sub(high.Big(), low.Big()).Int64() != 1 || low == high {
		t.Error("adjacent large integers are not distinct")
	}
	for _, c := range []struct {
		text string
		want int64
		ok   bool
	}{
		{"9223372036854775807", 9223372036854775807, true},
		{"-9223372036854775808", -9223372036854775808, true},
		{"9223372036854775808", 0, false},
		{"-9223372036854775809", 0, false},
		{"-1", -1, true},
	} {
		value, _ := ParseInteger(c.text)
		if got, ok := value.Int64(); got != c.want || ok != c.ok {
			t.Errorf("%s as int64: %d %v", c.text, got, ok)
		}
	}
}

func TestNoncanonicalIntegersAreRefused(t *testing.T) {
	for _, c := range []struct{ text, rule string }{
		{"", "exact-empty"},
		{"-", "exact-empty"},
		{"000123", "exact-leading-zero"},
		{"00", "exact-leading-zero"},
		{"-0", "exact-negative-zero"},
		{"+1", "exact-invalid-character"},
		{" 1", "exact-invalid-character"},
		{"1 ", "exact-invalid-character"},
		{"1e3", "exact-invalid-character"},
		{"1.0", "exact-point-mismatch"},
		{".5", "exact-malformed"},
		{"1.", "exact-malformed"},
		{"1..2", "exact-malformed"},
		{"-0.0", "exact-negative-zero"},
		{"--1", "exact-invalid-character"},
		{"١٢٣", "exact-invalid-character"},
		{"100000000000000000000000000000000000000", "exact-too-many-digits"},
		{"1000000000000000000000000000000000000.0", "exact-point-mismatch"},
	} {
		_, err := ParseInteger(c.text)
		if got := rule(t, err); got != c.rule {
			t.Errorf("%q: %s, want %s", c.text, got, c.rule)
		}
	}
}

func TestDecimalTypesAreBounded(t *testing.T) {
	for _, c := range []struct {
		precision, scale int
		ok               bool
	}{{1, 0, true}, {38, 38, true}, {0, 0, false}, {39, 0, false}, {4, 5, false}, {4, -1, false}} {
		_, err := NewDecimalType(c.precision, c.scale)
		if (err == nil) != c.ok {
			t.Errorf("decimal(%d,%d): %v", c.precision, c.scale, err)
		}
	}
	if _, err := ParseDecimal("1", DecimalType{}); rule(t, err) != "schema-member-invalid" {
		t.Errorf("zero decimal type: %v", err)
	}
}

func TestDecimalsRoundTripWithDeclaredScale(t *testing.T) {
	for _, c := range []struct {
		text             string
		precision, scale int
	}{
		{"123.4500", 7, 4}, {"0.01", 2, 2}, {"0.00", 2, 2}, {"-1234.56", 6, 2}, {"-0.05", 3, 2},
		{"42", 2, 0}, {"0", 1, 0}, {"0.99999999999999999999999999999999999999", 38, 38},
		{"-9999999999999999999999999.9999999999999", 38, 13},
	} {
		value := dec(t, c.text, c.precision, c.scale)
		if value.String() != c.text || value.Scale() != c.scale {
			t.Errorf("%s: %s at scale %d", c.text, value, value.Scale())
		}
	}
	if got := dec(t, "123.4500", 7, 4).Coefficient().Int64(); got != 1_234_500 {
		t.Errorf("coefficient %d", got)
	}
	if got := dec(t, "0.00", 2, 2).SignificantDigits(); got != 1 {
		t.Errorf("zero has %d significant digits", got)
	}
	if dec(t, "1.0", 3, 1) == dec(t, "1.00", 3, 2) {
		t.Error("the scale is part of a decimal's identity")
	}
}

func TestNoncanonicalOrOutOfTypeDecimalsAreRefused(t *testing.T) {
	for _, c := range []struct {
		text             string
		precision, scale int
		rule             string
	}{
		{"123.4500", 6, 4, "exact-precision-exceeded"},
		{"1.00", 2, 2, "exact-precision-exceeded"},
		{"123.45", 7, 4, "exact-scale-mismatch"},
		{"123.45000", 9, 4, "exact-scale-mismatch"},
		{"123", 7, 4, "exact-point-mismatch"},
		{"123.", 7, 0, "exact-malformed"},
		{"1.5", 3, 0, "exact-point-mismatch"},
		{".5", 2, 1, "exact-malformed"},
		{"-.5", 2, 1, "exact-malformed"},
		{"1.", 2, 1, "exact-malformed"},
		{"1..2", 2, 1, "exact-malformed"},
		{".", 2, 1, "exact-malformed"},
		{"01.5", 3, 1, "exact-leading-zero"},
		{"-0.00", 2, 2, "exact-negative-zero"},
		{"-0", 1, 0, "exact-negative-zero"},
		{"+1.5", 2, 1, "exact-invalid-character"},
		{"1.5e2", 4, 3, "exact-invalid-character"},
		{"1,5", 2, 1, "exact-invalid-character"},
		{"1.2.3", 3, 1, "exact-malformed"},
		// Grammar and the global digit bound come before the declared type.
		{"01.50", 3, 1, "exact-leading-zero"},
		{"-0.0", 2, 2, "exact-negative-zero"},
		{"1234567890123456789012345678901234567.89", 5, 2, "exact-too-many-digits"},
		{"123.45", 4, 2, "exact-precision-exceeded"},
		{"1.0", 3, 2, "exact-scale-mismatch"},
		{"", 2, 1, "exact-empty"},
	} {
		_, err := ParseDecimal(c.text, decimalType(t, c.precision, c.scale))
		if got := rule(t, err); got != c.rule {
			t.Errorf("%q as decimal(%d,%d): %s, want %s", c.text, c.precision, c.scale, got, c.rule)
		}
	}
}

func toWhole(t *testing.T, text string, mode RoundingMode) string {
	t.Helper()
	q, err := dec(t, text, 3, 1).Quantize(decimalType(t, 3, 0), mode)
	if err != nil {
		t.Fatal(err)
	}
	return q.Value.String()
}

func TestRoundingModesFollowTheirDirection(t *testing.T) {
	// input, half-even, half-away, toward-zero, toward-positive, toward-negative
	for _, row := range [][6]string{
		{"2.5", "2", "3", "2", "3", "2"},
		{"3.5", "4", "4", "3", "4", "3"},
		{"-2.5", "-2", "-3", "-2", "-2", "-3"},
		{"-3.5", "-4", "-4", "-3", "-3", "-4"},
		{"2.4", "2", "2", "2", "3", "2"},
		{"2.6", "3", "3", "2", "3", "2"},
		{"-2.4", "-2", "-2", "-2", "-2", "-3"},
		{"-2.6", "-3", "-3", "-2", "-2", "-3"},
		{"0.5", "0", "1", "0", "1", "0"},
		{"-0.5", "0", "-1", "0", "0", "-1"},
		{"2.0", "2", "2", "2", "2", "2"},
	} {
		for i, mode := range []RoundingMode{HalfEven, HalfAwayFromZero, TowardZero, TowardPositive, TowardNegative} {
			if got := toWhole(t, row[0], mode); got != row[i+1] {
				t.Errorf("%s %s: %s, want %s", mode, row[0], got, row[i+1])
			}
		}
	}
}

func TestQuantizeReportsChanges(t *testing.T) {
	for _, c := range []struct {
		input string
		mode  RoundingMode
		want  string
	}{
		{"1.245", HalfEven, "1.24"},
		{"-1.245", HalfEven, "-1.24"},
		{"1.245", HalfAwayFromZero, "1.25"},
		{"-1.245", HalfAwayFromZero, "-1.25"},
		{"1.255", HalfEven, "1.26"},
	} {
		q, err := dec(t, c.input, 4, 3).Quantize(decimalType(t, 3, 2), c.mode)
		if err != nil || q.Value.String() != c.want || !q.ValueChanged || !q.ScaleChanged {
			t.Errorf("%s %s: %+v %v", c.input, c.mode, q, err)
		}
	}
	same, _ := dec(t, "1.50", 3, 2).Quantize(decimalType(t, 3, 2), HalfEven)
	if same.ValueChanged || same.ScaleChanged {
		t.Errorf("same scale: %+v", same)
	}
	trimmed, _ := dec(t, "1.50", 3, 2).Quantize(decimalType(t, 3, 1), HalfEven)
	if trimmed.Value.String() != "1.5" || trimmed.ValueChanged || !trimmed.ScaleChanged {
		t.Errorf("trimmed: %+v", trimmed)
	}
	padded, _ := dec(t, "1.5", 2, 1).Quantize(decimalType(t, 5, 4), TowardZero)
	if padded.Value.String() != "1.5000" || padded.ValueChanged || !padded.ScaleChanged {
		t.Errorf("padded: %+v", padded)
	}
	small, _ := dec(t, "-0.004", 3, 3).Quantize(decimalType(t, 3, 2), HalfEven)
	if small.Value.String() != "0.00" || small.Value.Sign() != 0 || !small.ValueChanged {
		t.Errorf("a small negative rounded to zero has no sign: %+v", small)
	}
}

func TestQuantizeRefusesInsteadOfClipping(t *testing.T) {
	widest := strings.Repeat("9", 38)
	for _, c := range []struct {
		value            Decimal
		precision, scale int
		mode             RoundingMode
	}{
		{dec(t, "12.5", 3, 1), 4, 3, HalfEven},
		{dec(t, "9.5", 2, 1), 1, 0, HalfAwayFromZero},
		{dec(t, widest, 38, 0), 38, 1, HalfEven},
		{dec(t, widest, 38, 0), 38, 38, HalfEven},
		{dec(t, "-"+widest, 38, 0), 38, 38, TowardNegative},
	} {
		_, err := c.value.Quantize(decimalType(t, c.precision, c.scale), c.mode)
		if got := rule(t, err); got != "quantize-overflow" {
			t.Errorf("%s to decimal(%d,%d): %s", c.value, c.precision, c.scale, got)
		}
	}
	if _, err := dec(t, "1", 1, 0).Quantize(decimalType(t, 1, 0), RoundingMode(9)); rule(t, err) != "policy-member-invalid" {
		t.Errorf("unknown mode: %v", err)
	}
}

func TestRoundingAtTheWidestScale(t *testing.T) {
	q, err := dec(t, "0.99999999999999999999999999999999999999", 38, 38).Quantize(decimalType(t, 1, 0), HalfEven)
	if err != nil || q.Value.String() != "1" {
		t.Errorf("%+v %v", q, err)
	}
	q, err = dec(t, "0.50000000000000000000000000000000000000", 38, 38).Quantize(decimalType(t, 1, 0), HalfEven)
	if err != nil || q.Value.String() != "0" {
		t.Errorf("%+v %v", q, err)
	}
	q, err = dec(t, "-9999999999999999999999999999999999999.5", 38, 1).Quantize(decimalType(t, 37, 0), HalfEven)
	if rule(t, err) != "quantize-overflow" {
		t.Errorf("rounding past the target precision: %+v %v", q, err)
	}
	q, err = dec(t, "-9999999999999999999999999999999999999.5", 38, 1).Quantize(decimalType(t, 37, 0), TowardZero)
	if err != nil || q.Value.String() != "-9999999999999999999999999999999999999" {
		t.Errorf("%+v %v", q, err)
	}
}

func TestRefusalMessagesDoNotContainContent(t *testing.T) {
	_, err := ParseInteger("secret-9999")
	if strings.Contains(err.Error(), "secret") {
		t.Error(err)
	}
	_, err = ParseDecimal("secret", decimalType(t, 3, 1))
	if strings.Contains(err.Error(), "secret") {
		t.Error(err)
	}
}
