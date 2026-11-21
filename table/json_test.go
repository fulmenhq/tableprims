// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import (
	"strconv"
	"strings"
	"testing"
)

func parse8(text string) (jsonValue, jsonError) { return parseJSON(text, 8) }

func items(v jsonValue) []jsonValue {
	var out []jsonValue
	for item := range v.items {
		out = append(out, item)
	}
	return out
}

func membersList(v jsonValue) []jsonMember {
	var out []jsonMember
	for key, member := range v.members {
		out = append(out, jsonMember{key: key, value: member})
	}
	return out
}

func TestJSONValuesAreRead(t *testing.T) {
	cases := []struct {
		text string
		kind jsonKind
	}{
		{"null", jsonNull},
		{" true\n", jsonBoolean},
		{"false", jsonBoolean},
		{"0", jsonInteger},
		{"18446744073709551615", jsonInteger},
		{`"aé"`, jsonString},
		{"[]", jsonArray},
		{"{}", jsonObject},
	}
	for _, c := range cases {
		value, err := parse8(c.text)
		if err != 0 || value.kind != c.kind {
			t.Errorf("%q: kind %v, error %v", c.text, value.kind, err)
		}
	}
	if value, _ := parse8("18446744073709551615"); value.integer != ^uint64(0) {
		t.Errorf("largest uint64 read as %d", value.integer)
	}
	value, err := parse8("\r\n{ \"a\" : [ 1 , null ] ,\t\"b\" : { \"c\" : \"d\" } }\r\n")
	top := membersList(value)
	if err != 0 || len(top) != 2 || top[0].key != "a" || len(items(top[0].value)) != 2 ||
		membersList(top[1].value)[0].value.text != "d" || items(top[0].value)[0].integer != 1 {
		t.Errorf("nested document read as %+v, %v", value, err)
	}
}

func TestJSONMemberOrderIsKept(t *testing.T) {
	value, err := parse8(`{"b": 1, "a": 2}`)
	if m := membersList(value); err != 0 || m[0].key != "b" || m[1].key != "a" {
		t.Errorf("order not kept: %+v", m)
	}
}

func TestJSONDuplicateKeysAreRefusedAtEveryDepth(t *testing.T) {
	for _, text := range []string{
		`{"a": 1, "a": 2}`,
		`{"a": 1, "b": {"c": 1, "c": 1}}`,
		`[{"a": null, "a": null}]`,
		`{"x": [[{"k": 1, "k": 1}]]}`,
		// The same key spelled with and without an escape.
		`{"a": 1, "\u0061": 2}`,
		`{"é": 1, "é": 2}`,
		`{"": 1, "": 2}`,
	} {
		if _, err := parse8(text); err != jsonDuplicateKey {
			t.Errorf("%s: %v", text, err)
		}
	}
	// Keys that differ only by case or normalization form are distinct.
	for _, text := range []string{`{"a": 1, "A": 2}`, "{\"\u00e9\": 1, \"e\u0301\": 2}"} {
		if _, err := parse8(text); err != 0 {
			t.Errorf("%s: %v", text, err)
		}
	}
}

func TestJSONDuplicateDetectionScales(t *testing.T) {
	var b strings.Builder
	b.WriteString("{")
	for i := range 50_000 {
		b.WriteString(`"k` + strconv.Itoa(i) + `":null,`)
	}
	prefix := b.String()
	value, err := parse8(prefix + `"last":null}`)
	if m := membersList(value); err != 0 || len(m) != 50_001 || m[0].key != "k0" || m[50_000].key != "last" {
		t.Fatalf("unique keys: %v, %d members", err, len(m))
	}
	if _, err := parse8(prefix + `"k0":null}`); err != jsonDuplicateKey {
		t.Errorf("repeated key: %v", err)
	}
}

func TestJSONNumbers(t *testing.T) {
	for _, text := range []string{
		"-1", "-0", "1.5", "1.0", "1e3", "1E3", "1e+3", "0.5e-2",
		"-18446744073709551616", "18446744073709551616.0", "18446744073709551616e0", "[1, -2]",
	} {
		if _, err := parse8(text); err != jsonUnsupportedNumber {
			t.Errorf("%s: %v, want unsupported number", text, err)
		}
	}
	for _, text := range []string{
		"18446744073709551616",
		"184467440737095516150",
		"99999999999999999999999999999999999999999999999999",
	} {
		if value, err := parse8(text); err != 0 || value.kind != jsonBigInteger {
			t.Errorf("%s: %v %v, want a big integer", text, value.kind, err)
		}
	}
	value, err := parse8(`{"a": [18446744073709551616]}`)
	if err != 0 || items(membersList(value)[0].value)[0].kind != jsonBigInteger {
		t.Errorf("nested big integer: %v", err)
	}
	for _, text := range []string{
		"018446744073709551616", "01", "-", "1.", "1e", "1e+", "-a", "00", "-01",
		"+1", ".5", "NaN", "Infinity",
	} {
		if _, err := parse8(text); err != jsonMalformed {
			t.Errorf("%s: %v, want malformed", text, err)
		}
	}
	for _, text := range []string{"1 2", "1x", "0x10"} {
		if _, err := parse8(text); err != jsonTrailingContent {
			t.Errorf("%s: %v, want trailing content", text, err)
		}
	}
}

