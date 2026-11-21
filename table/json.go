// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import "unicode/utf8"

// The strict JSON reader for the documents, and the token readers it shares
// with the row reader.
//
// parseJSON reads one JSON value and refuses what a lenient reader would
// resolve silently: a repeated object key (compared after string decoding),
// an unpaired surrogate, and any number other than a non-negative integer
// written without sign, fraction, exponent or leading zero. No number is
// read through binary floating point. An integer above the largest uint64 is
// well formed and is read as jsonBigInteger, so that a caller can refuse it
// as out of range rather than as malformed. The text must be valid UTF-8;
// the callers check it first.

// maxNesting is the deepest nesting of arrays and objects the reader accepts
// whatever the caller allows; it bounds the recursion.
const maxNesting = 64

type jsonKind uint8

const (
	jsonNull jsonKind = iota
	jsonBoolean
	jsonInteger
	jsonBigInteger
	jsonString
	jsonArray
	jsonObject
)

// jsonValue is a JSON value. Only the fields of its kind are set.
type jsonValue struct {
	kind    jsonKind
	boolean bool
	integer uint64
	text    string
	items   []jsonValue
	members []jsonMember
}

// jsonMember is one member of an object, in the order written.
type jsonMember struct {
	key   string
	value jsonValue
}

// jsonError is why a document was refused.
type jsonError uint8

const (
	jsonEmpty jsonError = iota + 1
	jsonMalformed
	jsonInvalidString
	jsonDuplicateKey
	jsonUnsupportedNumber
	jsonTooDeep
	jsonTrailingContent
)

// stringError is why a string body could not be read.
type stringError uint8

const (
	stringOK stringError = iota
	// stringUnterminated: the text ended inside the string.
	stringUnterminated
	// stringInvalid: an invalid escape, an unpaired surrogate or an
	// unescaped control character.
	stringInvalid
)

// cursor is a position in JSON text.
type cursor struct {
	text string
	at   int
}

// end is what peek returns at the end of the text.
const end = -1

func (c *cursor) peek() int {
	if c.at < len(c.text) {
		return int(c.text[c.at])
	}
	return end
}

func (c *cursor) advance() { c.at++ }

// rest is the text from the current position to the end.
func (c *cursor) rest() string { return c.text[c.at:] }

// skip skips bytes for which isSpace holds.
func (c *cursor) skip(isSpace func(byte) bool) {
	for c.at < len(c.text) && isSpace(c.text[c.at]) {
		c.at++
	}
}

// take consumes word if the text continues with it.
func (c *cursor) take(word string) bool {
	if len(c.text)-c.at >= len(word) && c.text[c.at:c.at+len(word)] == word {
		c.at += len(word)
		return true
	}
	return false
}

// str reads a string body; the opening quote is already consumed.
func (c *cursor) str() (string, stringError) {
	start := c.at
	var out []byte // nil until the first escape
	for {
		run := c.at
		// Copy the run up to the next quote, backslash or control byte.
		// These are ASCII, so the run ends on a character boundary.
		for c.at < len(c.text) {
			b := c.text[c.at]
			if b == '"' || b == '\\' || b < 0x20 {
				break
			}
			c.at++
		}
		if out != nil {
			out = append(out, c.text[run:c.at]...)
		}
		switch c.peek() {
		case '"':
			c.at++
			if out == nil {
				return c.text[start : c.at-1], stringOK
			}
			return string(out), stringOK
		case '\\':
			if out == nil {
				out = append([]byte(nil), c.text[start:c.at]...)
			}
			c.at++
			r, err := c.escape()
			if err != stringOK {
				return "", err
			}
			out = utf8.AppendRune(out, r)
		case end:
			return "", stringUnterminated
		default:
			return "", stringInvalid
		}
	}
}

// escape reads one escape; the backslash is already consumed.
func (c *cursor) escape() (rune, stringError) {
	b := c.peek()
	if b == end {
		return 0, stringUnterminated
	}
	c.at++
	switch b {
	case '"':
		return '"', stringOK
	case '\\':
		return '\\', stringOK
	case '/':
		return '/', stringOK
	case 'b':
		return '\b', stringOK
	case 'f':
		return '\f', stringOK
	case 'n':
		return '\n', stringOK
	case 'r':
		return '\r', stringOK
	case 't':
		return '\t', stringOK
	case 'u':
		return c.unicodeEscape()
	}
	return 0, stringInvalid
}

