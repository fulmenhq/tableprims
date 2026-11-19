// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tablebolt xlsx write`: rows, a schema and a policy in; a workbook and a
//! result file out.
//!
//! Nothing is published under a name that already exists. The workbook is
//! written beside its destination, synced and then given its name; only
//! then is the result file written the same way. A conversion is complete
//! only when the result file exists, says `ok` and its digest and length
//! match the workbook.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde_json::{json, Map, Value};
use tableprims::documents::{parse_policy, parse_schema, DocumentError, ExactNumberPolicy};
use tableprims_xlsx::{Category, ErrorKind, Outcome, Plan, ResolvedLimits, WriteError};

use crate::digest::{self, HashingReader, HashingWriter, Sum};
use crate::identity;
use crate::publish::{directory, links, same_entry, Pending};

/// Format identifier of the result file.
pub(crate) const RESULT_FORMAT: &str = "tableprims.result/0";
/// Format identifier of the object the policy digest covers.
pub(crate) const RESOLVED_POLICY_FORMAT: &str = "tableprims.resolved-policy/0";
/// Largest schema or policy document, in bytes.
const MAX_DOCUMENT_BYTES: u64 = 16 << 20;

pub(crate) const EXIT_FAILURE: u8 = 1;
pub(crate) const EXIT_RESOURCE_EXHAUSTED: u8 = 33;
pub(crate) const EXIT_PERMISSION_DENIED: u8 = 50;
pub(crate) const EXIT_FILE_NOT_FOUND: u8 = 51;
pub(crate) const EXIT_FILE_READ_ERROR: u8 = 53;
pub(crate) const EXIT_FILE_WRITE_ERROR: u8 = 54;
pub(crate) const EXIT_DATA_INVALID: u8 = 60;
pub(crate) const EXIT_PARSE_ERROR: u8 = 61;
pub(crate) const EXIT_TRANSFORMATION_FAILED: u8 = 62;
pub(crate) const EXIT_SIGNAL_INT: u8 = 130;
pub(crate) const EXIT_USAGE: u8 = 64;

/// The files of one invocation.
pub(crate) struct Files {
    pub(crate) input: PathBuf,
    pub(crate) schema: PathBuf,
    pub(crate) policy: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) result: PathBuf,
}

/// Why a run did not complete.
#[derive(Debug)]
struct Failure {
    category: &'static str,
    rule: &'static str,
    row: Option<u64>,
    column: Option<String>,
    exit: u8,
}

impl Failure {
    fn new(category: Category, rule: &'static str, exit: u8) -> Self {
        Self {
            category: category.as_str(),
            rule,
            row: None,
            column: None,
            exit,
        }
    }

    /// `refused` for a run the inputs or the policy ruled out, `cancelled`
    /// for one that was stopped, `failed` for everything else.
    fn status(&self) -> &'static str {
        match self.exit {
            EXIT_DATA_INVALID | EXIT_PARSE_ERROR | EXIT_TRANSFORMATION_FAILED => "refused",
            EXIT_SIGNAL_INT => "cancelled",
            _ => "failed",
        }
    }
}

/// The rule for an I/O failure that is the system running out of storage
/// or memory.
fn exhaustion(error: &io::Error) -> Option<&'static str> {
    match error.kind() {
        io::ErrorKind::StorageFull => Some("storage-full"),
        io::ErrorKind::QuotaExceeded => Some("quota-exceeded"),
        io::ErrorKind::FileTooLarge => Some("file-too-large"),
        io::ErrorKind::OutOfMemory => Some("out-of-memory"),
        _ => None,
    }
}

/// The rules for failing to read one of the three inputs.
struct ReadRules {
    not_found: &'static str,
    permission_denied: &'static str,
    read_failed: &'static str,
}

const ROWS: ReadRules = ReadRules {
    not_found: "rows-not-found",
    permission_denied: "rows-permission-denied",
    read_failed: "rows-read-failed",
};
const SCHEMA: ReadRules = ReadRules {
    not_found: "schema-not-found",
    permission_denied: "schema-permission-denied",
    read_failed: "schema-read-failed",
};
const POLICY: ReadRules = ReadRules {
    not_found: "policy-not-found",
    permission_denied: "policy-permission-denied",
    read_failed: "policy-read-failed",
};

