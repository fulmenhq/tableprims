// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"strings"
	"unicode/utf8"
)

// Schema and policy documents.
//
// Each document is a single JSON object read with the strict reader. The
// reader checks the format member first and refuses any other value before
// looking at the rest; every member it does not know is refused, at every
// depth. A type-specific member on another type is refused.
//
// extensions is the one open member of a policy. Its keys are namespaces and
// its values are objects. The standard bounds it for every reader, and the
// bounds are judged before any namespace is looked up. This package
// implements one namespace, ExtensionNamespace, whose members are both
// optional: sheet, a string that is not empty, and limits, with max_rows,
// max_line_bytes and max_empty_ranges, each optional. A policy that carries
// any other namespace is refused. What a member of the namespace means to a
// destination, and whether the destination needs it, are the destination's.

// docFault is why a document was refused, before it is named for the kind
// of document.
type docFault uint8

const (
	faultJSON docFault = iota + 1
	faultNotAnObject
	faultFormat
	faultMissing
	faultUnknown
	faultInvalid
	faultTypeUnsupported
	faultColumns
	faultQuantizeDuplicate
	faultExtensionUnsupported
)

// docRefusal names a fault for a document whose rules start with prefix,
// "schema" or "policy", as the standard registers them.
func docRefusal(prefix string, fault docFault) *Refusal {
	switch fault {
	case faultJSON:
		return refuse(CategoryInvalidInput, prefix+"-malformed")
	case faultNotAnObject:
		return refuse(CategoryInvalidInput, prefix+"-not-an-object")
	case faultFormat:
		return refuse(CategoryInvalidInput, prefix+"-format-unsupported")
	case faultMissing:
		return refuse(CategoryInvalidInput, prefix+"-member-missing")
	case faultUnknown:
		return refuse(CategoryInvalidInput, prefix+"-member-unknown")
	case faultInvalid:
		return refuse(CategoryInvalidInput, prefix+"-member-invalid")
	case faultTypeUnsupported:
		return refuse(CategoryUnsupportedType, "schema-type-unsupported")
	case faultColumns:
		return refuse(CategoryInvalidInput, "schema-columns-invalid")
	case faultQuantizeDuplicate:
		return refuse(CategoryInvalidInput, "policy-quantize-duplicate")
	case faultExtensionUnsupported:
		return refuse(CategoryInvalidInput, "policy-extension-unsupported")
	}
	return refuse(CategoryInvalidInput, prefix+"-invalid")
}

// members are the members of one object, taken by name; whatever is left
// is unknown.
type members []jsonMember

func membersOf(value jsonValue) (members, docFault) {
	if value.kind != jsonObject {
		return nil, faultNotAnObject
	}
	return members(append([]jsonMember(nil), value.members...)), 0
}

func (m *members) take(name string) (jsonValue, bool) {
	for i, member := range *m {
		if member.key == name {
			last := len(*m) - 1
			(*m)[i] = (*m)[last]
			*m = (*m)[:last]
			return member.value, true
		}
	}
	return jsonValue{}, false
}

func (m *members) require(name string) (jsonValue, docFault) {
	value, ok := m.take(name)
	if !ok {
		return jsonValue{}, faultMissing
	}
	return value, 0
}

func (m *members) string(name string) (string, docFault) {
	value, fault := m.require(name)
	if fault != 0 {
		return "", fault
	}
	if value.kind != jsonString {
		return "", faultInvalid
	}
	return value.text, 0
}

func (m *members) boolean(name string) (bool, docFault) {
	value, fault := m.require(name)
	if fault != 0 {
		return false, fault
	}
	if value.kind != jsonBoolean {
		return false, faultInvalid
	}
	return value.boolean, 0
}

// small reads a number of at most 255.
func (m *members) small(name string) (int, docFault) {
	value, fault := m.require(name)
	if fault != 0 {
		return 0, fault
	}
	if value.kind != jsonInteger || value.integer > 255 {
		return 0, faultInvalid
	}
	return int(value.integer), 0
}

