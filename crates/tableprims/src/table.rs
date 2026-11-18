// SPDX-License-Identifier: MIT OR Apache-2.0

//! Typed schemas, values and rows.
//!
//! A [`Schema`] is an ordered list of columns with stable identifiers. A row
//! arrives as one [`Scalar`] per column, in schema order, and is checked into
//! typed [`Value`]s by [`Schema::check_row`]. The scalar spelling never decides
//! the type: the schema does. Exact numbers arrive as text and are parsed
//! with the grammar in [`crate::exact`]; a string column keeps any text
//! unchanged, including text that looks like a number. Dates and timestamps
//! arrive as text and are parsed with the grammar in [`crate::temporal`].
//!
//! The set of logical types is provisional and lists only what is
//! implemented.

use core::fmt;
use std::collections::HashSet;

use crate::exact::{Decimal, DecimalType, ExactError, Integer};
use crate::temporal::{Date, FractionPrecision, LocalTimestamp, OffsetTimestamp, TemporalError};

/// Logical type of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LogicalType {
    /// True or false.
    Boolean,
    /// Unicode text, carried unchanged.
    String,
    /// Exact integer.
    Integer,
    /// Exact decimal with declared precision and scale.
    Decimal(DecimalType),
    /// Calendar date.
    Date,
    /// Date and time of day with no offset, at a declared fractional
    /// precision.
    LocalTimestamp(FractionPrecision),
    /// Date and time of day with a fixed offset from UTC, at a declared
    /// fractional precision.
    OffsetTimestamp(FractionPrecision),
}

/// One column of a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    id: String,
    label: String,
    logical_type: LogicalType,
    nullable: bool,
}

impl Column {
    /// Creates a column. `id` identifies the column; `label` is display text.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        logical_type: LogicalType,
        nullable: bool,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            logical_type,
            nullable,
        }
    }

    /// Stable identifier, unique within a schema.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Display label. Labels may repeat within a schema.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Declared logical type.
    #[must_use]
    pub fn logical_type(&self) -> LogicalType {
        self.logical_type
    }

    /// Whether null is an accepted value.
    #[must_use]
    pub fn nullable(&self) -> bool {
        self.nullable
    }
}

/// Why a schema was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SchemaError {
    /// The schema has no columns.
    NoColumns,
    /// The column at this zero-based position has an empty identifier.
    EmptyColumnId {
        /// Zero-based column position.
        position: usize,
    },
    /// The column at this zero-based position repeats an earlier identifier.
    DuplicateColumnId {
        /// Zero-based column position of the repeat.
        position: usize,
    },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoColumns => f.write_str("schema has no columns"),
            Self::EmptyColumnId { position } => {
                write!(f, "column {position} has an empty identifier")
            }
            Self::DuplicateColumnId { position } => {
                write!(f, "column {position} repeats an earlier identifier")
            }
        }
    }
}

impl std::error::Error for SchemaError {}

/// An ordered list of columns with unique identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    columns: Vec<Column>,
}

impl Schema {
    /// Creates a schema.
    ///
    /// # Errors
    ///
    /// Refuses an empty column list, an empty identifier and a repeated
    /// identifier.
    pub fn new(columns: Vec<Column>) -> Result<Self, SchemaError> {
        if columns.is_empty() {
            return Err(SchemaError::NoColumns);
        }
        let mut seen = HashSet::with_capacity(columns.len());
        for (position, column) in columns.iter().enumerate() {
            if column.id.is_empty() {
                return Err(SchemaError::EmptyColumnId { position });
            }
            if !seen.insert(column.id.as_str()) {
                return Err(SchemaError::DuplicateColumnId { position });
            }
        }
        Ok(Self { columns })
    }

