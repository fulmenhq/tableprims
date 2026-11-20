// SPDX-License-Identifier: MIT OR Apache-2.0
//! Planning and the validating first read.

use std::hash::{DefaultHasher, Hasher};
use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};

use tableprims::binary64;
use tableprims::documents::{ExactNumberPolicy, Policy, RESERVED_SHEET};
use tableprims::exact::{DecimalType, RoundingMode};
use tableprims::ndjson::{Cell, ReadError, RowReader};
use tableprims::table::{LogicalType, Scalar, Schema, Value};

use crate::error::{ErrorKind, WriteError};
use crate::metadata;
use crate::{
    DEFAULT_MAX_LINE_BYTES, MAX_CELL_UNITS, MAX_COLUMNS, MAX_EMPTY_RANGES, MAX_LINE_BYTES, MAX_ROWS,
};

/// How a column's cells are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Representation {
    /// Numeric cells for an exact-number column, Boolean cells for a
    /// boolean column.
    Native,
    /// An exact-number column written as text because a value could not be
    /// stored as a numeric cell.
    TextFallback,
    /// An exact-number column written as text because the policy says so.
    TextForced,
    /// A string column, or dates and timestamps in their canonical text.
    Text,
}

impl Representation {
    /// The representation's stable name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::TextFallback => "text-fallback",
            Self::TextForced => "text-forced",
            Self::Text => "text",
        }
    }
}

/// The limits a write runs under: the policy's, or this profile's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedLimits {
    /// Largest number of data rows.
    pub max_rows: u64,
    /// Largest length of one input line in bytes.
    pub max_line_bytes: u64,
    /// Largest number of empty-string ranges.
    pub max_empty_ranges: u64,
}

/// A schema and policy checked against this profile.
#[derive(Debug, Clone)]
pub struct Plan {
    pub(crate) schema: Schema,
    pub(crate) sheet: String,
    pub(crate) exact: ExactNumberPolicy,
    pub(crate) quantize: Vec<Option<(DecimalType, RoundingMode)>>,
    pub(crate) limits: ResolvedLimits,
}

/// Length of a string in UTF-16 code units.
fn utf16_units(text: &str) -> usize {
    if text.is_ascii() {
        text.len()
    } else {
        text.encode_utf16().count()
    }
}

pub(crate) fn fits_cell(text: &str) -> bool {
    text.len() <= MAX_CELL_UNITS || utf16_units(text) <= MAX_CELL_UNITS
}

fn resolve(
    requested: Option<u64>,
    default: u64,
    ceiling: u64,
    name: &'static str,
) -> Result<u64, WriteError> {
    match requested {
        None => Ok(default),
        Some(value) if value <= ceiling => Ok(value),
        Some(_) => Err(WriteError::new(ErrorKind::LimitAboveCeiling(name))),
    }
}

impl Plan {
    /// Checks a schema and policy against this profile.
    ///
    /// # Errors
    ///
    /// Refuses a schema with more columns than a worksheet, a column
    /// identifier or label longer than a cell, a logical type or rounding
    /// mode this profile does not know, a policy that names no worksheet, a
    /// worksheet name a workbook does not allow, a quantize request that
    /// does not fit the schema and a limit above this profile's ceiling.
    pub fn new(schema: &Schema, policy: &Policy) -> Result<Self, WriteError> {
        let columns = schema.columns();
        if columns.len() > MAX_COLUMNS {
            return Err(WriteError::new(ErrorKind::TooManyColumns));
        }
        for column in columns {
            if !fits_cell(column.id()) || !fits_cell(column.label()) {
                return Err(WriteError::at_column(ErrorKind::CellTooLong, column.id()));
            }
        }
        for column in columns {
            if metadata::type_name(column.logical_type()).is_none() {
                return Err(WriteError::at_column(
                    ErrorKind::UnsupportedType,
                    column.id(),
                ));
            }
        }
        let Some(sheet) = policy.sheet() else {
            return Err(WriteError::new(ErrorKind::SheetNameMissing));
        };
        if sheet.eq_ignore_ascii_case(RESERVED_SHEET) {
            return Err(WriteError::new(ErrorKind::SheetNameReserved));
        }
        if rust_xlsxwriter::Worksheet::new().set_name(sheet).is_err() {
            return Err(WriteError::new(ErrorKind::SheetNameUnsupported));
        }
        let quantize = policy
            .quantize_plan(schema)
            .map_err(|error| WriteError::new(ErrorKind::Policy(error)))?;
        for (column, request) in columns.iter().zip(&quantize) {
            if matches!(request, Some((_, mode)) if metadata::rounding_name(*mode).is_none()) {
                return Err(WriteError::at_column(
                    ErrorKind::UnsupportedRounding,
                    column.id(),
                ));
            }
        }
        let requested = policy.limits();
        let limits = ResolvedLimits {
            max_rows: resolve(requested.max_rows, MAX_ROWS, MAX_ROWS, "max_rows")?,
            max_line_bytes: resolve(
                requested.max_line_bytes,
                DEFAULT_MAX_LINE_BYTES,
                MAX_LINE_BYTES,
                "max_line_bytes",
            )?,
            max_empty_ranges: resolve(
                requested.max_empty_ranges,
                MAX_EMPTY_RANGES,
                MAX_EMPTY_RANGES,
                "max_empty_ranges",
            )?,
        };
        Ok(Self {
            schema: schema.clone(),
            sheet: sheet.to_owned(),
            exact: policy.exact_numbers(),
            quantize,
            limits,
        })
    }

