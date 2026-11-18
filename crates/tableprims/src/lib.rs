// SPDX-License-Identifier: MIT OR Apache-2.0

//! Typed tabular conversion library.
//!
//! This crate is at the bootstrap stage. It implements no conversions:
//! [`conversions`] returns an empty list until an adapter is implemented and
//! tested. Callers must treat an absent conversion as unsupported.

pub mod exact;
pub mod table;

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

/// Conversions implemented by this build.
///
/// The list is empty until the first adapter lands. It reports implemented
/// behaviour only, never planned work.
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
