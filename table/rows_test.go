// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"strings"
	"testing"
	"testing/iotest"
)

func lineRule(t *testing.T, line string) string {
	t.Helper()
	_, err := parseLine(line)
	if err == nil {
		return ""
	}
	return err.Rule()
}

func TestLineScalarsAreRead(t *testing.T) {
	cells, err := parseLine(`["42", null, true, false, ""]`)
	want := []scalar{
		{kind: scalarText, text: "42"}, {kind: scalarNull}, {kind: scalarBoolean, boolean: true},
		{kind: scalarBoolean}, {kind: scalarText},
	}
	if err != nil || len(cells) != len(want) {
		t.Fatalf("%v %v", cells, err)
	}
	for i := range want {
		if cells[i] != want[i] {
			t.Errorf("cell %d: %+v", i, cells[i])
		}
	}
	if cells, err := parseLine("[]"); err != nil || len(cells) != 0 {
		t.Errorf("empty array: %v %v", cells, err)
	}
	if cells, err := parseLine(" \t[\t\"a\" ,\"b\" ] "); err != nil || len(cells) != 2 {
		t.Errorf("spaces and tabs: %v %v", cells, err)
	}
	cells, err = parseLine(`["a\"b\\c\/d\b\f\n\r\t", "é€", "😀", "\u0000", "000123", "-0"]`)
	if err != nil || cells[0].text != "a\"b\\c/d\b\f\n\r\t" || cells[1].text != "é€" ||
		cells[2].text != "😀" || cells[3].text != "\x00" || cells[4].text != "000123" || cells[5].text != "-0" {
		t.Errorf("strings: %+v %v", cells, err)
	}
}

func TestLineRefusals(t *testing.T) {
	for want, lines := range map[string][]string{
		"numeric-literal": {
			"[1]", "[-1]", "[0.5]", `["a", 12]`, "[1e5]", "[0]", "[-0]", "[1.5]", "[1E+3]",
			"[1 ,null]", "[1\t]", "[1", "[1,", "[1 2]", "[-1.5e-7,\"a\"", "[null, 12",
		},
		"malformed-row": {
			"[-]", "[01]", "[1.]", "[1e]", "[1x]", "[-a]", "[00]", "[1e+]", "[1.5.2]", "[0x10]",
			"[1\"a\"]", "[1[]", "[1{}]", "[-", "[1.", "[null, 1x]", "[1é]", "[12:30]",
			"[", "[null", "[null,", "[null,]", "[,null]", "[null null]", "[nul]", "[True]",
			"[NaN]", "[+1]", "[.5]", "['a']", "[\"a]", "[\"a\\", "[null\r]", "[\u00a0null]",
			"[\"a\",\f\"1\"]",
		},
		"nested-value":     {"[[]]", "[{}]", `["a", ["b"]]`, `[{"a": null}]`},
		"not-an-array":     {"{}", `"a"`, "null", "1", "x"},
		"blank-line":       {"", " ", "\t \t"},
		"trailing-content": {"[] []", "[null]x", "[null],", "[null]]", "[null]\r"},
		"invalid-string": {
			`["\x41"]`, `["\u12"]`, `["\u12G4"]`, `["\ud83d"]`, `["\ud83dx"]`, `["\ud83dA"]`,
			`["\ude00"]`, "[\"a\tb\"]", "[\"a\x01b\"]",
		},
	} {
		for _, line := range lines {
			if got := lineRule(t, line); got != want {
				t.Errorf("%q: %s, want %s", line, got, want)
			}
		}
	}
}

// textPlan is a plan of one nullable string column, or of the given columns.
func textPlan(t *testing.T, columns ...Column) *Plan {
	t.Helper()
	if len(columns) == 0 {
		columns = []Column{{ID: "a", Type: StringType(), Nullable: true}}
	}
	schema, err := NewSchema(columns)
	if err != nil {
		t.Fatal(err)
	}
	plan, err := NewPlan(schema, nil)
	if err != nil {
		t.Fatal(err)
	}
	return plan
}

// readAll returns the rows read and the error that stopped the reader.
func readAll(r io.Reader, plan *Plan, limit int) ([]Row, error) {
	reader := NewRowReader(r, plan, limit)
	var rows []Row
	for {
		row, err := reader.Next()
		if err == io.EOF {
			return rows, nil
		}
		if err != nil {
			return rows, err
		}
		rows = append(rows, row)
	}
}

func refusalOf(t *testing.T, err error) *Refusal {
	t.Helper()
	var refusal *Refusal
	if !errors.As(err, &refusal) {
		t.Fatalf("not a refusal: %v", err)
	}
	return refusal
}

