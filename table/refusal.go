// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"strconv"
	"strings"
)

// Categories of the standard that this package reports.
const (
	// CategoryInvalidInput is malformed bytes, serialization or scalar
	// spelling.
	CategoryInvalidInput = "invalid-input"
	// CategorySchemaMismatch is a well-formed value or policy that does not
	// fit the schema.
	CategorySchemaMismatch = "schema-mismatch"
	// CategoryUnsupportedType is a logical type this reader does not know.
	CategoryUnsupportedType = "unsupported-type"
	// CategoryResourceLimit is a bound or a configured limit that was
	// exceeded.
	CategoryResourceLimit = "resource-limit"
	// CategoryIOFailure is a failure of the underlying reader.
	CategoryIOFailure = "io-failure"
)

// Refusal is the error this package returns. It names the category and the
// rule identifier, spelled as the standard registers them, and locates the
// refusal at a zero-based row and a column identifier when it has them.
//
// The message never contains document or cell content. It names the column
// identifier, which locates the refusal. For an io-failure it ends with the
// error text of the underlying reader, which may name a file.
type Refusal struct {
	category  string
	rule      string
	row       uint64
	hasRow    bool
	column    string
	hasColumn bool
	err       error
}

func refuse(category, rule string) *Refusal {
	return &Refusal{category: category, rule: rule}
}

// atRow returns a copy of r located at row.
func (r *Refusal) atRow(row uint64) *Refusal {
	located := *r
	located.row, located.hasRow = row, true
	return &located
}

// atCell returns a copy of r located at row and column.
func (r *Refusal) atCell(row uint64, column string) *Refusal {
	located := r.atRow(row)
	located.column, located.hasColumn = column, true
	return located
}

// Category is the class of the refusal, such as "invalid-input".
func (r *Refusal) Category() string { return r.category }

// Rule is the stable identifier of the rule that refused, such as
// "exact-leading-zero".
func (r *Refusal) Rule() string { return r.rule }

// Row is the zero-based index of the row, and whether the refusal has one.
func (r *Refusal) Row() (uint64, bool) { return r.row, r.hasRow }

// Column is the identifier of the column, and whether the refusal has one.
// A row with more cells than the schema has columns has none.
func (r *Refusal) Column() (string, bool) { return r.column, r.hasColumn }

// Unwrap returns the error of the underlying reader for an io-failure, and
// nil otherwise.
func (r *Refusal) Unwrap() error { return r.err }

// Error names the category, the rule and the location.
func (r *Refusal) Error() string {
	var b strings.Builder
	b.WriteString(r.category)
	b.WriteString(": ")
	b.WriteString(r.rule)
	if r.hasRow {
		b.WriteString(" at row ")
		b.WriteString(strconv.FormatUint(r.row, 10))
	}
	if r.hasColumn {
		b.WriteString(" in column ")
		b.WriteString(strconv.Quote(r.column))
	}
	if r.err != nil {
		b.WriteString(": ")
		b.WriteString(r.err.Error())
	}
	return b.String()
}