    /// The limits a write under this plan runs under.
    #[must_use]
    pub fn limits(&self) -> ResolvedLimits {
        self.limits
    }

    /// The name of the data worksheet.
    #[must_use]
    pub fn sheet(&self) -> &str {
        &self.sheet
    }

    /// How exact numbers are represented.
    #[must_use]
    pub fn exact_numbers(&self) -> ExactNumberPolicy {
        self.exact
    }

    /// For each schema column in order, the decimal type it is quantized to
    /// and the rounding mode, or `None` for a column that is not quantized.
    #[must_use]
    pub fn quantize(&self) -> &[Option<(DecimalType, RoundingMode)>] {
        &self.quantize
    }

    /// The schema the plan was made for.
    #[must_use]
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Reads the input once, validates every row and decides how each
    /// column is stored. Nothing is written.
    ///
    /// `cancel` is checked before each row; setting it stops the read.
    ///
    /// # Errors
    ///
    /// Refuses the first row that is malformed, does not fit the schema or
    /// exceeds a limit, and, when the policy requires numeric cells, the
    /// first exact number that cannot be stored as one.
    pub fn scan<R: BufRead>(self, input: R, cancel: &AtomicBool) -> Result<Scan, WriteError> {
        let tally = pass(&self, input, cancel, None, |_, _| Ok(()))?;
        let representations = self
            .schema
            .columns()
            .iter()
            .zip(&tally.columns)
            .map(|(column, seen)| match column.logical_type() {
                LogicalType::Boolean => Representation::Native,
                LogicalType::Integer | LogicalType::Decimal(_) => match self.exact {
                    ExactNumberPolicy::TextAlways => Representation::TextForced,
                    _ if seen.needs_text => Representation::TextFallback,
                    _ => Representation::Native,
                },
                _ => Representation::Text,
            })
            .collect();
        Ok(Scan {
            plan: self,
            tally,
            representations,
        })
    }
}

/// What one read of the input observed about a column.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ColumnTally {
    pub(crate) null: u64,
    pub(crate) empty: u64,
    pub(crate) quantized: u64,
    pub(crate) scale_changed: u64,
    pub(crate) needs_text: bool,
    /// Inclusive, ascending, merged ranges of rows holding an empty string.
    pub(crate) ranges: Vec<(u64, u64)>,
}

/// What one read of the input observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tally {
    pub(crate) rows: u64,
    /// A fingerprint of every cell read, in order. It tells two reads in one
    /// process apart; it is not a digest.
    pub(crate) fingerprint: u64,
    pub(crate) range_count: u64,
    pub(crate) columns: Vec<ColumnTally>,
}

/// One cell, ready to write.
pub(crate) enum Prepared {
    Blank,
    Boolean(bool),
    Text(String),
    Number(f64),
}

