// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"runtime"
	"strconv"
	"strings"
	"testing"
	"time"
)

const fullSchema = `{
  "format": "tabular-exchange.schema/0",
  "columns": [
    {"id": "invoice", "label": "Invoice", "type": "string", "nullable": false},
    {"id": "qty", "label": "Qty", "type": "integer", "nullable": false},
    {"id": "amount", "label": "Amount", "type": "decimal", "precision": 12, "scale": 4, "nullable": true},
    {"id": "paid", "label": "Paid", "type": "boolean", "nullable": true},
    {"id": "day", "label": "", "type": "date", "nullable": true},
    {"id": "seen", "label": "Seen", "type": "local-timestamp", "fraction": 3, "nullable": true},
    {"id": "sent", "label": "Seen", "type": "offset-timestamp", "fraction": 0, "nullable": true}
  ]
}`

func readSchemaText(text string) (*Schema, error) { return ReadSchema([]byte(text)) }

func schemaWith(column string) (*Schema, error) {
	return readSchemaText(`{"format": "tabular-exchange.schema/0", "columns": [` + column + `]}`)
}

func readPolicyText(text string) (*Policy, error) { return ReadPolicy([]byte(text)) }

// policyWith is a policy with rest appended to its top-level members.
func policyWith(rest string) (*Policy, error) {
	return readPolicyText(`{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", ` +
		`"extensions": {"tableprims": {"sheet": "Data"}}` + rest + `}`)
}

// policyExtended is a policy whose extensions member is extensions.
func policyExtended(extensions string) (*Policy, error) {
	return readPolicyText(`{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "extensions": ` +
		extensions + `}`)
}

// policyNamespace is a policy whose own namespace holds content.
func policyNamespace(content string) (*Policy, error) {
	return policyExtended(`{"tableprims": {` + content + `}}`)
}

// refusedAs checks the category and rule of a refusal.
func refusedAs(t *testing.T, label string, err error, category, want string) {
	t.Helper()
	if err == nil {
		t.Errorf("%s: accepted, want %s", label, want)
		return
	}
	refusal := refusalOf(t, err)
	if refusal.Rule() != want || refusal.Category() != category {
		t.Errorf("%s: %v, want %s: %s", label, err, category, want)
	}
	if _, has := refusal.Row(); has {
		t.Errorf("%s: a document refusal has a row", label)
	}
	if _, has := refusal.Column(); has {
		t.Errorf("%s: a document refusal has a column", label)
	}
}

func invalid(t *testing.T, label string, err error, want string) {
	t.Helper()
	refusedAs(t, label, err, CategoryInvalidInput, want)
}

func TestSchemaIsRead(t *testing.T) {
	schema, err := readSchemaText(fullSchema)
	if err != nil {
		t.Fatal(err)
	}
	var types []string
	for _, column := range schema.Columns() {
		types = append(types, column.Type.String())
	}
	if got := strings.Join(types, " "); got !=
		"string integer decimal(12,4) boolean date local-timestamp(3) offset-timestamp(0)" {
		t.Errorf("types %s", got)
	}
	if c := schema.Column(0); c.ID != "invoice" || c.Nullable {
		t.Errorf("column 0 %+v", c)
	}
	if !schema.Column(2).Nullable || schema.Column(4).Label != "" || schema.Len() != 7 {
		t.Error("column members")
	}
	if at, ok := schema.Index("seen"); !ok || at != 5 {
		t.Errorf("index %d %v", at, ok)
	}
}

func TestFormatIsCheckedBeforeAnythingElse(t *testing.T) {
	for _, text := range []string{
		`{"columns": []}`,
		`{"format": "tabular-exchange.schema/1", "columns": "nonsense", "extra": 1}`,
		`{"format": "tabular-exchange.policy/0", "columns": []}`,
		`{"format": 0, "columns": []}`,
		`{"format": "TABULAR-EXCHANGE.SCHEMA/0", "columns": []}`,
		`{"format": "tableprims.schema/0", "columns": [{"id": "a", "label": "", "type": "string", "nullable": true}]}`,
	} {
		_, err := readSchemaText(text)
		invalid(t, text, err, "schema-format-unsupported")
	}
	for _, text := range []string{
		`{"format": "tabular-exchange.schema/0"}`,
		`{"format": "tableprims.policy/0", "exact_numbers": "require-exact", "sheet": "Data"}`,
	} {
		_, err := readPolicyText(text)
		invalid(t, text, err, "policy-format-unsupported")
	}
}

