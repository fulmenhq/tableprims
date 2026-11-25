// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"testing"
)

// The package knows exactly the capabilities the suite names, in its order,
// so the Go and Rust surfaces agree on which identifiers are unknown.
func TestRegistryMatchesSuite(t *testing.T) {
	data, err := os.ReadFile(filepath.FromSlash("../conformance/capabilities/v0/capabilities.json"))
	if err != nil {
		t.Fatal(err)
	}
	var suite struct {
		Capabilities []string `json:"capabilities"`
	}
	if err := json.Unmarshal(data, &suite); err != nil {
		t.Fatal(err)
	}
	var known []string
	for _, entry := range registry {
		known = append(known, entry.id)
	}
	if !slices.Equal(known, suite.Capabilities) {
		t.Errorf("registry %v, suite %v", known, suite.Capabilities)
	}
}