func (m *members) optionalCount(name string) (Limit, docFault) {
	value, ok := m.take(name)
	switch {
	case !ok:
		return Limit{}, 0
	case value.kind == jsonInteger && value.integer <= MaxCount:
		return Limit{Value: value.integer, Set: true}, 0
	}
	return Limit{}, faultInvalid
}

func (m members) finish() docFault {
	if len(m) != 0 {
		return faultUnknown
	}
	return 0
}

// document checks the bytes, reads the object and its format member, which
// is judged before any other member.
func document(data []byte, prefix, format string, maxDepth int) (members, *Refusal) {
	if len(data) > MaxDocumentBytes {
		return nil, refuse(CategoryResourceLimit, prefix+"-too-large")
	}
	if !utf8.Valid(data) {
		return nil, refuse(CategoryInvalidInput, prefix+"-not-utf8")
	}
	value, err := parseJSON(string(data), maxDepth)
	if err != 0 {
		return nil, docRefusal(prefix, faultJSON)
	}
	m, fault := membersOf(value)
	if fault != 0 {
		return nil, docRefusal(prefix, fault)
	}
	if found, ok := m.take("format"); !ok || found.kind != jsonString || found.text != format {
		return nil, docRefusal(prefix, faultFormat)
	}
	return m, nil
}

// ReadSchema reads a schema document of format SchemaFormat.
//
// It refuses, with a *Refusal naming the rule: a document larger than
// MaxDocumentBytes, bytes that are not UTF-8, text that is not one strict
// JSON object (a byte order mark included), another format, unknown
// members, unsupported column types, invalid type parameters and columns
// that do not form a schema.
func ReadSchema(data []byte) (*Schema, error) {
	m, err := document(data, "schema", SchemaFormat, schemaMaxDepth)
	if err != nil {
		return nil, err
	}
	schema, fault := readSchema(m)
	if fault != 0 {
		return nil, docRefusal("schema", fault)
	}
	return schema, nil
}

func readSchema(m members) (*Schema, docFault) {
	entries, fault := m.require("columns")
	if fault != 0 {
		return nil, fault
	}
	if entries.kind != jsonArray {
		return nil, faultInvalid
	}
	if fault := m.finish(); fault != 0 {
		return nil, fault
	}
	columns := make([]Column, 0, len(entries.items))
	for _, entry := range entries.items {
		column, fault := readColumn(entry)
		if fault != 0 {
			return nil, fault
		}
		columns = append(columns, column)
	}
	schema, err := NewSchema(columns)
	if err != nil {
		return nil, faultColumns
	}
	return schema, 0
}

func readColumn(entry jsonValue) (Column, docFault) {
	m, fault := membersOf(entry)
	if fault != 0 {
		return Column{}, fault
	}
	id, fault := m.string("id")
	if fault != 0 {
		return Column{}, fault
	}
	label, fault := m.string("label")
	if fault != 0 {
		return Column{}, fault
	}
	nullable, fault := m.boolean("nullable")
	if fault != 0 {
		return Column{}, fault
	}
	name, fault := m.string("type")
	if fault != 0 {
		return Column{}, fault
	}
	kind, known := kindByName(name)
	if !known {
		return Column{}, faultTypeUnsupported
	}
	t := Type{kind: kind}
	switch kind {
	case KindDecimal:
		precision, fault := m.small("precision")
		if fault != 0 {
			return Column{}, fault
		}
		scale, fault := m.small("scale")
		if fault != 0 {
			return Column{}, fault
		}
		decimal, err := NewDecimalType(precision, scale)
		if err != nil {
			return Column{}, faultInvalid
		}
		t.decimal = decimal
	case KindLocalTimestamp, KindOffsetTimestamp:
		digits, fault := m.small("fraction")
		if fault != 0 {
			return Column{}, fault
		}
		precision, err := NewFractionPrecision(digits)
		if err != nil {
			return Column{}, faultInvalid
		}
		t.fraction = precision
	}
	if fault := m.finish(); fault != 0 {
		return Column{}, fault
	}
	return Column{ID: id, Label: label, Type: t, Nullable: nullable}, 0
}