    /// Columns in order.
    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// Checks one row against the schema and returns its typed values.
    ///
    /// `row` is the zero-based logical row index; it is used only to locate
    /// an error. `cells` must hold exactly one scalar per column.
    ///
    /// # Errors
    ///
    /// Refuses a row of the wrong length, a null in a non-nullable column, a
    /// scalar of the wrong kind for the column, and exact-number, date or
    /// timestamp text outside the declared grammar or bounds. Row length is checked before any cell:
    /// a short row names its first missing column and a long row names none.
    /// Otherwise the error names the first offending column in schema order.
    /// An error never contains cell content.
    pub fn check_row(&self, row: u64, cells: &[Scalar<'_>]) -> Result<Vec<Value>, RowError> {
        if cells.len() != self.columns.len() {
            // A short row is located at the first missing column; a long row
            // has no column to name.
            let column = self.columns.get(cells.len()).map(|c| c.id.clone());
            return Err(RowError {
                row,
                column,
                rule: RowRule::ArityMismatch {
                    expected: self.columns.len(),
                    found: cells.len(),
                },
            });
        }
        let mut values = Vec::with_capacity(cells.len());
        for (column, cell) in self.columns.iter().zip(cells) {
            let value = check_cell(column, *cell).map_err(|rule| RowError {
                row,
                column: Some(column.id.clone()),
                rule,
            })?;
            values.push(value);
        }
        Ok(values)
    }
}

fn check_cell(column: &Column, cell: Scalar<'_>) -> Result<Value, RowRule> {
    match (column.logical_type, cell) {
        (_, Scalar::Null) => {
            if column.nullable {
                Ok(Value::Null)
            } else {
                Err(RowRule::NullNotAllowed)
            }
        }
        (LogicalType::Boolean, Scalar::Boolean(value)) => Ok(Value::Boolean(value)),
        (LogicalType::String, Scalar::Text(text)) => Ok(Value::String(text.to_owned())),
        (LogicalType::Integer, Scalar::Text(text)) => Integer::parse(text)
            .map(Value::Integer)
            .map_err(RowRule::InvalidExactNumber),
        (LogicalType::Decimal(ty), Scalar::Text(text)) => Decimal::parse(text, ty)
            .map(Value::Decimal)
            .map_err(RowRule::InvalidExactNumber),
        (LogicalType::Date, Scalar::Text(text)) => Date::parse(text)
            .map(Value::Date)
            .map_err(RowRule::InvalidTemporal),
        (LogicalType::LocalTimestamp(precision), Scalar::Text(text)) => {
            LocalTimestamp::parse(text, precision)
                .map(Value::LocalTimestamp)
                .map_err(RowRule::InvalidTemporal)
        }
        (LogicalType::OffsetTimestamp(precision), Scalar::Text(text)) => {
            OffsetTimestamp::parse(text, precision)
                .map(Value::OffsetTimestamp)
                .map_err(RowRule::InvalidTemporal)
        }
        (LogicalType::Boolean, Scalar::Text(_))
        | (
            LogicalType::String
            | LogicalType::Integer
            | LogicalType::Decimal(_)
            | LogicalType::Date
            | LogicalType::LocalTimestamp(_)
            | LogicalType::OffsetTimestamp(_),
            Scalar::Boolean(_),
        ) => Err(RowRule::WrongScalarKind),
    }
}

/// An untyped scalar as it arrives from a row transport.
///
/// Exact numbers arrive as [`Scalar::Text`]. There is no numeric variant: a
/// transport that sees a bare numeric literal has nothing to pass and must
/// refuse it itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scalar<'a> {
    /// Null.
    Null,
    /// True or false.
    Boolean(bool),
    /// Text.
    Text(&'a str),
}

/// A typed cell value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Value {
    /// Null. Distinct from an empty string.
    Null,
    /// True or false.
    Boolean(bool),
    /// Text, unchanged.
    String(String),
    /// Exact integer.
    Integer(Integer),
    /// Exact decimal at its declared scale.
    Decimal(Decimal),
    /// Calendar date.
    Date(Date),
    /// Date and time of day with no offset.
    LocalTimestamp(LocalTimestamp),
    /// Date and time of day with a fixed offset, as written.
    OffsetTimestamp(OffsetTimestamp),
}

/// The rule a row broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RowRule {
    /// The row does not have one cell per column.
    ArityMismatch {
        /// Number of columns in the schema.
        expected: usize,
        /// Number of cells in the row.
        found: usize,
    },
    /// Null in a column that does not accept it.
    NullNotAllowed,
    /// The scalar kind cannot be a value of the column's type.
    WrongScalarKind,
    /// Text in an exact-number column is outside the grammar or bounds.
    InvalidExactNumber(ExactError),
    /// Text in a date or timestamp column is outside the grammar or the
    /// calendar.
    InvalidTemporal(TemporalError),
}

/// A refused row, located by row index and column identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowError {
    row: u64,
    column: Option<String>,
    rule: RowRule,
}

impl RowError {
    /// Zero-based logical row index.
    #[must_use]
    pub fn row(&self) -> u64 {
        self.row
    }

    /// Identifier of the offending column. Absent only when the row has more
    /// cells than the schema has columns.
    #[must_use]
    pub fn column(&self) -> Option<&str> {
        self.column.as_deref()
    }

    /// The rule that was broken.
    #[must_use]
    pub fn rule(&self) -> RowRule {
        self.rule
    }
}

