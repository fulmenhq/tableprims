// SPDX-License-Identifier: MIT OR Apache-2.0

//! Schema and policy documents.
//!
//! A schema document declares the columns of a table. A policy document
//! says how exact numbers are to be represented and which decimal columns
//! are quantized. Both are the documents of the Tabular Exchange Standard,
//! contract revision `tabular-exchange/0.1.0`, published in
//! fulmenhq/crucible. Each is a single JSON object read with the strict
//! reader in [`crate::json`]. The reader checks the `format` member first
//! and refuses any other value before looking at the rest; every member it
//! does not know is refused, at every depth. A format identifier other than
//! the two below names another format and is refused, including the
//! identifiers this crate used before it adopted the standard.
//!
//! Schema, format `tabular-exchange.schema/0`:
//!
//! ```json
//! {"format": "tabular-exchange.schema/0", "columns": [
//!   {"id": "qty", "label": "Qty", "type": "integer", "nullable": false},
//!   {"id": "amount", "label": "Amount", "type": "decimal",
//!    "precision": 12, "scale": 2, "nullable": true},
//!   {"id": "seen", "label": "Seen", "type": "local-timestamp",
//!    "fraction": 3, "nullable": true}
//! ]}
//! ```
//!
//! Types are `string`, `boolean`, `integer`, `decimal` (with `precision` and
//! `scale`), `date`, `local-timestamp` and `offset-timestamp` (each with
//! `fraction`). A type-specific member on another type is refused.
//!
//! Policy, format `tabular-exchange.policy/0`:
//!
//! ```json
//! {"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact",
//!  "quantize": [{"column": "amount", "scale": 2, "rounding": "half-even"}],
//!  "extensions": {"tableprims": {"sheet": "Data", "limits": {"max_rows": 1000}}}}
//! ```
//!
//! `exact_numbers` is `require-exact`, `text-if-needed` or `text-always`.
//! `quantize` is optional; a target `scale` is at most 38.
//!
//! # Extensions
//!
//! `extensions` is the one open member of a policy. Its keys are namespaces
//! and its values are objects. The standard bounds it for every reader, and
//! the bounds are judged before any namespace is looked up: at most
//! [`MAX_EXTENSION_NAMESPACES`] namespaces, at most [`MAX_EXTENSION_VALUES`]
//! values, no string or member name longer than [`MAX_EXTENSION_STRING`]
//! code points, no number above [`MAX_COUNT`], and a namespace that matches
//! `[a-z][a-z0-9]*(-[a-z0-9]+)*` in at most [`MAX_NAMESPACE_LENGTH`]
//! characters. A policy nests at most six deep, which leaves a namespace
//! three levels beneath its own object.
//!
//! This build implements one namespace, [`EXTENSION_NAMESPACE`]. A policy
//! that carries any other is refused: a setting that is silently ignored
//! is a conversion the author did not ask for. The members of the namespace
//! are both optional here:
//!
//! - `sheet`: the name of the destination sheet, a string that is not
//!   empty. Requiring one, reserving names and any further naming rule
//!   belong to the destination format.
//! - `limits`: `max_rows`, `max_line_bytes` and `max_empty_ranges`, each
//!   optional. A limit is read as written: what it bounds in a particular
//!   destination, and its ceiling there, are left to the consumer of the
//!   policy.
//!
//! This reader knows every member of the namespace so that it can refuse
//! any other while the document is read. What a member means to a
//! destination, and whether the destination needs it, are the
//! destination's.
//!
//! A `sheet` or `limits` member at the top level of a policy is an unknown
//! member.

use core::fmt;

use crate::exact::{DecimalType, RoundingMode, MAX_DIGITS};
use crate::json::{parse_document, Json, JsonError};
use crate::table::{Column, LogicalType, Schema, SchemaError};
use crate::temporal::FractionPrecision;

/// Format identifier of the schema document this build reads.
pub const SCHEMA_FORMAT: &str = "tabular-exchange.schema/0";
/// Format identifier of the policy document this build reads.
pub const POLICY_FORMAT: &str = "tabular-exchange.policy/0";

/// The extension namespace this build implements in a policy.
pub const EXTENSION_NAMESPACE: &str = "tableprims";

/// Largest number a document may carry: 2^53 - 1.
pub const MAX_COUNT: u64 = 9_007_199_254_740_991;
/// Largest number of namespaces under `extensions`.
pub const MAX_EXTENSION_NAMESPACES: usize = 8;
/// Largest number of values under `extensions`: every member value and
/// every array element, at every depth, the namespace objects included.
pub const MAX_EXTENSION_VALUES: usize = 256;
/// Largest length, in code points, of a string or a member name under
/// `extensions`.
pub const MAX_EXTENSION_STRING: usize = 256;
/// Largest length of a namespace, in characters.
pub const MAX_NAMESPACE_LENGTH: usize = 64;

/// Deepest nesting of a schema: object, array, object.
const SCHEMA_MAX_DEPTH: usize = 3;
/// Deepest nesting of a policy. The members the standard defines reach
/// three; the rest can be reached only under `extensions`.
const POLICY_MAX_DEPTH: usize = 6;

/// Why a document was refused.
///
/// Member names in these errors are names this build defines, never text
/// taken from the document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentError {
    /// The text is not acceptable JSON.
    Json(JsonError),
    /// The document, or an entry that must be an object, is not one.
    NotAnObject,
    /// The `format` member is absent, not a string, or not a format this
    /// build reads.
    UnsupportedFormat,
    /// A required member is absent.
    Missing(&'static str),
    /// An object holds a member this format does not define, or a member
    /// that does not apply to the declared type.
    UnknownMember,
    /// A member has the wrong JSON type or a value out of range.
    Invalid(&'static str),
    /// A column declares a type this build does not implement.
    UnsupportedType,
    /// The columns do not form a schema.
    Schema(SchemaError),
    /// Two quantize entries name the same column.
    DuplicateQuantize,
    /// A quantize entry names a column the schema does not have.
    QuantizeUnknownColumn,
    /// A quantize entry names a column that is not a decimal.
    QuantizeNotDecimal,
    /// A quantize entry's scale exceeds the column's declared precision.
    QuantizeScaleTooLarge,
    /// A policy carries an extension namespace this build does not
    /// implement.
    UnsupportedExtension,
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(err) => write!(f, "{err}"),
            Self::NotAnObject => f.write_str("expected a JSON object"),
            Self::UnsupportedFormat => f.write_str("document format is missing or not supported"),
            Self::Missing(name) => write!(f, "required member {name:?} is missing"),
            Self::UnknownMember => f.write_str("object holds a member that is not defined here"),
            Self::Invalid(name) => write!(f, "member {name:?} has an invalid value"),
            Self::UnsupportedType => f.write_str("column type is not supported"),
            Self::Schema(err) => write!(f, "{err}"),
            Self::DuplicateQuantize => f.write_str("a column is quantized more than once"),
            Self::QuantizeUnknownColumn => f.write_str("quantize names an unknown column"),
            Self::QuantizeNotDecimal => {
                f.write_str("quantize names a column that is not a decimal")
            }
            Self::QuantizeScaleTooLarge => {
                f.write_str("quantize scale exceeds the column's precision")
            }
            Self::UnsupportedExtension => {
                f.write_str("policy carries an extension namespace that is not implemented")
            }
        }
    }
}

