// SPDX-License-Identifier: MIT OR Apache-2.0

//! Capability identifiers and the capability boundary of this crate.
//!
//! Each identifier names an operation of the tableprims library in either
//! of its implementations. [`capabilities`] lists the ones this crate
//! implements, and [`require`] answers for any identifier. The XLSX adapter
//! crate reports its own; this crate never reports that it writes a
//! workbook.
//!
//! A [`CapabilityError`] is not a refusal of the Tabular Exchange Standard:
//! no document, value or row has been read.

use core::fmt;

/// Reads a schema document: [`crate::documents::parse_schema`].
pub const READ_SCHEMA: &str = "read-schema";
/// Reads a policy document: [`crate::documents::parse_policy`].
pub const READ_POLICY: &str = "read-policy";
/// Reads and checks rows: [`crate::ndjson::RowReader`] and
/// [`crate::table::Schema::check_row`].
pub const READ_ROWS: &str = "read-rows";
/// Quantizes a decimal: [`crate::exact::Decimal::quantize`].
pub const QUANTIZE: &str = "quantize";
/// Decides whether an exact number can be stored as a binary64 number:
/// [`crate::binary64`].
pub const BINARY64_DECISION: &str = "binary64-decision";
/// Writes an XLSX workbook. The XLSX adapter crate implements it; this crate
/// does not.
pub const XLSX_WRITE: &str = "xlsx-write";

/// Every known capability identifier, in a fixed order.
pub const KNOWN: &[&str] = &[
    READ_SCHEMA,
    READ_POLICY,
    READ_ROWS,
    QUANTIZE,
    BINARY64_DECISION,
    XLSX_WRITE,
];

const HERE: &[&str] = &[
    READ_SCHEMA,
    READ_POLICY,
    READ_ROWS,
    QUANTIZE,
    BINARY64_DECISION,
];

/// The capabilities this crate implements, in a fixed order. The list is
/// fixed at build time.
#[must_use]
pub fn capabilities() -> &'static [&'static str] {
    HERE
}

/// Returns `Ok` for a capability this crate implements.
///
/// # Errors
///
/// Returns [`Disposition::Unsupported`] for a known capability this crate
/// does not implement, and [`Disposition::Unknown`] for an identifier that
/// names no capability.
pub fn require(capability: &str) -> Result<(), CapabilityError> {
    answer(HERE, capability)
}

/// Answers [`require`] for a surface that implements `here`. Adapter crates
/// use it for their own `require`.
///
/// Only [`KNOWN`] identifiers are capabilities: one that is not in it is
/// [`Disposition::Unknown`] even when `here` lists it.
///
/// # Errors
///
/// [`Disposition::Unknown`] for an identifier not in [`KNOWN`], and
/// [`Disposition::Unsupported`] for a known identifier not in `here`.
pub fn answer(here: &[&str], capability: &str) -> Result<(), CapabilityError> {
    let disposition = if !KNOWN.contains(&capability) {
        Disposition::Unknown
    } else if here.contains(&capability) {
        return Ok(());
    } else {
        Disposition::Unsupported
    };
    Err(CapabilityError {
        capability: capability.to_owned(),
        disposition,
    })
}

/// Why a capability is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// A known capability this surface does not implement.
    Unsupported,
    /// An identifier that names no capability.
    Unknown,
}

impl Disposition {
    /// `unsupported` or `unknown`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
        }
    }
}

/// The error [`require`] returns for a capability that is not available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityError {
    capability: String,
    disposition: Disposition,
}

impl CapabilityError {
    /// The identifier that was required.
    #[must_use]
    pub fn capability(&self) -> &str {
        &self.capability
    }

    /// Why it is not available.
    #[must_use]
    pub fn disposition(&self) -> Disposition {
        self.disposition
    }
}

impl fmt::Display for CapabilityError {
    /// `capability-unsupported` or `capability-unknown` and the identifier,
    /// quoted and escaped.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "capability-{}: {:?}",
            self.disposition.as_str(),
            self.capability
        )
    }
}

impl std::error::Error for CapabilityError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crate_never_reports_a_workbook() {
        assert!(!capabilities().contains(&XLSX_WRITE));
        assert_eq!(
            require(XLSX_WRITE).unwrap_err().disposition(),
            Disposition::Unsupported
        );
    }

    #[test]
    fn only_known_identifiers_are_capabilities() {
        assert_eq!(
            answer(&["made-up", QUANTIZE], "made-up")
                .unwrap_err()
                .disposition(),
            Disposition::Unknown
        );
        assert!(answer(&["made-up", QUANTIZE], QUANTIZE).is_ok());
        assert_eq!(
            answer(&[QUANTIZE], READ_ROWS).unwrap_err().disposition(),
            Disposition::Unsupported
        );
    }

    #[test]
    fn errors_name_the_disposition_and_the_identifier() {
        assert_eq!(
            require(XLSX_WRITE).unwrap_err().to_string(),
            r#"capability-unsupported: "xlsx-write""#
        );
        assert_eq!(
            require("no\nsuch").unwrap_err().to_string(),
            r#"capability-unknown: "no\nsuch""#
        );
    }
}