func TestDocumentBytes(t *testing.T) {
	_, err := ReadSchema([]byte("{\"format\": \"\xff\"}"))
	invalid(t, "not UTF-8", err, "schema-not-utf8")
	_, err = ReadPolicy([]byte("\xEF\xBB\xBF{}"))
	invalid(t, "a byte order mark", err, "policy-malformed")
	_, err = ReadSchema(nil)
	invalid(t, "empty", err, "schema-malformed")
	big := []byte(`{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "x": "` +
		strings.Repeat("x", MaxDocumentBytes) + `"}`)
	_, err = ReadPolicy(big)
	refusedAs(t, "too large", err, CategoryResourceLimit, "policy-too-large")
	_, err = ReadSchema(big)
	refusedAs(t, "too large", err, CategoryResourceLimit, "schema-too-large")
	exact := []byte(`{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact"}`)
	exact = append(exact, []byte(strings.Repeat(" ", MaxDocumentBytes-len(exact)))...)
	if _, err := ReadPolicy(exact); err != nil {
		t.Errorf("a document of exactly the largest size: %v", err)
	}
}

func TestANumberTooLargeForItsMemberIsInvalidNotMalformed(t *testing.T) {
	decimal := func(precision string) error {
		_, err := schemaWith(`{"id": "a", "label": "", "type": "decimal", "precision": ` + precision +
			`, "scale": 0, "nullable": true}`)
		return err
	}
	for _, precision := range []string{
		"256", "9007199254740992", "18446744073709551615", "18446744073709551616",
		"99999999999999999999999999999999999999999999999999",
	} {
		invalid(t, precision, decimal(precision), "schema-member-invalid")
	}
	_, err := schemaWith(`{"id": "a", "label": "", "type": "local-timestamp", "fraction": 18446744073709551616, "nullable": true}`)
	invalid(t, "fraction", err, "schema-member-invalid")
	_, err = policyWith(`, "quantize": [{"column": "a", "scale": 340282366920938463463374607431768211456, "rounding": "half-even"}]`)
	invalid(t, "scale", err, "policy-member-invalid")
	invalid(t, "leading zero", decimal("018446744073709551616"), "schema-malformed")
	invalid(t, "fraction", decimal("18446744073709551616.0"), "schema-malformed")
}

func TestDocumentsMustBeStrictJSONObjects(t *testing.T) {
	for _, c := range []struct{ text, rule string }{
		{"[]", "schema-not-an-object"},
		{"", "schema-malformed"},
		{`{"format": "tabular-exchange.schema/0", "format": "tabular-exchange.schema/0", "columns": []}`, "schema-malformed"},
		{`{"format": "tabular-exchange.schema/0", "columns": [{"id": "a", "id": "a", "label": "", "type": "string", "nullable": true}]}`, "schema-malformed"},
		{`{"format": "tabular-exchange.schema/0", "columns": [{"id": "a", "label": "", "type": "string", "nullable": true, "x": [[[[1]]]]}]}`, "schema-malformed"},
		{`{"format": "tabular-exchange.schema/0", "columns": [{"id": []}]}`, "schema-malformed"},
	} {
		_, err := readSchemaText(c.text)
		invalid(t, c.text, err, c.rule)
	}
}

