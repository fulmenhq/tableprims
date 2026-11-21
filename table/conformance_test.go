// SPDX-License-Identifier: MIT OR Apache-2.0

package table_test

// Runs the vectors and fixtures of the Tabular Exchange Standard, contract
// revision tabular-exchange/0.1.0, through this package and compares each
// outcome with the expectation the vector states.
//
// The files are vendored unedited under third_party/crucible/; see
// PROVENANCE.md there. Their bytes are checked against SHA256SUMS before any
// vector runs. Expectations are read from those files and never computed
// here.
//
// Not run, as in the Rust tests: the 92 result fixtures and the 70 matching
// vectors. This package reads schemas, policies and rows; it does not read
// results, and matching a result to an output and a request is done by a
// consumer of results. Their counts are asserted, so that a change to the
// release is seen.

import (
	"bufio"
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"maps"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/fulmenhq/tableprims/table"
)

// sumsSHA256 is the SHA-256 of third_party/crucible/SHA256SUMS, which lists
// the digest of every vendored file.
const sumsSHA256 = "e42d4d5282f0c2e3d8f8e671c13475c32a2bddb1de65af67757d4c3143f3c61f"

// The policy the value, row and document suites run under, and the schema a
// policy fixture is read beside when it names none. They are the ones the
// Rust tests use.
const (
	basePolicy = `{"format":"tabular-exchange.policy/0","exact_numbers":"text-always","extensions":{"tableprims":{"sheet":"Data"}}}`
	baseSchema = `{"format":"tabular-exchange.schema/0","columns":[{"id":"a","label":"A","type":"string","nullable":true}]}`
	// maxLine is the line limit of the runs: the default line limit of the
	// workbook profile of tablebolt.
	maxLine = 1 << 20
)

// wantRuns is how many vectors each suite runs; together 477.
var wantRuns = []struct {
	suite string
	runs  int
}{
	{"value/integer", 43},
	{"value/decimal", 45},
	{"value/date", 28},
	{"value/local-timestamp", 45},
	{"value/offset-timestamp", 39},
	{"value/cells", 25},
	{"value/quantize", 77},
	{"row-vectors", 76},
	{"document-vectors", 31},
	{"fixtures/table-schema", 36},
	{"fixtures/conversion-policy", 32},
}

const wantTotal = 477

func vendored() string { return filepath.Join("..", "third_party", "crucible") }

func data() string {
	return filepath.Join(vendored(), "v0.5.1", "config", "data", "tabular-exchange", "v0.1.0")
}

// fromSlash turns a path as the release writes it into a local path.
func fromSlash(root, path string) string { return filepath.Join(root, filepath.FromSlash(path)) }

func sha256Hex(b []byte) string {
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

// verifyVendored compares the files under v0.5.1 with SHA256SUMS and
// returns every difference: a file whose bytes differ, a listed file that is
// absent and a file that is not listed.
func verifyVendored() ([]string, error) {
	sums, err := os.ReadFile(filepath.Join(vendored(), "SHA256SUMS"))
	if err != nil {
		return nil, err
	}
	var problems []string
	if got := sha256Hex(sums); got != sumsSHA256 {
		problems = append(problems, "SHA256SUMS was changed")
	}
	root := filepath.Join(vendored(), "v0.5.1")
	listed := map[string]bool{}
	scanner := bufio.NewScanner(bytes.NewReader(sums))
	for scanner.Scan() {
		digest, path, ok := strings.Cut(scanner.Text(), "  ")
		if !ok {
			problems = append(problems, "unreadable line in SHA256SUMS")
			continue
		}
		if listed[path] {
			problems = append(problems, "listed twice: "+path)
		}
		listed[path] = true
		content, err := os.ReadFile(fromSlash(root, path))
		switch {
		case err != nil:
			problems = append(problems, "missing: "+path)
		case sha256Hex(content) != digest:
			problems = append(problems, "changed: "+path)
		}
	}
	if len(listed) != 173 {
		problems = append(problems, fmt.Sprintf("SHA256SUMS lists %d files, not 173", len(listed)))
	}
	err = filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil || entry.IsDir() {
			return err
		}
		relative, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		if !listed[filepath.ToSlash(relative)] {
			problems = append(problems, "not listed: "+filepath.ToSlash(relative))
		}
		return nil
	})
	return problems, err
}

// entry is one vector or fixture, each member kept as its JSON spelling.
type entry map[string]json.RawMessage