// ExactNumbers is how exact-number columns are represented in a destination
// that has a native numeric form.
type ExactNumbers uint8

// The representation policies of the standard.
const (
	// RequireExact uses the native numeric form and refuses a value it
	// cannot hold exactly.
	RequireExact ExactNumbers = iota + 1
	// TextIfNeeded uses the native numeric form, and writes a whole column
	// as text when any of its values cannot be held exactly.
	TextIfNeeded
	// TextAlways writes every exact-number column as text.
	TextAlways
)

var exactNumbersNames = [...]string{
	RequireExact: "require-exact",
	TextIfNeeded: "text-if-needed",
	TextAlways:   "text-always",
}

// String is the name of the policy in a policy document.
func (e ExactNumbers) String() string {
	if e >= RequireExact && int(e) < len(exactNumbersNames) {
		return exactNumbersNames[e]
	}
	return "exact-numbers(?)"
}

// Quantize is a request to re-express one decimal column at another scale.
type Quantize struct {
	// Column is the identifier of the column.
	Column string
	// Scale is the target scale, at most MaxDigits.
	Scale int
	// Rounding is the mode applied when the scale is lowered.
	Rounding RoundingMode
}

// Limit is one limit a policy may set.
type Limit struct {
	// Value is the limit, at most MaxCount.
	Value uint64
	// Set says whether the policy sets the limit. An absent limit leaves
	// the consumer's default.
	Set bool
}

// Limits are the limits a policy sets under ExtensionNamespace. A limit is
// read as written: what it bounds in a particular destination, and its
// ceiling there, are left to the consumer of the policy.
type Limits struct {
	// MaxRows is the largest number of rows.
	MaxRows Limit
	// MaxLineBytes is the largest length of one input line in bytes.
	MaxLineBytes Limit
	// MaxEmptyRanges is the largest number of empty-string ranges recorded
	// for a table.
	MaxEmptyRanges Limit
}

// Policy is a conversion policy as written in a policy document. It is not
// changed once read.
type Policy struct {
	exactNumbers ExactNumbers
	quantize     []Quantize
	sheet        string
	hasSheet     bool
	limits       Limits
}

// ExactNumbers is the representation policy for exact-number columns.
func (p *Policy) ExactNumbers() ExactNumbers { return p.exactNumbers }

// Quantize returns a copy of the quantize requests, in the order written.
func (p *Policy) Quantize() []Quantize { return append([]Quantize(nil), p.quantize...) }

// Sheet is the name of the destination sheet the policy gives under
// ExtensionNamespace, and whether it gives one. Requiring a name, reserving
// names and any further naming rule belong to the destination.
func (p *Policy) Sheet() (string, bool) { return p.sheet, p.hasSheet }

// Limits are the limits the policy sets under ExtensionNamespace.
func (p *Policy) Limits() Limits { return p.limits }

// ReadPolicy reads a policy document of format PolicyFormat.
//
// It refuses, with a *Refusal naming the rule: a document larger than
// MaxDocumentBytes, bytes that are not UTF-8, text that is not one strict
// JSON object (a byte order mark included), another format, unknown
// members, invalid values, a column quantized more than once, an extensions
// member outside its bounds and an extension namespace other than
// ExtensionNamespace. A policy that names no sheet is accepted.
func ReadPolicy(data []byte) (*Policy, error) {
	m, err := document(data, "policy", PolicyFormat, policyMaxDepth)
	if err != nil {
		return nil, err
	}
	policy, fault := readPolicy(m)
	if fault != 0 {
		return nil, docRefusal("policy", fault)
	}
	return policy, nil
}

