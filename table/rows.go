// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"bufio"
	"bytes"
	"errors"
	"io"
	"unicode/utf8"
)

// Rows as newline-delimited JSON arrays.
//
// Each line is one JSON array holding one scalar per column: null, true,
// false or a string. Exact numbers, dates and timestamps travel as strings;
// a bare JSON number is refused, never read through binary floating point.
// Nested arrays and objects are refused.
//
// Input is UTF-8 without a byte order mark. Lines end with LF or CRLF and
// the final line may omit its terminator. A blank line is an error, not a
// skipped row. An empty input holds zero rows. Whitespace inside a line is
// limited to spaces and tabs between tokens.
//
// A cell that begins with - or a digit is judged by its token: the
// characters up to the next space, tab, comma, ] or the end of the line. A
// token that is a JSON number is a numeric-literal: the line is JSON and the
// value must be quoted. Any other token, such as 01, 1. or 1x, is a
// malformed-row: the line is not JSON.

// The refusals of the rows encoding.
var (
	errReadFailed      = refuse(CategoryIOFailure, "read-failed")
	errByteOrderMark   = refuse(CategoryInvalidInput, "byte-order-mark")
	errInvalidUTF8     = refuse(CategoryInvalidInput, "invalid-utf8")
	errLineTooLong     = refuse(CategoryResourceLimit, "line-too-long")
	errBlankLine       = refuse(CategoryInvalidInput, "blank-line")
	errNotAnArray      = refuse(CategoryInvalidInput, "not-an-array")
	errMalformedRow    = refuse(CategoryInvalidInput, "malformed-row")
	errNumericLiteral  = refuse(CategoryInvalidInput, "numeric-literal")
	errNestedValue     = refuse(CategoryInvalidInput, "nested-value")
	errInvalidString   = refuse(CategoryInvalidInput, "invalid-string")
	errTrailingContent = refuse(CategoryInvalidInput, "trailing-content")
)

func isLineSpace(b byte) bool { return b == ' ' || b == '\t' }

// parseLine parses one line, without its terminator, into cells.
func parseLine(line string) ([]scalar, *Refusal) {
	c := &cursor{text: line}
	c.skip(isLineSpace)
	switch c.peek() {
	case end:
		return nil, errBlankLine
	case '[':
		c.advance()
	default:
		return nil, errNotAnArray
	}
	cells := []scalar{}
	c.skip(isLineSpace)
	if c.peek() == ']' {
		c.advance()
	} else {
	elements:
		for {
			c.skip(isLineSpace)
			cell, err := parseCell(c)
			if err != nil {
				return nil, err
			}
			cells = append(cells, cell)
			c.skip(isLineSpace)
			switch c.peek() {
			case ',':
				c.advance()
			case ']':
				c.advance()
				break elements
			default:
				return nil, errMalformedRow
			}
		}
	}
	c.skip(isLineSpace)
	if c.peek() != end {
		return nil, errTrailingContent
	}
	return cells, nil
}

func parseCell(c *cursor) (scalar, *Refusal) {
	literal := func(word string, cell scalar) (scalar, *Refusal) {
		if c.take(word) {
			return cell, nil
		}
		return scalar{}, errMalformedRow
	}
	switch b := c.peek(); {
	case b == '"':
		c.advance()
		text, err := c.str()
		switch err {
		case stringOK:
			return scalar{kind: scalarText, text: text}, nil
		case stringInvalid:
			return scalar{}, errInvalidString
		}
		// The line ended inside the string.
		return scalar{}, errMalformedRow
	case b == 'n':
		return literal("null", scalar{kind: scalarNull})
	case b == 't':
		return literal("true", scalar{kind: scalarBoolean, boolean: true})
	case b == 'f':
		return literal("false", scalar{kind: scalarBoolean})
	case b == '-' || isDigit(b):
		rest := c.rest()
		token := len(rest)
		for i := 0; i < len(rest); i++ {
			if r := rest[i]; r == ' ' || r == '\t' || r == ',' || r == ']' {
				token = i
				break
			}
		}
		if isNumber(rest[:token]) {
			return scalar{}, errNumericLiteral
		}
		return scalar{}, errMalformedRow
	case b == '[' || b == '{':
		return scalar{}, errNestedValue
	}
	return scalar{}, errMalformedRow
}

// Change says what quantization did to one cell.
type Change uint8

// The changes quantization reports.
const (
	// ValueChanged is set when the mathematical value changed.
	ValueChanged Change = 1 << iota
	// ScaleChanged is set when the scale changed, even if the value did not.
	ScaleChanged
)