fn read_failure(rules: &ReadRules, error: &io::Error) -> Failure {
    if let Some(rule) = exhaustion(error) {
        return Failure::new(Category::ResourceLimit, rule, EXIT_RESOURCE_EXHAUSTED);
    }
    match error.kind() {
        io::ErrorKind::NotFound => {
            Failure::new(Category::IoFailure, rules.not_found, EXIT_FILE_NOT_FOUND)
        }
        io::ErrorKind::PermissionDenied => Failure::new(
            Category::IoFailure,
            rules.permission_denied,
            EXIT_PERMISSION_DENIED,
        ),
        _ => Failure::new(Category::IoFailure, rules.read_failed, EXIT_FILE_READ_ERROR),
    }
}

fn write_failure(rule: &'static str, error: &io::Error) -> Failure {
    if let Some(rule) = exhaustion(error) {
        return Failure::new(Category::ResourceLimit, rule, EXIT_RESOURCE_EXHAUSTED);
    }
    let exit = match error.kind() {
        io::ErrorKind::PermissionDenied => EXIT_PERMISSION_DENIED,
        _ => EXIT_FILE_WRITE_ERROR,
    };
    Failure::new(Category::IoFailure, rule, exit)
}

/// Maps an adapter error to a category, rule and exit code.
fn adapter_failure(error: &WriteError) -> Failure {
    let exit = match (error.category(), error.kind()) {
        (_, ErrorKind::Read(source)) if !error.is_system_exhaustion() => {
            return Failure {
                row: error.row(),
                ..read_failure(&ROWS, source)
            };
        }
        (Category::InvalidInput, _) => EXIT_PARSE_ERROR,
        (Category::SchemaMismatch, _) => EXIT_DATA_INVALID,
        (Category::UnsupportedType | Category::UnsupportedRepresentation, _) => {
            EXIT_TRANSFORMATION_FAILED
        }
        (Category::ResourceLimit, _) if error.is_system_exhaustion() => EXIT_RESOURCE_EXHAUSTED,
        (Category::ResourceLimit, _) => EXIT_DATA_INVALID,
        (Category::IoFailure, ErrorKind::InputChanged) => EXIT_FILE_READ_ERROR,
        (Category::IoFailure, ErrorKind::Write(source) | ErrorKind::Staging(source))
            if source.kind() == io::ErrorKind::PermissionDenied =>
        {
            EXIT_PERMISSION_DENIED
        }
        (Category::IoFailure, _) => EXIT_FILE_WRITE_ERROR,
        (Category::Cancelled, _) => EXIT_SIGNAL_INT,
        _ => EXIT_FAILURE,
    };
    Failure {
        category: error.category().as_str(),
        rule: error.rule(),
        row: error.row(),
        column: error.column().map(str::to_owned),
        exit,
    }
}

/// The rules for a schema or policy document that was refused.
struct DocumentRules {
    not_utf8: &'static str,
    too_large: &'static str,
    json: &'static str,
    not_an_object: &'static str,
    format: &'static str,
    missing: &'static str,
    unknown: &'static str,
    invalid: &'static str,
    other: &'static str,
}

const SCHEMA_DOCUMENT: DocumentRules = DocumentRules {
    not_utf8: "schema-not-utf8",
    too_large: "schema-too-large",
    json: "schema-malformed",
    not_an_object: "schema-not-an-object",
    format: "schema-format-unsupported",
    missing: "schema-member-missing",
    unknown: "schema-member-unknown",
    invalid: "schema-member-invalid",
    other: "schema-invalid",
};
const POLICY_DOCUMENT: DocumentRules = DocumentRules {
    not_utf8: "policy-not-utf8",
    too_large: "policy-too-large",
    json: "policy-malformed",
    not_an_object: "policy-not-an-object",
    format: "policy-format-unsupported",
    missing: "policy-member-missing",
    unknown: "policy-member-unknown",
    invalid: "policy-member-invalid",
    other: "policy-invalid",
};

