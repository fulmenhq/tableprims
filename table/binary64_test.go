// SPDX-License-Identifier: MIT OR Apache-2.0

package table_test

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"strconv"
	"testing"

	"github.com/fulmenhq/tableprims/table"
)

// binary64Cases is the number of cases in the suite; a shorter suite fails.
const binary64Cases = 63

type binary64Case struct {
	Kind              string `json:"kind"`
	Text              string `json:"text"`
	Precision         *int   `json:"precision"`
	Scale             *int   `json:"scale"`
	CoefficientDigits *int   `json:"coefficient_digits"`
	Bits              string `json:"bits"`
	Exact             bool   `json:"exact"`
	RoundTrips        bool   `json:"round_trips"`
	XLSXWriteR1       string `json:"xlsx_write_r1"`
}

// loadBinary64 reads conformance/binary64/v0/binary64.json, whose
// expectations are derived independently of this package.
func loadBinary64(t *testing.T) []binary64Case {
	t.Helper()
	raw, err := os.ReadFile("../conformance/binary64/v0/binary64.json")
	if err != nil {
		t.Fatal(err)
	}
	var suite struct {
		Suite       string `json:"suite"`
		Description string `json:"description"`
		Reasons     []struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"reasons"`
		Cases []binary64Case `json:"cases"`
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&suite); err != nil {
		t.Fatal(err)
	}
	if suite.Suite != "tableprims-binary64/0" {
		t.Fatalf("suite %q", suite.Suite)
	}
	return suite.Cases
}

// binary64Disagreements returns every disagreement between the cases and
// this package: the facts and the verdict of the XLSX write profile's rule.
func binary64Disagreements(cases []binary64Case) []string {
	var out []string
	for _, c := range cases {
		id := c.Kind + " " + c.Text
		var facts table.Binary64
		var stored float64
		var verdict error
		switch c.Kind {
		case "integer":
			if c.Precision != nil || c.Scale != nil || c.CoefficientDigits != nil {
				out = append(out, id+": decimal members on an integer")
				continue
			}
			value, err := table.ParseInteger(c.Text)
			if err != nil {
				out = append(out, id+": "+err.Error())
				continue
			}
			facts = value.Binary64()
			stored, verdict = table.XLSXWriteR1Binary64FromInteger(value)
		case "decimal":
			if c.Precision == nil || c.Scale == nil || c.CoefficientDigits == nil {
				out = append(out, id+": decimal members missing")
				continue
			}
			ty, err := table.NewDecimalType(*c.Precision, *c.Scale)
			if err != nil {
				out = append(out, id+": "+err.Error())
				continue
			}
			value, err := table.ParseDecimal(c.Text, ty)
			if err != nil {
				out = append(out, id+": "+err.Error())
				continue
			}
			if value.SignificantDigits() != *c.CoefficientDigits {
				out = append(out, fmt.Sprintf("%s: coefficient digits %d", id, value.SignificantDigits()))
			}
			facts = value.Binary64()
			stored, verdict = table.XLSXWriteR1Binary64FromDecimal(value)
		default:
			out = append(out, id+": unknown kind")
			continue
		}
		bits := fmt.Sprintf("0x%016x", math.Float64bits(facts.Value()))
		if bits != c.Bits {
			out = append(out, id+": bits "+bits)
		}
		if facts.Exact() != c.Exact {
			out = append(out, id+": exact "+strconv.FormatBool(facts.Exact()))
		}
		if facts.RoundTrips() != c.RoundTrips {
			out = append(out, id+": round_trips "+strconv.FormatBool(facts.RoundTrips()))
		}
		var b64 *table.Binary64Error
		switch {
		case verdict == nil && c.XLSXWriteR1 == "ok" && fmt.Sprintf("0x%016x", math.Float64bits(stored)) == c.Bits:
		case errors.As(verdict, &b64) && b64.Reason() == c.XLSXWriteR1 && stored == 0:
		default:
			out = append(out, fmt.Sprintf("%s: xlsx_write_r1 %v %v", id, stored, verdict))
		}
	}
	return out
}

func TestBinary64Suite(t *testing.T) {
	cases := loadBinary64(t)
	if len(cases) != binary64Cases {
		t.Fatalf("%d cases, want %d", len(cases), binary64Cases)
	}
	for _, d := range binary64Disagreements(cases) {
		t.Error(d)
	}
}

func TestBinary64SuiteCoversEachFactAndVerdict(t *testing.T) {
	cases := loadBinary64(t)
	for _, kind := range []string{"integer", "decimal"} {
		for _, pair := range [][2]bool{{true, true}, {true, false}, {false, true}, {false, false}} {
			found := false
			for _, c := range cases {
				found = found || (c.Kind == kind && c.Exact == pair[0] && c.RoundTrips == pair[1])
			}
			if !found {
				t.Errorf("%s: no case with exact=%v round_trips=%v", kind, pair[0], pair[1])
			}
		}
	}
	// Binary64NotRoundTrip cannot occur under the rule: a value of at most
	// 15 significant digits always round-trips.
	for _, reason := range []string{"ok", table.Binary64TooManyDigits, table.Binary64NotExactInteger} {
		found := false
		for _, c := range cases {
			found = found || c.XLSXWriteR1 == reason
		}
		if !found {
			t.Errorf("no case with xlsx_write_r1 %q", reason)
		}
	}
	scaled := false
	for _, c := range cases {
		if c.XLSXWriteR1 == table.Binary64NotRoundTrip {
			t.Errorf("%s: not-round-trip is unreachable under the rule", c.Text)
		}
		scaled = scaled || (c.XLSXWriteR1 == "ok" && c.CoefficientDigits != nil && *c.CoefficientDigits > 15)
	}
	if !scaled {
		t.Error("no eligible case whose coefficient has more digits than are significant")
	}
}

func TestBinary64SuiteDetectsAlteredExpectations(t *testing.T) {
	mutations := map[string]func(*binary64Case){
		"bits": func(c *binary64Case) {
			n, _ := strconv.ParseUint(c.Bits[2:], 16, 64)
			c.Bits = fmt.Sprintf("0x%016x", n^1)
		},
		"exact":       func(c *binary64Case) { c.Exact = !c.Exact },
		"round_trips": func(c *binary64Case) { c.RoundTrips = !c.RoundTrips },
		"xlsx_write_r1": func(c *binary64Case) {
			if c.XLSXWriteR1 == "ok" {
				c.XLSXWriteR1 = table.Binary64NotRoundTrip
			} else {
				c.XLSXWriteR1 = "ok"
			}
		},
	}
	for name, mutate := range mutations {
		for at := range binary64Cases {
			cases := loadBinary64(t)
			mutate(&cases[at])
			if len(binary64Disagreements(cases[at:at+1])) == 0 {
				t.Errorf("%s at %d was not detected", name, at)
			}
		}
	}
}

func TestBinary64ErrorIsNeitherRefusalNorCapabilityError(t *testing.T) {
	value, err := table.ParseInteger("100000000000000000000000")
	if err != nil {
		t.Fatal(err)
	}
	_, err = table.XLSXWriteR1Binary64FromInteger(value)
	var b64 *table.Binary64Error
	if !errors.As(err, &b64) || b64.Reason() != table.Binary64NotExactInteger {
		t.Fatalf("got %v", err)
	}
	var refusal *table.Refusal
	var capability *table.CapabilityError
	if errors.As(err, &refusal) || errors.As(err, &capability) {
		t.Fatalf("%T is a refusal or a capability error", err)
	}
	if err.Error() != "integer is not exactly representable as a binary number" {
		t.Fatalf("text %q", err.Error())
	}
}