// hex4 reads four hexadecimal digits.
func (c *cursor) hex4() (rune, stringError) {
	if len(c.text)-c.at < 4 {
		return 0, stringInvalid
	}
	var value rune
	for _, b := range []byte(c.text[c.at : c.at+4]) {
		var digit byte
		switch {
		case b >= '0' && b <= '9':
			digit = b - '0'
		case b >= 'a' && b <= 'f':
			digit = b - 'a' + 10
		case b >= 'A' && b <= 'F':
			digit = b - 'A' + 10
		default:
			return 0, stringInvalid
		}
		value = value*16 + rune(digit)
	}
	c.at += 4
	return value, stringOK
}

// unicodeEscape reads the digits of a \u escape, pairing surrogates.
func (c *cursor) unicodeEscape() (rune, stringError) {
	first, err := c.hex4()
	if err != stringOK {
		return 0, err
	}
	switch {
	case first >= 0xD800 && first <= 0xDBFF:
		if !c.take(`\u`) {
			return 0, stringInvalid
		}
		second, err := c.hex4()
		if err != stringOK {
			return 0, err
		}
		if second < 0xDC00 || second > 0xDFFF {
			return 0, stringInvalid
		}
		return 0x10000 + (first-0xD800)<<10 + (second - 0xDC00), stringOK
	case first >= 0xDC00 && first <= 0xDFFF:
		return 0, stringInvalid
	}
	return first, stringOK
}

func isJSONSpace(b byte) bool {
	return b == ' ' || b == '\t' || b == '\n' || b == '\r'
}

func stringFault(err stringError) jsonError {
	if err == stringUnterminated {
		return jsonMalformed
	}
	return jsonInvalidString
}

// parseJSON parses one JSON document whose arrays and objects nest at most
// maxDepth deep; a scalar document has depth zero. A limit above maxNesting
// is treated as maxNesting.
func parseJSON(text string, maxDepth int) (jsonValue, jsonError) {
	c := &cursor{text: text}
	c.skip(isJSONSpace)
	if c.peek() == end {
		return jsonValue{}, jsonEmpty
	}
	value, err := parseValue(c, min(maxDepth, maxNesting))
	if err != 0 {
		return jsonValue{}, err
	}
	c.skip(isJSONSpace)
	if c.peek() != end {
		return jsonValue{}, jsonTrailingContent
	}
	return value, 0
}

func parseValue(c *cursor, depthLeft int) (jsonValue, jsonError) {
	switch b := c.peek(); {
	case b == '"':
		c.advance()
		text, err := c.str()
		if err != stringOK {
			return jsonValue{}, stringFault(err)
		}
		return jsonValue{kind: jsonString, text: text}, 0
	case b == 'n' && c.take("null"):
		return jsonValue{kind: jsonNull}, 0
	case b == 't' && c.take("true"):
		return jsonValue{kind: jsonBoolean, boolean: true}, 0
	case b == 'f' && c.take("false"):
		return jsonValue{kind: jsonBoolean}, 0
	case b == '-' || (b >= '0' && b <= '9'):
		return parseNumber(c)
	case b == '[':
		if depthLeft <= 0 {
			return jsonValue{}, jsonTooDeep
		}
		c.advance()
		return parseArray(c, depthLeft-1)
	case b == '{':
		if depthLeft <= 0 {
			return jsonValue{}, jsonTooDeep
		}
		c.advance()
		return parseObject(c, depthLeft-1)
	}
	return jsonValue{}, jsonMalformed
}

func isDigit(b int) bool { return b >= '0' && b <= '9' }