// wantRefusal checks the rule and row of a refusal; row -1 is no row.
func wantRefusal(t *testing.T, label string, err error, rule string, row int) {
	t.Helper()
	if err == nil {
		t.Errorf("%s: accepted, want %s", label, rule)
		return
	}
	refusal := refusalOf(t, err)
	at, has := refusal.Row()
	if refusal.Rule() != rule || has != (row >= 0) || (has && at != uint64(row)) {
		t.Errorf("%s: %v, want %s at row %d", label, err, rule, row)
	}
}

func TestReaderTerminators(t *testing.T) {
	for _, input := range []string{
		"[\"a\"]\n[null]\n", "[\"a\"]\r\n[null]\r\n", "[\"a\"]\n[null]", "[\"a\"]\r\n[null]",
	} {
		rows, err := readAll(strings.NewReader(input), textPlan(t), 64)
		if err != nil || len(rows) != 2 || rows[0].Values[0] != Text("a") || rows[1].Values[0] != (Null{}) ||
			rows[1].Index != 1 {
			t.Errorf("%q: %v %v", input, rows, err)
		}
	}
	if rows, err := readAll(strings.NewReader(""), textPlan(t), 64); err != nil || len(rows) != 0 {
		t.Errorf("empty input: %v %v", rows, err)
	}
	for _, c := range []struct {
		input string
		rule  string
		row   int
	}{
		{"[null]\n\n[null]\n", "blank-line", 1},
		{"[null]\n\r\n[null]\n", "blank-line", 1},
		{"[null]\n \n", "blank-line", 1},
		{"\n", "blank-line", 0},
		{"[null]\r[null]\n", "trailing-content", 0},
		{"[null]\r", "trailing-content", 0},
		{"\xEF\xBB\xBF[null]\n", "byte-order-mark", -1},
		{"[null]\n[\"\xFF\"]\n", "invalid-utf8", 1},
		{"[null]\n[\"\xED\xA0\x80\"]\n", "invalid-utf8", 1},
		{"[null]\n\xEF\xBB\xBF[null]\n", "not-an-array", 1},
	} {
		_, err := readAll(strings.NewReader(c.input), textPlan(t), 64)
		wantRefusal(t, c.input, err, c.rule, c.row)
	}
}

func TestLineLengthLimitExcludesTheTerminator(t *testing.T) {
	// "[null]" is six bytes.
	for _, input := range []string{"[null]\n", "[null]\r\n", "[null]"} {
		if rows, err := readAll(strings.NewReader(input), textPlan(t), 6); err != nil || len(rows) != 1 {
			t.Errorf("%q at 6: %v", input, err)
		}
		_, err := readAll(strings.NewReader(input), textPlan(t), 5)
		wantRefusal(t, input, err, "line-too-long", 0)
	}
	_, err := readAll(strings.NewReader("[null] \n"), textPlan(t), 6)
	wantRefusal(t, "a trailing space", err, "line-too-long", 0)
	_, err = readAll(bytes.NewReader(bytes.Repeat([]byte(" "), 10_000)), textPlan(t), 16)
	wantRefusal(t, "spaces", err, "line-too-long", 0)
	// Longer than the buffer of the reader, with and without a terminator.
	long := `["` + strings.Repeat("x", 200_000) + `"]`
	for _, input := range []string{long, long + "\n", long + "\r\n"} {
		if rows, err := readAll(strings.NewReader(input), textPlan(t), len(long)); err != nil || len(rows) != 1 {
			t.Errorf("a long line at its length: %v", err)
		}
		_, err := readAll(strings.NewReader(input), textPlan(t), len(long)-1)
		wantRefusal(t, "a long line", err, "line-too-long", 0)
	}
	// The limit is judged before the byte order mark and UTF-8.
	_, err = readAll(strings.NewReader("\xEF\xBB\xBF[null]\n"), textPlan(t), 6)
	wantRefusal(t, "a long first line with a mark", err, "line-too-long", 0)
	_, err = readAll(strings.NewReader("[null]\n[null]\n"), textPlan(t), -1)
	wantRefusal(t, "a negative limit", err, "line-too-long", 0)
}

func TestReaderStopsAtTheFirstError(t *testing.T) {
	reader := NewRowReader(strings.NewReader("[1]\n[null]\n"), textPlan(t), 64)
	_, first := reader.Next()
	_, second := reader.Next()
	if first == nil || first != second {
		t.Errorf("%v then %v", first, second)
	}
	reader = NewRowReader(strings.NewReader("[null]"), textPlan(t), 64)
	reader.Next()
	for range 2 {
		if _, err := reader.Next(); err != io.EOF {
			t.Errorf("after the end: %v", err)
		}
	}
}

