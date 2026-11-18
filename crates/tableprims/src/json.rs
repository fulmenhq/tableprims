// SPDX-License-Identifier: MIT OR Apache-2.0

//! A strict JSON reader for small documents.
//!
//! [`parse_document`] reads one JSON value and refuses what a lenient reader
//! would resolve silently: a repeated object key (compared after string
//! decoding), an unpaired surrogate, and any number other than a
//! non-negative integer written without sign, fraction, exponent or leading
//! zero. No number is read through binary floating point. Nesting depth is
//! bounded by the caller.
//!
//! The reader is written here rather than taken from a JSON library so that
//! every refusal is explicit.

use core::fmt;

/// A JSON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean(bool),
    /// A non-negative integer.
    Integer(u64),
    /// A string, unescaped.
    String(String),
    /// An array, in order.
    Array(Vec<Json>),
    /// An object: its members in the order written, with distinct keys.
    Object(Vec<(String, Json)>),
}

/// Why a document was refused.
///
/// The messages never include document content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum JsonError {
    /// The text is empty or holds only whitespace.
    Empty,
    /// The text is not well-formed JSON.
    Malformed,
    /// A string holds an invalid escape, an unpaired surrogate or an
    /// unescaped control character.
    InvalidString,
    /// An object repeats a key.
    DuplicateKey,
    /// A number is negative, fractional, written with an exponent or a
    /// leading zero, or too large.
    UnsupportedNumber,
    /// Arrays and objects are nested more deeply than the limit.
    TooDeep,
    /// Something other than whitespace follows the value.
    TrailingContent,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "document is empty",
            Self::Malformed => "document is not well-formed",
            Self::InvalidString => "string is not well-formed",
            Self::DuplicateKey => "object repeats a key",
            Self::UnsupportedNumber => "number is not a plain non-negative integer",
            Self::TooDeep => "document is nested too deeply",
            Self::TrailingContent => "content follows the document",
        })
    }
}

impl std::error::Error for JsonError {}

/// Why a string body could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StringError {
    /// The text ended inside the string.
    Unterminated,
    /// Invalid escape, unpaired surrogate or unescaped control character.
    Invalid,
}

/// A position in JSON text, with the token readers shared by this module
/// and the row reader.
pub(crate) struct Cursor<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self { text, at: 0 }
    }

    fn bytes(&self) -> &'a [u8] {
        self.text.as_bytes()
    }

    pub(crate) fn peek(&self) -> Option<u8> {
        self.bytes().get(self.at).copied()
    }

    pub(crate) fn advance(&mut self) {
        self.at += 1;
    }

    /// Skips bytes for which `is_whitespace` holds.
    pub(crate) fn skip(&mut self, is_whitespace: fn(u8) -> bool) {
        while self.peek().is_some_and(is_whitespace) {
            self.at += 1;
        }
    }

    /// Consumes `word` if the text continues with it.
    pub(crate) fn take(&mut self, word: &str) -> bool {
        if self.bytes()[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            true
        } else {
            false
        }
    }

    /// Reads a string body; the opening quote is already consumed.
    pub(crate) fn string(&mut self) -> Result<String, StringError> {
        let mut out = String::new();
        loop {
            let start = self.at;
            // Copy the run up to the next quote, backslash or control byte.
            // These are ASCII, so the run ends on a character boundary.
            while let Some(byte) = self.peek() {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.at += 1;
            }
            out.push_str(&self.text[start..self.at]);
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    out.push(self.escape()?);
                }
                Some(_) => return Err(StringError::Invalid),
                None => return Err(StringError::Unterminated),
            }
        }
    }

    /// Reads one escape; the backslash is already consumed.
    fn escape(&mut self) -> Result<char, StringError> {
        let byte = self.peek().ok_or(StringError::Unterminated)?;
        self.at += 1;
        Ok(match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode_escape(),
            _ => return Err(StringError::Invalid),
        })
    }

    fn hex4(&mut self) -> Result<u32, StringError> {
        let digits = self
            .bytes()
            .get(self.at..self.at + 4)
            .ok_or(StringError::Invalid)?;
        let mut value = 0;
        for byte in digits {
            let digit = char::from(*byte).to_digit(16).ok_or(StringError::Invalid)?;
            value = value * 16 + digit;
        }
        self.at += 4;
        Ok(value)
    }

    /// Reads the digits of a `\u` escape, pairing surrogates.
    fn unicode_escape(&mut self) -> Result<char, StringError> {
        let first = self.hex4()?;
        let code = match first {
            0xD800..=0xDBFF => {
                if !self.take("\\u") {
                    return Err(StringError::Invalid);
                }
                let second = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&second) {
                    return Err(StringError::Invalid);
                }
                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(StringError::Invalid),
            other => other,
        };
        char::from_u32(code).ok_or(StringError::Invalid)
    }
}