func TestUnknownMembersAreRefusedAtEveryDepth(t *testing.T) {
	_, err := readSchemaText(`{"format": "tabular-exchange.schema/0", "columns": [], "note": "x"}`)
	invalid(t, "top", err, "schema-member-unknown")
	for _, column := range []string{
		`{"id": "a", "label": "", "type": "string", "nullable": true, "note": 1}`,
		// Type parameters belong to their types.
		`{"id": "a", "label": "", "type": "string", "nullable": true, "precision": 5, "scale": 2}`,
		`{"id": "a", "label": "", "type": "integer", "nullable": true, "scale": 0}`,
		`{"id": "a", "label": "", "type": "date", "nullable": true, "fraction": 0}`,
		`{"id": "a", "label": "", "type": "decimal", "precision": 5, "scale": 2, "fraction": 0, "nullable": true}`,
	} {
		_, err := schemaWith(column)
		invalid(t, column, err, "schema-member-unknown")
	}
	for _, rest := range []string{
		`, "note": 1`,
		`, "quantize": [{"column": "a", "scale": 1, "rounding": "half-even", "x": 1}]`,
		`, "note": [[[[[1]]]]]`,
	} {
		_, err := policyWith(rest)
		invalid(t, rest, err, "policy-member-unknown")
	}
	for _, content := range []string{
		`"sheet": "Data", "limits": {"max_cols": 1}`,
		`"sheet": "Data", "note": 1`,
		`"sheet": "Data", "x": {"a": {"b": 1}}`,
	} {
		_, err := policyNamespace(content)
		invalid(t, content, err, "policy-member-unknown")
	}
	whole := func(body string) error {
		_, err := readPolicyText(`{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact"` + body + `}`)
		return err
	}
	for _, body := range []string{
		`, "sheet": "Data"`,
		`, "limits": {"max_rows": 10}`,
		`, "limits": {}`,
		`, "sheet": "Data", "extensions": {"tableprims": {"sheet": "Data"}}`,
		`, "tableprims": {"sheet": "Data"}`,
	} {
		invalid(t, body, whole(body), "policy-member-unknown")
	}
}

func TestColumnMembersAreValidated(t *testing.T) {
	for _, c := range []struct{ column, rule string }{
		{`{"label": "", "type": "string", "nullable": true}`, "schema-member-missing"},
		{`{"id": "a", "type": "string", "nullable": true}`, "schema-member-missing"},
		{`{"id": "a", "label": "", "type": "string"}`, "schema-member-missing"},
		{`{"id": "a", "label": "", "nullable": true}`, "schema-member-missing"},
		{`{"id": "a", "label": "", "type": "decimal", "scale": 2, "nullable": true}`, "schema-member-missing"},
		{`{"id": "a", "label": "", "type": "local-timestamp", "nullable": true}`, "schema-member-missing"},
		{`{"id": 1, "label": "", "type": "string", "nullable": true}`, "schema-member-invalid"},
		{`{"id": "a", "label": "", "type": "string", "nullable": "yes"}`, "schema-member-invalid"},
		{`{"id": "a", "label": "", "type": "decimal", "precision": 39, "scale": 2, "nullable": true}`, "schema-member-invalid"},
		{`{"id": "a", "label": "", "type": "decimal", "precision": 0, "scale": 0, "nullable": true}`, "schema-member-invalid"},
		{`{"id": "a", "label": "", "type": "decimal", "precision": 4, "scale": 5, "nullable": true}`, "schema-member-invalid"},
		{`{"id": "a", "label": "", "type": "decimal", "precision": 300, "scale": 2, "nullable": true}`, "schema-member-invalid"},
		{`{"id": "a", "label": "", "type": "decimal", "precision": "12", "scale": 2, "nullable": true}`, "schema-member-invalid"},
		{`{"id": "a", "label": "", "type": "offset-timestamp", "fraction": 10, "nullable": true}`, "schema-member-invalid"},
		{`{"id": "", "label": "", "type": "string", "nullable": true}`, "schema-columns-invalid"},
		{``, "schema-columns-invalid"},
		{`{"id": "a", "label": "", "type": "string", "nullable": true}, {"id": "a", "label": "", "type": "string", "nullable": true}`, "schema-columns-invalid"},
		{`"a"`, "schema-not-an-object"},
	} {
		_, err := schemaWith(c.column)
		invalid(t, c.column, err, c.rule)
	}
	for _, column := range []string{
		`{"id": "a", "label": "", "type": "float", "nullable": true}`,
		`{"id": "a", "label": "", "type": "String", "nullable": true}`,
	} {
		_, err := schemaWith(column)
		refusedAs(t, column, err, CategoryUnsupportedType, "schema-type-unsupported")
	}
	_, err := readSchemaText(`{"format": "tabular-exchange.schema/0", "columns": {}}`)
	invalid(t, "columns", err, "schema-member-invalid")
	_, err = readSchemaText(`{"format": "tabular-exchange.schema/0"}`)
	invalid(t, "columns", err, "schema-member-missing")
}