fn document_failure(rules: &DocumentRules, error: &DocumentError) -> Failure {
    let invalid = |rule| Failure::new(Category::InvalidInput, rule, EXIT_PARSE_ERROR);
    match error {
        DocumentError::Json(_) => invalid(rules.json),
        DocumentError::NotAnObject => invalid(rules.not_an_object),
        DocumentError::UnsupportedFormat => invalid(rules.format),
        DocumentError::Missing(_) => invalid(rules.missing),
        DocumentError::UnknownMember => invalid(rules.unknown),
        DocumentError::Invalid(_) => invalid(rules.invalid),
        DocumentError::UnsupportedType => Failure::new(
            Category::UnsupportedType,
            "schema-type-unsupported",
            EXIT_TRANSFORMATION_FAILED,
        ),
        DocumentError::Schema(_) => invalid("schema-columns-invalid"),
        DocumentError::DuplicateQuantize => invalid("policy-quantize-duplicate"),
        _ => invalid(rules.other),
    }
}

/// What a run has established so far. Everything here goes into the result
/// file whether or not the run completes.
#[derive(Default)]
struct Report {
    rows: Option<Sum>,
    schema: Option<Sum>,
    policy: Option<Sum>,
    limits: Option<ResolvedLimits>,
    policy_digest: Option<String>,
}

/// A completed conversion.
struct Done {
    outcome: Outcome,
    output: Sum,
}

fn sum_json(sum: &Sum) -> Value {
    json!({ "sha256": sum.sha256, "bytes": sum.bytes })
}

fn exact_name(policy: ExactNumberPolicy) -> &'static str {
    match policy {
        ExactNumberPolicy::RequireExact => "require-exact",
        ExactNumberPolicy::TextIfNeeded => "text-if-needed",
        _ => "text-always",
    }
}

fn limits_json(limits: ResolvedLimits) -> Value {
    json!({
        "max_rows": limits.max_rows,
        "max_line_bytes": limits.max_line_bytes,
        "max_empty_ranges": limits.max_empty_ranges,
    })
}

fn profile_json() -> Value {
    json!({
        "id": tableprims_xlsx::PROFILE_ID,
        "sha256": digest::sha256(tableprims_xlsx::profile_definition().as_bytes()),
    })
}

/// The policy a plan runs under with every default filled in, as the JSON
/// object the policy digest covers. Quantize entries are in schema order.
pub(crate) fn resolved_policy(plan: &Plan) -> Value {
    let quantize: Vec<Value> = plan
        .schema()
        .columns()
        .iter()
        .zip(plan.quantize())
        .filter_map(|(column, request)| {
            let (target, mode) = (*request)?;
            Some(json!({
                "column": column.id(),
                "scale": target.scale(),
                "rounding": tableprims_xlsx::metadata::rounding_name(mode)?,
            }))
        })
        .collect();
    json!({
        "format": RESOLVED_POLICY_FORMAT,
        "exact_numbers": exact_name(plan.exact_numbers()),
        "quantize": quantize,
        "sheet": plan.sheet(),
        "limits": limits_json(plan.limits()),
        "profile": profile_json(),
    })
}

/// SHA-256 of the RFC 8785 canonical form of the resolved policy.
pub(crate) fn policy_digest(plan: &Plan) -> Option<String> {
    let canonical = digest::canonical_json(resolved_policy(plan).to_string().as_bytes())?;
    Some(digest::sha256(&canonical))
}

/// Reads a schema or policy document completely, under a size limit.
fn read_document(
    path: &Path,
    read: &ReadRules,
    rules: &DocumentRules,
    sum: &mut Option<Sum>,
) -> Result<String, Failure> {
    let file = File::open(path).map_err(|error| read_failure(read, &error))?;
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| read_failure(read, &error))?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(Failure::new(
            Category::ResourceLimit,
            rules.too_large,
            EXIT_DATA_INVALID,
        ));
    }
    *sum = Some(Sum::of(&bytes));
    String::from_utf8(bytes)
        .map_err(|_| Failure::new(Category::InvalidInput, rules.not_utf8, EXIT_PARSE_ERROR))
}

fn open_rows(path: &Path) -> Result<BufReader<HashingReader<File>>, Failure> {
    let file = File::open(path).map_err(|error| read_failure(&ROWS, &error))?;
    Ok(BufReader::with_capacity(64 << 10, HashingReader::new(file)))
}