func TestReadFailureIsAnIOFailure(t *testing.T) {
	cause := errors.New("device gone")
	_, err := readAll(io.MultiReader(strings.NewReader("[null]\n"), iotest.ErrReader(cause)), textPlan(t), 64)
	refusal := refusalOf(t, err)
	if refusal.Category() != CategoryIOFailure || refusal.Rule() != "read-failed" || !errors.Is(err, cause) {
		t.Errorf("%v", err)
	}
	// The cause is reachable through Unwrap and is not in the message.
	if err.Error() != "io-failure: read-failed" {
		t.Errorf("message: %q", err.Error())
	}
	if _, has := refusal.Row(); has {
		t.Error("a read failure has no row")
	}
}

func testSchema(t *testing.T) *Schema {
	t.Helper()
	schema, err := NewSchema([]Column{
		{ID: "id", Label: "Id", Type: IntegerType()},
		{ID: "amount", Label: "Amount", Type: DecimalOf(decimalType(t, 7, 4)), Nullable: true},
		{ID: "code", Label: "Code", Type: StringType(), Nullable: true},
		{ID: "active", Label: "Active", Type: BooleanType()},
	})
	if err != nil {
		t.Fatal(err)
	}
	return schema
}

func TestSchemaRefusesEmptyAndRepeatedIdentifiers(t *testing.T) {
	for _, columns := range [][]Column{
		nil,
		{{ID: "", Type: StringType()}},
		{{ID: "a", Type: StringType()}, {ID: "a", Type: IntegerType()}},
	} {
		if _, err := NewSchema(columns); rule(t, err) != "schema-columns-invalid" {
			t.Errorf("%v: %v", columns, err)
		}
	}
	if _, err := NewSchema([]Column{{ID: "a"}}); rule(t, err) != "schema-type-unsupported" {
		t.Errorf("zero type: %v", err)
	}
	if _, err := NewSchema([]Column{{ID: "a", Type: DecimalOf(DecimalType{})}}); rule(t, err) != "schema-member-invalid" {
		t.Errorf("zero decimal type: %v", err)
	}
	if _, err := NewSchema([]Column{{ID: "a", Label: "T", Type: IntegerType()}, {ID: "b", Label: "T", Type: IntegerType()}}); err != nil {
		t.Errorf("labels may repeat: %v", err)
	}
}

// check reads one line under the test schema.
func check(t *testing.T, line string) (Row, error) {
	t.Helper()
	plan, err := NewPlan(testSchema(t), nil)
	if err != nil {
		t.Fatal(err)
	}
	return NewRowReader(strings.NewReader(line), plan, 1024).Next()
}

func TestRowsAreCheckedAndLocated(t *testing.T) {
	row, err := check(t, `["42","123.4500","000123",true]`)
	if err != nil {
		t.Fatal(err)
	}
	integer, _ := ParseInteger("42")
	if row.Values[0] != integer || row.Values[1].String() != "123.4500" ||
		row.Values[2] != Text("000123") || row.Values[3] != Boolean(true) || row.Changes != nil {
		t.Errorf("values %v", row.Values)
	}
	null, _ := check(t, `["1",null,null,false]`)
	empty, _ := check(t, `["1",null,"",false]`)
	if null.Values[2] != (Null{}) || empty.Values[2] != Text("") {
		t.Error("null and the empty string are not distinct")
	}
	for _, c := range []struct {
		line, rule, column string
	}{
		{`[null,null,null,true]`, "null-not-allowed", "id"},
		{`["1",null]`, "arity-mismatch", "code"},
		{`[]`, "arity-mismatch", "id"},
		{`[null,null,null,null,null]`, "arity-mismatch", ""},
		{`[true,null,null,true]`, "wrong-scalar-kind", "id"},
		{`["1",null,null,"true"]`, "wrong-scalar-kind", "active"},
		{`["1",null,true,true]`, "wrong-scalar-kind", "code"},
		{`["000123","1.5",null,true]`, "exact-leading-zero", "id"},
		{`["1","1.5",null,true]`, "exact-scale-mismatch", "amount"},
	} {
		_, err := check(t, c.line)
		refusal := refusalOf(t, err)
		column, has := refusal.Column()
		if refusal.Rule() != c.rule || column != c.column || has != (c.column != "") {
			t.Errorf("%s: %v, want %s in %q", c.line, err, c.rule, c.column)
		}
		if row, ok := refusal.Row(); !ok || row != 0 {
			t.Errorf("%s: row %d %v", c.line, row, ok)
		}
	}
	_, err = check(t, `["secret-9999",null,null,true]`)
	if strings.Contains(err.Error(), "secret") {
		t.Errorf("message contains cell content: %v", err)
	}
}