fn is_json_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

impl From<StringError> for JsonError {
    fn from(err: StringError) -> Self {
        match err {
            StringError::Unterminated => Self::Malformed,
            StringError::Invalid => Self::InvalidString,
        }
    }
}

/// Parses one JSON document.
///
/// `max_depth` is the deepest permitted nesting of arrays and objects: a
/// document that is a scalar has depth zero.
///
/// # Errors
///
/// Refuses text that is not exactly one well-formed JSON value, a repeated
/// object key, an unsupported number and nesting beyond `max_depth`.
pub fn parse_document(text: &str, max_depth: usize) -> Result<Json, JsonError> {
    let mut cursor = Cursor::new(text);
    cursor.skip(is_json_whitespace);
    if cursor.peek().is_none() {
        return Err(JsonError::Empty);
    }
    let value = value(&mut cursor, max_depth)?;
    cursor.skip(is_json_whitespace);
    if cursor.peek().is_some() {
        return Err(JsonError::TrailingContent);
    }
    Ok(value)
}

fn value(cursor: &mut Cursor<'_>, depth_left: usize) -> Result<Json, JsonError> {
    match cursor.peek() {
        Some(b'"') => {
            cursor.advance();
            Ok(Json::String(cursor.string()?))
        }
        Some(b'n') if cursor.take("null") => Ok(Json::Null),
        Some(b't') if cursor.take("true") => Ok(Json::Boolean(true)),
        Some(b'f') if cursor.take("false") => Ok(Json::Boolean(false)),
        Some(b'-' | b'0'..=b'9') => number(cursor),
        Some(b'[') => {
            let depth_left = depth_left.checked_sub(1).ok_or(JsonError::TooDeep)?;
            cursor.advance();
            array(cursor, depth_left)
        }
        Some(b'{') => {
            let depth_left = depth_left.checked_sub(1).ok_or(JsonError::TooDeep)?;
            cursor.advance();
            object(cursor, depth_left)
        }
        _ => Err(JsonError::Malformed),
    }
}

/// Reads `0|[1-9][0-9]*` as a `u64`. Any other well-formed JSON number is
/// unsupported; anything else is malformed.
fn number(cursor: &mut Cursor<'_>) -> Result<Json, JsonError> {
    let negative = cursor.peek() == Some(b'-');
    if negative {
        cursor.advance();
    }
    let mut value: Option<u64> = Some(0);
    let mut digits = 0usize;
    let mut leading_zero = false;
    while let Some(byte @ b'0'..=b'9') = cursor.peek() {
        if digits == 1 && value == Some(0) {
            leading_zero = true;
        }
        value = value
            .and_then(|v| v.checked_mul(10))
            .and_then(|v| v.checked_add(u64::from(byte - b'0')));
        digits += 1;
        cursor.advance();
    }
    if digits == 0 || leading_zero {
        return Err(JsonError::Malformed);
    }
    let mut plain = !negative;
    if cursor.peek() == Some(b'.') {
        plain = false;
        cursor.advance();
        if !matches!(cursor.peek(), Some(b'0'..=b'9')) {
            return Err(JsonError::Malformed);
        }
        while matches!(cursor.peek(), Some(b'0'..=b'9')) {
            cursor.advance();
        }
    }
    if matches!(cursor.peek(), Some(b'e' | b'E')) {
        plain = false;
        cursor.advance();
        if matches!(cursor.peek(), Some(b'+' | b'-')) {
            cursor.advance();
        }
        if !matches!(cursor.peek(), Some(b'0'..=b'9')) {
            return Err(JsonError::Malformed);
        }
        while matches!(cursor.peek(), Some(b'0'..=b'9')) {
            cursor.advance();
        }
    }
    match value {
        Some(value) if plain => Ok(Json::Integer(value)),
        _ => Err(JsonError::UnsupportedNumber),
    }
}