/// Reads every row, updates the tally and hands each prepared row to `emit`.
///
/// On the first read `frozen` is `None` and the policy decides what happens
/// to an exact number that cannot be a numeric cell. On the second read
/// `frozen` holds the decided representations and such a value in a native
/// column means the input changed.
pub(crate) fn pass<R: BufRead>(
    plan: &Plan,
    input: R,
    cancel: &AtomicBool,
    frozen: Option<&[Representation]>,
    mut emit: impl FnMut(u64, Vec<Prepared>) -> Result<(), WriteError>,
) -> Result<Tally, WriteError> {
    let columns = plan.schema.columns();
    let mut fingerprint = DefaultHasher::new();
    let mut tally = Tally {
        rows: 0,
        fingerprint: 0,
        range_count: 0,
        columns: vec![ColumnTally::default(); columns.len()],
    };
    let max_line = usize::try_from(plan.limits.max_line_bytes).unwrap_or(usize::MAX);
    for item in RowReader::new(input, max_line) {
        if cancel.load(Ordering::Relaxed) {
            return Err(WriteError::new(ErrorKind::Cancelled));
        }
        let (row, cells): (u64, Vec<Cell>) = item.map_err(read_error)?;
        if row >= plan.limits.max_rows {
            return Err(WriteError::at_row(ErrorKind::TooManyRows, row));
        }
        let scalars: Vec<Scalar<'_>> = cells.iter().map(Cell::as_scalar).collect();
        fingerprint.write_usize(scalars.len());
        for scalar in &scalars {
            match scalar {
                Scalar::Null => fingerprint.write_u8(0),
                Scalar::Boolean(flag) => fingerprint.write_u8(1 + u8::from(*flag)),
                Scalar::Text(text) => {
                    fingerprint.write_u8(3);
                    fingerprint.write_usize(text.len());
                    fingerprint.write(text.as_bytes());
                }
            }
        }
        let values = plan.schema.check_row(row, &scalars).map_err(|error| {
            let kind = ErrorKind::Row(error.rule());
            match error.column() {
                Some(column) => WriteError::at_cell(kind, row, column),
                None => WriteError::at_row(kind, row),
            }
        })?;
        let mut prepared = Vec::with_capacity(values.len());
        for (at, value) in values.into_iter().enumerate() {
            let id = columns[at].id();
            let seen = &mut tally.columns[at];
            let exact = match value {
                Value::Null => {
                    seen.null += 1;
                    prepared.push(Prepared::Blank);
                    continue;
                }
                Value::Boolean(flag) => {
                    prepared.push(Prepared::Boolean(flag));
                    continue;
                }
                Value::String(text) => {
                    if text.is_empty() {
                        seen.empty += 1;
                        match seen.ranges.last_mut() {
                            Some(last) if last.1 + 1 == row => last.1 = row,
                            _ => {
                                tally.range_count += 1;
                                if tally.range_count > plan.limits.max_empty_ranges {
                                    return Err(WriteError::at_cell(
                                        ErrorKind::TooManyEmptyRanges,
                                        row,
                                        id,
                                    ));
                                }
                                seen.ranges.push((row, row));
                            }
                        }
                        prepared.push(Prepared::Blank);
                    } else if fits_cell(&text) {
                        prepared.push(Prepared::Text(text));
                    } else {
                        return Err(WriteError::at_cell(ErrorKind::CellTooLong, row, id));
                    }
                    continue;
                }
                Value::Date(date) => {
                    prepared.push(Prepared::Text(date.to_string()));
                    continue;
                }
                Value::LocalTimestamp(stamp) => {
                    prepared.push(Prepared::Text(stamp.to_string()));
                    continue;
                }
                Value::OffsetTimestamp(stamp) => {
                    prepared.push(Prepared::Text(stamp.to_string()));
                    continue;
                }
                Value::Integer(integer) => (integer.to_string(), binary64::from_integer(integer)),
                Value::Decimal(mut decimal) => {
                    if let Some((target, mode)) = plan.quantize[at] {
                        let quantized = decimal.quantize(target, mode).map_err(|_| {
                            WriteError::at_cell(ErrorKind::QuantizeOverflow, row, id)
                        })?;
                        seen.quantized += u64::from(quantized.value_changed);
                        seen.scale_changed += u64::from(quantized.scale_changed);
                        decimal = quantized.value;
                    }
                    (decimal.to_string(), binary64::from_decimal(decimal))
                }
                // The plan refused every logical type this profile does not know.
                _ => return Err(WriteError::at_cell(ErrorKind::UnsupportedType, row, id)),
            };
            let (text, native) = exact;
            if plan.exact == ExactNumberPolicy::TextAlways {
                prepared.push(Prepared::Text(text));
                continue;
            }
            match (native, frozen.map(|all| all[at])) {
                (Ok(number), None | Some(Representation::Native)) => {
                    prepared.push(Prepared::Number(number));
                }
                (Ok(_), Some(_)) => prepared.push(Prepared::Text(text)),
                (Err(error), None) => {
                    if plan.exact == ExactNumberPolicy::RequireExact {
                        return Err(WriteError::at_cell(ErrorKind::NotNative(error), row, id));
                    }
                    seen.needs_text = true;
                    prepared.push(Prepared::Text(text));
                }
                (Err(_), Some(Representation::Native)) => {
                    return Err(WriteError::at_cell(ErrorKind::InputChanged, row, id));
                }
                (Err(_), Some(_)) => {
                    seen.needs_text = true;
                    prepared.push(Prepared::Text(text));
                }
            }
        }
        emit(row, prepared)?;
        tally.rows += 1;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(WriteError::new(ErrorKind::Cancelled));
    }
    tally.fingerprint = fingerprint.finish();
    Ok(tally)
}

