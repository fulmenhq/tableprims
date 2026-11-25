// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"encoding/json"
	"os"
	"testing"
)

// TestBinary64ErrorTexts checks the reason codes and error texts against
// conformance/binary64/v0/binary64.json, which the Rust tests check too, so
// the two implementations give the same text for each reason. The
// not-round-trip reason cannot be produced through the public API.
func TestBinary64ErrorTexts(t *testing.T) {
	raw, err := os.ReadFile("../conformance/binary64/v0/binary64.json")
	if err != nil {
		t.Fatal(err)
	}
	var suite struct {
		Reasons []struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"reasons"`
	}
	if err := json.Unmarshal(raw, &suite); err != nil {
		t.Fatal(err)
	}
	codes := []string{Binary64TooManyDigits, Binary64NotRoundTrip, Binary64NotExactInteger}
	if len(suite.Reasons) != len(codes) {
		t.Fatalf("%d reasons in the suite, want %d", len(suite.Reasons), len(codes))
	}
	for at, reason := range suite.Reasons {
		if reason.Code != codes[at] {
			t.Errorf("reason %d: code %q, want %q", at, reason.Code, codes[at])
		}
		e := &Binary64Error{reason: reason.Code}
		if e.Reason() != reason.Code || e.Error() != reason.Message {
			t.Errorf("%s: %q, want %q", reason.Code, e.Error(), reason.Message)
		}
	}
	if (&Binary64Error{}).Error() == suite.Reasons[2].Message {
		t.Error("an error without a reason reads as not-exact-integer")
	}
}
