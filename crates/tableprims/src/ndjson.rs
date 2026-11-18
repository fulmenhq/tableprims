// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rows as newline-delimited JSON arrays.
//!
//! Each line is one JSON array holding one scalar per column: `null`,
//! `true`, `false` or a string. Exact numbers, dates and timestamps travel
//! as strings; a bare JSON number is refused, never read through binary
//! floating point. Nested arrays and objects are refused. The format is
//! provisional and may change before the first release.
//!
//! Input is UTF-8 without a byte order mark. Lines end with LF or CRLF and
//! the final line may omit its terminator. A blank line is an error, not a
//! skipped row. An empty input holds zero rows. Whitespace inside a line is
//! limited to spaces and tabs between tokens.
//!
//! The parser is written here rather than taken from a JSON library so that
//! every refusal is explicit and no numeric conversion can occur.

use core::fmt;
use std::io::{self, BufRead, Read};

use crate::table::Scalar;

/// One cell of a row as read from a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// JSON `null`.
    Null,
    /// JSON `true` or `false`.
    Boolean(bool),
    /// A JSON string, unescaped.
    Text(String),
}

impl Cell {
    /// Borrows the cell as a transport scalar for a row check.
    #[must_use]
    pub fn as_scalar(&self) -> Scalar<'_> {
        match self {
            Self::Null => Scalar::Null,
            Self::Boolean(value) => Scalar::Boolean(*value),
            Self::Text(text) => Scalar::Text(text),
        }
    }
}

/// Why a line was refused.
///
/// The messages never include line content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LineError {
    /// The line is empty or holds only whitespace.
    Blank,
    /// The line is not a JSON array.
    NotAnArray,
    /// The array or the text around it is not well-formed.
    Malformed,
    /// A cell is a bare JSON number.
    NumericLiteral,
    /// A cell is an array or an object.
    NestedValue,
    /// A string holds an invalid escape, an unpaired surrogate or an
    /// unescaped control character.
    InvalidString,
    /// Something other than whitespace follows the array.
    TrailingContent,
}

impl fmt::Display for LineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Blank => "line is blank",
            Self::NotAnArray => "line is not a JSON array",
            Self::Malformed => "array is not well-formed",
            Self::NumericLiteral => "bare numbers are not accepted; write numbers as strings",
            Self::NestedValue => "nested arrays and objects are not accepted",
            Self::InvalidString => "string is not well-formed",
            Self::TrailingContent => "content follows the array",
        })
    }
}

impl std::error::Error for LineError {}

/// Parses one line, without its terminator, into cells.
///
/// # Errors
///
/// Refuses anything that is not exactly one JSON array of `null`, `true`,
/// `false` and string values.
pub fn parse_line(line: &str) -> Result<Vec<Cell>, LineError> {
    let mut parser = Parser {
        bytes: line.as_bytes(),
        text: line,
        at: 0,
    };
    parser.skip_whitespace();
    match parser.peek() {
        None => return Err(LineError::Blank),
        Some(b'[') => parser.at += 1,
        Some(_) => return Err(LineError::NotAnArray),
    }
    let mut cells = Vec::new();
    parser.skip_whitespace();
    if parser.peek() == Some(b']') {
        parser.at += 1;
    } else {
        loop {
            parser.skip_whitespace();
            cells.push(parser.cell()?);
            parser.skip_whitespace();
            match parser.peek() {
                Some(b',') => parser.at += 1,
                Some(b']') => {
                    parser.at += 1;
                    break;
                }
                _ => return Err(LineError::Malformed),
            }
        }
    }
    parser.skip_whitespace();
    if parser.peek().is_some() {
        return Err(LineError::TrailingContent);
    }
    Ok(cells)
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.at += 1;
        }
    }

    fn literal(&mut self, word: &str, cell: Cell) -> Result<Cell, LineError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(cell)
        } else {
            Err(LineError::Malformed)
        }
    }

    fn cell(&mut self) -> Result<Cell, LineError> {
        match self.peek() {
            Some(b'"') => {
                self.at += 1;
                self.string().map(Cell::Text)
            }
            Some(b'n') => self.literal("null", Cell::Null),
            Some(b't') => self.literal("true", Cell::Boolean(true)),
            Some(b'f') => self.literal("false", Cell::Boolean(false)),
            Some(b'-' | b'0'..=b'9') => Err(LineError::NumericLiteral),
            Some(b'[' | b'{') => Err(LineError::NestedValue),
            _ => Err(LineError::Malformed),
        }
    }

    /// Reads a string body; the opening quote is already consumed.
    fn string(&mut self) -> Result<String, LineError> {
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
                // A control character, or the line ended inside the string.
                Some(_) => return Err(LineError::InvalidString),
                None => return Err(LineError::Malformed),
            }
        }
    }

    /// Reads one escape; the backslash is already consumed.
    fn escape(&mut self) -> Result<char, LineError> {
        let byte = self.peek().ok_or(LineError::Malformed)?;
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
            _ => return Err(LineError::InvalidString),
        })
    }

    fn hex4(&mut self) -> Result<u32, LineError> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or(LineError::InvalidString)?;
        let mut value = 0;
        for byte in digits {
            let digit = char::from(*byte)
                .to_digit(16)
                .ok_or(LineError::InvalidString)?;
            value = value * 16 + digit;
        }
        self.at += 4;
        Ok(value)
    }

    /// Reads the digits of a `\u` escape, pairing surrogates.
    fn unicode_escape(&mut self) -> Result<char, LineError> {
        let first = self.hex4()?;
        let code = match first {
            0xD800..=0xDBFF => {
                if !self.bytes[self.at..].starts_with(b"\\u") {
                    return Err(LineError::InvalidString);
                }
                self.at += 2;
                let second = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&second) {
                    return Err(LineError::InvalidString);
                }
                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(LineError::InvalidString),
            other => other,
        };
        char::from_u32(code).ok_or(LineError::InvalidString)
    }
}