fn read_error(error: ReadError) -> WriteError {
    match error {
        ReadError::Io(error) => WriteError::new(ErrorKind::Read(error)),
        ReadError::ByteOrderMark => WriteError::new(ErrorKind::ByteOrderMark),
        ReadError::InvalidUtf8 { row } => WriteError::at_row(ErrorKind::InvalidUtf8, row),
        ReadError::LineTooLong { row } => WriteError::at_row(ErrorKind::LineTooLong, row),
        ReadError::Line { row, error } => WriteError::at_row(ErrorKind::Line(error), row),
        _ => WriteError::new(ErrorKind::Backend(String::from("unrecognized read error"))),
    }
}

/// A validated input and the representation decided for each column.
#[derive(Debug, Clone)]
pub struct Scan {
    pub(crate) plan: Plan,
    pub(crate) tally: Tally,
    pub(crate) representations: Vec<Representation>,
}

impl Scan {
    /// The plan this scan ran under.
    #[must_use]
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// Number of data rows.
    #[must_use]
    pub fn rows(&self) -> u64 {
        self.tally.rows
    }

    /// The representation decided for each column, in schema order.
    #[must_use]
    pub fn representations(&self) -> &[Representation] {
        &self.representations
    }

    pub(crate) fn outcome(&self) -> Outcome {
        let columns = self
            .plan
            .schema
            .columns()
            .iter()
            .zip(&self.tally.columns)
            .zip(&self.representations)
            .map(|((column, seen), &representation)| {
                let exact = matches!(
                    column.logical_type(),
                    LogicalType::Integer | LogicalType::Decimal(_)
                );
                let cells = self.tally.rows - seen.null;
                let count = |wanted| u64::from(exact && representation == wanted) * cells;
                ColumnOutcome {
                    id: column.id().to_owned(),
                    representation,
                    exact_number: exact,
                    null: seen.null,
                    empty: seen.empty,
                    numeric: count(Representation::Native),
                    text_fallback: count(Representation::TextFallback),
                    text_forced: count(Representation::TextForced),
                    quantized: seen.quantized,
                    scale_changed: seen.scale_changed,
                }
            })
            .collect();
        Outcome {
            rows: self.tally.rows,
            columns,
        }
    }
}

/// What a completed write stored, per column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnOutcome {
    /// The column identifier.
    pub id: String,
    /// How the column's cells are stored.
    pub representation: Representation,
    /// Whether the column is an integer or decimal column. The five
    /// counters after `empty` apply only to such a column.
    pub exact_number: bool,
    /// Null cells.
    pub null: u64,
    /// Empty-string cells.
    pub empty: u64,
    /// Exact numbers stored as numeric cells.
    pub numeric: u64,
    /// Exact numbers stored as text because the column fell back to text.
    pub text_fallback: u64,
    /// Exact numbers stored as text because the policy says so.
    pub text_forced: u64,
    /// Cells whose value changed when quantized.
    pub quantized: u64,
    /// Cells whose scale changed when quantized.
    pub scale_changed: u64,
}

/// What a completed write stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Number of data rows.
    pub rows: u64,
    /// One entry per column, in schema order.
    pub columns: Vec<ColumnOutcome>,
}