fn convert(files: &Files, token: &str, report: &mut Report) -> Result<Done, Failure> {
    if files.output.exists() {
        return Err(Failure::new(
            Category::IoFailure,
            "output-exists",
            EXIT_FILE_WRITE_ERROR,
        ));
    }
    let schema_text = read_document(&files.schema, &SCHEMA, &SCHEMA_DOCUMENT, &mut report.schema)?;
    let schema =
        parse_schema(&schema_text).map_err(|error| document_failure(&SCHEMA_DOCUMENT, &error))?;
    let policy_text = read_document(&files.policy, &POLICY, &POLICY_DOCUMENT, &mut report.policy)?;
    let policy =
        parse_policy(&policy_text).map_err(|error| document_failure(&POLICY_DOCUMENT, &error))?;
    let plan = Plan::new(&schema, &policy).map_err(|error| adapter_failure(&error))?;
    let limits = plan.limits();
    report.limits = Some(limits);
    report.policy_digest =
        Some(policy_digest(&plan).ok_or_else(|| {
            Failure::new(Category::Internal, "policy-digest-failed", EXIT_FAILURE)
        })?);

    // No signal handler is installed: an interrupt ends the process and
    // leaves no result file.
    let cancel = AtomicBool::new(false);
    let mut first = open_rows(&files.input)?;
    let scan = plan
        .scan(&mut first, &cancel)
        .map_err(|error| adapter_failure(&error))?;
    let first = first
        .into_inner()
        .finish()
        .map_err(|error| read_failure(&ROWS, &error))?;
    report.rows = Some(first.clone());

    let (pending, file) = Pending::create(&files.output, token, "workbook")
        .map_err(|error| write_failure("output-create-failed", &error))?;
    let mut sink = HashingWriter::new(BufWriter::new(file));
    let mut second = open_rows(&files.input)?;
    // Row data is staged beside the destination and nowhere else.
    let staging = directory(&files.output);
    let outcome = scan
        .write(&mut second, &mut sink, Some(staging), &cancel)
        .map_err(|error| adapter_failure(&error))?;
    let second = second
        .into_inner()
        .finish()
        .map_err(|error| read_failure(&ROWS, &error))?;
    if second != first {
        return Err(Failure::new(
            Category::IoFailure,
            "input-changed",
            EXIT_FILE_READ_ERROR,
        ));
    }
    let (buffered, output) = sink.finish();
    let file = buffered
        .into_inner()
        .map_err(|error| write_failure("output-write-failed", error.error()))?;
    file.sync_all()
        .map_err(|error| write_failure("output-write-failed", &error))?;
    drop(file);
    pending.publish().map_err(|error| match error.kind() {
        io::ErrorKind::AlreadyExists => {
            Failure::new(Category::IoFailure, "output-exists", EXIT_FILE_WRITE_ERROR)
        }
        _ => write_failure("output-publish-failed", &error),
    })?;
    Ok(Done { outcome, output })
}

fn counts_json(outcome: &Outcome) -> Value {
    let columns: Vec<Value> = outcome
        .columns
        .iter()
        .map(|column| {
            let mut entry = Map::new();
            entry.insert("id".into(), json!(column.id));
            entry.insert(
                "representation".into(),
                json!(column.representation.as_str()),
            );
            entry.insert("null".into(), json!(column.null));
            entry.insert("empty".into(), json!(column.empty));
            if column.exact_number {
                entry.insert("numeric".into(), json!(column.numeric));
                entry.insert("text_fallback".into(), json!(column.text_fallback));
                entry.insert("text_forced".into(), json!(column.text_forced));
                entry.insert("quantized".into(), json!(column.quantized));
                entry.insert("scale_changed".into(), json!(column.scale_changed));
            }
            Value::Object(entry)
        })
        .collect();
    json!({ "rows": outcome.rows, "columns": columns })
}

