// SPDX-License-Identifier: MIT OR Apache-2.0
//! Typed write errors with a category and a stable rule identifier.

use core::fmt;
use std::io;

use tableprims::binary64::Binary64Error;
use tableprims::documents::DocumentError;
use tableprims::exact::ExactError;
use tableprims::ndjson::LineError;
use tableprims::table::RowRule;
use tableprims::temporal::TemporalError;

/// The class of a refusal or failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Category {
    /// Malformed bytes, serialization or scalar spelling.
    InvalidInput,
    /// A well-formed value that does not fit the schema.
    SchemaMismatch,
    /// A logical type the destination cannot carry.
    UnsupportedType,
    /// A value or name the destination cannot represent under the policy.
    UnsupportedRepresentation,
    /// A configured limit or a destination capacity was exceeded.
    ResourceLimit,
    /// Reading the input or writing the output failed.
    IoFailure,
    /// The caller asked the operation to stop.
    Cancelled,
    /// An unexpected failure in the adapter or the workbook writer.
    Internal,
}

impl Category {
    /// The category's stable name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid-input",
            Self::SchemaMismatch => "schema-mismatch",
            Self::UnsupportedType => "unsupported-type",
            Self::UnsupportedRepresentation => "unsupported-representation",
            Self::ResourceLimit => "resource-limit",
            Self::IoFailure => "io-failure",
            Self::Cancelled => "cancelled",
            Self::Internal => "internal",
        }
    }
}

/// What went wrong.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The policy does not fit the schema.
    Policy(DocumentError),
    /// A policy limit is above what this profile supports. Names the limit.
    LimitAboveCeiling(&'static str),
    /// A column has a logical type this profile does not know.
    UnsupportedType,
    /// A quantize request uses a rounding mode this profile does not know.
    UnsupportedRounding,
    /// The schema has more columns than a worksheet.
    TooManyColumns,
    /// The policy names no worksheet.
    SheetNameMissing,
    /// The worksheet name is the one reserved for table metadata.
    SheetNameReserved,
    /// The worksheet name is not valid in a workbook.
    SheetNameUnsupported,
    /// A string is longer than one cell can hold.
    CellTooLong,
    /// Reading the input failed.
    Read(io::Error),
    /// The input starts with a byte order mark.
    ByteOrderMark,
    /// A line is not valid UTF-8.
    InvalidUtf8,
    /// A line is longer than the configured limit.
    LineTooLong,
    /// A line is not a row.
    Line(LineError),
    /// A row does not fit the schema.
    Row(RowRule),
    /// A quantized value no longer fits the declared precision.
    QuantizeOverflow,
    /// The input has more rows than the configured limit.
    TooManyRows,
    /// The empty strings need more ranges than the configured limit.
    TooManyEmptyRanges,
    /// An exact number cannot be stored as a numeric cell and the policy
    /// requires one.
    NotNative(Binary64Error),
    /// The second read of the input differs from the validated first read.
    InputChanged,
    /// Writing the workbook to the sink failed.
    Write(io::Error),
    /// The workbook writer failed on its temporary files. Only the kind of
    /// the underlying failure is kept.
    Staging(io::Error),
    /// The workbook writer reported an unexpected error. The text comes
    /// from the writer and may name parts of the workbook.
    Backend(String),
    /// The workbook writer stopped without reporting an error.
    BackendPanic,
    /// The caller asked the operation to stop.
    Cancelled,
}

/// The rule for an I/O failure that is the system running out of storage or
/// memory.
fn exhaustion(error: &io::Error) -> Option<&'static str> {
    match error.kind() {
        io::ErrorKind::StorageFull => Some("storage-full"),
        io::ErrorKind::QuotaExceeded => Some("quota-exceeded"),
        io::ErrorKind::FileTooLarge => Some("file-too-large"),
        io::ErrorKind::OutOfMemory => Some("out-of-memory"),
        _ => None,
    }
}

/// A write error, located at a row and column where one applies.
#[derive(Debug)]
pub struct WriteError {
    kind: ErrorKind,
    row: Option<u64>,
    column: Option<String>,
}

