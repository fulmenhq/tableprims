// SPDX-License-Identifier: MIT OR Apache-2.0

//! Typed tabular conversion library.
//!
//! This crate holds exact numbers, typed rows and schemas, and the schema
//! and policy documents. Conversions are implemented by adapter crates, so
//! [`conversions`] here returns an empty list.
//!
//! The value grammar, the rows encoding and the two documents are those of
//! the Tabular Exchange Standard, contract revision
//! `tabular-exchange/0.1.0`, published in fulmenhq/crucible.

pub mod binary64;
pub mod documents;
pub mod exact;
pub mod json;
pub mod ndjson;
pub mod table;
pub mod temporal;

/// Version of this library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A conversion this build of the library can perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Conversion {
    /// Source format identifier.
    pub source: &'static str,
    /// Destination format identifier.
    pub destination: &'static str,
}

/// Conversions implemented by this crate.
///
/// The list is empty: conversions are implemented by adapter crates, which
/// report their own.
#[must_use]
pub fn conversions() -> &'static [Conversion] {
    &[]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_a_release_triple() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "version: {VERSION}");
        for part in parts {
            part.parse::<u64>().expect("numeric version component");
        }
    }

    #[test]
    fn bootstrap_build_reports_no_conversions() {
        assert!(conversions().is_empty());
    }
}