func TestTemporalColumnsAreChecked(t *testing.T) {
	ms := precision(t, 3)
	plan := textPlan(t,
		Column{ID: "day", Type: DateType()},
		Column{ID: "seen", Type: LocalTimestampOf(ms), Nullable: true},
		Column{ID: "sent", Type: OffsetTimestampOf(ms), Nullable: true},
	)
	rows, err := readAll(strings.NewReader(
		`["2024-02-29","2024-02-29T12:00:00.000","2024-02-29T12:00:00.250+05:30"]`+"\n"+
			`["2024-02-29",null,"2024-02-29T12:00:00.250"]`+"\n"), plan, 1024)
	if len(rows) != 1 || rows[0].Values[2].String() != "2024-02-29T12:00:00.250+05:30" {
		t.Errorf("rows %v", rows)
	}
	refusal := refusalOf(t, err)
	row, _ := refusal.Row()
	column, _ := refusal.Column()
	if refusal.Rule() != "temporal-offset-mismatch" || row != 1 || column != "sent" {
		t.Errorf("%v", err)
	}
}

func TestQuantizedRows(t *testing.T) {
	schema, err := NewSchema([]Column{
		{ID: "id", Type: IntegerType()},
		{ID: "low", Type: DecimalOf(decimalType(t, 5, 3)), Nullable: true},
		{ID: "pad", Type: DecimalOf(decimalType(t, 4, 1)), Nullable: true},
	})
	if err != nil {
		t.Fatal(err)
	}
	policy, err := ReadPolicy([]byte(`{"format":"tabular-exchange.policy/0","exact_numbers":"require-exact",
		"quantize":[{"column":"pad","scale":3,"rounding":"toward-zero"},{"column":"low","scale":1,"rounding":"half-even"}]}`))
	if err != nil {
		t.Fatal(err)
	}
	plan, err := NewPlan(schema, policy)
	if err != nil {
		t.Fatal(err)
	}
	if ty, mode, ok := plan.Target(1); !ok || ty.Precision() != 5 || ty.Scale() != 1 || mode != HalfEven {
		t.Errorf("target %v %v %v", ty, mode, ok)
	}
	if _, _, ok := plan.Target(0); ok {
		t.Error("the integer column is not quantized")
	}
	input := `["1","12.345","1.5"]` + "\n" + `["2","1.250",null]` + "\n" + `["3",null,"0.0"]` + "\n" +
		`["4","1.000","12.5"]` + "\n"
	rows, err := readAll(strings.NewReader(input), plan, 1024)
	if err == nil || len(rows) != 3 {
		t.Fatalf("%d rows, %v", len(rows), err)
	}
	for _, c := range []struct {
		row, column int
		text        string
		change      Change
	}{
		{0, 1, "12.3", ValueChanged | ScaleChanged},
		{0, 2, "1.500", ScaleChanged},
		{1, 1, "1.2", ValueChanged | ScaleChanged},
		{1, 2, "null", 0},
		{2, 1, "null", 0},
		{2, 2, "0.000", ScaleChanged},
		{2, 0, "3", 0},
	} {
		got := rows[c.row]
		if got.Values[c.column].String() != c.text || got.Changes[c.column] != c.change {
			t.Errorf("row %d column %d: %v %v", c.row, c.column, got.Values[c.column], got.Changes[c.column])
		}
	}
	refusal := refusalOf(t, err)
	row, _ := refusal.Row()
	column, _ := refusal.Column()
	if refusal.Rule() != "quantize-overflow" || refusal.Category() != CategorySchemaMismatch || row != 3 || column != "pad" {
		t.Errorf("%v", err)
	}
}

// eofAfter returns its text, then an error that wraps io.EOF.
type eofAfter struct{ text string }

func (r *eofAfter) Read(p []byte) (int, error) {
	if r.text == "" {
		return 0, fmt.Errorf("stream closed: %w", io.EOF)
	}
	n := copy(p, r.text)
	r.text = r.text[n:]
	return n, nil
}

func TestAnErrorWrappingEOFIsAReadFailure(t *testing.T) {
	rows, err := readAll(&eofAfter{text: "[null]\n"}, textPlan(t), 64)
	refusal := refusalOf(t, err)
	if len(rows) != 1 || refusal.Rule() != "read-failed" || !errors.Is(err, io.EOF) {
		t.Errorf("%d rows, %v", len(rows), err)
	}
}

func TestRowReaderNeedsAReaderAndAPlan(t *testing.T) {
	plan := textPlan(t)
	for name, build := range map[string]func(){
		"a nil reader":         func() { NewRowReader(nil, plan, 64) },
		"a nil plan":           func() { NewRowReader(strings.NewReader("[null]"), nil, 64) },
		"a zero plan":          func() { NewRowReader(strings.NewReader("[null]"), &Plan{}, 64) },
		"a zero schema's plan": func() { NewRowReader(strings.NewReader("[]"), &Plan{schema: &Schema{}}, 64) },
	} {
		func() {
			defer func() {
				if recover() == nil {
					t.Errorf("%s: no panic", name)
				}
			}()
			build()
		}()
	}
}