impl fmt::Display for RowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "row {}", self.row)?;
        if let Some(column) = &self.column {
            write!(f, ", column {column:?}")?;
        }
        match self.rule {
            RowRule::ArityMismatch { expected, found } => {
                write!(f, ": expected {expected} cells, found {found}")
            }
            RowRule::NullNotAllowed => f.write_str(": null is not allowed"),
            RowRule::WrongScalarKind => f.write_str(": value kind does not match the column type"),
            RowRule::InvalidExactNumber(err) => write!(f, ": {err}"),
            RowRule::InvalidTemporal(err) => write!(f, ": {err}"),
        }
    }
}

impl std::error::Error for RowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.rule {
            RowRule::InvalidExactNumber(err) => Some(err),
            RowRule::InvalidTemporal(err) => Some(err),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Schema {
        Schema::new(vec![
            Column::new("id", "Id", LogicalType::Integer, false),
            Column::new(
                "amount",
                "Amount",
                LogicalType::Decimal(DecimalType::new(7, 4).unwrap()),
                true,
            ),
            Column::new("code", "Code", LogicalType::String, true),
            Column::new("active", "Active", LogicalType::Boolean, false),
        ])
        .unwrap()
    }

    #[test]
    fn schema_refuses_empty_and_repeated_identifiers() {
        assert_eq!(Schema::new(vec![]), Err(SchemaError::NoColumns));
        let empty = Column::new("", "x", LogicalType::String, true);
        assert_eq!(
            Schema::new(vec![empty]),
            Err(SchemaError::EmptyColumnId { position: 0 })
        );
        let a = Column::new("a", "A", LogicalType::String, true);
        let again = Column::new("a", "Other", LogicalType::Integer, false);
        assert_eq!(
            Schema::new(vec![a, again]),
            Err(SchemaError::DuplicateColumnId { position: 1 })
        );
    }

    #[test]
    fn labels_may_repeat() {
        let a = Column::new("a", "Total", LogicalType::Integer, false);
        let b = Column::new("b", "Total", LogicalType::Integer, false);
        assert!(Schema::new(vec![a, b]).is_ok());
    }

    #[test]
    fn typed_row_is_accepted() {
        let row = schema()
            .check_row(
                0,
                &[
                    Scalar::Text("42"),
                    Scalar::Text("123.4500"),
                    Scalar::Text("000123"),
                    Scalar::Boolean(true),
                ],
            )
            .unwrap();
        assert_eq!(row[0], Value::Integer(Integer::parse("42").unwrap()));
        match &row[1] {
            Value::Decimal(d) => assert_eq!(d.to_string(), "123.4500"),
            other => panic!("unexpected {other:?}"),
        }
        // A string column keeps number-looking text unchanged.
        assert_eq!(row[2], Value::String("000123".to_owned()));
        assert_eq!(row[3], Value::Boolean(true));
    }

    #[test]
    fn null_and_empty_string_stay_distinct() {
        let s = schema();
        let null = s
            .check_row(
                0,
                &[
                    Scalar::Text("1"),
                    Scalar::Null,
                    Scalar::Null,
                    Scalar::Boolean(false),
                ],
            )
            .unwrap();
        let empty = s
            .check_row(
                1,
                &[
                    Scalar::Text("1"),
                    Scalar::Null,
                    Scalar::Text(""),
                    Scalar::Boolean(false),
                ],
            )
            .unwrap();
        assert_eq!(null[2], Value::Null);
        assert_eq!(empty[2], Value::String(String::new()));
        assert_ne!(null[2], empty[2]);
    }

    #[test]
    fn null_in_required_column_is_located() {
        let err = schema()
            .check_row(
                7,
                &[
                    Scalar::Null,
                    Scalar::Null,
                    Scalar::Null,
                    Scalar::Boolean(true),
                ],
            )
            .unwrap_err();
        assert_eq!(err.row(), 7);
        assert_eq!(err.column(), Some("id"));
        assert_eq!(err.rule(), RowRule::NullNotAllowed);
    }

    #[test]
    fn short_row_names_the_first_missing_column() {
        let err = schema()
            .check_row(3, &[Scalar::Text("1"), Scalar::Null])
            .unwrap_err();
        assert_eq!(err.column(), Some("code"));
        assert_eq!(
            err.rule(),
            RowRule::ArityMismatch {
                expected: 4,
                found: 2
            }
        );
    }

