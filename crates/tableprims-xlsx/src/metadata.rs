// SPDX-License-Identifier: MIT OR Apache-2.0
//! The records written to the hidden `_tableprims` worksheet.
//!
//! Every cell on the worksheet is a string cell. Each row is one record;
//! column A holds the record kind and the remaining cells are positional.
//! A field that does not apply is left blank.
//!
//! | Kind | Fields |
//! | --- | --- |
//! | `format` | layout revision, family, write profile |
//! | `sheet` | data worksheet name, header rows, data rows, columns |
//! | `column` | zero-based index, identifier, logical type, precision, scale, nullable, fraction digits, representation, source scale, rounding mode |
//! | `empty` | zero-based column index, row ranges |
//! | `end` | number of records before this one |
//!
//! Records appear in that order: one `format`, one `sheet`, one `column`
//! per schema column in schema order, any `empty` records in ascending
//! column order, then `end`.
//!
//! A `column` record is the authority for the column's type and scale. For
//! a quantized decimal column, `scale` is the scale written and `source
//! scale` and `rounding mode` record the declared scale and the rounding
//! applied.
//!
//! An `empty` record lists the data rows whose cell in that column holds an
//! empty string, as comma-separated inclusive ranges `first-last` of
//! zero-based data row indices, ascending and merged. A column with many
//! ranges has several `empty` records. A blank data cell that no `empty`
//! record lists is a null.

use tableprims::exact::RoundingMode;
use tableprims::table::LogicalType;

use crate::plan::Scan;
use crate::PROFILE_ID;

/// Name of the hidden worksheet that holds the records. It is reserved: a
/// policy may not name it as the data worksheet in any letter case, judged
/// by Unicode case mapping.
pub const WORKSHEET: &str = "_tableprims";
/// Revision of the record layout.
pub const LAYOUT: &str = "1";
/// Family name written in the `format` record.
pub const FAMILY: &str = "tableprims";
/// Number of header rows on the data worksheet.
pub const HEADER_ROWS: u32 = 1;
/// Largest number of ranges in one `empty` record.
pub const RANGES_PER_RECORD: usize = 512;

/// The name of a rounding mode, or `None` for one this profile does not know.
#[must_use]
pub fn rounding_name(mode: RoundingMode) -> Option<&'static str> {
    Some(match mode {
        RoundingMode::HalfEven => "half-even",
        RoundingMode::HalfAwayFromZero => "half-away-from-zero",
        RoundingMode::TowardZero => "toward-zero",
        RoundingMode::TowardPositive => "toward-positive",
        RoundingMode::TowardNegative => "toward-negative",
        _ => return None,
    })
}

/// The name of a logical type, or `None` for one this profile does not know.
pub(crate) fn type_name(logical_type: LogicalType) -> Option<&'static str> {
    Some(match logical_type {
        LogicalType::Boolean => "boolean",
        LogicalType::String => "string",
        LogicalType::Integer => "integer",
        LogicalType::Decimal(_) => "decimal",
        LogicalType::Date => "date",
        LogicalType::LocalTimestamp(_) => "local-timestamp",
        LogicalType::OffsetTimestamp(_) => "offset-timestamp",
        _ => return None,
    })
}

/// Builds the records for a scanned input, one inner vector per row.
pub(crate) fn records(scan: &Scan) -> Vec<Vec<String>> {
    let plan = &scan.plan;
    let columns = plan.schema.columns();
    let mut records = vec![
        vec![
            "format".into(),
            LAYOUT.into(),
            FAMILY.into(),
            PROFILE_ID.into(),
        ],
        vec![
            "sheet".into(),
            plan.sheet.clone(),
            HEADER_ROWS.to_string(),
            scan.tally.rows.to_string(),
            columns.len().to_string(),
        ],
    ];
    for (at, column) in columns.iter().enumerate() {
        // The plan refused any type or rounding mode without a name.
        let name = type_name(column.logical_type()).unwrap_or_default();
        let (mut precision, mut scale, mut fraction) =
            (String::new(), String::new(), String::new());
        match column.logical_type() {
            LogicalType::Decimal(declared) => {
                precision = declared.precision().to_string();
                scale = declared.scale().to_string();
            }
            LogicalType::LocalTimestamp(digits) | LogicalType::OffsetTimestamp(digits) => {
                fraction = digits.digits().to_string();
            }
            _ => {}
        }
        let (mut source_scale, mut rounding) = (String::new(), String::new());
        if let Some((target, mode)) = plan.quantize[at] {
            source_scale = std::mem::replace(&mut scale, target.scale().to_string());
            rounding = rounding_name(mode).unwrap_or_default().to_owned();
        }
        records.push(vec![
            "column".into(),
            at.to_string(),
            column.id().to_owned(),
            name.to_owned(),
            precision,
            scale,
            column.nullable().to_string(),
            fraction,
            scan.representations[at].as_str().to_owned(),
            source_scale,
            rounding,
        ]);
    }
    for (at, seen) in scan.tally.columns.iter().enumerate() {
        for chunk in seen.ranges.chunks(RANGES_PER_RECORD) {
            let ranges: Vec<String> = chunk
                .iter()
                .map(|(first, last)| format!("{first}-{last}"))
                .collect();
            records.push(vec!["empty".into(), at.to_string(), ranges.join(",")]);
        }
    }
    let count = records.len();
    records.push(vec!["end".into(), count.to_string()]);
    records
}
