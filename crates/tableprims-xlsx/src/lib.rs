// SPDX-License-Identifier: MIT OR Apache-2.0
//! XLSX write adapter for the `tableprims` library.
//!
//! The adapter writes rows of a typed table to one worksheet. Every value
//! is checked against the schema before the first cell is written, so the
//! representation of each column is fixed for the whole column:
//!
//! 1. [`Plan::new`] checks the schema and policy against this profile.
//! 2. [`Plan::scan`] reads the input once, validates every row and decides
//!    how each column is stored.
//! 3. [`Scan::write`] reads the input a second time and writes the
//!    workbook. A second read that differs from the first is refused.
//!
//! An exact number is stored as a numeric cell only when the decimal text
//! of the stored binary64 value equals the source value; see
//! [`tableprims::binary64`]. Dates and timestamps are stored as text.
//!
//! Types, scale, nullability and the positions of empty strings are
//! recorded on a hidden worksheet named `_tableprims`; see [`metadata`].
//! A blank data cell is a null unless that worksheet lists it as an empty
//! string.
//!
//! The adapter writes to a caller-supplied sink. Choosing the destination,
//! refusing to overwrite and syncing are the caller's.
//!
//! The formats are provisional and may change before the first release.

mod error;
pub mod metadata;
mod plan;
mod write;

pub use error::{Category, ErrorKind, WriteError};
pub use plan::{ColumnOutcome, Outcome, Plan, Representation, ResolvedLimits, Scan};

/// Identifier of the write profile this adapter implements.
pub const PROFILE_ID: &str = "xlsx-write/r1";
/// Name of the workbook writer.
pub const BACKEND_NAME: &str = "rust_xlsxwriter";
/// Version of the workbook writer this profile is pinned to.
pub const BACKEND_VERSION: &str = "0.102.0";

/// Largest number of data rows: a worksheet's rows less the header row.
pub const MAX_ROWS: u64 = 1_048_575;
/// Largest number of columns in a worksheet.
pub const MAX_COLUMNS: usize = 16_384;
/// Largest length of one cell's string, in UTF-16 code units.
pub const MAX_CELL_UNITS: usize = 32_767;
/// Largest number of empty-string ranges recorded for a workbook.
pub const MAX_EMPTY_RANGES: u64 = 65_536;
/// Length limit of one input line when the policy sets none, in bytes.
pub const DEFAULT_MAX_LINE_BYTES: u64 = 1 << 20;
/// Largest length limit of one input line a policy may set, in bytes.
pub const MAX_LINE_BYTES: u64 = 1 << 24;