fn result_json(run_id: &str, report: &Report, end: &Result<Done, Failure>) -> Value {
    let mut result = Map::new();
    result.insert("format".into(), json!(RESULT_FORMAT));
    result.insert("run_id".into(), json!(run_id));
    result.insert(
        "tool".into(),
        json!({
            "name": identity::BINARY_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "library": tableprims::VERSION,
        }),
    );
    result.insert(
        "backend".into(),
        json!({
            "name": tableprims_xlsx::BACKEND_NAME,
            "version": tableprims_xlsx::BACKEND_VERSION,
        }),
    );
    result.insert("profile".into(), profile_json());
    let mut inputs = Map::new();
    for (name, sum) in [
        ("rows", &report.rows),
        ("schema", &report.schema),
        ("policy", &report.policy),
    ] {
        if let Some(sum) = sum {
            inputs.insert(name.into(), sum_json(sum));
        }
    }
    result.insert("inputs".into(), Value::Object(inputs));
    if let Some(digest) = &report.policy_digest {
        result.insert("policy_digest".into(), json!(digest));
    }
    if let Some(limits) = report.limits {
        result.insert("limits".into(), limits_json(limits));
    }
    match end {
        Ok(done) => {
            result.insert("status".into(), json!("ok"));
            result.insert("output".into(), sum_json(&done.output));
            result.insert("counts".into(), counts_json(&done.outcome));
        }
        Err(failure) => {
            result.insert("status".into(), json!(failure.status()));
            let mut refusal = Map::new();
            refusal.insert("category".into(), json!(failure.category));
            refusal.insert("rule".into(), json!(failure.rule));
            if let Some(row) = failure.row {
                refusal.insert("row".into(), json!(row));
            }
            if let Some(column) = &failure.column {
                refusal.insert("column".into(), json!(column));
            }
            result.insert("refusal".into(), Value::Object(refusal));
        }
    }
    Value::Object(result)
}

fn publish_result(pending: Pending, mut file: File, result: &Value) -> io::Result<()> {
    file.write_all(format!("{result}\n").as_bytes())?;
    file.sync_all()?;
    drop(file);
    pending.publish()
}

/// 16 bytes from the operating system as lowercase hexadecimal.
fn random_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Checks a caller-supplied run identifier: 1 to 128 characters of
/// `[A-Za-z0-9._-]`.
pub(crate) fn parse_run_id(text: &str) -> Result<String, String> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
    if text.is_empty() || text.len() > 128 || !text.chars().all(allowed) {
        return Err(String::from(
            "a run identifier is 1 to 128 characters of A-Z, a-z, 0-9, '.', '_' and '-'",
        ));
    }
    Ok(text.to_owned())
}