/// Why reading rows stopped.
#[derive(Debug)]
#[non_exhaustive]
pub enum ReadError {
    /// The underlying reader failed.
    Io(io::Error),
    /// The input starts with a byte order mark.
    ByteOrderMark,
    /// A line is not valid UTF-8.
    InvalidUtf8 {
        /// Zero-based index of the line.
        row: u64,
    },
    /// A line is longer than the configured limit.
    LineTooLong {
        /// Zero-based index of the line.
        row: u64,
    },
    /// A line was refused.
    Line {
        /// Zero-based index of the line.
        row: u64,
        /// The reason.
        error: LineError,
    },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "cannot read rows: {err}"),
            Self::ByteOrderMark => f.write_str("input starts with a byte order mark"),
            Self::InvalidUtf8 { row } => write!(f, "row {row}: line is not valid UTF-8"),
            Self::LineTooLong { row } => write!(f, "row {row}: line exceeds the length limit"),
            Self::Line { row, error } => write!(f, "row {row}: {error}"),
        }
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Line { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Reads rows from newline-delimited JSON, one line at a time.
///
/// Memory use is bounded by the line length limit. After the first error the
/// reader yields nothing further.
#[derive(Debug)]
pub struct RowReader<R> {
    input: R,
    max_line_bytes: usize,
    next_row: u64,
    buffer: Vec<u8>,
    done: bool,
}

impl<R: BufRead> RowReader<R> {
    /// Creates a reader that refuses any line longer than `max_line_bytes`,
    /// not counting its terminator.
    pub fn new(input: R, max_line_bytes: usize) -> Self {
        Self {
            input,
            max_line_bytes,
            next_row: 0,
            buffer: Vec::new(),
            done: false,
        }
    }

    fn read_row(&mut self) -> Result<Option<(u64, Vec<Cell>)>, ReadError> {
        let row = self.next_row;
        self.buffer.clear();
        // Content, an optional CR, the LF, and one byte to detect excess.
        let limit = (self.max_line_bytes as u64).saturating_add(3);
        let read = (&mut self.input)
            .take(limit)
            .read_until(b'\n', &mut self.buffer)
            .map_err(ReadError::Io)?;
        if read == 0 {
            return Ok(None);
        }
        let mut line = self.buffer.as_slice();
        let terminated = line.last() == Some(&b'\n');
        if terminated {
            line = &line[..line.len() - 1];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
        }
        if line.len() > self.max_line_bytes {
            return Err(ReadError::LineTooLong { row });
        }
        if row == 0 && line.starts_with(b"\xEF\xBB\xBF") {
            return Err(ReadError::ByteOrderMark);
        }
        let text = core::str::from_utf8(line).map_err(|_| ReadError::InvalidUtf8 { row })?;
        let cells = parse_line(text).map_err(|error| ReadError::Line { row, error })?;
        self.next_row += 1;
        Ok(Some((row, cells)))
    }
}

impl<R: BufRead> Iterator for RowReader<R> {
    /// Zero-based row index and the row's cells.
    type Item = Result<(u64, Vec<Cell>), ReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.read_row() {
            Ok(Some(item)) => Some(Ok(item)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(err) => {
                self.done = true;
                Some(Err(err))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Cell {
        Cell::Text(s.to_owned())
    }

    fn rows(input: &[u8], limit: usize) -> Result<Vec<Vec<Cell>>, ReadError> {
        RowReader::new(input, limit)
            .map(|item| item.map(|(_, cells)| cells))
            .collect()
    }

    #[test]
    fn scalars_are_read() {
        assert_eq!(
            parse_line(r#"["42", null, true, false, ""]"#).unwrap(),
            vec![
                text("42"),
                Cell::Null,
                Cell::Boolean(true),
                Cell::Boolean(false),
                text("")
            ]
        );
        assert_eq!(parse_line("[]").unwrap(), vec![]);
        assert_eq!(parse_line(" \t[\t\"a\" ,\"b\" ] ").unwrap().len(), 2);
    }

    #[test]
    fn strings_are_unescaped() {
        let cells = parse_line(r#"["a\"b\\c\/d\b\f\n\r\t", "é€", "😀", "é😀"]"#).unwrap();
        assert_eq!(cells[0], text("a\"b\\c/d\u{8}\u{c}\n\r\t"));
        assert_eq!(cells[1], text("é€"));
        assert_eq!(cells[2], text("😀"));
        assert_eq!(cells[3], text("é😀"));
        assert_eq!(parse_line(r#"["\u0000"]"#).unwrap()[0], text("\0"));
    }

    #[test]
    fn number_looking_strings_stay_text() {
        assert_eq!(
            parse_line(r#"["000123", "1e5", "-0"]"#).unwrap(),
            vec![text("000123"), text("1e5"), text("-0")]
        );
    }

    #[test]
    fn bare_numbers_are_refused() {
        for line in ["[1]", "[-1]", "[0.5]", "[\"a\", 12]", "[1e5]"] {
            assert_eq!(parse_line(line), Err(LineError::NumericLiteral), "{line}");
        }
    }

    #[test]
    fn nested_values_are_refused() {
        for line in ["[[]]", "[{}]", "[\"a\", [\"b\"]]", "[{\"a\": null}]"] {
            assert_eq!(parse_line(line), Err(LineError::NestedValue), "{line}");
        }
    }

    #[test]
    fn lines_that_are_not_arrays_are_refused() {
        for line in ["{}", "\"a\"", "null", "1", "x"] {
            assert_eq!(parse_line(line), Err(LineError::NotAnArray), "{line}");
        }
        for line in ["", " ", "\t \t"] {
            assert_eq!(parse_line(line), Err(LineError::Blank), "{line:?}");
        }
    }

    #[test]
    fn malformed_arrays_are_refused() {
        for line in [
            "[",
            "[null",
            "[null,",
            "[null,]",
            "[,null]",
            "[null null]",
            "[nul]",
            "[True]",
            "[NaN]",
            "[+1]",
            "[.5]",
            "['a']",
            "[\"a]",
            "[\"a\\",
            "[null\r]",
            "[\u{a0}null]",
        ] {
            assert_eq!(parse_line(line), Err(LineError::Malformed), "{line:?}");
        }
    }

    #[test]
    fn trailing_content_is_refused() {
        for line in ["[] []", "[null]x", "[null],", "[null]]"] {
            assert_eq!(parse_line(line), Err(LineError::TrailingContent), "{line}");
        }
    }

    #[test]
    fn bad_strings_are_refused() {
        for line in [
            r#"["\x41"]"#,
            r#"["\u12"]"#,
            r#"["\u12G4"]"#,
            r#"["\ud83d"]"#,
            r#"["\ud83dx"]"#,
            r#"["\ud83dA"]"#,
            r#"["\ude00"]"#,
            "[\"a\tb\"]",
            "[\"a\u{1}b\"]",
        ] {
            assert_eq!(parse_line(line), Err(LineError::InvalidString), "{line:?}");
        }
    }

    #[test]
    fn cells_borrow_as_scalars() {
        assert_eq!(Cell::Null.as_scalar(), Scalar::Null);
        assert_eq!(Cell::Boolean(true).as_scalar(), Scalar::Boolean(true));
        assert_eq!(text("x").as_scalar(), Scalar::Text("x"));
    }

    #[test]
    fn reader_accepts_lf_crlf_and_missing_final_terminator() {
        let expected = vec![vec![text("a")], vec![Cell::Null]];
        assert_eq!(rows(b"[\"a\"]\n[null]\n", 64).unwrap(), expected);
        assert_eq!(rows(b"[\"a\"]\r\n[null]\r\n", 64).unwrap(), expected);
        assert_eq!(rows(b"[\"a\"]\n[null]", 64).unwrap(), expected);
        assert_eq!(rows(b"[\"a\"]\r\n[null]", 64).unwrap(), expected);
    }

    #[test]
    fn empty_input_has_no_rows() {
        assert!(rows(b"", 64).unwrap().is_empty());
    }

    #[test]
    fn reader_numbers_rows_from_zero() {
        let indices: Vec<u64> = RowReader::new(&b"[]\n[]\n[]"[..], 8)
            .map(|item| item.unwrap().0)
            .collect();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn blank_lines_are_errors_with_their_row() {
        for input in [&b"[]\n\n[]\n"[..], b"[]\n\r\n[]\n", b"[]\n \n"] {
            let mut reader = RowReader::new(input, 64);
            assert!(reader.next().unwrap().is_ok());
            match reader.next().unwrap() {
                Err(ReadError::Line { row: 1, error }) => assert_eq!(error, LineError::Blank),
                other => panic!("unexpected {other:?}"),
            }
            assert!(reader.next().is_none());
        }
        // A lone terminator is a blank first line, not an empty input.
        assert!(matches!(
            rows(b"\n", 64),
            Err(ReadError::Line {
                row: 0,
                error: LineError::Blank
            })
        ));
    }

    #[test]
    fn carriage_return_is_only_a_terminator_before_line_feed() {
        assert!(matches!(
            rows(b"[null]\r[null]\n", 64),
            Err(ReadError::Line {
                row: 0,
                error: LineError::TrailingContent
            })
        ));
        assert!(matches!(
            rows(b"[null]\r", 64),
            Err(ReadError::Line {
                row: 0,
                error: LineError::TrailingContent
            })
        ));
    }

    #[test]
    fn byte_order_mark_is_refused() {
        assert!(matches!(
            rows(b"\xEF\xBB\xBF[null]\n", 64),
            Err(ReadError::ByteOrderMark)
        ));
    }

    #[test]
    fn invalid_utf8_is_located() {
        assert!(matches!(
            rows(b"[null]\n[\"\xFF\"]\n", 64),
            Err(ReadError::InvalidUtf8 { row: 1 })
        ));
    }

    #[test]
    fn line_length_limit_excludes_the_terminator() {
        // "[null]" is six bytes.
        assert_eq!(rows(b"[null]\n", 6).unwrap().len(), 1);
        assert_eq!(rows(b"[null]\r\n", 6).unwrap().len(), 1);
        assert_eq!(rows(b"[null]", 6).unwrap().len(), 1);
        for input in [&b"[null]\n"[..], b"[null]\r\n", b"[null]", b"[null] \n"] {
            let limit = if input.starts_with(b"[null] ") { 6 } else { 5 };
            assert!(
                matches!(rows(input, limit), Err(ReadError::LineTooLong { row: 0 })),
                "{input:?}"
            );
        }
        assert!(matches!(
            rows(&[b' '; 10_000], 16),
            Err(ReadError::LineTooLong { row: 0 })
        ));
    }

    #[test]
    fn reader_stops_after_the_first_error() {
        let mut reader = RowReader::new(&b"[1]\n[null]\n"[..], 64);
        assert!(reader.next().unwrap().is_err());
        assert!(reader.next().is_none());
    }

    #[test]
    fn messages_do_not_contain_line_content() {
        let err = rows(b"[\"secret-9999\", 5]\n", 64).unwrap_err();
        assert!(!err.to_string().contains("secret"));
    }
}