func TestPolicyIsRead(t *testing.T) {
	policy, err := readPolicyText(`{"format": "tabular-exchange.policy/0", "exact_numbers": "text-if-needed",
		"quantize": [{"column": "amount", "scale": 2, "rounding": "half-even"}],
		"extensions": {"tableprims": {"sheet": "Data", "limits": {"max_rows": 10, "max_empty_ranges": 5}}}}`)
	if err != nil {
		t.Fatal(err)
	}
	sheet, ok := policy.Sheet()
	q := policy.Quantize()
	if policy.ExactNumbers() != TextIfNeeded || sheet != "Data" || !ok || len(q) != 1 ||
		q[0] != (Quantize{Column: "amount", Scale: 2, Rounding: HalfEven}) {
		t.Errorf("policy %+v", policy)
	}
	want := Limits{MaxRows: Limit{10, true}, MaxEmptyRanges: Limit{5, true}}
	if policy.Limits() != want {
		t.Errorf("limits %+v", policy.Limits())
	}
	for _, text := range []string{
		`{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact"}`,
		`{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "quantize": []}`,
		`{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "extensions": {}}`,
		`{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "extensions": {"tableprims": {}}}`,
		`{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "extensions": {"tableprims": {"limits": {}}}}`,
	} {
		policy, err := readPolicyText(text)
		if err != nil {
			t.Errorf("%s: %v", text, err)
			continue
		}
		if _, ok := policy.Sheet(); ok || policy.Limits() != (Limits{}) || len(policy.Quantize()) != 0 {
			t.Errorf("%s: %+v", text, policy)
		}
	}
	for name, want := range map[string]ExactNumbers{
		"require-exact": RequireExact, "text-if-needed": TextIfNeeded, "text-always": TextAlways,
	} {
		policy, err := readPolicyText(`{"format": "tabular-exchange.policy/0", "exact_numbers": "` + name + `"}`)
		if err != nil || policy.ExactNumbers() != want || want.String() != name {
			t.Errorf("%s: %v", name, err)
		}
	}
	for _, mode := range []RoundingMode{HalfEven, HalfAwayFromZero, TowardZero, TowardPositive, TowardNegative} {
		policy, err := policyWith(`, "quantize": [{"column": "a", "scale": 0, "rounding": "` + mode.String() + `"}]`)
		if err != nil || policy.Quantize()[0].Rounding != mode {
			t.Errorf("%s: %v", mode, err)
		}
	}
	// A name a destination reserves is that destination's refusal.
	for _, allowed := range []string{"tableprims", "_tableprims", "_TaBlEpRiMs", " _tableprims"} {
		policy, err := policyNamespace(`"sheet": "` + allowed + `"`)
		if sheet, _ := policy.Sheet(); err != nil || sheet != allowed {
			t.Errorf("%q: %v", allowed, err)
		}
	}
}