// Row is one row read and checked under a schema.
type Row struct {
	// Index is the zero-based index of the row in the input.
	Index uint64
	// Values holds one typed value per column, in schema order. A decimal
	// column the plan quantizes holds the quantized value.
	Values []Value
	// Changes holds, per column, what quantization did to the cell. It is
	// nil when the plan quantizes no column, and zero for a column that is
	// not quantized and for a null.
	Changes []Change
}

// RowReader reads rows from newline-delimited JSON, one line at a time, and
// checks each under the schema of a plan.
//
// Memory use is bounded by the line length limit and the buffer of the
// reader. A RowReader belongs to one goroutine.
type RowReader struct {
	input    *bufio.Reader
	plan     *Plan
	maxLine  int
	quantize bool
	next     uint64
	line     []byte
	err      error
}

// NewRowReader returns a reader of the rows in r under plan that refuses
// any line longer than maxLineBytes bytes, not counting its terminator. A
// negative limit is treated as zero.
func NewRowReader(r io.Reader, plan *Plan, maxLineBytes int) *RowReader {
	quantize := false
	for _, target := range plan.targets {
		quantize = quantize || target.set
	}
	return &RowReader{
		input:    bufio.NewReaderSize(r, 64<<10),
		plan:     plan,
		maxLine:  max(maxLineBytes, 0),
		quantize: quantize,
	}
}

// Next returns the next row. At the end of the input it returns io.EOF.
//
// Any other error is a *Refusal: of the line (located at its row), of the
// row against the schema (located at its row and, where there is one, its
// column), of a quantized value (quantize-overflow, at its row and column),
// a byte order mark at the start of the input, or, with the category
// io-failure and the rule read-failed, a failure of r, which Unwrap returns.
// After an error, Next returns the same error again.
func (rr *RowReader) Next() (Row, error) {
	if rr.err != nil {
		return Row{}, rr.err
	}
	row, err := rr.read()
	if err != nil {
		rr.err = err
		return Row{}, err
	}
	return row, nil
}

// readLine reads the next line with its terminator into rr.line, stopping
// once the line is certainly longer than the limit. It reports whether any
// byte was read.
func (rr *RowReader) readLine() (bool, error) {
	rr.line = rr.line[:0]
	// The content, an optional CR, the LF, and one byte to show excess.
	limit := rr.maxLine
	if limit < int(^uint(0)>>1)-3 {
		limit += 3
	}
	for {
		chunk, err := rr.input.ReadSlice('\n')
		rr.line = append(rr.line, chunk...)
		switch {
		case err == nil:
			return true, nil
		case errors.Is(err, bufio.ErrBufferFull):
			if len(rr.line) >= limit {
				return true, nil
			}
		case errors.Is(err, io.EOF):
			return len(rr.line) > 0, nil
		default:
			return false, err
		}
	}
}

func (rr *RowReader) read() (Row, error) {
	index := rr.next
	more, err := rr.readLine()
	if err != nil {
		failure := errReadFailed.fresh()
		failure.err = err
		return Row{}, failure
	}
	if !more {
		return Row{}, io.EOF
	}
	line := rr.line
	if n := len(line); n > 0 && line[n-1] == '\n' {
		line = line[:n-1]
		if n := len(line); n > 0 && line[n-1] == '\r' {
			line = line[:n-1]
		}
	}
	if len(line) > rr.maxLine {
		return Row{}, errLineTooLong.atRow(index)
	}
	if index == 0 && bytes.HasPrefix(line, []byte("\xEF\xBB\xBF")) {
		return Row{}, errByteOrderMark.fresh()
	}
	if !utf8.Valid(line) {
		return Row{}, errInvalidUTF8.atRow(index)
	}
	cells, refusal := parseLine(string(line))
	if refusal != nil {
		return Row{}, refusal.atRow(index)
	}
	values, refusal := rr.plan.schema.checkRow(index, cells)
	if refusal != nil {
		return Row{}, refusal
	}
	row := Row{Index: index, Values: values}
	if rr.quantize {
		row.Changes = make([]Change, len(values))
		for i, target := range rr.plan.targets {
			decimal, ok := values[i].(Decimal)
			if !target.set || !ok {
				continue
			}
			result, err := decimal.Quantize(target.decimal, target.rounding)
			if err != nil {
				return Row{}, errQuantizeOverflow.atCell(index, rr.plan.schema.columns[i].ID)
			}
			values[i] = result.Value
			if result.ValueChanged {
				row.Changes[i] |= ValueChanged
			}
			if result.ScaleChanged {
				row.Changes[i] |= ScaleChanged
			}
		}
	}
	rr.next++
	return row, nil
}
