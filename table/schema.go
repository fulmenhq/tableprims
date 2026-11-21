// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import "strconv"

// Kind is a logical type of the standard's closed list.
type Kind uint8

// The logical types.
const (
	// KindString is Unicode text, carried unchanged.
	KindString Kind = iota + 1
	// KindBoolean is true or false.
	KindBoolean
	// KindInteger is an exact integer.
	KindInteger
	// KindDecimal is an exact decimal with a declared precision and scale.
	KindDecimal
	// KindDate is a calendar date.
	KindDate
	// KindLocalTimestamp is a date and time of day with no offset, at a
	// declared fractional precision.
	KindLocalTimestamp
	// KindOffsetTimestamp is a date and time of day with a fixed offset from
	// UTC, at a declared fractional precision.
	KindOffsetTimestamp
)

var kindNames = [...]string{
	KindString:          "string",
	KindBoolean:         "boolean",
	KindInteger:         "integer",
	KindDecimal:         "decimal",
	KindDate:            "date",
	KindLocalTimestamp:  "local-timestamp",
	KindOffsetTimestamp: "offset-timestamp",
}

// String is the name of the type in a schema document, such as "decimal".
func (k Kind) String() string {
	if k >= KindString && int(k) < len(kindNames) {
		return kindNames[k]
	}
	return "kind(" + strconv.Itoa(int(k)) + ")"
}

func kindByName(name string) (Kind, bool) {
	for kind := KindString; int(kind) < len(kindNames); kind++ {
		if kindNames[kind] == name {
			return kind, true
		}
	}
	return 0, false
}

// Type is the logical type of a column with its parameters. Build one with
// StringType, BooleanType, IntegerType, DecimalOf, DateType,
// LocalTimestampOf or OffsetTimestampOf. The zero value is not a type.
type Type struct {
	kind     Kind
	decimal  DecimalType
	fraction FractionPrecision
}

// StringType is the string type.
func StringType() Type { return Type{kind: KindString} }

// BooleanType is the boolean type.
func BooleanType() Type { return Type{kind: KindBoolean} }

// IntegerType is the integer type.
func IntegerType() Type { return Type{kind: KindInteger} }

// DecimalOf is the decimal type with the given precision and scale.
func DecimalOf(t DecimalType) Type { return Type{kind: KindDecimal, decimal: t} }

// DateType is the date type.
func DateType() Type { return Type{kind: KindDate} }

// LocalTimestampOf is the local timestamp type at the given precision.
func LocalTimestampOf(p FractionPrecision) Type {
	return Type{kind: KindLocalTimestamp, fraction: p}
}

// OffsetTimestampOf is the offset timestamp type at the given precision.
func OffsetTimestampOf(p FractionPrecision) Type {
	return Type{kind: KindOffsetTimestamp, fraction: p}
}

// Kind is the logical type.
func (t Type) Kind() Kind { return t.kind }

// Decimal is the precision and scale of a decimal type, and whether the
// type is a decimal.
func (t Type) Decimal() (DecimalType, bool) { return t.decimal, t.kind == KindDecimal }

// Fraction is the fractional precision of a timestamp type, and whether the
// type is a timestamp.
func (t Type) Fraction() (FractionPrecision, bool) {
	return t.fraction, t.kind == KindLocalTimestamp || t.kind == KindOffsetTimestamp
}

// String names the type with its parameters, such as "decimal(12,4)".
func (t Type) String() string {
	switch t.kind {
	case KindDecimal:
		return "decimal(" + strconv.Itoa(t.decimal.Precision()) + "," +
			strconv.Itoa(t.decimal.Scale()) + ")"
	case KindLocalTimestamp, KindOffsetTimestamp:
		return t.kind.String() + "(" + strconv.Itoa(t.fraction.Digits()) + ")"
	}
	return t.kind.String()
}

// Column is one column of a schema.
type Column struct {
	// ID identifies the column; it is unique and not empty within a schema.
	ID string
	// Label is display text. Labels may repeat within a schema.
	Label string
	// Type is the declared logical type.
	Type Type
	// Nullable says whether null is an accepted value.
	Nullable bool
}

// Schema is an ordered list of columns with unique identifiers. It is not
// changed once built.
type Schema struct {
	columns []Column
}

var (
	errColumnsInvalid  = refuse(CategoryInvalidInput, "schema-columns-invalid")
	errTypeUnsupported = refuse(CategoryUnsupportedType, "schema-type-unsupported")
)

// NewSchema creates a schema from a copy of columns.
//
// It refuses an empty column list, an empty identifier and a repeated
// identifier with the rule schema-columns-invalid, a type that is not one
// of the logical types with schema-type-unsupported, and a decimal type
// built without NewDecimalType with schema-member-invalid.
func NewSchema(columns []Column) (*Schema, error) {
	if len(columns) == 0 {
		return nil, errColumnsInvalid.fresh()
	}
	seen := make(map[string]struct{}, len(columns))
	for _, column := range columns {
		if column.ID == "" {
			return nil, errColumnsInvalid.fresh()
		}
		if _, repeated := seen[column.ID]; repeated {
			return nil, errColumnsInvalid.fresh()
		}
		seen[column.ID] = struct{}{}
		switch kind := column.Type.kind; {
		case kind < KindString || int(kind) >= len(kindNames):
			return nil, errTypeUnsupported.fresh()
		case kind == KindDecimal && !column.Type.decimal.valid():
			return nil, errTypeInvalid.fresh()
		}
	}
	return &Schema{columns: append([]Column(nil), columns...)}, nil
}

