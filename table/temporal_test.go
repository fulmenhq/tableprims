// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import "testing"

func precision(t *testing.T, digits int) FractionPrecision {
	t.Helper()
	p, err := NewFractionPrecision(digits)
	if err != nil {
		t.Fatal(err)
	}
	return p
}

func TestFractionPrecisionIsBounded(t *testing.T) {
	if _, err := NewFractionPrecision(9); err != nil {
		t.Error(err)
	}
	for _, digits := range []int{10, -1} {
		if _, err := NewFractionPrecision(digits); rule(t, err) != "schema-member-invalid" {
			t.Errorf("%d: %v", digits, err)
		}
	}
}

func TestDates(t *testing.T) {
	for _, text := range []string{"0001-01-01", "2024-02-29", "2000-02-29", "9999-12-31"} {
		if d, err := ParseDate(text); err != nil || d.String() != text {
			t.Errorf("%s: %v %v", text, d, err)
		}
	}
	d, _ := ParseDate("2024-02-29")
	if d.Year() != 2024 || d.Month() != 2 || d.Day() != 29 {
		t.Errorf("fields %d %d %d", d.Year(), d.Month(), d.Day())
	}
	for _, text := range []string{
		"2023-02-29", "1900-02-29", "2024-04-31", "2024-00-10", "2024-13-01", "2024-01-00",
		"2024-01-32", "0000-01-01",
	} {
		if _, err := ParseDate(text); rule(t, err) != "temporal-invalid-date" {
			t.Errorf("%s: %v", text, err)
		}
	}
	for _, text := range []string{
		"", "2024-1-01", "24-01-01", "2024/01/01", "2024-01-01 ", " 2024-01-01", "+2024-01-01",
		"2024-W01-1", "2024-001", "20240101", "2024-01-0१",
	} {
		if _, err := ParseDate(text); rule(t, err) != "temporal-malformed" {
			t.Errorf("%q: %v", text, err)
		}
	}
}

func TestLocalTimestampsRoundTrip(t *testing.T) {
	for _, c := range []struct {
		text   string
		digits int
	}{
		{"2024-02-29T00:00:00", 0}, {"2024-02-29T23:59:59", 0}, {"2024-02-29T12:34:56.7", 1},
		{"2024-02-29T12:34:56.000", 3}, {"2024-02-29T12:34:56.007", 3},
		{"2024-02-29T12:34:56.123456789", 9},
	} {
		v, err := ParseLocalTimestamp(c.text, precision(t, c.digits))
		if err != nil || v.String() != c.text || v.Precision().Digits() != c.digits {
			t.Errorf("%s: %v %v", c.text, v, err)
		}
	}
	v, _ := ParseLocalTimestamp("2024-02-29T12:34:56.007", precision(t, 3))
	if v.Fraction() != 7 || v.Hour() != 12 || v.Minute() != 34 || v.Second() != 56 {
		t.Errorf("fields of %v", v)
	}
	tenths, _ := ParseLocalTimestamp("2026-01-01T00:00:00.1", precision(t, 1))
	hundredths, _ := ParseLocalTimestamp("2026-01-01T00:00:00.10", precision(t, 2))
	if tenths == hundredths {
		t.Error("timestamps of different precision are equal")
	}
}

// temporalCase is a text, a declared precision and the rule it breaks.
type temporalCase struct {
	text   string
	digits int
	rule   string
}

func checkLocal(t *testing.T, cases []temporalCase) {
	t.Helper()
	for _, c := range cases {
		if _, err := ParseLocalTimestamp(c.text, precision(t, c.digits)); rule(t, err) != c.rule {
			t.Errorf("local %q at %d: %v, want %s", c.text, c.digits, err, c.rule)
		}
	}
}

func checkOffset(t *testing.T, cases []temporalCase) {
	t.Helper()
	for _, c := range cases {
		if _, err := ParseOffsetTimestamp(c.text, precision(t, c.digits)); rule(t, err) != c.rule {
			t.Errorf("offset %q at %d: %v, want %s", c.text, c.digits, err, c.rule)
		}
	}
}

