// SPDX-License-Identifier: MIT OR Apache-2.0

package table_test

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/fulmenhq/tableprims/table"
)

// The capability suite is owned by this repository and kept apart from the
// vendored vectors of the standard.
const (
	capabilitySuitePath = "../conformance/capabilities/v0/capabilities.json"
	repoDeclarationPath = "../.fulmen/repo.yaml"
	goSurface           = "go/table"
)

type capabilitySuite struct {
	Suite        string   `json:"suite"`
	Description  string   `json:"description"`
	Capabilities []string `json:"capabilities"`
	Surfaces     []struct {
		Surface string   `json:"surface"`
		Report  []string `json:"report"`
	} `json:"surfaces"`
	Cases   []capabilityCase `json:"cases"`
	Unknown []string         `json:"unknown"`
}

type capabilityCase struct {
	Surface     string `json:"surface"`
	Capability  string `json:"capability"`
	Disposition string `json:"disposition"`
}

func loadCapabilitySuite(t *testing.T) capabilitySuite {
	t.Helper()
	data, err := os.ReadFile(filepath.FromSlash(capabilitySuitePath))
	if err != nil {
		t.Fatal(err)
	}
	var s capabilitySuite
	decoder := json.NewDecoder(strings.NewReader(string(data)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&s); err != nil {
		t.Fatal(err)
	}
	if s.Suite != "tableprims-capabilities/0" {
		t.Fatalf("suite %q", s.Suite)
	}
	return s
}

// exercise runs the operation a capability names, through the public API,
// and checks its result. A capability with no operation here fails.
func exercise(capability string) error {
	const schema = `{"format":"tabular-exchange.schema/0","columns":[
		{"id":"amount","label":"Amount","type":"decimal","precision":3,"scale":2,"nullable":false}]}`
	const policy = `{"format":"tabular-exchange.policy/0","exact_numbers":"require-exact",
		"quantize":[{"column":"amount","scale":1,"rounding":"half-even"}]}`
	switch capability {
	case table.CapabilityReadSchema:
		s, err := table.ReadSchema([]byte(schema))
		if err != nil {
			return err
		}
		if columns := s.Columns(); len(columns) != 1 || columns[0].ID != "amount" {
			return fmt.Errorf("schema columns %v", columns)
		}
		return nil
	case table.CapabilityReadPolicy:
		p, err := table.ReadPolicy([]byte(policy))
		if err != nil {
			return err
		}
		if q := p.Quantize(); len(q) != 1 || q[0].Column != "amount" || q[0].Scale != 1 {
			return fmt.Errorf("policy quantize %v", q)
		}
		return nil
	case table.CapabilityReadRows:
		s, err := table.ReadSchema([]byte(schema))
		if err != nil {
			return err
		}
		p, err := table.ReadPolicy([]byte(policy))
		if err != nil {
			return err
		}
		plan, err := table.NewPlan(s, p)
		if err != nil {
			return err
		}
		rows := table.NewRowReader(strings.NewReader("[\"1.25\"]\n"), plan, 1<<10)
		row, err := rows.Next()
		if err != nil {
			return err
		}
		if got := row.Values[0].String(); got != "1.2" || row.Changes[0]&table.ValueChanged == 0 {
			return fmt.Errorf("row %v %v", row.Values, row.Changes)
		}
		if _, err := rows.Next(); err != io.EOF {
			return fmt.Errorf("after the last row: %v", err)
		}
		return nil
	case table.CapabilityQuantize:
		from, err := table.NewDecimalType(3, 2)
		if err != nil {
			return err
		}
		to, err := table.NewDecimalType(3, 1)
		if err != nil {
			return err
		}
		d, err := table.ParseDecimal("1.35", from)
		if err != nil {
			return err
		}
		q, err := d.Quantize(to, table.HalfEven)
		if err != nil {
			return err
		}
		if q.Value.String() != "1.4" || !q.ValueChanged || !q.ScaleChanged {
			return fmt.Errorf("quantized %v", q)
		}
		return nil
	}
	return fmt.Errorf("no operation exercises %q", capability)
}

// evaluateGo returns every disagreement between the suite and this package.
func evaluateGo(s capabilitySuite) []string {
	var out []string
	var report []string
	found := 0
	for _, surface := range s.Surfaces {
		if surface.Surface == goSurface {
			report = surface.Report
			found++
		}
	}
	if found != 1 {
		out = append(out, fmt.Sprintf("%d report entries for %s", found, goSurface))
	}
	if got := table.Capabilities(); !reflect.DeepEqual(got, report) {
		out = append(out, fmt.Sprintf("report: got %v, want %v", got, report))
	}
	for _, c := range s.Cases {
		if c.Surface != goSurface {
			continue
		}
		err := table.Require(c.Capability)
		var capErr *table.CapabilityError
		switch c.Disposition {
		case "supported":
			if err != nil {
				out = append(out, fmt.Sprintf("%s: Require: %v", c.Capability, err))
			} else if err := exercise(c.Capability); err != nil {
				out = append(out, fmt.Sprintf("%s: operation: %v", c.Capability, err))
			}
			if !slices.Contains(report, c.Capability) {
				out = append(out, fmt.Sprintf("%s: supported but not reported", c.Capability))
			}
		case table.DispositionUnsupported, table.DispositionUnknown:
			if !errors.As(err, &capErr) {
				out = append(out, fmt.Sprintf("%s: Require returned %v, want a capability error", c.Capability, err))
				continue
			}
			if capErr.Disposition() != c.Disposition || capErr.Capability() != c.Capability {
				out = append(out, fmt.Sprintf("%s: got %s %q, want %s", c.Capability,
					capErr.Disposition(), capErr.Capability(), c.Disposition))
			}
			var refusal *table.Refusal
			if errors.As(err, &refusal) {
				out = append(out, fmt.Sprintf("%s: a capability error is not a refusal", c.Capability))
			}
		default:
			out = append(out, fmt.Sprintf("%s: disposition %q", c.Capability, c.Disposition))
		}
	}
	// Identifiers near a known one, and the empty one, are unknown.
	for _, id := range s.Unknown {
		var capErr *table.CapabilityError
		err := table.Require(id)
		if !errors.As(err, &capErr) || capErr.Disposition() != table.DispositionUnknown || capErr.Capability() != id {
			out = append(out, fmt.Sprintf("%q: Require returned %v, want unknown", id, err))
		}
	}
	return out
}

func TestCapabilitySuite(t *testing.T) {
	s := loadCapabilitySuite(t)
	if len(s.Cases) != 18 || len(s.Surfaces) != 3 || len(s.Capabilities) != 6 || len(s.Unknown) != 9 {
		t.Fatalf("suite holds %d cases, %d surfaces, %d capabilities, %d unknown; want 18, 3, 6, 9",
			len(s.Cases), len(s.Surfaces), len(s.Capabilities), len(s.Unknown))
	}
	ran := 0
	for _, c := range s.Cases {
		if c.Surface == goSurface {
			ran++
		}
	}
	if ran != 6 {
		t.Fatalf("%d cases for %s, want 6", ran, goSurface)
	}
	// Every capability the suite names is known to this package, each
	// surface has one case for every capability in order, and no case is an
	// unknown control: those are listed once for every surface.
	for _, id := range s.Capabilities {
		var capErr *table.CapabilityError
		if err := table.Require(id); errors.As(err, &capErr) && capErr.Disposition() == table.DispositionUnknown {
			t.Errorf("%s is not known to the package", id)
		}
	}
	for _, surface := range s.Surfaces {
		var named []string
		for _, c := range s.Cases {
			if c.Surface == surface.Surface {
				if c.Disposition == table.DispositionUnknown {
					t.Errorf("%s: case %q is an unknown control", surface.Surface, c.Capability)
				}
				named = append(named, c.Capability)
			}
		}
		if !reflect.DeepEqual(named, s.Capabilities) {
			t.Errorf("%s: cases %v", surface.Surface, named)
		}
	}
	for _, d := range evaluateGo(s) {
		t.Error(d)
	}
}

// Each kind of change to the expectations must be reported.
func TestCapabilitySuiteDetectsChanges(t *testing.T) {
	mutations := map[string]func(*capabilitySuite){
		"supported as unsupported": func(s *capabilitySuite) {
			s.Cases[slices.IndexFunc(s.Cases, func(c capabilityCase) bool {
				return c.Surface == goSurface && c.Capability == table.CapabilityQuantize
			})].Disposition = table.DispositionUnsupported
		},
		"unsupported as supported": func(s *capabilitySuite) {
			s.Cases[slices.IndexFunc(s.Cases, func(c capabilityCase) bool {
				return c.Surface == goSurface && c.Capability == table.CapabilityXLSXWrite
			})].Disposition = "supported"
		},
		"unsupported as unknown": func(s *capabilitySuite) {
			s.Cases[slices.IndexFunc(s.Cases, func(c capabilityCase) bool {
				return c.Surface == goSurface && c.Capability == table.CapabilityBinary64Decision
			})].Disposition = table.DispositionUnknown
		},
		"known identifier as an unknown control": func(s *capabilitySuite) {
			s.Unknown = append(s.Unknown, table.CapabilityReadSchema)
		},
		"unsupported identifier as an unknown control": func(s *capabilitySuite) {
			s.Unknown = append(s.Unknown, table.CapabilityXLSXWrite)
		},
		"report entry missing": func(s *capabilitySuite) {
			s.Surfaces[0].Report = s.Surfaces[0].Report[1:]
		},
		"report entry repeated": func(s *capabilitySuite) {
			s.Surfaces[0].Report = append(s.Surfaces[0].Report, s.Surfaces[0].Report[0])
		},
		"report entry from another surface": func(s *capabilitySuite) {
			s.Surfaces[0].Report = append(s.Surfaces[0].Report, table.CapabilityBinary64Decision)
		},
	}
	for name, mutate := range mutations {
		s := loadCapabilitySuite(t)
		if s.Surfaces[0].Surface != goSurface {
			t.Fatalf("first surface is %s", s.Surfaces[0].Surface)
		}
		mutate(&s)
		if len(evaluateGo(s)) == 0 {
			t.Errorf("%s: no disagreement reported", name)
		}
	}
}

func TestCapabilityErrors(t *testing.T) {
	err := table.Require(table.CapabilityXLSXWrite)
	if err == nil || err.Error() != `capability-unsupported: "xlsx-write"` {
		t.Errorf("unsupported: %v", err)
	}
	// Go and Rust give the same text for these identifiers.
	for id, want := range map[string]string{
		"no\nsuch": `capability-unknown: "no\nsuch"`,
		`a"b\c'd`:  `capability-unknown: "a\"b\\c'd"`,
	} {
		if err := table.Require(id); err == nil || err.Error() != want {
			t.Errorf("unknown %q: %v", id, err)
		}
	}
	// The caller owns the list.
	list := table.Capabilities()
	list[0] = "changed"
	if table.Capabilities()[0] != table.CapabilityReadSchema {
		t.Error("Capabilities returned shared storage")
	}
}

// extensionScopes reads the extensions list of a repository declaration
// and returns the identifiers of each scope. It accepts the block form the
// declaration uses: one "- key: value" line per entry, further "key: value"
// lines for the same entry, keys in any order, values plain or quoted, and
// trailing comments. An entry without an id or a scope is an error.
func extensionScopes(text string) (map[string][]string, error) {
	scopes := map[string][]string{}
	var entry map[string]string
	flush := func() error {
		if entry == nil {
			return nil
		}
		if entry["id"] == "" || entry["scope"] == "" {
			return fmt.Errorf("extension without an id or a scope: %v", entry)
		}
		scopes[entry["scope"]] = append(scopes[entry["scope"]], entry["id"])
		entry = nil
		return nil
	}
	in := false
	scanner := bufio.NewScanner(strings.NewReader(text))
	for scanner.Scan() {
		raw := scanner.Text()
		line := strings.TrimSpace(raw)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		if !strings.HasPrefix(raw, " ") && !strings.HasPrefix(raw, "-") {
			if err := flush(); err != nil {
				return nil, err
			}
			in = strings.HasPrefix(raw, "extensions:")
			continue
		}
		if !in {
			continue
		}
		if rest, ok := strings.CutPrefix(line, "- "); ok {
			if err := flush(); err != nil {
				return nil, err
			}
			entry = map[string]string{}
			line = rest
		}
		if entry == nil {
			return nil, fmt.Errorf("extension line outside an entry: %q", raw)
		}
		key, value, ok := strings.Cut(line, ":")
		if !ok {
			return nil, fmt.Errorf("extension line without a key: %q", raw)
		}
		value = strings.TrimSpace(value)
		if at := strings.Index(value, " #"); at >= 0 {
			value = strings.TrimSpace(value[:at])
		}
		if len(value) >= 2 && (value[0] == '"' || value[0] == '\'') && value[len(value)-1] == value[0] {
			value = value[1 : len(value)-1]
		}
		entry[strings.TrimSpace(key)] = value
	}
	if err := scanner.Err(); err != nil {
		return nil, err
	}
	if err := flush(); err != nil {
		return nil, err
	}
	return scopes, nil
}

func TestExtensionScopes(t *testing.T) {
	got, err := extensionScopes(`class: prims  # a comment
extensions:
  - id: cli-binary-name
    scope: cli
    reason: "a reason: with a colon"
  - id: a  # trailing comment
    scope: go-only  # trailing comment
  - scope: "go-only"
    id: 'b'
  - id: c
    scope: rust-only
target: after
`)
	want := map[string][]string{"cli": {"cli-binary-name"}, "go-only": {"a", "b"}, "rust-only": {"c"}}
	if err != nil || !reflect.DeepEqual(got, want) {
		t.Errorf("got %v, %v; want %v", got, err, want)
	}
	if _, err := extensionScopes("extensions:\n  - id: a\n    reason: r\n"); err == nil {
		t.Error("an entry without a scope was accepted")
	}
}

// The per-tree extensions of the repository declaration and the suite name
// the same capabilities: a rust-only extension is supported by a Rust
// surface and unsupported here, and a go-only one the reverse.
func TestCapabilitySuiteMatchesRepoDeclaration(t *testing.T) {
	s := loadCapabilitySuite(t)
	text, err := os.ReadFile(filepath.FromSlash(repoDeclarationPath))
	if err != nil {
		t.Fatal(err)
	}
	declared, err := extensionScopes(string(text))
	if err != nil {
		t.Fatal(err)
	}
	supported := func(prefix, capability string) bool {
		for _, c := range s.Cases {
			if strings.HasPrefix(c.Surface, prefix) && c.Capability == capability && c.Disposition == "supported" {
				return true
			}
		}
		return false
	}
	derived := map[string][]string{}
	for _, capability := range s.Capabilities {
		rust, golang := supported("rust/", capability), supported("go/", capability)
		switch {
		case rust && !golang:
			derived["rust-only"] = append(derived["rust-only"], capability)
		case golang && !rust:
			derived["go-only"] = append(derived["go-only"], capability)
		}
	}
	for _, scope := range []string{"rust-only", "go-only"} {
		slices.Sort(declared[scope])
		slices.Sort(derived[scope])
		if !slices.Equal(declared[scope], derived[scope]) {
			t.Errorf("%s: declared %v, suite %v", scope, declared[scope], derived[scope])
		}
	}
	if len(declared["rust-only"]) != 2 || len(declared["cli"]) != 1 {
		t.Errorf("extensions %v", declared)
	}
}