/// Runs the conversion and returns the exit code and a diagnostic line.
pub(crate) fn run(files: &Files, run_id: Option<String>) -> (u8, Option<String>) {
    // The token names this run's temporary files and, without a caller
    // identifier, the run itself. Without one nothing is written.
    let Ok(token) = random_token() else {
        return (
            EXIT_FAILURE,
            Some(String::from("cannot obtain random bytes from the system")),
        );
    };
    let run_id = run_id.unwrap_or_else(|| format!("auto-{token}"));
    if same_entry(&files.output, &files.result) {
        return (
            EXIT_USAGE,
            Some(String::from("--output and --result name the same file")),
        );
    }
    if files.result.exists() {
        return (
            EXIT_FILE_WRITE_ERROR,
            Some(String::from("the result file already exists")),
        );
    }
    // The result file is opened before anything is converted, so a result
    // that cannot be created stops the run before a workbook exists.
    let (pending, file) = match Pending::create(&files.result, &token, "result") {
        Ok(reserved) => reserved,
        Err(error) => {
            return (
                write_failure("result-write-failed", &error).exit,
                Some(format!("cannot create the result file ({})", error.kind())),
            );
        }
    };
    // Files are published by linking. A file system that cannot link is
    // found out before anything is converted.
    if matches!(links(&files.result, &token, "result"), Ok(false)) {
        return (
            EXIT_FILE_WRITE_ERROR,
            Some(String::from(
                "the file system holding --result does not support hard links",
            )),
        );
    }
    let mut report = Report::default();
    let end = match links(&files.output, &token, "workbook") {
        Ok(false) => Err(Failure::new(
            Category::IoFailure,
            "hard-links-unsupported",
            EXIT_FILE_WRITE_ERROR,
        )),
        // Anything else wrong with the directory is reported by the step
        // that needs it.
        _ => convert(files, &token, &mut report),
    };
    let result = result_json(&run_id, &report, &end);
    if let Err(error) = publish_result(pending, file, &result) {
        let exit = write_failure("result-write-failed", &error).exit;
        // A workbook without its result is incomplete. It is left where it
        // is: by now the name may refer to a file that is not this run's.
        let workbook = match &end {
            Ok(_) => "; the workbook at --output has no result and is incomplete",
            Err(_) => "",
        };
        return (
            exit,
            Some(format!(
                "cannot write the result file ({}){workbook}",
                error.kind()
            )),
        );
    }
    match end {
        Ok(_) => (0, None),
        Err(failure) => {
            let mut line = format!(
                "{}: {}: {}",
                failure.status(),
                failure.category,
                failure.rule
            );
            if let Some(row) = failure.row {
                line.push_str(&format!(" at row {row}"));
            }
            if let Some(column) = &failure.column {
                line.push_str(&format!(" in column {column}"));
            }
            (failure.exit, Some(line))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expectations written independently of this program.
    const FIXTURES: &str = include_str!("../tests/fixtures/policy-digest.json");

    #[test]
    fn the_policy_digest_matches_the_published_fixtures() {
        let fixtures: Value = serde_json::from_str(FIXTURES).unwrap();
        assert_eq!(
            fixtures["profile_definition"],
            json!(tableprims_xlsx::profile_definition())
        );
        assert_eq!(fixtures["profile_sha256"], profile_json()["sha256"]);
        let cases = fixtures["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 8);
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let schema = parse_schema(case["schema"].as_str().unwrap()).unwrap();
            let policy = parse_policy(case["policy"].as_str().unwrap()).unwrap();
            let plan = Plan::new(&schema, &policy).unwrap();
            let canonical =
                digest::canonical_json(resolved_policy(&plan).to_string().as_bytes()).unwrap();
            assert_eq!(
                String::from_utf8(canonical).unwrap(),
                case["canonical"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(
                policy_digest(&plan).unwrap(),
                case["sha256"].as_str().unwrap(),
                "{name}"
            );
        }
        let digest = |name: &str| {
            let case = cases.iter().find(|case| case["name"] == name).unwrap();
            case["sha256"].as_str().unwrap().to_owned()
        };
        assert_eq!(digest("defaults"), digest("members reordered"));
        assert_eq!(digest("defaults"), digest("defaults written out"));
        assert_eq!(
            digest("quantize entries in schema order"),
            digest("quantize entries in another order")
        );
        assert_ne!(digest("defaults"), digest("one changed value"));
        assert_ne!(digest("defaults"), digest("a lowered limit"));
    }

    #[test]
    fn the_profile_definition_is_already_canonical() {
        let definition = tableprims_xlsx::profile_definition();
        assert_eq!(
            digest::canonical_json(definition.as_bytes()).unwrap(),
            definition.as_bytes()
        );
    }

    #[test]
    fn run_identifiers_are_bounded() {
        assert!(parse_run_id("case-0001_a.B").is_ok());
        assert!(parse_run_id(&"a".repeat(128)).is_ok());
        for bad in ["", "a b", "a/b", "é", &"a".repeat(129)] {
            assert!(parse_run_id(bad).is_err(), "{bad}");
        }
        let token = random_token().unwrap();
        assert_eq!(token.len(), 32);
        assert!(token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_ne!(token, random_token().unwrap());
    }

    #[test]
    fn exit_codes_follow_the_category() {
        use std::io::ErrorKind as Io;
        let read = |kind: Io| read_failure(&ROWS, &kind.into());
        assert_eq!(
            (read(Io::NotFound).exit, read(Io::NotFound).rule),
            (51, "rows-not-found")
        );
        assert_eq!(read(Io::PermissionDenied).exit, 50);
        assert_eq!(
            (read(Io::Interrupted).exit, read(Io::Interrupted).status()),
            (53, "failed")
        );
        assert_eq!(
            (read(Io::OutOfMemory).exit, read(Io::OutOfMemory).category),
            (33, "resource-limit")
        );
        let write = |kind: Io| write_failure("output-write-failed", &kind.into());
        assert_eq!(write(Io::Other).exit, 54);
        assert_eq!(write(Io::PermissionDenied).exit, 50);
        assert_eq!(
            (write(Io::StorageFull).exit, write(Io::StorageFull).rule),
            (33, "storage-full")
        );
        let refused = Failure::new(Category::SchemaMismatch, "x", EXIT_DATA_INVALID);
        assert_eq!(refused.status(), "refused");
        let cancelled = Failure::new(Category::Cancelled, "cancelled", EXIT_SIGNAL_INT);
        assert_eq!(cancelled.status(), "cancelled");
    }
}
