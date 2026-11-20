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
//!    workbook.
//!
//! The caller must give both reads the same bytes: an input that cannot
//! change between them, or digests of the bytes of both reads that the
//! caller compares before it publishes the workbook. The adapter does not
//! establish this. As a safeguard it refuses a second read with a different
//! outcome or different cells, comparing the cells by a 64-bit fingerprint
//! that is not a digest.
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
//! The pinned workbook writer panics when it cannot write its temporary
//! files. [`Scan::write`] catches that and returns an error, which needs a
//! build that unwinds on panic; the panic message still reaches the panic
//! hook.
//!
//! The worksheet name and the limits come from the `tableprims` extension
//! namespace of the policy. [`Plan::new`] refuses a policy that names no
//! worksheet. Refusals of rows and values carry the categories and rule
//! identifiers of the Tabular Exchange Standard, contract revision
//! `tabular-exchange/0.1.0`, published in fulmenhq/crucible; the rules of
//! this profile are its own.
//!
//! The workbook layout is provisional and may change before the first
//! release.

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

// The limits below and the record layout in `metadata` are settings of the
// profile named by `PROFILE_ID`; changing one changes the profile.

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

/// The settings of this profile as one JSON object, with member names in
/// ascending order and no insignificant whitespace. Its digest identifies
/// the profile a write ran under.
#[must_use]
pub fn profile_definition() -> String {
    format!(
        concat!(
            r#"{{"backend":{{"features":["constant_memory"],"name":"{backend}","version":"{version}"}},"#,
            r#""format":"tableprims.profile/0","id":"{id}","#,
            r#""limits":{{"default_max_line_bytes":{line_default},"max_cell_units":{cell},"#,
            r#""max_columns":{columns},"max_empty_ranges":{ranges},"max_line_bytes":{line},"max_rows":{rows}}},"#,
            r#""metadata":{{"header_rows":{header},"layout":"{layout}","ranges_per_record":{per_record},"#,
            r#""sheet":"{sheet}"}},"#,
            r#""native":{{"max_format_scale":{format_scale},"max_significant_digits":{digits}}}}}"#,
        ),
        backend = BACKEND_NAME,
        version = BACKEND_VERSION,
        id = PROFILE_ID,
        line_default = DEFAULT_MAX_LINE_BYTES,
        cell = MAX_CELL_UNITS,
        columns = MAX_COLUMNS,
        ranges = MAX_EMPTY_RANGES,
        line = MAX_LINE_BYTES,
        rows = MAX_ROWS,
        header = metadata::HEADER_ROWS,
        layout = metadata::LAYOUT,
        per_record = metadata::RANGES_PER_RECORD,
        sheet = tableprims::documents::RESERVED_SHEET,
        format_scale = write::MAX_FORMAT_SCALE,
        digits = tableprims::binary64::MAX_SIGNIFICANT_DIGITS,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_profile_definition_is_pinned() {
        assert_eq!(
            super::profile_definition(),
            concat!(
                r#"{"backend":{"features":["constant_memory"],"name":"rust_xlsxwriter","version":"0.102.0"},"#,
                r#""format":"tableprims.profile/0","id":"xlsx-write/r1","#,
                r#""limits":{"default_max_line_bytes":1048576,"max_cell_units":32767,"max_columns":16384,"#,
                r#""max_empty_ranges":65536,"max_line_bytes":16777216,"max_rows":1048575},"#,
                r#""metadata":{"header_rows":1,"layout":"1","ranges_per_record":512,"sheet":"_tableprims"},"#,
                r#""native":{"max_format_scale":30,"max_significant_digits":15}}"#,
            )
        );
    }
}