func TestNumberTokensFollowTheJSONGrammar(t *testing.T) {
	for _, token := range []string{
		"0", "-0", "1", "-1", "10", "1.5", "0.0", "-0.5", "1e3", "1E3", "1e+3", "1e-3",
		"1.5e10", "0e0", "123456789012345678901234567890",
	} {
		if !isNumber(token) {
			t.Errorf("%q is a number", token)
		}
	}
	for _, token := range []string{
		"", "-", "01", "-01", "00", "1.", "1.e3", ".5", "1e", "1e+", "1e-", "1x", "1.5x",
		"1e3x", "--1", "+1", "1-", "0x10", "1 ", "1.2.3", "1e3e4", "-a", "١",
	} {
		if isNumber(token) {
			t.Errorf("%q is not a number", token)
		}
	}
}

func TestJSONMalformedDocumentsAreRefused(t *testing.T) {
	for _, text := range []string{
		"{", "[", "[1,", "[1,]", "[,1]", "[1 2]", `{"a"}`, `{"a":}`, `{"a":1,}`, "{,}",
		"{a: 1}", "{'a': 1}", `{"a" 1}`, "{1: 2}", "nul", "tru", "True", `"abc`, `"abc\`,
		"\ufeff{}", "\u00a01",
	} {
		if _, err := parse8(text); err != jsonMalformed {
			t.Errorf("%q: %v, want malformed", text, err)
		}
	}
	for _, text := range []string{"", " ", "\n\t\r "} {
		if _, err := parse8(text); err != jsonEmpty {
			t.Errorf("%q: %v, want empty", text, err)
		}
	}
	for _, text := range []string{"{} {}", "[] x", "null,", "{}}", `"a""b"`} {
		if _, err := parse8(text); err != jsonTrailingContent {
			t.Errorf("%q: %v, want trailing content", text, err)
		}
	}
}

func TestJSONBadStringsAreRefused(t *testing.T) {
	for _, text := range []string{
		`"\x41"`, `"\u12"`, `"\ud83d"`, `"\ude00"`, "\"a\tb\"", "\"a\nb\"", `{"\ud83d": 1}`,
	} {
		if _, err := parse8(text); err != jsonInvalidString {
			t.Errorf("%q: %v, want invalid string", text, err)
		}
	}
	value, err := parse8(`"café 😀 \"q\" \\ \/ \b\f\n\r\t"`)
	if err != 0 || value.text != "café 😀 \"q\" \\ / \b\f\n\r\t" {
		t.Errorf("escapes decoded as %q, %v", value.text, err)
	}
}

func TestJSONDepthIsBounded(t *testing.T) {
	if _, err := parseJSON("1", 0); err != 0 {
		t.Error("a scalar has depth zero")
	}
	for _, c := range []struct {
		text  string
		depth int
		want  jsonError
	}{
		{"[]", 0, jsonTooDeep},
		{"{}", 0, jsonTooDeep},
		{"[[1]]", 2, 0},
		{"[[[1]]]", 2, jsonTooDeep},
		{`{"a": {"b": {"c": 1}}}`, 2, jsonTooDeep},
		{"[[1], [2], [3]]", 2, 0},
		{strings.Repeat("[", 100_000), 64, jsonTooDeep},
	} {
		if _, err := parseJSON(c.text, c.depth); err != c.want {
			t.Errorf("%.20q at %d: %v, want %v", c.text, c.depth, err, c.want)
		}
	}
	nested := func(depth int) string { return strings.Repeat("[", depth) + strings.Repeat("]", depth) }
	if _, err := parseJSON(nested(maxNesting), 1<<30); err != 0 {
		t.Errorf("at the ceiling: %v", err)
	}
	if _, err := parseJSON(nested(maxNesting+1), 1<<30); err != jsonTooDeep {
		t.Errorf("past the ceiling: %v", err)
	}
	objects := strings.Repeat(`{"a":`, 200_000) + "1" + strings.Repeat("}", 200_000)
	if _, err := parseJSON(objects, 1<<30); err != jsonTooDeep {
		t.Errorf("deep objects: %v", err)
	}
}