impl WriteError {
    pub(crate) fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            row: None,
            column: None,
        }
    }

    pub(crate) fn at_row(kind: ErrorKind, row: u64) -> Self {
        Self {
            kind,
            row: Some(row),
            column: None,
        }
    }

    pub(crate) fn at_cell(kind: ErrorKind, row: u64, column: &str) -> Self {
        Self {
            kind,
            row: Some(row),
            column: Some(column.to_owned()),
        }
    }

    pub(crate) fn at_column(kind: ErrorKind, column: &str) -> Self {
        Self {
            kind,
            row: None,
            column: Some(column.to_owned()),
        }
    }

    /// What went wrong.
    #[must_use]
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    /// Zero-based index of the input row, when the error has one.
    #[must_use]
    pub fn row(&self) -> Option<u64> {
        self.row
    }

    /// Identifier of the column, when the error has one.
    #[must_use]
    pub fn column(&self) -> Option<&str> {
        self.column.as_deref()
    }

    /// The class of the error.
    #[must_use]
    pub fn category(&self) -> Category {
        self.classify().0
    }

    /// A stable identifier for the rule that refused or failed.
    #[must_use]
    pub fn rule(&self) -> &'static str {
        self.classify().1
    }

    /// Whether the error is the system running out of storage or memory,
    /// as opposed to a configured limit or a capacity of the destination.
    /// Both are in the [`Category::ResourceLimit`] category.
    #[must_use]
    pub fn is_system_exhaustion(&self) -> bool {
        match &self.kind {
            ErrorKind::Read(error) | ErrorKind::Write(error) | ErrorKind::Staging(error) => {
                exhaustion(error).is_some()
            }
            _ => false,
        }
    }

    fn classify(&self) -> (Category, &'static str) {
        use Category as C;
        match &self.kind {
            ErrorKind::Policy(error) => match error {
                DocumentError::QuantizeUnknownColumn => {
                    (C::SchemaMismatch, "quantize-unknown-column")
                }
                DocumentError::QuantizeNotDecimal => (C::SchemaMismatch, "quantize-not-decimal"),
                DocumentError::QuantizeScaleTooLarge => {
                    (C::SchemaMismatch, "quantize-scale-too-large")
                }
                _ => (C::SchemaMismatch, "policy-mismatch"),
            },
            ErrorKind::LimitAboveCeiling(_) => (C::ResourceLimit, "limit-above-profile"),
            ErrorKind::UnsupportedType => (C::UnsupportedType, "unsupported-type"),
            ErrorKind::UnsupportedRounding => (C::UnsupportedType, "unsupported-rounding"),
            ErrorKind::TooManyColumns => (C::ResourceLimit, "too-many-columns"),
            ErrorKind::SheetNameMissing => (C::UnsupportedRepresentation, "sheet-name-missing"),
            ErrorKind::SheetNameReserved => (C::UnsupportedRepresentation, "sheet-name-reserved"),
            ErrorKind::SheetNameUnsupported => {
                (C::UnsupportedRepresentation, "sheet-name-unsupported")
            }
            ErrorKind::CellTooLong => (C::ResourceLimit, "cell-too-long"),
            ErrorKind::Read(error) | ErrorKind::Write(error) | ErrorKind::Staging(error)
                if exhaustion(error).is_some() =>
            {
                (C::ResourceLimit, exhaustion(error).unwrap_or_default())
            }
            ErrorKind::Read(_) => (C::IoFailure, "read-failed"),
            ErrorKind::ByteOrderMark => (C::InvalidInput, "byte-order-mark"),
            ErrorKind::InvalidUtf8 => (C::InvalidInput, "invalid-utf8"),
            ErrorKind::LineTooLong => (C::ResourceLimit, "line-too-long"),
            ErrorKind::Line(error) => (
                C::InvalidInput,
                match error {
                    LineError::Blank => "blank-line",
                    LineError::NotAnArray => "not-an-array",
                    LineError::NumericLiteral => "numeric-literal",
                    LineError::NestedValue => "nested-value",
                    LineError::InvalidString => "invalid-string",
                    LineError::TrailingContent => "trailing-content",
                    _ => "malformed-row",
                },
            ),
            ErrorKind::Row(rule) => match rule {
                RowRule::ArityMismatch { .. } => (C::SchemaMismatch, "arity-mismatch"),
                RowRule::NullNotAllowed => (C::SchemaMismatch, "null-not-allowed"),
                RowRule::WrongScalarKind => (C::SchemaMismatch, "wrong-scalar-kind"),
                RowRule::InvalidExactNumber(error) => match error {
                    ExactError::Empty => (C::InvalidInput, "exact-empty"),
                    ExactError::InvalidCharacter => (C::InvalidInput, "exact-invalid-character"),
                    ExactError::LeadingZero => (C::InvalidInput, "exact-leading-zero"),
                    ExactError::NegativeZero => (C::InvalidInput, "exact-negative-zero"),
                    ExactError::Malformed => (C::InvalidInput, "exact-malformed"),
                    ExactError::DigitLimitExceeded => (C::ResourceLimit, "exact-too-many-digits"),
                    ExactError::PointMismatch => (C::SchemaMismatch, "exact-point-mismatch"),
                    ExactError::ScaleMismatch => (C::SchemaMismatch, "exact-scale-mismatch"),
                    ExactError::PrecisionExceeded => {
                        (C::SchemaMismatch, "exact-precision-exceeded")
                    }
                    _ => (C::InvalidInput, "exact-invalid"),
                },
                RowRule::InvalidTemporal(error) => match error {
                    TemporalError::InvalidDate => (C::InvalidInput, "temporal-invalid-date"),
                    TemporalError::InvalidTime => (C::InvalidInput, "temporal-invalid-time"),
                    TemporalError::FractionMismatch => {
                        (C::SchemaMismatch, "temporal-fraction-mismatch")
                    }
                    TemporalError::InvalidOffset => (C::InvalidInput, "temporal-invalid-offset"),
                    TemporalError::OffsetMismatch => {
                        (C::SchemaMismatch, "temporal-offset-mismatch")
                    }
                    TemporalError::NegativeZeroOffset => {
                        (C::InvalidInput, "temporal-negative-zero-offset")
                    }
                    _ => (C::InvalidInput, "temporal-malformed"),
                },
                _ => (C::SchemaMismatch, "row-mismatch"),
            },
            ErrorKind::QuantizeOverflow => (C::SchemaMismatch, "quantize-overflow"),
            ErrorKind::TooManyRows => (C::ResourceLimit, "too-many-rows"),
            ErrorKind::TooManyEmptyRanges => (C::ResourceLimit, "too-many-empty-ranges"),
            ErrorKind::NotNative(error) => (
                C::UnsupportedRepresentation,
                match error {
                    Binary64Error::TooManyDigits => "too-many-significant-digits",
                    Binary64Error::NotExactInteger => "not-exact-integer",
                    _ => "not-round-trip",
                },
            ),
            ErrorKind::InputChanged => (C::IoFailure, "input-changed"),
            ErrorKind::Write(_) => (C::IoFailure, "write-failed"),
            ErrorKind::Staging(_) => (C::IoFailure, "staging-failed"),
            ErrorKind::Backend(_) => (C::Internal, "writer-error"),
            ErrorKind::BackendPanic => (C::Internal, "writer-panic"),
            ErrorKind::Cancelled => (C::Cancelled, "cancelled"),
        }
    }
}

impl fmt::Display for WriteError {
    /// Names the category, the rule and the location. Never includes cell
    /// content.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (category, rule) = self.classify();
        write!(f, "{}: {rule}", category.as_str())?;
        if let Some(row) = self.row {
            write!(f, " at row {row}")?;
        }
        if let Some(column) = &self.column {
            write!(f, " in column {column}")?;
        }
        Ok(())
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::Read(error) | ErrorKind::Write(error) | ErrorKind::Staging(error) => {
                Some(error)
            }
            ErrorKind::Policy(error) => Some(error),
            _ => None,
        }
    }
}