func readPolicy(m members) (*Policy, docFault) {
	p := &Policy{}
	name, fault := m.string("exact_numbers")
	if fault != 0 {
		return nil, fault
	}
	for value, known := range exactNumbersNames {
		if known != "" && known == name {
			p.exactNumbers = ExactNumbers(value)
		}
	}
	if p.exactNumbers == 0 {
		return nil, faultInvalid
	}
	if requests, ok := m.take("quantize"); ok {
		if requests.kind != jsonArray {
			return nil, faultInvalid
		}
		for _, entry := range requests.items {
			request, fault := readQuantize(entry)
			if fault != 0 {
				return nil, fault
			}
			for _, seen := range p.quantize {
				if seen.Column == request.Column {
					return nil, faultQuantizeDuplicate
				}
			}
			p.quantize = append(p.quantize, request)
		}
	}
	if value, ok := m.take("extensions"); ok {
		if fault := p.readExtensions(value); fault != 0 {
			return nil, fault
		}
	}
	if fault := m.finish(); fault != 0 {
		return nil, fault
	}
	return p, 0
}

func readQuantize(entry jsonValue) (Quantize, docFault) {
	m, fault := membersOf(entry)
	if fault != 0 {
		return Quantize{}, fault
	}
	column, fault := m.string("column")
	if fault != 0 {
		return Quantize{}, fault
	}
	scale, fault := m.small("scale")
	if fault != 0 {
		return Quantize{}, fault
	}
	if scale > MaxDigits {
		return Quantize{}, faultInvalid
	}
	name, fault := m.string("rounding")
	if fault != 0 {
		return Quantize{}, fault
	}
	rounding, known := roundingByName(name)
	if !known {
		return Quantize{}, faultInvalid
	}
	if fault := m.finish(); fault != 0 {
		return Quantize{}, fault
	}
	return Quantize{Column: column, Scale: scale, Rounding: rounding}, 0
}

// isNamespace reports whether name matches [a-z][a-z0-9]*(-[a-z0-9]+)* in
// at most MaxNamespaceLength characters.
func isNamespace(name string) bool {
	if len(name) > MaxNamespaceLength || name == "" || name[0] < 'a' || name[0] > 'z' {
		return false
	}
	for _, group := range strings.Split(name, "-") {
		if group == "" {
			return false
		}
		for i := 0; i < len(group); i++ {
			if b := group[i]; (b < 'a' || b > 'z') && (b < '0' || b > '9') {
				return false
			}
		}
	}
	return true
}

// tally is what the bounds on extensions measure.
type tally struct {
	values      int
	longString  bool
	largeNumber bool
}

func (t *tally) string(text string) {
	// A string of at most the bound in bytes is within it in code points.
	if len(text) > MaxExtensionString && utf8.RuneCountInString(text) > MaxExtensionString {
		t.longString = true
	}
}

// walk counts the values held by value and inspects its strings and
// numbers. The reader has already bounded the nesting.
func (t *tally) walk(value jsonValue) {
	switch value.kind {
	case jsonString:
		t.string(value.text)
	case jsonInteger:
		t.largeNumber = t.largeNumber || value.integer > MaxCount
	case jsonBigInteger:
		t.largeNumber = true
	case jsonArray:
		for _, item := range value.items {
			t.values++
			t.walk(item)
		}
	case jsonObject:
		for _, member := range value.members {
			t.values++
			t.string(member.key)
			t.walk(member.value)
		}
	}
}