    #[test]
    fn long_row_is_refused_without_a_column() {
        let cells = [Scalar::Null; 5];
        let err = schema().check_row(0, &cells).unwrap_err();
        assert_eq!(err.column(), None);
        assert_eq!(
            err.rule(),
            RowRule::ArityMismatch {
                expected: 4,
                found: 5
            }
        );
    }

    #[test]
    fn scalar_kind_must_fit_the_column() {
        let s = schema();
        let bool_in_integer = s
            .check_row(
                0,
                &[
                    Scalar::Boolean(true),
                    Scalar::Null,
                    Scalar::Null,
                    Scalar::Boolean(true),
                ],
            )
            .unwrap_err();
        assert_eq!(bool_in_integer.rule(), RowRule::WrongScalarKind);
        let text_in_boolean = s
            .check_row(
                0,
                &[
                    Scalar::Text("1"),
                    Scalar::Null,
                    Scalar::Null,
                    Scalar::Text("true"),
                ],
            )
            .unwrap_err();
        assert_eq!(text_in_boolean.column(), Some("active"));
        assert_eq!(text_in_boolean.rule(), RowRule::WrongScalarKind);
        let bool_in_string = s
            .check_row(
                0,
                &[
                    Scalar::Text("1"),
                    Scalar::Null,
                    Scalar::Boolean(true),
                    Scalar::Boolean(true),
                ],
            )
            .unwrap_err();
        assert_eq!(bool_in_string.column(), Some("code"));
    }

    #[test]
    fn exact_number_errors_carry_the_cause_and_first_column() {
        let s = schema();
        let err = s
            .check_row(
                2,
                &[
                    Scalar::Text("000123"),
                    Scalar::Text("1.5"),
                    Scalar::Null,
                    Scalar::Boolean(true),
                ],
            )
            .unwrap_err();
        assert_eq!(err.column(), Some("id"));
        assert_eq!(
            err.rule(),
            RowRule::InvalidExactNumber(ExactError::LeadingZero)
        );
        let err = s
            .check_row(
                2,
                &[
                    Scalar::Text("1"),
                    Scalar::Text("1.5"),
                    Scalar::Null,
                    Scalar::Boolean(true),
                ],
            )
            .unwrap_err();
        assert_eq!(err.column(), Some("amount"));
        assert_eq!(
            err.rule(),
            RowRule::InvalidExactNumber(ExactError::ScaleMismatch)
        );
    }

    #[test]
    fn temporal_columns_are_checked_and_located() {
        let ms = FractionPrecision::new(3).unwrap();
        let s = Schema::new(vec![
            Column::new("day", "Day", LogicalType::Date, false),
            Column::new("seen", "Seen", LogicalType::LocalTimestamp(ms), true),
            Column::new("sent", "Sent", LogicalType::OffsetTimestamp(ms), true),
        ])
        .unwrap();
        let row = s
            .check_row(
                0,
                &[
                    Scalar::Text("2024-02-29"),
                    Scalar::Text("2024-02-29T12:00:00.000"),
                    Scalar::Text("2024-02-29T12:00:00.250+05:30"),
                ],
            )
            .unwrap();
        match &row[2] {
            Value::OffsetTimestamp(v) => {
                assert_eq!(v.to_string(), "2024-02-29T12:00:00.250+05:30");
            }
            other => panic!("unexpected {other:?}"),
        }
        let err = s
            .check_row(
                4,
                &[
                    Scalar::Text("2024-02-29"),
                    Scalar::Null,
                    Scalar::Text("2024-02-29T12:00:00.250"),
                ],
            )
            .unwrap_err();
        assert_eq!((err.row(), err.column()), (4, Some("sent")));
        assert_eq!(
            err.rule(),
            RowRule::InvalidTemporal(TemporalError::InvalidOffset)
        );
        let err = s
            .check_row(0, &[Scalar::Boolean(true), Scalar::Null, Scalar::Null])
            .unwrap_err();
        assert_eq!(err.rule(), RowRule::WrongScalarKind);
        let err = s
            .check_row(0, &[Scalar::Text("2023-02-29"), Scalar::Null, Scalar::Null])
            .unwrap_err();
        assert_eq!(
            err.rule(),
            RowRule::InvalidTemporal(TemporalError::InvalidDate)
        );
    }

    #[test]
    fn messages_do_not_contain_cell_content() {
        let err = schema()
            .check_row(
                0,
                &[
                    Scalar::Text("secret-9999"),
                    Scalar::Null,
                    Scalar::Null,
                    Scalar::Boolean(true),
                ],
            )
            .unwrap_err();
        assert!(!err.to_string().contains("secret"));
    }
}