// Columns returns a copy of the columns in order.
func (s *Schema) Columns() []Column { return append([]Column(nil), s.columns...) }

// Len is the number of columns.
func (s *Schema) Len() int { return len(s.columns) }

// Column returns the column at a zero-based position.
func (s *Schema) Column(i int) Column { return s.columns[i] }

// Index returns the position of the column with the given identifier, and
// whether there is one.
func (s *Schema) Index(id string) (int, bool) {
	for i, column := range s.columns {
		if column.ID == id {
			return i, true
		}
	}
	return 0, false
}

// Value is a typed cell value: Null, Boolean, Text, Integer, Decimal, Date,
// LocalTimestamp or OffsetTimestamp. Use a type switch to read one.
type Value interface {
	// String is the canonical text of the value. Null is "null".
	String() string
	isValue()
}

// Null is the null value. It is distinct from an empty string.
type Null struct{}

// Boolean is a boolean value.
type Boolean bool

// Text is a string value, carried unchanged.
type Text string

// String is "null".
func (Null) String() string { return "null" }

// String is "true" or "false".
func (b Boolean) String() string { return strconv.FormatBool(bool(b)) }

// String is the text itself.
func (t Text) String() string { return string(t) }

func (Null) isValue()            {}
func (Boolean) isValue()         {}
func (Text) isValue()            {}
func (Integer) isValue()         {}
func (Decimal) isValue()         {}
func (Date) isValue()            {}
func (LocalTimestamp) isValue()  {}
func (OffsetTimestamp) isValue() {}

// scalarKind is the kind of an untyped scalar as it arrives from a row.
type scalarKind uint8

const (
	scalarNull scalarKind = iota
	scalarBoolean
	scalarText
)

// scalar is an untyped cell. Exact numbers arrive as text; there is no
// numeric scalar.
type scalar struct {
	kind    scalarKind
	boolean bool
	text    string
}

// The refusals of a row against the schema.
var (
	errArityMismatch   = refuse(CategorySchemaMismatch, "arity-mismatch")
	errNullNotAllowed  = refuse(CategorySchemaMismatch, "null-not-allowed")
	errWrongScalarKind = refuse(CategorySchemaMismatch, "wrong-scalar-kind")
)

// checkRow checks one row against the schema and returns its typed values.
//
// Row length is checked before any cell: a short row names its first
// missing column and a long row names none. Otherwise the refusal names the
// first offending column in schema order.
func (s *Schema) checkRow(row uint64, cells []scalar) ([]Value, *Refusal) {
	if len(cells) != len(s.columns) {
		if len(cells) < len(s.columns) {
			return nil, errArityMismatch.atCell(row, s.columns[len(cells)].ID)
		}
		return nil, errArityMismatch.atRow(row)
	}
	values := make([]Value, len(cells))
	for i, column := range s.columns {
		value, err := checkCell(column, cells[i])
		if err != nil {
			return nil, err.atCell(row, column.ID)
		}
		values[i] = value
	}
	return values, nil
}

func checkCell(column Column, cell scalar) (Value, *Refusal) {
	switch {
	case cell.kind == scalarNull:
		if column.Nullable {
			return Null{}, nil
		}
		return nil, errNullNotAllowed
	case column.Type.kind == KindBoolean:
		if cell.kind == scalarBoolean {
			return Boolean(cell.boolean), nil
		}
		return nil, errWrongScalarKind
	case cell.kind != scalarText:
		return nil, errWrongScalarKind
	}
	text := cell.text
	switch column.Type.kind {
	case KindString:
		return Text(text), nil
	case KindInteger:
		l, err := lex(text)
		if err != nil {
			return nil, err
		}
		if l.hasPoint {
			return nil, errExactPointMismatch
		}
		return Integer{negative: l.negative, magnitude: l.magnitude}, nil
	case KindDecimal:
		value, err := ParseDecimal(text, column.Type.decimal)
		if err != nil {
			return nil, err.(*Refusal)
		}
		return value, nil
	case KindDate:
		value, err := parseDate(text)
		if err != nil {
			return nil, err
		}
		return value, nil
	case KindLocalTimestamp:
		value, err := parseLocalTimestamp(text, column.Type.fraction)
		if err != nil {
			return nil, err
		}
		return value, nil
	case KindOffsetTimestamp:
		value, err := parseOffsetTimestamp(text, column.Type.fraction)
		if err != nil {
			return nil, err
		}
		return value, nil
	}
	// NewSchema refuses every other type.
	return nil, errTypeUnsupported
}