func TestPolicyMembersAreValidated(t *testing.T) {
	whole := func(body string) error {
		_, err := readPolicyText(`{"format": "tabular-exchange.policy/0"` + body + `}`)
		return err
	}
	invalid(t, "missing", whole(`, "extensions": {"tableprims": {"sheet": "S"}}`), "policy-member-missing")
	invalid(t, "unknown name", whole(`, "exact_numbers": "exact"`), "policy-member-invalid")
	invalid(t, "not a string", whole(`, "exact_numbers": 1`), "policy-member-invalid")
	for _, value := range []string{`""`, "null", "1", "true", "[]", "{}"} {
		_, err := policyNamespace(`"sheet": ` + value)
		invalid(t, value, err, "policy-member-invalid")
	}
	for _, c := range []struct{ rest, rule string }{
		{`, "quantize": {}`, "policy-member-invalid"},
		{`, "quantize": ["a"]`, "policy-not-an-object"},
		{`, "quantize": [{"scale": 1, "rounding": "half-even"}]`, "policy-member-missing"},
		{`, "quantize": [{"column": "a", "rounding": "half-even"}]`, "policy-member-missing"},
		{`, "quantize": [{"column": "a", "scale": 1}]`, "policy-member-missing"},
		{`, "quantize": [{"column": "a", "scale": 256, "rounding": "half-even"}]`, "policy-member-invalid"},
		{`, "quantize": [{"column": "a", "scale": 39, "rounding": "half-even"}]`, "policy-member-invalid"},
		{`, "quantize": [{"column": "a", "scale": 1, "rounding": "half-up"}]`, "policy-member-invalid"},
		{`, "quantize": [{"column": {"a": {"b": {}}}, "scale": 1, "rounding": "half-even"}]`, "policy-member-invalid"},
		{`, "quantize": [{"column": "a", "scale": 1, "rounding": "half-even"}, {"column": "a", "scale": 2, "rounding": "half-even"}]`, "policy-quantize-duplicate"},
		{`, "note": [[[[[[1]]]]]]`, "policy-malformed"},
	} {
		_, err := policyWith(c.rest)
		invalid(t, c.rest, err, c.rule)
	}
	if _, err := policyWith(`, "quantize": [{"column": "a", "scale": 38, "rounding": "half-even"}]`); err != nil {
		t.Errorf("scale 38: %v", err)
	}
	for _, c := range []struct{ content, rule string }{
		{`"limits": []`, "policy-not-an-object"},
		{`"limits": {"max_rows": "10"}`, "policy-member-invalid"},
		{`"limits": {"max_line_bytes": null}`, "policy-member-invalid"},
	} {
		_, err := policyNamespace(c.content)
		invalid(t, c.content, err, c.rule)
	}
}

func TestExtensionShapeIsChecked(t *testing.T) {
	long := strings.Repeat("x", MaxExtensionString+1)
	for _, c := range []struct{ extensions, rule string }{
		// A namespace that is not an object is reported before a bound on
		// strings, numbers or the number of values, and after its own name.
		{`{"a":1,"tableprims":{"k":"` + long + `"}}`, "policy-not-an-object"},
		{`{"A":1}`, "policy-member-invalid"},
		{`{"a":1,"B":{}}`, "policy-not-an-object"},
		{"[]", "policy-not-an-object"},
		{"null", "policy-not-an-object"},
		{`"x"`, "policy-not-an-object"},
	} {
		_, err := policyExtended(c.extensions)
		invalid(t, c.extensions, err, c.rule)
	}
	for _, value := range []string{"null", "true", "1", `"x"`, "[]"} {
		for _, name := range []string{"tableprims", "example"} {
			_, err := policyExtended(`{"` + name + `": ` + value + `}`)
			invalid(t, name+" "+value, err, "policy-not-an-object")
		}
	}
	for _, name := range []string{
		"", "Tableprims", "1a", "-a", "a-", "a--b", "a_b", "a.b", "a b", "é", strings.Repeat("a", 65),
	} {
		_, err := policyExtended(`{"` + name + `": {}}`)
		invalid(t, name, err, "policy-member-invalid")
	}
	for _, extensions := range []string{
		`{"example": {}}`, `{"example-tool": {"setting": true}}`, `{"xlsx": {"sheet": "Data"}}`,
		`{"tableprims": {"sheet": "Data"}, "example": {}}`, `{"example": {}, "tableprims": {"sheet": "Data"}}`,
		`{"tableprims2": {}}`, `{"tableprims-x": {}}`, `{"a": {}}`, `{"a-1-b2": {}}`, `{"x0-0": {}}`,
		`{"` + strings.Repeat("a", 64) + `": {}}`,
		// The namespace object is at depth 3; its content reaches depth 6.
		`{"example": {"a": {"b": {"c": {"d": 1}}}}}`,
		`{"example": {"a": [{"b": [1]}]}}`,
	} {
		_, err := policyExtended(extensions)
		invalid(t, extensions, err, "policy-extension-unsupported")
	}
	for _, extensions := range []string{
		`{"example": {"a": {"b": {"c": {"d": {}}}}}}`,
		`{"example": {"a": {"b": {"c": [[]]}}}}`,
		`{"tableprims": {"sheet": "Data", "x": [[[[]]]]}}`,
	} {
		_, err := policyExtended(extensions)
		invalid(t, extensions, err, "policy-malformed")
	}
}

