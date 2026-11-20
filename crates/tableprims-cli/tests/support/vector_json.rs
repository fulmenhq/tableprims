// SPDX-License-Identifier: MIT OR Apache-2.0

//! A reader for the vendored vector files that keeps every number and string
//! as the file spells it, and keeps object members in file order.
//!
//! A vector states an input exactly: `1.50`, `1e3` and `-0` are different
//! inputs, and a general JSON library reads them as numbers and forgets the
//! spelling. This reader is shared by the test targets that run the vectors
//! and is no part of the program under test.

#![allow(dead_code)]

/// A JSON value with its source spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    Null,
    Bool(bool),
    /// The number as the file spells it.
    Number(String),
    /// The decoded text, and the string token as the file spells it.
    String {
        text: String,
        raw: String,
    },
    Array(Vec<Json>),
    /// Members in file order. A repeated name is kept.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// A string value that was not read from a file.
    pub fn string(text: &str) -> Self {
        let mut raw = String::from("\"");
        for ch in text.chars() {
            match ch {
                '"' => raw.push_str("\\\""),
                '\\' => raw.push_str("\\\\"),
                ch if (ch as u32) < 0x20 => raw.push_str(&format!("\\u{:04x}", ch as u32)),
                ch => raw.push(ch),
            }
        }
        raw.push('"');
        Json::String {
            text: text.to_owned(),
            raw,
        }
    }

    /// The first member named `name`, when this is an object that has one.
    pub fn get(&self, name: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members
                .iter()
                .find(|(member, _)| member == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The member named `name`; panics when it is absent.
    pub fn at(&self, name: &str) -> &Json {
        self.get(name)
            .unwrap_or_else(|| panic!("no member {name:?} in {}", self.compact()))
    }

    pub fn text(&self) -> &str {
        match self {
            Json::String { text, .. } => text,
            other => panic!("not a string: {}", other.compact()),
        }
    }

    pub fn items(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            other => panic!("not an array: {}", other.compact()),
        }
    }

    pub fn members(&self) -> &[(String, Json)] {
        match self {
            Json::Object(members) => members,
            other => panic!("not an object: {}", other.compact()),
        }
    }

    /// Sets the member `name`, replacing the first of that name or adding one.
    pub fn set(&mut self, name: &str, value: Json) {
        let Json::Object(members) = self else {
            panic!("not an object");
        };
        match members.iter_mut().find(|(member, _)| member == name) {
            Some((_, slot)) => *slot = value,
            None => members.push((name.to_owned(), value)),
        }
    }

    /// Removes every member named `name`.
    pub fn remove(&mut self, name: &str) {
        let Json::Object(members) = self else {
            panic!("not an object");
        };
        members.retain(|(member, _)| member != name);
    }

    /// The value on one line with no white space between tokens. Numbers,
    /// strings and member names keep the spelling they were read with.
    pub fn compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Number(spelling) => out.push_str(spelling),
            Json::String { raw, .. } => out.push_str(raw),
            Json::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Object(members) => {
                out.push('{');
                for (index, (name, value)) in members.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&Json::string(name).compact());
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// Reads one JSON value from UTF-8 bytes.
pub fn parse(bytes: &[u8]) -> Result<Json, String> {
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let mut reader = Reader {
        text,
        bytes: text.as_bytes(),
        at: 0,
    };
    reader.space();
    let value = reader.value()?;
    reader.space();
    if reader.at != reader.bytes.len() {
        return Err(reader.fail("text after the value"));
    }
    Ok(value)
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn fail(&self, what: &str) -> String {
        format!("{what} at byte {}", self.at)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.text[self.at..].starts_with(word) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.fail("unknown literal"))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => {
                let start = self.at;
                let text = self.string()?;
                Ok(Json::String {
                    text,
                    raw: self.text[start..self.at].to_owned(),
                })
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            _ => Err(self.fail("no value")),
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        let leading_zero = self.peek() == Some(b'0');
        let whole = self.digits();
        if whole == 0 || (leading_zero && whole > 1) {
            return Err(self.fail("number"));
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return Err(self.fail("number"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return Err(self.fail("number"));
            }
        }
        Ok(Json::Number(self.text[start..self.at].to_owned()))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .text
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.fail("escape"))?;
        let unit = u32::from_str_radix(digits, 16).map_err(|_| self.fail("escape"))?;
        self.at += 4;
        Ok(unit)
    }

    /// Reads a string token and returns its decoded text.
    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let rest = &self.text[self.at..];
            let ch = rest.chars().next().ok_or_else(|| self.fail("string"))?;
            self.at += ch.len_utf8();
            match ch {
                '"' => return Ok(out),
                '\\' => {
                    let escape = self.peek().ok_or_else(|| self.fail("escape"))?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut unit = self.hex4()?;
                            if (0xd800..0xdc00).contains(&unit) {
                                if !self.text[self.at..].starts_with("\\u") {
                                    return Err(self.fail("unpaired surrogate"));
                                }
                                self.at += 2;
                                let low = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&low) {
                                    return Err(self.fail("unpaired surrogate"));
                                }
                                unit = 0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00);
                            }
                            out.push(
                                char::from_u32(unit)
                                    .ok_or_else(|| self.fail("unpaired surrogate"))?,
                            );
                        }
                        _ => return Err(self.fail("escape")),
                    }
                }
                ch if (ch as u32) < 0x20 => return Err(self.fail("control character")),
                ch => out.push(ch),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.at += 1;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.space();
            items.push(self.value()?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.fail("array")),
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.at += 1;
        let mut members = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.space();
            if self.peek() != Some(b'"') {
                return Err(self.fail("member name"));
            }
            let name = self.string()?;
            self.space();
            if self.peek() != Some(b':') {
                return Err(self.fail("colon"));
            }
            self.at += 1;
            self.space();
            members.push((name, self.value()?));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(self.fail("object")),
            }
        }
    }
}