func TestTimestampRefusals(t *testing.T) {
	const (
		fraction  = "temporal-fraction-mismatch"
		malformed = "temporal-malformed"
		mismatch  = "temporal-offset-mismatch"
		offset    = "temporal-invalid-offset"
		negative  = "temporal-negative-zero-offset"
		clock     = "temporal-invalid-time"
		calendar  = "temporal-invalid-date"
	)
	checkLocal(t, []temporalCase{
		{"2024-01-01T00:00:00.0", 0, fraction},
		{"2024-01-01T00:00:00", 3, fraction},
		{"2024-01-01T00:00:00.12", 3, fraction},
		{"2024-01-01T00:00:00.1234", 3, fraction},
		{"2024-01-01T00:00:00.1234567890", 9, fraction},
		{"2024-01-01T00:00:00.12345678901234567890", 0, fraction},
		// A point needs a digit whatever the declared precision.
		{"2026-11-19T08:30:00.", 0, malformed},
		{"2026-11-19T08:30:00.Z", 3, malformed},
		// A well-formed timestamp of the other type.
		{"2026-11-19T08:30:00Z", 0, mismatch},
		{"2026-11-19T08:30:00+00:00", 0, mismatch},
		{"2026-11-19T08:30:00.123456789+05:30", 9, mismatch},
		// Well-formedness is judged before the declared type.
		{"2026-11-19T08:30:00,000", 3, offset},
		{"2026-11-19T08:30:00.12+24:00", 3, offset},
		{"2026-11-19T08:30:00-00:00", 3, negative},
		{"2026-11-19T08:30:00.5+24:00", 0, offset},
		{"2026-11-19T08:30:00 ", 0, offset},
		{"2026-11-19T08:30:00+05:60", 0, offset},
		{"2026-11-19T08:30:00z", 0, offset},
		{"2026-11-19T08:30:00.123-00:00", 3, negative},
		{"2026-02-30T08:30:00+24:00", 3, calendar},
		// A fraction mismatch is reported before an offset mismatch.
		{"2026-11-19T08:30:00.12Z", 3, fraction},
		{"2024-01-01T24:00:00", 0, clock},
		{"2024-01-01T23:60:00", 0, clock},
		{"2024-06-30T23:59:60", 0, clock},
		{"2024-01-01", 0, malformed},
		{"2024-01-01 00:00:00", 0, malformed},
		{"2024-01-01t00:00:00", 0, malformed},
		{"2024-01-01T00:00", 0, malformed},
		{"2024-01-01T0:00:000", 0, malformed},
		{"2024-01-01T00-00-00", 0, malformed},
		{"2024-01-01T00:00:00.12x", 3, offset},
		{"2024-01-01T00:00:00.123x", 3, offset},
	})
	checkOffset(t, []temporalCase{
		{"2026-11-19T08:30:00", 0, mismatch},
		{"2026-11-19T08:30:00.123", 3, mismatch},
		{"2026-11-19T08:30:00.", 3, malformed},
		{"2026-11-19T08:30:00.12+99:99", 3, offset},
		{"2026-11-19T08:30:00.12-00:00", 3, negative},
		{"2026-11-19T24:00:00", 3, clock},
		{"2026-11-19T08:30:00", 3, fraction},
		{"2024-01-01T00:00:00.1Z", 0, fraction},
		{"2024-01-01T00:00:00.1234Z", 3, fraction},
		{"2023-02-29T00:00:00Z", 0, calendar},
		{"2024-01-01T00:00:00z", 0, offset},
		{"2024-01-01T00:00:00+0530", 0, offset},
		{"2024-01-01T00:00:00+05", 0, offset},
		{"2024-01-01T00:00:00+5:30", 0, offset},
		{"2024-01-01T00:00:00+24:00", 0, offset},
		{"2024-01-01T00:00:00 +05:30", 0, offset},
		{"2024-01-01T00:00:00+05:30 ", 0, offset},
		{"2024-01-01T00:00:00ZZ", 0, offset},
		{"2024-01-01T00:00:00[UTC]", 0, offset},
		{"2024-01-01T00:00:00-00:00", 0, negative},
	})
}

func TestOffsetTimestampsRoundTripAsWritten(t *testing.T) {
	for _, c := range []struct {
		text   string
		digits int
	}{
		{"2024-02-29T12:34:56Z", 0}, {"2024-02-29T12:34:56+00:00", 0},
		{"2024-02-29T12:34:56+05:30", 0}, {"2024-02-29T12:34:56-08:00", 0},
		{"2024-02-29T12:34:56.123Z", 3}, {"2024-02-29T12:34:56.123456789-00:01", 9},
		{"2024-02-29T12:34:56.5+23:59", 1},
	} {
		if v, err := ParseOffsetTimestamp(c.text, precision(t, c.digits)); err != nil || v.String() != c.text {
			t.Errorf("%s: %v %v", c.text, v, err)
		}
	}
	z, _ := ParseOffsetTimestamp("2024-01-01T00:00:00Z", precision(t, 0))
	zero, _ := ParseOffsetTimestamp("2024-01-01T00:00:00+00:00", precision(t, 0))
	if z == zero || !z.Offset().UTCDesignator() || zero.Offset().UTCDesignator() ||
		z.Offset().Minutes() != 0 || zero.Offset().Minutes() != 0 || z.Local() != zero.Local() {
		t.Error("Z and +00:00 are not kept distinct")
	}
	east, _ := ParseOffsetTimestamp("2024-01-01T00:00:00+05:30", precision(t, 0))
	west, _ := ParseOffsetTimestamp("2024-01-01T00:00:00-08:00", precision(t, 0))
	if east.Offset().Minutes() != 330 || west.Offset().Minutes() != -480 {
		t.Errorf("offset minutes %d %d", east.Offset().Minutes(), west.Offset().Minutes())
	}
}