func TestExtensionBounds(t *testing.T) {
	namespaces := func(count int) string {
		text := `{"tableprims": {"sheet": "Data"}`
		for i := 1; i < count; i++ {
			text += `, "n` + strconv.Itoa(i) + `": {}`
		}
		return text + "}"
	}
	_, err := policyExtended(namespaces(8))
	invalid(t, "8 namespaces", err, "policy-extension-unsupported")
	_, err = policyExtended(namespaces(9))
	invalid(t, "9 namespaces", err, "policy-member-invalid")

	filled := func(fill int) string {
		members := make([]string, fill)
		for i := range members {
			members[i] = `"k` + strconv.Itoa(i) + `": null`
		}
		return `{"tableprims": {"sheet": "Data", "limits": {` + strings.Join(members, ", ") + `}}}`
	}
	_, err = policyExtended(filled(253))
	invalid(t, "256 values", err, "policy-member-unknown")
	_, err = policyExtended(filled(254))
	invalid(t, "257 values", err, "policy-member-invalid")
	array := func(count int) string {
		return `{"example": {"a": [` + strings.TrimSuffix(strings.Repeat("null, ", count), ", ") + `]}}`
	}
	_, err = policyExtended(array(254))
	invalid(t, "array of 254", err, "policy-extension-unsupported")
	_, err = policyExtended(array(255))
	invalid(t, "array of 255", err, "policy-member-invalid")

	value := func(text string) error {
		_, err := policyNamespace(`"sheet": "` + text + `"`)
		return err
	}
	for _, unit := range []string{"a", "é", "😀"} {
		if err := value(strings.Repeat(unit, 256)); err != nil {
			t.Errorf("256 of %s: %v", unit, err)
		}
		invalid(t, "257 of "+unit, value(strings.Repeat(unit, 257)), "policy-member-invalid")
	}
	name := func(length int) error {
		_, err := policyNamespace(`"sheet": "Data", "` + strings.Repeat("k", length) + `": 1`)
		return err
	}
	invalid(t, "name of 256", name(256), "policy-member-unknown")
	invalid(t, "name of 257", name(257), "policy-member-invalid")

	limit := func(number string) (*Policy, error) {
		return policyNamespace(`"sheet": "Data", "limits": {"max_rows": ` + number + `}`)
	}
	if policy, err := limit("9007199254740991"); err != nil || policy.Limits().MaxRows != (Limit{MaxCount, true}) {
		t.Errorf("largest count: %v", err)
	}
	for _, number := range []string{"9007199254740992", "18446744073709551615", "18446744073709551616"} {
		_, err := limit(number)
		invalid(t, number, err, "policy-member-invalid")
	}
	_, err = policyExtended(`{"example": {"a": [9007199254740992]}}`)
	invalid(t, "a large number in another namespace", err, "policy-member-invalid")
	for _, number := range []string{"-1", "1.0", "1e3", "01"} {
		_, err := limit(number)
		invalid(t, number, err, "policy-malformed")
	}
}