impl std::error::Error for DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(err) => Some(err),
            Self::Schema(err) => Some(err),
            _ => None,
        }
    }
}

/// The members of one object, taken by name; whatever is left is unknown.
struct Members(Vec<(String, Json)>);

impl Members {
    fn of(value: Json) -> Result<Self, DocumentError> {
        match value {
            Json::Object(members) => Ok(Self(members)),
            _ => Err(DocumentError::NotAnObject),
        }
    }

    fn take(&mut self, name: &str) -> Option<Json> {
        let at = self.0.iter().position(|(key, _)| key == name)?;
        Some(self.0.swap_remove(at).1)
    }

    fn require(&mut self, name: &'static str) -> Result<Json, DocumentError> {
        self.take(name).ok_or(DocumentError::Missing(name))
    }

    fn string(&mut self, name: &'static str) -> Result<String, DocumentError> {
        match self.require(name)? {
            Json::String(text) => Ok(text),
            _ => Err(DocumentError::Invalid(name)),
        }
    }

    fn boolean(&mut self, name: &'static str) -> Result<bool, DocumentError> {
        match self.require(name)? {
            Json::Boolean(value) => Ok(value),
            _ => Err(DocumentError::Invalid(name)),
        }
    }

    fn small(&mut self, name: &'static str) -> Result<u8, DocumentError> {
        match self.require(name)? {
            Json::Integer(value) => u8::try_from(value).map_err(|_| DocumentError::Invalid(name)),
            _ => Err(DocumentError::Invalid(name)),
        }
    }

    fn optional_count(&mut self, name: &'static str) -> Result<Option<u64>, DocumentError> {
        match self.take(name) {
            None => Ok(None),
            Some(Json::Integer(value)) if value <= MAX_COUNT => Ok(Some(value)),
            Some(_) => Err(DocumentError::Invalid(name)),
        }
    }

    /// Checks the format member. Called before any other member is read.
    fn expect_format(&mut self, expected: &str) -> Result<(), DocumentError> {
        match self.take("format") {
            Some(Json::String(format)) if format == expected => Ok(()),
            _ => Err(DocumentError::UnsupportedFormat),
        }
    }

    fn finish(self) -> Result<(), DocumentError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(DocumentError::UnknownMember)
        }
    }
}

fn document(text: &str, format: &str, max_depth: usize) -> Result<Members, DocumentError> {
    let value = parse_document(text, max_depth).map_err(DocumentError::Json)?;
    let mut members = Members::of(value)?;
    members.expect_format(format)?;
    Ok(members)
}

/// Reads a schema document.
///
/// # Errors
///
/// Refuses text that is not a schema document of format [`SCHEMA_FORMAT`],
/// unknown members, unsupported column types, invalid type parameters and
/// columns that do not form a schema.
pub fn parse_schema(text: &str) -> Result<Schema, DocumentError> {
    let mut members = document(text, SCHEMA_FORMAT, SCHEMA_MAX_DEPTH)?;
    let Json::Array(entries) = members.require("columns")? else {
        return Err(DocumentError::Invalid("columns"));
    };
    members.finish()?;
    let columns = entries
        .into_iter()
        .map(column)
        .collect::<Result<Vec<_>, _>>()?;
    Schema::new(columns).map_err(DocumentError::Schema)
}

fn column(entry: Json) -> Result<Column, DocumentError> {
    let mut members = Members::of(entry)?;
    let id = members.string("id")?;
    let label = members.string("label")?;
    let nullable = members.boolean("nullable")?;
    let logical_type = match members.string("type")?.as_str() {
        "string" => LogicalType::String,
        "boolean" => LogicalType::Boolean,
        "integer" => LogicalType::Integer,
        "decimal" => {
            let precision = members.small("precision")?;
            let scale = members.small("scale")?;
            let ty = DecimalType::new(precision, scale)
                .map_err(|_| DocumentError::Invalid("precision"))?;
            LogicalType::Decimal(ty)
        }
        "date" => LogicalType::Date,
        "local-timestamp" => LogicalType::LocalTimestamp(fraction(&mut members)?),
        "offset-timestamp" => LogicalType::OffsetTimestamp(fraction(&mut members)?),
        _ => return Err(DocumentError::UnsupportedType),
    };
    members.finish()?;
    Ok(Column::new(id, label, logical_type, nullable))
}

fn fraction(members: &mut Members) -> Result<FractionPrecision, DocumentError> {
    FractionPrecision::new(members.small("fraction")?)
        .map_err(|_| DocumentError::Invalid("fraction"))
}

/// How exact-number columns are represented in a destination that has a
/// native numeric form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExactNumberPolicy {
    /// Use the native numeric form; refuse a value it cannot hold exactly.
    RequireExact,
    /// Use the native numeric form, and write a whole column as text when
    /// any of its values cannot be held exactly.
    TextIfNeeded,
    /// Write every exact-number column as text.
    TextAlways,
}

/// A request to re-express one decimal column at another scale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quantize {
    column: String,
    scale: u8,
    rounding: RoundingMode,
}

impl Quantize {
    /// Identifier of the column.
    #[must_use]
    pub fn column(&self) -> &str {
        &self.column
    }

    /// Target scale.
    #[must_use]
    pub fn scale(&self) -> u8 {
        self.scale
    }

    /// Rounding mode applied when the scale is lowered.
    #[must_use]
    pub fn rounding(&self) -> RoundingMode {
        self.rounding
    }
}

/// Limits a policy sets. An absent limit leaves the consumer's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Limits {
    /// Largest number of rows.
    pub max_rows: Option<u64>,
    /// Largest length of one input line in bytes.
    pub max_line_bytes: Option<u64>,
    /// Largest number of empty-string ranges recorded for a table.
    pub max_empty_ranges: Option<u64>,
}

/// A conversion policy as written in a policy document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    exact_numbers: ExactNumberPolicy,
    quantize: Vec<Quantize>,
    sheet: Option<String>,
    limits: Limits,
}

impl Policy {
    /// Representation policy for exact-number columns.
    #[must_use]
    pub fn exact_numbers(&self) -> ExactNumberPolicy {
        self.exact_numbers
    }

