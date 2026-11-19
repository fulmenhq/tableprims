// SPDX-License-Identifier: MIT OR Apache-2.0
//! Digests: bytes in, lowercase hexadecimal SHA-256 out.
//!
//! This is the only module that names the canonicalization and hashing
//! crates. Nothing here appears in another module's signature except
//! strings, byte counts and the two wrappers.

use std::io::{self, Read, Write};

use canonprims_core::Limits;
use sha2::{Digest, Sha256};

fn hex(digest: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// SHA-256 of `bytes`.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The RFC 8785 canonical form of a JSON document, or `None` when the text
/// is not a document that form covers.
pub(crate) fn canonical_json(document: &[u8]) -> Option<Vec<u8>> {
    canonprims_json::canonicalize(document, &Limits::default()).ok()
}

/// A digest and the number of bytes it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sum {
    pub(crate) sha256: String,
    pub(crate) bytes: u64,
}

impl Sum {
    pub(crate) fn of(bytes: &[u8]) -> Self {
        Self {
            sha256: sha256(bytes),
            bytes: bytes.len() as u64,
        }
    }
}

/// Hashes everything read through it.
pub(crate) struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
    bytes: u64,
}

impl<R: Read> HashingReader<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes: 0,
        }
    }

    /// Reads to the end of the input, so the digest covers all of it.
    pub(crate) fn finish(mut self) -> io::Result<Sum> {
        io::copy(&mut self, &mut io::sink())?;
        Ok(Sum {
            sha256: hex(&self.hasher.finalize()),
            bytes: self.bytes,
        })
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..count]);
        self.bytes += count as u64;
        Ok(count)
    }
}

/// Hashes everything written through it.
pub(crate) struct HashingWriter<W> {
    inner: W,
    hasher: Sha256,
    bytes: u64,
}

impl<W: Write> HashingWriter<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes: 0,
        }
    }

    /// The writer and the digest of what it received.
    pub(crate) fn finish(self) -> (W, Sum) {
        let sum = Sum {
            sha256: hex(&self.hasher.finalize()),
            bytes: self.bytes,
        };
        (self.inner, sum)
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(buffer)?;
        self.hasher.update(&buffer[..count]);
        self.bytes += count as u64;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_match_known_values() {
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut reader = HashingReader::new(&b"abc"[..]);
        let mut one = [0_u8; 1];
        reader.read_exact(&mut one).unwrap();
        assert_eq!(reader.finish().unwrap(), Sum::of(b"abc"));
        let mut writer = HashingWriter::new(Vec::new());
        writer.write_all(b"ab").unwrap();
        writer.write_all(b"c").unwrap();
        let (inner, sum) = writer.finish();
        assert_eq!((inner.as_slice(), sum), (&b"abc"[..], Sum::of(b"abc")));
    }

    #[test]
    fn canonical_json_sorts_members_and_drops_whitespace() {
        let canonical = canonical_json(r#"{ "b": [1, "é"], "a": "x" }"#.as_bytes()).unwrap();
        assert_eq!(canonical, "{\"a\":\"x\",\"b\":[1,\"é\"]}".as_bytes());
        assert!(canonical_json(b"{").is_none());
    }
}