func TestPlans(t *testing.T) {
	schema, err := readSchemaText(fullSchema)
	if err != nil {
		t.Fatal(err)
	}
	plan := func(column string, scale int) error {
		policy, err := policyWith(`, "quantize": [{"column": "` + column + `", "scale": ` +
			strconv.Itoa(scale) + `, "rounding": "toward-zero"}]`)
		if err != nil {
			t.Fatal(err)
		}
		_, err = NewPlan(schema, policy)
		return err
	}
	for _, c := range []struct {
		column string
		scale  int
		rule   string
	}{
		{"missing", 2, "quantize-unknown-column"},
		{"qty", 0, "quantize-not-decimal"},
		{"invoice", 0, "quantize-not-decimal"},
		{"amount", 13, "quantize-scale-too-large"},
		{"amount", 12, ""},
		{"amount", 0, ""},
	} {
		err := plan(c.column, c.scale)
		if c.rule == "" {
			if err != nil {
				t.Errorf("%s at %d: %v", c.column, c.scale, err)
			}
			continue
		}
		refusedAs(t, c.column, err, CategorySchemaMismatch, c.rule)
	}
	policy, _ := policyWith(`, "quantize": [{"column": "amount", "scale": 2, "rounding": "toward-zero"}]`)
	p, err := NewPlan(schema, policy)
	if err != nil {
		t.Fatal(err)
	}
	for i := range schema.Len() {
		ty, mode, ok := p.Target(i)
		if ok != (i == 2) || (ok && (ty.Precision() != 12 || ty.Scale() != 2 || mode != TowardZero)) {
			t.Errorf("column %d: %v %v %v", i, ty, mode, ok)
		}
	}
	if p.Schema() != schema || p.Policy() != policy {
		t.Error("the plan does not keep its schema and policy")
	}
	if p, err := NewPlan(schema, nil); err != nil || p.Policy() != nil {
		t.Errorf("no policy: %v", err)
	}
}

func TestDocumentMessagesDoNotContainContent(t *testing.T) {
	_, err1 := schemaWith(`{"id": "a", "label": "", "type": "secret-9999", "nullable": true}`)
	_, err2 := policyWith(`, "secret-9999": 1`)
	_, err3 := policyExtended(`{"secret-9999": {}}`)
	_, err4 := policyNamespace(`"secret-9999": 1`)
	for _, err := range []error{err1, err2, err3, err4} {
		if err == nil || strings.Contains(err.Error(), "secret") {
			t.Errorf("%v", err)
		}
	}
}

// allocated returns the bytes allocated while read runs.
func allocated(read func()) uint64 {
	var before, after runtime.MemStats
	runtime.GC()
	runtime.ReadMemStats(&before)
	read()
	runtime.ReadMemStats(&after)
	return after.TotalAlloc - before.TotalAlloc
}

// hostile is a document of about size bytes: head, then unit repeated, then
// last and tail.
func hostile(size int, head, unit, last, tail string) []byte {
	var b strings.Builder
	b.WriteString(head)
	for b.Len()+len(unit)+len(last)+len(tail) < size {
		b.WriteString(unit)
	}
	b.WriteString(last + tail)
	return []byte(b.String())
}

