// SPDX-License-Identifier: MIT OR Apache-2.0

//! Application identity compiled into the binary.
//!
//! These constants mirror `.fulmen/app.yaml` at the repository root. The
//! `identity` integration test fails if the two disagree.

/// Vendor namespace.
pub const VENDOR: &str = "fulmenhq";
/// Installed binary name.
pub const BINARY_NAME: &str = "tablebolt";
/// Prefix for environment variables read by the binary.
pub const ENV_PREFIX: &str = "TABLEBOLT_";
/// One-line description shown in help output.
pub const DESCRIPTION: &str = "Typed tabular conversion CLI built on the tableprims library";