// readExtensions reads the extensions member: the bounds every reader
// applies, then the namespaces, then the content of this package's
// namespace.
func (p *Policy) readExtensions(value jsonValue) docFault {
	if value.kind != jsonObject {
		return faultNotAnObject
	}
	namespaces := value.members
	// The bounds and the shape, without knowing any namespace: the number
	// of namespaces; then each namespace in document order, its name before
	// its value; then the strings, numbers and number of values.
	if len(namespaces) > MaxExtensionNamespaces {
		return faultInvalid
	}
	for _, namespace := range namespaces {
		if !isNamespace(namespace.key) {
			return faultInvalid
		}
		if namespace.value.kind != jsonObject {
			return faultNotAnObject
		}
	}
	var t tally
	for _, namespace := range namespaces {
		t.values++
		t.walk(namespace.value)
	}
	if t.values > MaxExtensionValues || t.longString || t.largeNumber {
		return faultInvalid
	}
	// The namespaces. One that is not implemented refuses the policy.
	for _, namespace := range namespaces {
		if namespace.key != ExtensionNamespace {
			return faultExtensionUnsupported
		}
	}
	if len(namespaces) == 0 {
		return 0
	}
	m, fault := membersOf(namespaces[0].value)
	if fault != 0 {
		return fault
	}
	if sheet, ok := m.take("sheet"); ok {
		if sheet.kind != jsonString || sheet.text == "" {
			return faultInvalid
		}
		p.sheet, p.hasSheet = sheet.text, true
	}
	if limits, ok := m.take("limits"); ok {
		lm, fault := membersOf(limits)
		if fault != 0 {
			return fault
		}
		for _, limit := range []struct {
			name string
			into *Limit
		}{
			{"max_rows", &p.limits.MaxRows},
			{"max_line_bytes", &p.limits.MaxLineBytes},
			{"max_empty_ranges", &p.limits.MaxEmptyRanges},
		} {
			value, fault := lm.optionalCount(limit.name)
			if fault != 0 {
				return fault
			}
			*limit.into = value
		}
		if fault := lm.finish(); fault != 0 {
			return fault
		}
	}
	return m.finish()
}

// Plan is a policy checked against a schema: for each column, the decimal
// type it is quantized to and the rounding mode, if it is quantized. A
// RowReader reads rows under a plan.
type Plan struct {
	schema  *Schema
	policy  *Policy
	targets []target
}

type target struct {
	set      bool
	decimal  DecimalType
	rounding RoundingMode
}

// The refusals of a policy against a schema.
var (
	errQuantizeUnknownColumn = refuse(CategorySchemaMismatch, "quantize-unknown-column")
	errQuantizeNotDecimal    = refuse(CategorySchemaMismatch, "quantize-not-decimal")
	errQuantizeScaleTooLarge = refuse(CategorySchemaMismatch, "quantize-scale-too-large")
)

// NewPlan checks the quantize requests of policy against schema. A nil
// policy quantizes nothing. The target type of a quantized column keeps the
// column's declared precision.
//
// It refuses a request that names an unknown column
// (quantize-unknown-column), a column that is not a decimal
// (quantize-not-decimal), or a target scale above the column's declared
// precision (quantize-scale-too-large), judging the requests in the order
// written.
func NewPlan(schema *Schema, policy *Policy) (*Plan, error) {
	plan := &Plan{schema: schema, policy: policy, targets: make([]target, len(schema.columns))}
	if policy == nil {
		return plan, nil
	}
	for _, request := range policy.quantize {
		at, ok := schema.Index(request.Column)
		if !ok {
			return nil, errQuantizeUnknownColumn.fresh()
		}
		declared, isDecimal := schema.columns[at].Type.Decimal()
		if !isDecimal {
			return nil, errQuantizeNotDecimal.fresh()
		}
		decimal, err := NewDecimalType(declared.Precision(), request.Scale)
		if err != nil {
			return nil, errQuantizeScaleTooLarge.fresh()
		}
		plan.targets[at] = target{set: true, decimal: decimal, rounding: request.Rounding}
	}
	return plan, nil
}

// Schema is the schema of the plan.
func (p *Plan) Schema() *Schema { return p.schema }

// Policy is the policy the plan was built with; it is nil when none was
// given.
func (p *Plan) Policy() *Policy { return p.policy }

// Target is the decimal type the column at a zero-based position is
// quantized to and the rounding mode, and whether the column is quantized.
func (p *Plan) Target(column int) (DecimalType, RoundingMode, bool) {
	t := p.targets[column]
	return t.decimal, t.rounding, t.set
}