func (e entry) has(name string) bool { _, ok := e[name]; return ok }

// text decodes a member that is a JSON string.
func (e entry) text(t *testing.T, name string) string {
	t.Helper()
	var s string
	if err := json.Unmarshal(e[name], &s); err != nil {
		t.Fatalf("member %s is not a string: %v", name, err)
	}
	return s
}

// compact is a member's JSON spelling without insignificant whitespace.
func (e entry) compact(t *testing.T, name string) string {
	t.Helper()
	var b bytes.Buffer
	if err := json.Compact(&b, e[name]); err != nil {
		t.Fatalf("member %s: %v", name, err)
	}
	return b.String()
}

// scalarText is how an outcome holds a member: a string decoded, any other
// JSON value as written.
func (e entry) scalarText(t *testing.T, name string) string {
	t.Helper()
	raw := bytes.TrimSpace(e[name])
	if len(raw) > 0 && raw[0] == '"' {
		return e.text(t, name)
	}
	return string(raw)
}

func load(t *testing.T, name string, into any) {
	t.Helper()
	content, err := os.ReadFile(fromSlash(data(), name))
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(content, into); err != nil {
		t.Fatalf("%s: %v", name, err)
	}
}

// ran is what one run through the package produced.
type ran struct {
	err     error
	rows    int
	last    table.Row
	located bool
}

// run reads a schema, a policy and rows, as tablebolt does: the schema, then
// the policy, then the policy against the schema, then the rows.
func run(schema, policy, rows []byte) ran {
	s, err := table.ReadSchema(schema)
	if err != nil {
		return ran{err: err}
	}
	p, err := table.ReadPolicy(policy)
	if err != nil {
		return ran{err: err}
	}
	plan, err := table.NewPlan(s, p)
	if err != nil {
		return ran{err: err}
	}
	reader := table.NewRowReader(bytes.NewReader(rows), plan, maxLine)
	var out ran
	for {
		row, err := reader.Next()
		if errors.Is(err, io.EOF) {
			return out
		}
		if err != nil {
			out.err = err
			return out
		}
		out.rows++
		out.last = row
	}
}

// outcome holds named members; an absent member is not in the map.
type outcome map[string]string

// expected is the outcome a vector states. A refusal states its category
// and rule, and with located its row and column; an accepted vector states
// the members named in accepted that it has.
func expected(t *testing.T, e entry, located bool, accepted ...string) outcome {
	t.Helper()
	out := outcome{"verdict": e.text(t, "verdict")}
	if out["verdict"] == "refuse" {
		for _, name := range []string{"category", "rule"} {
			if e.has(name) {
				out[name] = e.scalarText(t, name)
			}
		}
		if located {
			for _, name := range []string{"row", "column"} {
				if e.has(name) {
					out[name] = e.scalarText(t, name)
				}
			}
		}
		return out
	}
	for _, name := range accepted {
		if e.has(name) {
			out[name] = e.scalarText(t, name)
		}
	}
	return out
}

// actual is the outcome of a run in the same members. A refusal always
// shows its location, so that a location where a vector states none
// disagrees.
func actual(r ran) outcome {
	if r.err == nil {
		return outcome{"verdict": "accept"}
	}
	var refusal *table.Refusal
	if !errors.As(r.err, &refusal) {
		return outcome{"verdict": "error that is not a refusal: " + r.err.Error()}
	}
	out := outcome{
		"verdict":  "refuse",
		"category": refusal.Category(),
		"rule":     refusal.Rule(),
	}
	if row, ok := refusal.Row(); ok {
		out["row"] = strconv.FormatUint(row, 10)
	}
	if column, ok := refusal.Column(); ok {
		out["column"] = column
	}
	return out
}

// cellLocated adds the location every value refusal must have: the only
// cell there is, row 0 of column v.
func cellLocated(want outcome) outcome {
	if want["verdict"] == "refuse" {
		want["row"], want["column"] = "0", "v"
	}
	return want
}

// suite counts the vectors one suite ran and records disagreements.
type suite struct {
	t    *testing.T
	name string
	runs int
}

func (s *suite) record(label string, want, got outcome) {
	s.t.Helper()
	s.runs++
	if !maps.Equal(want, got) {
		s.t.Errorf("%s: %s\n    expected: %v\n    actual:   %v", s.name, label, want, got)
	}
}

func schemaOf(column string) []byte {
	return []byte(`{"format":"tabular-exchange.schema/0","columns":[` + column + `]}`)
}