// parseNumber reads 0|[1-9][0-9]* as a uint64, or as jsonBigInteger when it
// does not fit one. Any other well-formed JSON number is unsupported;
// anything else is malformed.
func parseNumber(c *cursor) (jsonValue, jsonError) {
	negative := c.peek() == '-'
	if negative {
		c.advance()
	}
	var value uint64
	overflow := false
	digits := 0
	leadingZero := false
	for isDigit(c.peek()) {
		if digits == 1 && !overflow && value == 0 {
			leadingZero = true
		}
		digit := uint64(c.peek() - '0')
		if !overflow {
			if value > (^uint64(0)-digit)/10 {
				overflow = true
			} else {
				value = value*10 + digit
			}
		}
		digits++
		c.advance()
	}
	if digits == 0 || leadingZero {
		return jsonValue{}, jsonMalformed
	}
	plain := !negative
	if c.peek() == '.' {
		plain = false
		c.advance()
		if !isDigit(c.peek()) {
			return jsonValue{}, jsonMalformed
		}
		for isDigit(c.peek()) {
			c.advance()
		}
	}
	if b := c.peek(); b == 'e' || b == 'E' {
		plain = false
		c.advance()
		if b := c.peek(); b == '+' || b == '-' {
			c.advance()
		}
		if !isDigit(c.peek()) {
			return jsonValue{}, jsonMalformed
		}
		for isDigit(c.peek()) {
			c.advance()
		}
	}
	switch {
	case !plain:
		return jsonValue{}, jsonUnsupportedNumber
	case overflow:
		return jsonValue{kind: jsonBigInteger}, 0
	}
	return jsonValue{kind: jsonInteger, integer: value}, 0
}

// isNumber reports whether token is a number in the grammar of RFC 8259:
// -?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?.
func isNumber(token string) bool {
	digits := func(s string) int {
		n := 0
		for n < len(s) && s[n] >= '0' && s[n] <= '9' {
			n++
		}
		return n
	}
	rest := token
	if len(rest) > 0 && rest[0] == '-' {
		rest = rest[1:]
	}
	switch run := digits(rest); {
	case run == 0:
		return false
	case run > 1 && rest[0] == '0':
		return false
	default:
		rest = rest[run:]
	}
	if len(rest) > 0 && rest[0] == '.' {
		run := digits(rest[1:])
		if run == 0 {
			return false
		}
		rest = rest[1+run:]
	}
	if len(rest) > 0 && (rest[0] == 'e' || rest[0] == 'E') {
		rest = rest[1:]
		if len(rest) > 0 && (rest[0] == '+' || rest[0] == '-') {
			rest = rest[1:]
		}
		run := digits(rest)
		if run == 0 {
			return false
		}
		rest = rest[run:]
	}
	return rest == ""
}

// parseArray reads array elements; the opening bracket is already consumed.
func parseArray(c *cursor, depthLeft int) (jsonValue, jsonError) {
	items := []jsonValue{}
	c.skip(isJSONSpace)
	if c.peek() == ']' {
		c.advance()
		return jsonValue{kind: jsonArray, items: items}, 0
	}
	for {
		c.skip(isJSONSpace)
		item, err := parseValue(c, depthLeft)
		if err != 0 {
			return jsonValue{}, err
		}
		items = append(items, item)
		c.skip(isJSONSpace)
		switch c.peek() {
		case ',':
			c.advance()
		case ']':
			c.advance()
			return jsonValue{kind: jsonArray, items: items}, 0
		default:
			return jsonValue{}, jsonMalformed
		}
	}
}

// parseObject reads object members; the opening brace is already consumed.
func parseObject(c *cursor, depthLeft int) (jsonValue, jsonError) {
	members := []jsonMember{}
	seen := map[string]struct{}{}
	c.skip(isJSONSpace)
	if c.peek() == '}' {
		c.advance()
		return jsonValue{kind: jsonObject, members: members}, 0
	}
	for {
		c.skip(isJSONSpace)
		if c.peek() != '"' {
			return jsonValue{}, jsonMalformed
		}
		c.advance()
		key, serr := c.str()
		if serr != stringOK {
			return jsonValue{}, stringFault(serr)
		}
		if _, repeated := seen[key]; repeated {
			return jsonValue{}, jsonDuplicateKey
		}
		seen[key] = struct{}{}
		c.skip(isJSONSpace)
		if c.peek() != ':' {
			return jsonValue{}, jsonMalformed
		}
		c.advance()
		c.skip(isJSONSpace)
		member, err := parseValue(c, depthLeft)
		if err != 0 {
			return jsonValue{}, err
		}
		members = append(members, jsonMember{key: key, value: member})
		c.skip(isJSONSpace)
		switch c.peek() {
		case ',':
			c.advance()
		case '}':
			c.advance()
			return jsonValue{kind: jsonObject, members: members}, 0
		default:
			return jsonValue{}, jsonMalformed
		}
	}
}