    /// Quantize requests, in the order written.
    #[must_use]
    pub fn quantize(&self) -> &[Quantize] {
        &self.quantize
    }

    /// Name of the destination sheet, when the policy gives one under the
    /// [`EXTENSION_NAMESPACE`] extension.
    #[must_use]
    pub fn sheet(&self) -> Option<&str> {
        self.sheet.as_deref()
    }

    /// Limits the policy sets under the [`EXTENSION_NAMESPACE`] extension.
    #[must_use]
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Checks the quantize requests against a schema and returns, for each
    /// schema column in order, the decimal type it is quantized to and the
    /// rounding mode, or `None` for a column that is not quantized.
    ///
    /// The target type keeps the column's declared precision.
    ///
    /// # Errors
    ///
    /// Refuses a request that names an unknown column or a column that is
    /// not a decimal, and a target scale above the declared precision. A
    /// target scale above 38 is refused earlier, when the policy is read.
    pub fn quantize_plan(
        &self,
        schema: &Schema,
    ) -> Result<Vec<Option<(DecimalType, RoundingMode)>>, DocumentError> {
        let mut plan = vec![None; schema.columns().len()];
        for request in &self.quantize {
            let at = schema
                .columns()
                .iter()
                .position(|column| column.id() == request.column)
                .ok_or(DocumentError::QuantizeUnknownColumn)?;
            let LogicalType::Decimal(declared) = schema.columns()[at].logical_type() else {
                return Err(DocumentError::QuantizeNotDecimal);
            };
            let target = DecimalType::new(declared.precision(), request.scale)
                .map_err(|_| DocumentError::QuantizeScaleTooLarge)?;
            plan[at] = Some((target, request.rounding));
        }
        Ok(plan)
    }
}

/// Reads a policy document.
///
/// # Errors
///
/// Refuses text that is not a policy document of format [`POLICY_FORMAT`],
/// unknown members, invalid values, a column quantized more than once, an
/// `extensions` member outside its bounds and an extension namespace other
/// than [`EXTENSION_NAMESPACE`].
pub fn parse_policy(text: &str) -> Result<Policy, DocumentError> {
    let mut members = document(text, POLICY_FORMAT, POLICY_MAX_DEPTH)?;
    let exact_numbers = match members.string("exact_numbers")?.as_str() {
        "require-exact" => ExactNumberPolicy::RequireExact,
        "text-if-needed" => ExactNumberPolicy::TextIfNeeded,
        "text-always" => ExactNumberPolicy::TextAlways,
        _ => return Err(DocumentError::Invalid("exact_numbers")),
    };
    let mut quantize: Vec<Quantize> = Vec::new();
    match members.take("quantize") {
        None => {}
        Some(Json::Array(entries)) => {
            for entry in entries {
                let request = quantize_entry(entry)?;
                if quantize.iter().any(|seen| seen.column == request.column) {
                    return Err(DocumentError::DuplicateQuantize);
                }
                quantize.push(request);
            }
        }
        Some(_) => return Err(DocumentError::Invalid("quantize")),
    }
    let (sheet, limits) = match members.take("extensions") {
        None => (None, Limits::default()),
        Some(value) => extensions(value)?,
    };
    members.finish()?;
    Ok(Policy {
        exact_numbers,
        quantize,
        sheet,
        limits,
    })
}

/// Whether `name` matches `[a-z][a-z0-9]*(-[a-z0-9]+)*` in at most
/// [`MAX_NAMESPACE_LENGTH`] characters.
fn is_namespace(name: &str) -> bool {
    let group = |group: &str| {
        !group.is_empty()
            && group
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    };
    name.len() <= MAX_NAMESPACE_LENGTH
        && name.starts_with(|first: char| first.is_ascii_lowercase())
        && name.split('-').all(group)
}

/// What the bounds on `extensions` measure.
#[derive(Default)]
struct ExtensionTally {
    values: usize,
    long_string: bool,
    large_number: bool,
}

impl ExtensionTally {
    fn string(&mut self, text: &str) {
        // A string of at most the bound in bytes is within it in code points.
        if text.len() > MAX_EXTENSION_STRING && text.chars().count() > MAX_EXTENSION_STRING {
            self.long_string = true;
        }
    }

    /// Counts the values held by `value` and inspects its strings and
    /// numbers. The reader has already bounded the nesting.
    fn walk(&mut self, value: &Json) {
        match value {
            Json::String(text) => self.string(text),
            Json::Integer(number) => self.large_number |= *number > MAX_COUNT,
            Json::BigInteger => self.large_number = true,
            Json::Array(items) => {
                for item in items {
                    self.values += 1;
                    self.walk(item);
                }
            }
            Json::Object(members) => {
                for (name, member) in members {
                    self.values += 1;
                    self.string(name);
                    self.walk(member);
                }
            }
            Json::Null | Json::Boolean(_) => {}
        }
    }
}

/// Reads the `extensions` member: the bounds every reader applies, then the
/// namespaces, then the content of this build's namespace.
fn extensions(value: Json) -> Result<(Option<String>, Limits), DocumentError> {
    const INVALID: DocumentError = DocumentError::Invalid("extensions");
    let namespaces = Members::of(value)?.0;
    // The bounds and the shape, without knowing any namespace: the number
    // of namespaces; then each namespace in document order, its name before
    // its value; then the strings, numbers and number of values.
    if namespaces.len() > MAX_EXTENSION_NAMESPACES {
        return Err(INVALID);
    }
    for (name, content) in &namespaces {
        if !is_namespace(name) {
            return Err(INVALID);
        }
        if !matches!(content, Json::Object(_)) {
            return Err(DocumentError::NotAnObject);
        }
    }
    let mut tally = ExtensionTally::default();
    for (_, content) in &namespaces {
        tally.values += 1;
        tally.walk(content);
    }
    if tally.values > MAX_EXTENSION_VALUES || tally.long_string || tally.large_number {
        return Err(INVALID);
    }
    // The namespaces. One that is not implemented refuses the policy.
    if namespaces
        .iter()
        .any(|(name, _)| name != EXTENSION_NAMESPACE)
    {
        return Err(DocumentError::UnsupportedExtension);
    }
    let Some((_, content)) = namespaces.into_iter().next() else {
        return Ok((None, Limits::default()));
    };
    let mut members = Members::of(content)?;
    let sheet = match members.take("sheet") {
        None => None,
        Some(Json::String(sheet)) if !sheet.is_empty() => Some(sheet),
        Some(_) => return Err(DocumentError::Invalid("sheet")),
    };
    let limits = match members.take("limits") {
        None => Limits::default(),
        Some(value) => {
            let mut members = Members::of(value)?;
            let limits = Limits {
                max_rows: members.optional_count("max_rows")?,
                max_line_bytes: members.optional_count("max_line_bytes")?,
                max_empty_ranges: members.optional_count("max_empty_ranges")?,
            };
            members.finish()?;
            limits
        }
    };
    members.finish()?;
    Ok((sheet, limits))
}