func runTexts(t *testing.T, s *suite, kind string, entries []entry) {
	for i, e := range entries {
		column := `{"id":"v","label":"V","type":"` + kind + `","nullable":false`
		for _, name := range []string{"precision", "scale", "fraction"} {
			if e.has(name) {
				column += `,"` + name + `":` + e.compact(t, name)
			}
		}
		column += "}"
		rows := "[" + e.compact(t, "text") + "]\n"
		r := run(schemaOf(column), []byte(basePolicy), []byte(rows))
		s.record(fmt.Sprintf("%s[%d] %s", kind, i, e.compact(t, "text")),
			cellLocated(expected(t, e, false)), actual(r))
	}
}

func runCells(t *testing.T, s *suite, entries []entry) {
	for i, e := range entries {
		var column map[string]json.RawMessage
		if err := json.Unmarshal(e["column"], &column); err != nil {
			t.Fatal(err)
		}
		spelled := e.compact(t, "column")
		for _, member := range []struct{ name, value string }{{"label", "V"}, {"id", "v"}} {
			if _, ok := column[member.name]; !ok {
				spelled = `{"` + member.name + `":"` + member.value + `",` + spelled[1:]
			}
		}
		rows := "[" + e.compact(t, "value") + "]\n"
		r := run(schemaOf(spelled), []byte(basePolicy), []byte(rows))
		s.record(fmt.Sprintf("cells[%d] %s <- %s", i, spelled, rows),
			cellLocated(expected(t, e, false)), actual(r))
	}
}

func runQuantize(t *testing.T, s *suite, entries []entry) (flagged int) {
	for i, e := range entries {
		column := `{"id":"v","label":"V","type":"decimal","precision":` + e.compact(t, "precision") +
			`,"scale":` + e.compact(t, "scale") + `,"nullable":false}`
		policy := `{"format":"tabular-exchange.policy/0","exact_numbers":"text-always","quantize":[{"column":"v","scale":` +
			e.compact(t, "target_scale") + `,"rounding":` + e.compact(t, "rounding") +
			`}],"extensions":{"tableprims":{"sheet":"Data"}}}`
		rows := "[" + e.compact(t, "text") + "]\n"
		r := run(schemaOf(column), []byte(policy), []byte(rows))
		got := actual(r)
		if r.err == nil && r.rows == 1 {
			got["result"] = r.last.Values[0].String()
			change := r.last.Changes[0]
			got["value_changed"] = strconv.FormatBool(change&table.ValueChanged != 0)
			got["scale_changed"] = strconv.FormatBool(change&table.ScaleChanged != 0)
		}
		want := cellLocated(expected(t, e, false, "result", "value_changed", "scale_changed"))
		if want["verdict"] == "accept" {
			if len(want) != 4 {
				t.Errorf("quantize[%d] does not state its result and both flags", i)
			}
			flagged++
		}
		s.record(fmt.Sprintf("quantize[%d] %s at scale %s %s", i, e.compact(t, "text"),
			e.compact(t, "target_scale"), e.compact(t, "rounding")), want, got)
	}
	return flagged
}

// caseBytes are the bytes a case gives: input_hex when it has one, or input
// as UTF-8.
func caseBytes(t *testing.T, e entry) []byte {
	t.Helper()
	if e.has("input_hex") {
		b, err := hex.DecodeString(e.text(t, "input_hex"))
		if err != nil {
			t.Fatal(err)
		}
		return b
	}
	return []byte(e.text(t, "input"))
}