// Reading a document allocates in proportion to what it keeps, not to the
// number of values it holds: no tree of values is built.
func TestHostileDocumentsAllocateInProportionToTheirSize(t *testing.T) {
	const size = 4 << 20
	head := `{"format":"tabular-exchange.schema/0","columns":[`
	keys := func() []byte {
		var b strings.Builder
		b.WriteString(head + "{")
		for i := 0; b.Len() < size-16; i++ {
			b.WriteString(`"k` + strconv.Itoa(i) + `":0,`)
		}
		b.WriteString(`"z":0}]}`)
		return []byte(b.String())
	}
	for _, c := range []struct {
		name     string
		document []byte
		rule     string
		multiple uint64
	}{
		// The text is copied once into a string; nothing else grows with it.
		{"numbers", hostile(size, head, "0,", "0", "]}"), "schema-not-an-object", 2},
		{"empty objects", hostile(size, head, "{},", "{}", "]}"), "schema-member-missing", 2},
		// One object of many members needs a set of its keys to find a
		// repeat.
		{"one object of many members", keys(), "schema-member-missing", 10},
	} {
		var err error
		bytes := allocated(func() { _, err = ReadSchema(c.document) })
		invalid(t, c.name, err, c.rule)
		if bytes > c.multiple*uint64(len(c.document)) {
			t.Errorf("%s: %d bytes allocated for a document of %d, more than %d times its size",
				c.name, bytes, len(c.document), c.multiple)
		}
	}
	// A policy of many distinct quantize requests is read in linear time
	// and keeps about what it holds.
	var b strings.Builder
	b.WriteString(`{"format":"tabular-exchange.policy/0","exact_numbers":"text-always","quantize":[`)
	const requests = 100_000
	for i := range requests {
		if i > 0 {
			b.WriteString(",")
		}
		b.WriteString(`{"column":"c` + strconv.Itoa(i) + `","scale":1,"rounding":"half-even"}`)
	}
	b.WriteString("]}")
	document := []byte(b.String())
	var policy *Policy
	var err error
	start := time.Now()
	bytes := allocated(func() { policy, err = ReadPolicy(document) })
	if elapsed := time.Since(start); err != nil || len(policy.Quantize()) != requests || elapsed > 5*time.Second {
		t.Errorf("%d requests: %v in %v", requests, err, elapsed)
	}
	if bytes > 12*uint64(len(document)) {
		t.Errorf("%d bytes allocated for a policy of %d", bytes, len(document))
	}
	// The first repeat in document order is the one reported, after
	// everything before it was read.
	repeated := strings.Replace(string(document), `"c99999"`, `"c5"`, 1)
	repeated = strings.Replace(repeated, `"c99998","scale":1,"rounding":"half-even"`, `"c99998","scale":1,"rounding":"half-up"`, 1)
	_, err = readPolicyText(repeated)
	invalid(t, "a late invalid request before a repeat", err, "policy-member-invalid")
	repeated = strings.Replace(string(document), `"c99999"`, `"c5"`, 1)
	_, err = readPolicyText(repeated)
	invalid(t, "a repeat", err, "policy-quantize-duplicate")
}

func TestPlansNeedASchemaWithColumns(t *testing.T) {
	for _, schema := range []*Schema{nil, {}} {
		_, err := NewPlan(schema, nil)
		invalid(t, "a schema without columns", err, "schema-columns-invalid")
	}
	// A schema of many columns is planned in linear time.
	columns := make([]Column, 50_000)
	var b strings.Builder
	b.WriteString(`{"format":"tabular-exchange.policy/0","exact_numbers":"text-always","quantize":[`)
	for i := range columns {
		id := "c" + strconv.Itoa(i)
		columns[i] = Column{ID: id, Type: DecimalOf(decimalType(t, 10, 2))}
		if i > 0 {
			b.WriteString(",")
		}
		b.WriteString(`{"column":"` + id + `","scale":1,"rounding":"half-even"}`)
	}
	b.WriteString("]}")
	schema, err := NewSchema(columns)
	if err != nil {
		t.Fatal(err)
	}
	policy, err := readPolicyText(b.String())
	if err != nil {
		t.Fatal(err)
	}
	start := time.Now()
	plan, err := NewPlan(schema, policy)
	if elapsed := time.Since(start); err != nil || elapsed > 5*time.Second {
		t.Errorf("%v in %v", err, elapsed)
	}
	if _, _, ok := plan.Target(len(columns) - 1); !ok {
		t.Error("the last column is not quantized")
	}
}

func TestANilPolicySetsNothing(t *testing.T) {
	var policy *Policy
	sheet, ok := policy.Sheet()
	if policy.ExactNumbers() != 0 || policy.Quantize() != nil || sheet != "" || ok || policy.Limits() != (Limits{}) {
		t.Error("a nil policy sets something")
	}
	schema, err := readSchemaText(fullSchema)
	if err != nil {
		t.Fatal(err)
	}
	plan, err := NewPlan(schema, nil)
	if err != nil || plan.Policy().Limits() != (Limits{}) {
		t.Errorf("a plan without a policy: %v", err)
	}
}