fn quantize_entry(entry: Json) -> Result<Quantize, DocumentError> {
    let mut members = Members::of(entry)?;
    let column = members.string("column")?;
    let scale = members.small("scale")?;
    if scale > MAX_DIGITS {
        return Err(DocumentError::Invalid("scale"));
    }
    let rounding = match members.string("rounding")?.as_str() {
        "half-even" => RoundingMode::HalfEven,
        "half-away-from-zero" => RoundingMode::HalfAwayFromZero,
        "toward-zero" => RoundingMode::TowardZero,
        "toward-positive" => RoundingMode::TowardPositive,
        "toward-negative" => RoundingMode::TowardNegative,
        _ => return Err(DocumentError::Invalid("rounding")),
    };
    members.finish()?;
    Ok(Quantize {
        column,
        scale,
        rounding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = r#"{
      "format": "tabular-exchange.schema/0",
      "columns": [
        {"id": "invoice", "label": "Invoice", "type": "string", "nullable": false},
        {"id": "qty", "label": "Qty", "type": "integer", "nullable": false},
        {"id": "amount", "label": "Amount", "type": "decimal", "precision": 12, "scale": 4, "nullable": true},
        {"id": "paid", "label": "Paid", "type": "boolean", "nullable": true},
        {"id": "day", "label": "", "type": "date", "nullable": true},
        {"id": "seen", "label": "Seen", "type": "local-timestamp", "fraction": 3, "nullable": true},
        {"id": "sent", "label": "Seen", "type": "offset-timestamp", "fraction": 0, "nullable": true}
      ]
    }"#;

    fn schema_with(column: &str) -> Result<Schema, DocumentError> {
        parse_schema(&format!(
            r#"{{"format": "tabular-exchange.schema/0", "columns": [{column}]}}"#
        ))
    }

    /// A policy with `rest` appended to its top-level members.
    fn policy_with(rest: &str) -> Result<Policy, DocumentError> {
        parse_policy(&format!(
            r#"{{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "extensions": {{"tableprims": {{"sheet": "Data"}}}}{rest}}}"#
        ))
    }

    /// A policy whose `extensions` member is `extensions`.
    fn policy_extended(extensions: &str) -> Result<Policy, DocumentError> {
        parse_policy(&format!(
            r#"{{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "extensions": {extensions}}}"#
        ))
    }

    /// A policy whose own namespace holds `content`.
    fn policy_namespace(content: &str) -> Result<Policy, DocumentError> {
        policy_extended(&format!(r#"{{"tableprims": {{{content}}}}}"#))
    }

    #[test]
    fn schema_is_read() {
        let schema = parse_schema(SCHEMA).unwrap();
        let types: Vec<LogicalType> = schema.columns().iter().map(Column::logical_type).collect();
        assert_eq!(
            types,
            vec![
                LogicalType::String,
                LogicalType::Integer,
                LogicalType::Decimal(DecimalType::new(12, 4).unwrap()),
                LogicalType::Boolean,
                LogicalType::Date,
                LogicalType::LocalTimestamp(FractionPrecision::new(3).unwrap()),
                LogicalType::OffsetTimestamp(FractionPrecision::new(0).unwrap()),
            ]
        );
        assert_eq!(schema.columns()[0].id(), "invoice");
        assert!(!schema.columns()[0].nullable());
        assert!(schema.columns()[2].nullable());
        assert_eq!(schema.columns()[4].label(), "");
    }

    #[test]
    fn format_is_checked_before_anything_else() {
        for text in [
            r#"{"columns": []}"#,
            r#"{"format": "tabular-exchange.schema/1", "columns": "nonsense", "extra": 1}"#,
            r#"{"format": "tabular-exchange.policy/0", "columns": []}"#,
            r#"{"format": 0, "columns": []}"#,
            r#"{"format": "TABULAR-EXCHANGE.SCHEMA/0", "columns": []}"#,
        ] {
            assert_eq!(
                parse_schema(text),
                Err(DocumentError::UnsupportedFormat),
                "{text}"
            );
        }
        assert_eq!(
            parse_policy(r#"{"format": "tabular-exchange.schema/0"}"#).unwrap_err(),
            DocumentError::UnsupportedFormat
        );
    }

    #[test]
    fn earlier_format_identifiers_are_other_formats() {
        assert_eq!(
            parse_schema(
                r#"{"format": "tableprims.schema/0", "columns": [
                    {"id": "a", "label": "", "type": "string", "nullable": true}]}"#
            ),
            Err(DocumentError::UnsupportedFormat)
        );
        for policy in [
            r#"{"format": "tableprims.policy/0", "exact_numbers": "require-exact", "sheet": "Data"}"#,
            r#"{"format": "tableprims.policy/0", "exact_numbers": "require-exact",
                "extensions": {"tableprims": {"sheet": "Data"}}}"#,
        ] {
            assert_eq!(
                parse_policy(policy),
                Err(DocumentError::UnsupportedFormat),
                "{policy}"
            );
        }
    }

    #[test]
    fn a_number_too_large_for_its_member_is_invalid_not_malformed() {
        let decimal = |precision: &str| {
            schema_with(&format!(
                r#"{{"id": "a", "label": "", "type": "decimal", "precision": {precision}, "scale": 0, "nullable": true}}"#
            ))
        };
        for precision in [
            "256",
            "9007199254740992",
            "18446744073709551615",
            "18446744073709551616",
            "99999999999999999999999999999999999999999999999999",
        ] {
            assert_eq!(
                decimal(precision),
                Err(DocumentError::Invalid("precision")),
                "{precision}"
            );
        }
        assert_eq!(
            schema_with(
                r#"{"id": "a", "label": "", "type": "local-timestamp", "fraction": 18446744073709551616, "nullable": true}"#
            ),
            Err(DocumentError::Invalid("fraction"))
        );
        assert_eq!(
            policy_with(
                r#", "quantize": [{"column": "a", "scale": 340282366920938463463374607431768211456, "rounding": "half-even"}]"#
            ),
            Err(DocumentError::Invalid("scale"))
        );
        // The spelling still decides whether the text is in the grammar.
        assert_eq!(
            decimal("018446744073709551616"),
            Err(DocumentError::Json(JsonError::Malformed))
        );
        assert_eq!(
            decimal("18446744073709551616.0"),
            Err(DocumentError::Json(JsonError::UnsupportedNumber))
        );
    }

    #[test]
    fn documents_must_be_strict_json_objects() {
        assert_eq!(parse_schema("[]"), Err(DocumentError::NotAnObject));
        assert_eq!(parse_schema(""), Err(DocumentError::Json(JsonError::Empty)));
        assert_eq!(
            parse_schema(
                r#"{"format": "tabular-exchange.schema/0", "format": "tabular-exchange.schema/0", "columns": []}"#
            ),
            Err(DocumentError::Json(JsonError::DuplicateKey))
        );
        assert_eq!(
            schema_with(
                r#"{"id": "a", "id": "a", "label": "", "type": "string", "nullable": true}"#
            ),
            Err(DocumentError::Json(JsonError::DuplicateKey))
        );
        assert_eq!(
            schema_with(
                r#"{"id": "a", "label": "", "type": "string", "nullable": true, "x": [[[[1]]]]}"#
            ),
            Err(DocumentError::Json(JsonError::TooDeep))
        );
    }

    #[test]
    fn unknown_members_are_refused_at_every_depth() {
        assert_eq!(
            parse_schema(r#"{"format": "tabular-exchange.schema/0", "columns": [], "note": "x"}"#),
            Err(DocumentError::UnknownMember)
        );
        assert_eq!(
            schema_with(
                r#"{"id": "a", "label": "", "type": "string", "nullable": true, "note": 1}"#
            ),
            Err(DocumentError::UnknownMember)
        );
        assert_eq!(
            policy_with(r#", "note": 1"#),
            Err(DocumentError::UnknownMember)
        );
        assert_eq!(
            policy_namespace(r#""sheet": "Data", "limits": {"max_cols": 1}"#),
            Err(DocumentError::UnknownMember)
        );
        assert_eq!(
            policy_namespace(r#""sheet": "Data", "note": 1"#),
            Err(DocumentError::UnknownMember)
        );
        assert_eq!(
            policy_with(
                r#", "quantize": [{"column": "a", "scale": 1, "rounding": "half-even", "x": 1}]"#
            ),
            Err(DocumentError::UnknownMember)
        );
    }

    #[test]
    fn type_parameters_belong_to_their_types() {
        for column in [
            r#"{"id": "a", "label": "", "type": "string", "nullable": true, "precision": 5, "scale": 2}"#,
            r#"{"id": "a", "label": "", "type": "integer", "nullable": true, "scale": 0}"#,
            r#"{"id": "a", "label": "", "type": "date", "nullable": true, "fraction": 0}"#,
            r#"{"id": "a", "label": "", "type": "decimal", "precision": 5, "scale": 2, "fraction": 0, "nullable": true}"#,
        ] {
            assert_eq!(
                schema_with(column),
                Err(DocumentError::UnknownMember),
                "{column}"
            );
        }
        assert_eq!(
            schema_with(
                r#"{"id": "a", "label": "", "type": "decimal", "scale": 2, "nullable": true}"#
            ),
            Err(DocumentError::Missing("precision"))
        );
        assert_eq!(
            schema_with(r#"{"id": "a", "label": "", "type": "local-timestamp", "nullable": true}"#),
            Err(DocumentError::Missing("fraction"))
        );
    }

    #[test]
    fn column_members_are_validated() {
        let cases = [
            (
                r#"{"label": "", "type": "string", "nullable": true}"#,
                DocumentError::Missing("id"),
            ),
            (
                r#"{"id": "a", "type": "string", "nullable": true}"#,
                DocumentError::Missing("label"),
            ),
            (
                r#"{"id": "a", "label": "", "type": "string"}"#,
                DocumentError::Missing("nullable"),
            ),
            (
                r#"{"id": "a", "label": "", "nullable": true}"#,
                DocumentError::Missing("type"),
            ),
            (
                r#"{"id": 1, "label": "", "type": "string", "nullable": true}"#,
                DocumentError::Invalid("id"),
            ),
            (
                r#"{"id": "a", "label": "", "type": "string", "nullable": "yes"}"#,
                DocumentError::Invalid("nullable"),
            ),
            (
                r#"{"id": "a", "label": "", "type": "float", "nullable": true}"#,
                DocumentError::UnsupportedType,
            ),
            (
                r#"{"id": "a", "label": "", "type": "String", "nullable": true}"#,
                DocumentError::UnsupportedType,
            ),
            (
                r#"{"id": "a", "label": "", "type": "decimal", "precision": 39, "scale": 2, "nullable": true}"#,
                DocumentError::Invalid("precision"),
            ),
            (
                r#"{"id": "a", "label": "", "type": "decimal", "precision": 0, "scale": 0, "nullable": true}"#,
                DocumentError::Invalid("precision"),
            ),
            (
                r#"{"id": "a", "label": "", "type": "decimal", "precision": 4, "scale": 5, "nullable": true}"#,
                DocumentError::Invalid("precision"),
            ),
            (
                r#"{"id": "a", "label": "", "type": "decimal", "precision": 300, "scale": 2, "nullable": true}"#,
                DocumentError::Invalid("precision"),
            ),
            (
                r#"{"id": "a", "label": "", "type": "decimal", "precision": "12", "scale": 2, "nullable": true}"#,
                DocumentError::Invalid("precision"),
            ),
            (
                r#"{"id": "a", "label": "", "type": "offset-timestamp", "fraction": 10, "nullable": true}"#,
                DocumentError::Invalid("fraction"),
            ),
            (
                r#"{"id": "", "label": "", "type": "string", "nullable": true}"#,
                DocumentError::Schema(SchemaError::EmptyColumnId { position: 0 }),
            ),
            (r#""a""#, DocumentError::NotAnObject),
        ];
        for (column, expected) in cases {
            assert_eq!(schema_with(column), Err(expected), "{column}");
        }
        assert_eq!(
            schema_with(""),
            Err(DocumentError::Schema(SchemaError::NoColumns))
        );
        let a = r#"{"id": "a", "label": "", "type": "string", "nullable": true}"#;
        assert_eq!(
            schema_with(&format!("{a}, {a}")),
            Err(DocumentError::Schema(SchemaError::DuplicateColumnId {
                position: 1
            }))
        );
        assert_eq!(
            parse_schema(r#"{"format": "tabular-exchange.schema/0", "columns": {}}"#),
            Err(DocumentError::Invalid("columns"))
        );
        assert_eq!(
            parse_schema(r#"{"format": "tabular-exchange.schema/0"}"#),
            Err(DocumentError::Missing("columns"))
        );
    }

    #[test]
    fn policy_is_read() {
        let policy = parse_policy(
            r#"{"format": "tabular-exchange.policy/0", "exact_numbers": "text-if-needed",
                "quantize": [{"column": "amount", "scale": 2, "rounding": "half-even"}],
                "extensions": {"tableprims": {
                  "sheet": "Data",
                  "limits": {"max_rows": 10, "max_empty_ranges": 5}}}}"#,
        )
        .unwrap();
        assert_eq!(policy.exact_numbers(), ExactNumberPolicy::TextIfNeeded);
        assert_eq!(policy.sheet(), Some("Data"));
        assert_eq!(policy.quantize().len(), 1);
        assert_eq!(policy.quantize()[0].column(), "amount");
        assert_eq!(policy.quantize()[0].scale(), 2);
        assert_eq!(policy.quantize()[0].rounding(), RoundingMode::HalfEven);
        assert_eq!(
            policy.limits(),
            Limits {
                max_rows: Some(10),
                max_line_bytes: None,
                max_empty_ranges: Some(5)
            }
        );
        let minimal = policy_with("").unwrap();
        assert_eq!(minimal.exact_numbers(), ExactNumberPolicy::RequireExact);
        assert!(minimal.quantize().is_empty());
        assert_eq!(minimal.limits(), Limits::default());
    }

    #[test]
    fn a_policy_without_extensions_is_complete() {
        for text in [
            r#"{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact"}"#,
            r#"{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "quantize": []}"#,
            r#"{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "extensions": {}}"#,
            r#"{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "extensions": {"tableprims": {}}}"#,
            r#"{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact", "extensions": {"tableprims": {"limits": {}}}}"#,
        ] {
            let policy = parse_policy(text).unwrap();
            assert_eq!(policy.sheet(), None, "{text}");
            assert_eq!(policy.limits(), Limits::default(), "{text}");
            assert!(policy.quantize().is_empty(), "{text}");
        }
    }

    #[test]
    fn destination_settings_outside_extensions_are_unknown_members() {
        let whole = |body: &str| {
            parse_policy(&format!(
                r#"{{"format": "tabular-exchange.policy/0", "exact_numbers": "require-exact"{body}}}"#
            ))
        };
        for body in [
            r#", "sheet": "Data""#,
            r#", "limits": {"max_rows": 10}"#,
            r#", "limits": {}"#,
            r#", "sheet": "Data", "extensions": {"tableprims": {"sheet": "Data"}}"#,
            r#", "tableprims": {"sheet": "Data"}"#,
        ] {
            assert_eq!(whole(body), Err(DocumentError::UnknownMember), "{body}");
        }
    }

    #[test]
    fn a_namespace_that_is_not_implemented_refuses_the_policy() {
        for extensions in [
            r#"{"example": {}}"#,
            r#"{"example-tool": {"setting": true}}"#,
            r#"{"xlsx": {"sheet": "Data"}}"#,
            r#"{"tableprims": {"sheet": "Data"}, "example": {}}"#,
            r#"{"example": {}, "tableprims": {"sheet": "Data"}}"#,
            r#"{"tableprims2": {}}"#,
            r#"{"tableprims-x": {}}"#,
        ] {
            assert_eq!(
                policy_extended(extensions),
                Err(DocumentError::UnsupportedExtension),
                "{extensions}"
            );
        }
    }

    #[test]
    fn extension_shape_is_checked() {
        // A namespace that is not an object is reported before a bound on
        // strings, numbers or the number of values, and after its own name.
        let long = "x".repeat(MAX_EXTENSION_STRING + 1);
        assert_eq!(
            policy_extended(&format!(r#"{{"a":1,"tableprims":{{"k":"{long}"}}}}"#)),
            Err(DocumentError::NotAnObject),
        );
        assert_eq!(
            policy_extended(r#"{"A":1}"#),
            Err(DocumentError::Invalid("extensions")),
        );
        assert_eq!(
            policy_extended(r#"{"a":1,"B":{}}"#),
            Err(DocumentError::NotAnObject),
        );
        assert_eq!(policy_extended("[]"), Err(DocumentError::NotAnObject));
        assert_eq!(policy_extended("null"), Err(DocumentError::NotAnObject));
        assert_eq!(policy_extended(r#""x""#), Err(DocumentError::NotAnObject));
        for value in ["null", "true", "1", r#""x""#, "[]"] {
            assert_eq!(
                policy_extended(&format!(r#"{{"tableprims": {value}}}"#)),
                Err(DocumentError::NotAnObject),
                "{value}"
            );
            // The shape is judged before the namespace is looked up.
            assert_eq!(
                policy_extended(&format!(r#"{{"example": {value}}}"#)),
                Err(DocumentError::NotAnObject),
                "{value}"
            );
        }
        let long = "a".repeat(65);
        for name in [
            "",
            "Tableprims",
            "1a",
            "-a",
            "a-",
            "a--b",
            "a_b",
            "a.b",
            "a b",
            "é",
            long.as_str(),
        ] {
            assert_eq!(
                policy_extended(&format!(r#"{{"{name}": {{}}}}"#)),
                Err(DocumentError::Invalid("extensions")),
                "{name:?}"
            );
        }
        // Well-formed names that this build does not implement.
        let longest = "a".repeat(64);
        for name in ["a", "a1", "a-b", "a-1-b2", "x0-0", longest.as_str()] {
            assert_eq!(
                policy_extended(&format!(r#"{{"{name}": {{}}}}"#)),
                Err(DocumentError::UnsupportedExtension),
                "{name:?}"
            );
        }
    }

    /// `count` namespaces, the first of them this build's.
    fn namespaces(count: usize) -> String {
        let mut text = String::from(r#"{"tableprims": {"sheet": "Data"}"#);
        for i in 1..count {
            text.push_str(&format!(r#", "n{i}": {{}}"#));
        }
        text.push('}');
        text
    }

    #[test]
    fn extension_namespaces_are_bounded() {
        // At the bound the namespaces are looked up; past it they are not.
        assert_eq!(
            policy_extended(&namespaces(8)),
            Err(DocumentError::UnsupportedExtension)
        );
        assert_eq!(
            policy_extended(&namespaces(9)),
            Err(DocumentError::Invalid("extensions"))
        );
    }

    /// This build's namespace holding `sheet`, `limits` with `fill` unknown
    /// members: `fill + 3` values.
    fn filled(fill: usize) -> String {
        let members: Vec<String> = (0..fill).map(|i| format!(r#""k{i}": null"#)).collect();
        format!(
            r#"{{"tableprims": {{"sheet": "Data", "limits": {{{}}}}}}}"#,
            members.join(", ")
        )
    }

    #[test]
    fn extension_values_are_bounded() {
        // 256 values: within the bound, so the content is read and its
        // unknown members are what is refused.
        assert_eq!(
            policy_extended(&filled(253)),
            Err(DocumentError::UnknownMember)
        );
        // 257 values: the bound is judged first.
        assert_eq!(
            policy_extended(&filled(254)),
            Err(DocumentError::Invalid("extensions"))
        );
        // Array elements and nested members count, at every depth; the
        // namespace object counts one. Here 1 + 1 + 254 and 1 + 1 + 255.
        let array = |count: usize| {
            format!(
                r#"{{"example": {{"a": [{}]}}}}"#,
                vec!["null"; count].join(", ")
            )
        };
        assert_eq!(
            policy_extended(&array(254)),
            Err(DocumentError::UnsupportedExtension)
        );
        assert_eq!(
            policy_extended(&array(255)),
            Err(DocumentError::Invalid("extensions"))
        );
        // The count is over all namespaces together.
        let two = |count: usize| {
            format!(
                r#"{{"example": {{"a": [{}]}}, "example-b": {{"b": {{"c": [true]}}}}}}"#,
                vec!["0"; count].join(", ")
            )
        };
        assert_eq!(
            policy_extended(&two(250)),
            Err(DocumentError::UnsupportedExtension)
        );
        assert_eq!(
            policy_extended(&two(251)),
            Err(DocumentError::Invalid("extensions"))
        );
    }

    #[test]
    fn extension_strings_are_bounded_in_code_points() {
        let value = |text: &str| policy_namespace(&format!(r#""sheet": "{text}""#));
        // 256 code points, in one, two and four bytes each.
        for unit in ["a", "é", "😀"] {
            let policy = value(&unit.repeat(256)).unwrap();
            assert_eq!(policy.sheet().unwrap().chars().count(), 256, "{unit}");
            assert_eq!(
                value(&unit.repeat(257)),
                Err(DocumentError::Invalid("extensions")),
                "{unit}"
            );
        }
        // Member names are bounded like values, at every depth.
        let name = |length: usize| {
            policy_namespace(&format!(r#""sheet": "Data", "{}": 1"#, "k".repeat(length)))
        };
        assert_eq!(name(256), Err(DocumentError::UnknownMember));
        assert_eq!(name(257), Err(DocumentError::Invalid("extensions")));
        let nested = |length: usize| {
            policy_extended(&format!(
                r#"{{"example": {{"a": [{{"{}": "{}"}}]}}}}"#,
                "k".repeat(length),
                "v".repeat(256)
            ))
        };
        assert_eq!(nested(256), Err(DocumentError::UnsupportedExtension));
        assert_eq!(nested(257), Err(DocumentError::Invalid("extensions")));
        let element = |length: usize| {
            policy_extended(&format!(
                r#"{{"example": {{"a": ["{}"]}}}}"#,
                "v".repeat(length)
            ))
        };
        assert_eq!(element(256), Err(DocumentError::UnsupportedExtension));
        assert_eq!(element(257), Err(DocumentError::Invalid("extensions")));
    }

    #[test]
    fn extension_numbers_are_bounded() {
        let limit = |number: &str| {
            policy_namespace(&format!(
                r#""sheet": "Data", "limits": {{"max_rows": {number}}}"#
            ))
        };
        assert_eq!(
            limit("9007199254740991").unwrap().limits().max_rows,
            Some(9_007_199_254_740_991)
        );
        for number in [
            "9007199254740992",
            "18446744073709551615",
            "18446744073709551616",
            "99999999999999999999999999999999",
        ] {
            assert_eq!(
                limit(number),
                Err(DocumentError::Invalid("extensions")),
                "{number}"
            );
        }
        // The bound holds in a namespace that is not implemented.
        assert_eq!(
            policy_extended(r#"{"example": {"a": [9007199254740991]}}"#),
            Err(DocumentError::UnsupportedExtension)
        );
        assert_eq!(
            policy_extended(r#"{"example": {"a": [9007199254740992]}}"#),
            Err(DocumentError::Invalid("extensions"))
        );
        // Numbers under extensions follow the document grammar.
        for number in ["-1", "1.0", "1e3", "01"] {
            assert!(
                matches!(limit(number), Err(DocumentError::Json(_))),
                "{number}"
            );
        }
    }

    #[test]
    fn a_policy_nests_six_deep_and_a_schema_three() {
        // The namespace object is at depth 3; its content reaches depth 6.
        assert_eq!(
            policy_extended(r#"{"example": {"a": {"b": {"c": {"d": 1}}}}}"#),
            Err(DocumentError::UnsupportedExtension)
        );
        assert_eq!(
            policy_extended(r#"{"example": {"a": [{"b": [1]}]}}"#),
            Err(DocumentError::UnsupportedExtension)
        );
        assert_eq!(
            policy_namespace(r#""sheet": "Data", "x": {"a": {"b": 1}}"#),
            Err(DocumentError::UnknownMember)
        );
        // Depth 7 is outside the grammar, whatever the namespace.
        for extensions in [
            r#"{"example": {"a": {"b": {"c": {"d": {}}}}}}"#,
            r#"{"example": {"a": {"b": {"c": [[]]}}}}"#,
            r#"{"tableprims": {"sheet": "Data", "x": [[[[]]]]}}"#,
        ] {
            assert_eq!(
                policy_extended(extensions),
                Err(DocumentError::Json(JsonError::TooDeep)),
                "{extensions}"
            );
        }
        // The bound is on the whole document, not only on extensions.
        assert_eq!(
            policy_with(r#", "note": [[[[[1]]]]]"#),
            Err(DocumentError::UnknownMember)
        );
        assert_eq!(
            policy_with(r#", "note": [[[[[[1]]]]]]"#),
            Err(DocumentError::Json(JsonError::TooDeep))
        );
        assert_eq!(
            policy_with(
                r#", "quantize": [{"column": {"a": {"b": {}}}, "scale": 1, "rounding": "half-even"}]"#
            ),
            Err(DocumentError::Invalid("column"))
        );
        // A schema keeps its bound of three.
        assert_eq!(
            parse_schema(r#"{"format": "tabular-exchange.schema/0", "columns": [{"id": []}]}"#),
            Err(DocumentError::Json(JsonError::TooDeep))
        );
    }

    #[test]
    fn policy_names_are_exact() {
        for (name, expected) in [
            ("require-exact", ExactNumberPolicy::RequireExact),
            ("text-if-needed", ExactNumberPolicy::TextIfNeeded),
            ("text-always", ExactNumberPolicy::TextAlways),
        ] {
            let text = format!(
                r#"{{"format": "tabular-exchange.policy/0", "exact_numbers": "{name}", "extensions": {{"tableprims": {{"sheet": "S"}}}}}}"#
            );
            assert_eq!(parse_policy(&text).unwrap().exact_numbers(), expected);
        }
        for (mode, expected) in [
            ("half-even", RoundingMode::HalfEven),
            ("half-away-from-zero", RoundingMode::HalfAwayFromZero),
            ("toward-zero", RoundingMode::TowardZero),
            ("toward-positive", RoundingMode::TowardPositive),
            ("toward-negative", RoundingMode::TowardNegative),
        ] {
            let policy = policy_with(&format!(
                r#", "quantize": [{{"column": "a", "scale": 0, "rounding": "{mode}"}}]"#
            ))
            .unwrap();
            assert_eq!(policy.quantize()[0].rounding(), expected);
        }
    }

    #[test]
    fn policy_members_are_validated() {
        let whole = |body: &str| {
            parse_policy(&format!(
                r#"{{"format": "tabular-exchange.policy/0"{body}}}"#
            ))
        };
        assert_eq!(
            whole(r#", "extensions": {"tableprims": {"sheet": "S"}}"#),
            Err(DocumentError::Missing("exact_numbers"))
        );
        // The sheet is a setting of a destination: a policy without one is
        // read, and the destination decides whether it needs one.
        assert_eq!(
            whole(r#", "exact_numbers": "require-exact""#)
                .unwrap()
                .sheet(),
            None
        );
        assert_eq!(
            whole(r#", "exact_numbers": "exact", "extensions": {"tableprims": {"sheet": "S"}}"#),
            Err(DocumentError::Invalid("exact_numbers"))
        );
        assert_eq!(
            policy_namespace(r#""sheet": """#),
            Err(DocumentError::Invalid("sheet"))
        );
        for value in ["null", "1", "true", "[]", "{}"] {
            assert_eq!(
                policy_namespace(&format!(r#""sheet": {value}"#)),
                Err(DocumentError::Invalid("sheet")),
                "{value}"
            );
        }
        // A name a destination reserves is that destination's refusal.
        for allowed in [
            "tableprims",
            "_tableprims",
            "_TaBlEpRiMs",
            "_tableprims2",
            " _tableprims",
        ] {
            assert_eq!(
                policy_namespace(&format!(r#""sheet": "{allowed}""#))
                    .unwrap()
                    .sheet(),
                Some(allowed),
                "{allowed}"
            );
        }
        let cases = [
            (r#", "quantize": {}"#, DocumentError::Invalid("quantize")),
            (r#", "quantize": ["a"]"#, DocumentError::NotAnObject),
            (
                r#", "quantize": [{"scale": 1, "rounding": "half-even"}]"#,
                DocumentError::Missing("column"),
            ),
            (
                r#", "quantize": [{"column": "a", "rounding": "half-even"}]"#,
                DocumentError::Missing("scale"),
            ),
            (
                r#", "quantize": [{"column": "a", "scale": 1}]"#,
                DocumentError::Missing("rounding"),
            ),
            (
                r#", "quantize": [{"column": "a", "scale": 256, "rounding": "half-even"}]"#,
                DocumentError::Invalid("scale"),
            ),
            (
                r#", "quantize": [{"column": "a", "scale": 39, "rounding": "half-even"}]"#,
                DocumentError::Invalid("scale"),
            ),
            (
                r#", "quantize": [{"column": "a", "scale": 255, "rounding": "half-even"}]"#,
                DocumentError::Invalid("scale"),
            ),
            (
                r#", "quantize": [{"column": "a", "scale": 1, "rounding": "half-up"}]"#,
                DocumentError::Invalid("rounding"),
            ),
            (
                r#", "quantize": [{"column": "a", "scale": 1, "rounding": "half-even"}, {"column": "a", "scale": 2, "rounding": "half-even"}]"#,
                DocumentError::DuplicateQuantize,
            ),
        ];
        for (rest, expected) in cases {
            assert_eq!(policy_with(rest), Err(expected), "{rest}");
        }
        assert!(policy_with(
            r#", "quantize": [{"column": "a", "scale": 38, "rounding": "half-even"}]"#
        )
        .is_ok());
        let cases = [
            (r#""limits": []"#, DocumentError::NotAnObject),
            (
                r#""limits": {"max_rows": "10"}"#,
                DocumentError::Invalid("max_rows"),
            ),
            (
                r#""limits": {"max_line_bytes": null}"#,
                DocumentError::Invalid("max_line_bytes"),
            ),
        ];
        for (content, expected) in cases {
            assert_eq!(policy_namespace(content), Err(expected), "{content}");
        }
    }

    #[test]
    fn quantize_plan_follows_schema_order_and_keeps_precision() {
        let schema = parse_schema(SCHEMA).unwrap();
        let policy = policy_with(
            r#", "quantize": [{"column": "amount", "scale": 2, "rounding": "toward-zero"}]"#,
        )
        .unwrap();
        let plan = policy.quantize_plan(&schema).unwrap();
        assert_eq!(plan.len(), 7);
        assert_eq!(
            plan[2],
            Some((DecimalType::new(12, 2).unwrap(), RoundingMode::TowardZero))
        );
        assert!(plan.iter().enumerate().all(|(i, p)| i == 2 || p.is_none()));
        assert!(policy_with("")
            .unwrap()
            .quantize_plan(&schema)
            .unwrap()
            .iter()
            .all(Option::is_none));
    }

    #[test]
    fn quantize_plan_refuses_bad_targets() {
        let schema = parse_schema(SCHEMA).unwrap();
        let plan = |column: &str, scale: u8| {
            policy_with(&format!(
                r#", "quantize": [{{"column": "{column}", "scale": {scale}, "rounding": "half-even"}}]"#
            ))
            .unwrap()
            .quantize_plan(&schema)
        };
        assert_eq!(
            plan("missing", 2),
            Err(DocumentError::QuantizeUnknownColumn)
        );
        assert_eq!(plan("qty", 0), Err(DocumentError::QuantizeNotDecimal));
        assert_eq!(plan("invoice", 0), Err(DocumentError::QuantizeNotDecimal));
        assert_eq!(
            plan("amount", 13),
            Err(DocumentError::QuantizeScaleTooLarge)
        );
        assert!(plan("amount", 12).is_ok());
        assert!(plan("amount", 0).is_ok());
    }

    #[test]
    fn messages_do_not_contain_document_content() {
        let err =
            schema_with(r#"{"id": "a", "label": "", "type": "secret-9999", "nullable": true}"#)
                .unwrap_err();
        assert!(!err.to_string().contains("secret"));
        let err = policy_with(r#", "secret-9999": 1"#).unwrap_err();
        assert!(!err.to_string().contains("secret"));
        let err = policy_extended(r#"{"secret-9999": {}}"#).unwrap_err();
        assert!(!err.to_string().contains("secret"));
        let err = policy_namespace(r#""secret-9999": 1"#).unwrap_err();
        assert!(!err.to_string().contains("secret"));
    }
}