func TestConformance(t *testing.T) {
	problems, err := verifyVendored()
	if err != nil {
		t.Fatalf("reading the vendored files: %v", err)
	}
	if len(problems) > 0 {
		t.Fatalf("the vendored files are not the bytes of the release:\n%s", strings.Join(problems, "\n"))
	}

	runs := map[string]int{}
	add := func(s *suite) { runs[s.name] = s.runs }

	// Value vectors.
	var values map[string]json.RawMessage
	load(t, "value-vectors.json", &values)
	entriesOf := func(raw json.RawMessage) []entry {
		var entries []entry
		if err := json.Unmarshal(raw, &entries); err != nil {
			t.Fatal(err)
		}
		return entries
	}
	for _, kind := range []string{"integer", "decimal", "date", "local-timestamp", "offset-timestamp"} {
		s := &suite{t: t, name: "value/" + kind}
		entries := entriesOf(values[kind])
		runTexts(t, s, kind, entries)
		add(s)
	}
	cells := &suite{t: t, name: "value/cells"}
	runCells(t, cells, entriesOf(values["cells"]))
	add(cells)
	quantize := &suite{t: t, name: "value/quantize"}
	if flagged := runQuantize(t, quantize, entriesOf(values["quantize"])); flagged != 75 {
		t.Errorf("%d accepted quantize vectors, not 75", flagged)
	}
	add(quantize)

	// Row vectors: each case under its own schema or the file's. An
	// accepted case states its row count; a refusal its row and column, or
	// that it has none.
	var rowFile struct {
		Schema json.RawMessage `json:"schema"`
		Cases  []entry         `json:"cases"`
	}
	load(t, "row-vectors.json", &rowFile)
	rows := &suite{t: t, name: "row-vectors"}
	for _, c := range rowFile.Cases {
		schema := rowFile.Schema
		if c.has("schema") {
			schema = c["schema"]
		}
		r := run(schema, []byte(basePolicy), caseBytes(t, c))
		got := actual(r)
		if r.err == nil {
			got["rows"] = strconv.Itoa(r.rows)
		}
		want := expected(t, c, true, "rows")
		if want["verdict"] == "accept" && want["rows"] == "" {
			t.Errorf("row case %q states no row count", c.text(t, "name"))
		}
		rows.record(c.text(t, "name"), want, got)
	}
	add(rows)

	// Document vectors: a table schema as exact bytes, beside the base
	// policy and an empty input.
	var documentFile struct {
		Cases []entry `json:"cases"`
	}
	load(t, "document-vectors.json", &documentFile)
	documents := &suite{t: t, name: "document-vectors"}
	for _, c := range documentFile.Cases {
		if kind := c.text(t, "document"); kind != "table-schema" {
			t.Fatalf("document vector %q is a %s, which no suite runs", c.text(t, "name"), kind)
		}
		r := run(caseBytes(t, c), []byte(basePolicy), nil)
		documents.record(c.text(t, "name"), expected(t, c, false), actual(r))
	}
	add(documents)

	// Fixtures, as files, read as released. A policy fixture is read beside
	// the schema it names, or the base schema.
	var manifest struct {
		Fixtures []entry `json:"fixtures"`
	}
	load(t, "fixtures/manifest.json", &manifest)
	schemas := &suite{t: t, name: "fixtures/table-schema"}
	policies := &suite{t: t, name: "fixtures/conversion-policy"}
	results := 0
	for _, f := range manifest.Fixtures {
		path := f.text(t, "path")
		content, err := os.ReadFile(fromSlash(data(), path))
		if err != nil {
			t.Fatal(err)
		}
		switch kind := f.text(t, "document"); kind {
		case "table-schema":
			schemas.record(path, expected(t, f, false), actual(run(content, []byte(basePolicy), nil)))
		case "conversion-policy":
			schema := []byte(baseSchema)
			if f.has("with_schema") {
				if schema, err = os.ReadFile(fromSlash(data(), f.text(t, "with_schema"))); err != nil {
					t.Fatal(err)
				}
			}
			policies.record(path, expected(t, f, false), actual(run(schema, content, nil)))
		case "conversion-result":
			results++ // not run: see the comment at the top of this file
		default:
			t.Errorf("fixture %s is a %s, which no suite runs", path, kind)
		}
	}
	add(schemas)
	add(policies)
	if results != 92 {
		t.Errorf("%d result fixtures, not 92", results)
	}
	var matching struct {
		Cases []json.RawMessage `json:"cases"`
	}
	load(t, "matching-vectors.json", &matching)
	if len(matching.Cases) != 70 {
		t.Errorf("%d matching vectors, not 70", len(matching.Cases))
	}

	// Every suite ran exactly the vectors the release holds.
	total := 0
	for _, want := range wantRuns {
		if runs[want.suite] != want.runs {
			t.Errorf("%s: %d run, want %d", want.suite, runs[want.suite], want.runs)
		}
		total += runs[want.suite]
		t.Logf("%-28s %3d run", want.suite, runs[want.suite])
	}
	if len(runs) != len(wantRuns) {
		t.Errorf("%d suites ran, want %d", len(runs), len(wantRuns))
	}
	if total != wantTotal {
		t.Errorf("%d vectors run, want %d", total, wantTotal)
	}
	t.Logf("%-28s %3d run", "total", total)
}