/// Reads array elements; the opening bracket is already consumed.
fn array(cursor: &mut Cursor<'_>, depth_left: usize) -> Result<Json, JsonError> {
    let mut items = Vec::new();
    cursor.skip(is_json_whitespace);
    if cursor.peek() == Some(b']') {
        cursor.advance();
        return Ok(Json::Array(items));
    }
    loop {
        cursor.skip(is_json_whitespace);
        items.push(value(cursor, depth_left)?);
        cursor.skip(is_json_whitespace);
        match cursor.peek() {
            Some(b',') => cursor.advance(),
            Some(b']') => {
                cursor.advance();
                return Ok(Json::Array(items));
            }
            _ => return Err(JsonError::Malformed),
        }
    }
}

/// Reads object members; the opening brace is already consumed.
fn object(cursor: &mut Cursor<'_>, depth_left: usize) -> Result<Json, JsonError> {
    let mut members: Vec<(String, Json)> = Vec::new();
    cursor.skip(is_json_whitespace);
    if cursor.peek() == Some(b'}') {
        cursor.advance();
        return Ok(Json::Object(members));
    }
    loop {
        cursor.skip(is_json_whitespace);
        if cursor.peek() != Some(b'"') {
            return Err(JsonError::Malformed);
        }
        cursor.advance();
        let key = cursor.string()?;
        // Documents read here are small; a linear scan keeps member order
        // and needs no hashing.
        if members.iter().any(|(existing, _)| *existing == key) {
            return Err(JsonError::DuplicateKey);
        }
        cursor.skip(is_json_whitespace);
        if cursor.peek() != Some(b':') {
            return Err(JsonError::Malformed);
        }
        cursor.advance();
        cursor.skip(is_json_whitespace);
        let member = value(cursor, depth_left)?;
        members.push((key, member));
        cursor.skip(is_json_whitespace);
        match cursor.peek() {
            Some(b',') => cursor.advance(),
            Some(b'}') => {
                cursor.advance();
                return Ok(Json::Object(members));
            }
            _ => return Err(JsonError::Malformed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Json, JsonError> {
        parse_document(text, 8)
    }

    fn s(text: &str) -> Json {
        Json::String(text.to_owned())
    }

    #[test]
    fn values_are_read() {
        assert_eq!(parse("null").unwrap(), Json::Null);
        assert_eq!(parse(" true\n").unwrap(), Json::Boolean(true));
        assert_eq!(parse("false").unwrap(), Json::Boolean(false));
        assert_eq!(parse("0").unwrap(), Json::Integer(0));
        assert_eq!(
            parse("18446744073709551615").unwrap(),
            Json::Integer(u64::MAX)
        );
        assert_eq!(parse(r#""aé""#).unwrap(), s("aé"));
        assert_eq!(parse("[]").unwrap(), Json::Array(vec![]));
        assert_eq!(parse("{}").unwrap(), Json::Object(vec![]));
        assert_eq!(
            parse("\r\n{ \"a\" : [ 1 , null ] ,\t\"b\" : { \"c\" : \"d\" } }\r\n").unwrap(),
            Json::Object(vec![
                (
                    "a".to_owned(),
                    Json::Array(vec![Json::Integer(1), Json::Null])
                ),
                ("b".to_owned(), Json::Object(vec![("c".to_owned(), s("d"))])),
            ])
        );
    }

    #[test]
    fn member_order_is_kept() {
        let Json::Object(members) = parse(r#"{"b": 1, "a": 2}"#).unwrap() else {
            panic!("not an object");
        };
        let keys: Vec<&str> = members.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["b", "a"]);
    }

    #[test]
    fn duplicate_keys_are_refused_at_every_depth() {
        for text in [
            r#"{"a": 1, "a": 2}"#,
            r#"{"a": 1, "b": {"c": 1, "c": 1}}"#,
            r#"[{"a": null, "a": null}]"#,
            r#"{"x": [[{"k": 1, "k": 1}]]}"#,
            // The same key spelled with and without an escape.
            r#"{"a": 1, "\u0061": 2}"#,
            r#"{"é": 1, "é": 2}"#,
            r#"{"": 1, "": 2}"#,
        ] {
            assert_eq!(parse(text), Err(JsonError::DuplicateKey), "{text}");
        }
        // Keys that differ only by case or normalization form are distinct.
        assert!(parse(r#"{"a": 1, "A": 2}"#).is_ok());
        assert!(parse("{\"\u{e9}\": 1, \"e\u{301}\": 2}").is_ok());
    }

    #[test]
    fn unsupported_numbers_are_refused() {
        for text in [
            "-1",
            "-0",
            "1.5",
            "1.0",
            "1e3",
            "1E3",
            "1e+3",
            "0.5e-2",
            "18446744073709551616",
            "184467440737095516150",
            "[1, -2]",
        ] {
            assert_eq!(parse(text), Err(JsonError::UnsupportedNumber), "{text}");
        }
    }

    #[test]
    fn malformed_numbers_are_refused() {
        for text in ["01", "-", "1.", "1e", "1e+", "-a", "00", "-01"] {
            assert_eq!(parse(text), Err(JsonError::Malformed), "{text}");
        }
        for text in ["+1", ".5", "NaN", "Infinity"] {
            assert_eq!(parse(text), Err(JsonError::Malformed), "{text}");
        }
        assert_eq!(parse("1 2"), Err(JsonError::TrailingContent));
        assert_eq!(parse("1x"), Err(JsonError::TrailingContent));
        assert_eq!(parse("0x10"), Err(JsonError::TrailingContent));
    }

    #[test]
    fn malformed_documents_are_refused() {
        for text in [
            "{",
            "[",
            "[1,",
            "[1,]",
            "[,1]",
            "[1 2]",
            "{\"a\"}",
            "{\"a\":}",
            "{\"a\":1,}",
            "{,}",
            "{a: 1}",
            "{'a': 1}",
            "{\"a\" 1}",
            "{1: 2}",
            "nul",
            "tru",
            "True",
            "\"abc",
            "\"abc\\",
            "\u{feff}{}",
            "\u{a0}1",
        ] {
            assert_eq!(parse(text), Err(JsonError::Malformed), "{text:?}");
        }
        for text in ["", " ", "\n\t\r "] {
            assert_eq!(parse(text), Err(JsonError::Empty), "{text:?}");
        }
        for text in ["{} {}", "[] x", "null,", "{}}", "\"a\"\"b\""] {
            assert_eq!(parse(text), Err(JsonError::TrailingContent), "{text}");
        }
    }

    #[test]
    fn bad_strings_are_refused() {
        for text in [
            r#""\x41""#,
            r#""\u12""#,
            r#""\ud83d""#,
            r#""\ude00""#,
            "\"a\tb\"",
            "\"a\nb\"",
            r#"{"\ud83d": 1}"#,
        ] {
            assert_eq!(parse(text), Err(JsonError::InvalidString), "{text:?}");
        }
    }

    #[test]
    fn depth_is_bounded() {
        assert!(parse_document("1", 0).is_ok());
        assert_eq!(parse_document("[]", 0), Err(JsonError::TooDeep));
        assert_eq!(parse_document("{}", 0), Err(JsonError::TooDeep));
        assert!(parse_document("[[1]]", 2).is_ok());
        assert_eq!(parse_document("[[[1]]]", 2), Err(JsonError::TooDeep));
        assert_eq!(
            parse_document(r#"{"a": {"b": {"c": 1}}}"#, 2),
            Err(JsonError::TooDeep)
        );
        // Siblings do not add depth.
        assert!(parse_document("[[1], [2], [3]]", 2).is_ok());
        // Deep input fails on the limit, not on the call stack.
        let deep = "[".repeat(100_000);
        assert_eq!(parse_document(&deep, 64), Err(JsonError::TooDeep));
    }

    #[test]
    fn messages_do_not_contain_document_content() {
        let err = parse(r#"{"secret-9999": 1, "secret-9999": 2}"#).unwrap_err();
        assert!(!err.to_string().contains("secret"));
    }
}
