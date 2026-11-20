// SPDX-License-Identifier: MIT OR Apache-2.0

//! Runs the vectors and fixtures of the Tabular Exchange Standard, contract
//! revision `tabular-exchange/0.1.0`, through `tablebolt xlsx write` and
//! compares each outcome with the expectation the vector states.
//!
//! The files are vendored unedited under `third_party/crucible/`; see
//! `PROVENANCE.md` there. Expectations are read from those files and are
//! never computed here. Every vector is either run or counted in [`LEDGER`]
//! with the reason it is not, and the counts are exact.
//!
//! What this does not show: `tablebolt` writes results and does not read
//! them, so the result fixtures and the matching vectors are not run. The
//! results written during these runs are checked against the rules a test
//! can apply with the facts it holds ([`check_result`]); that is not a
//! reader of results.

#[path = "support/vector_json.rs"]
mod vector_json;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sha2::{Digest, Sha256};
use vector_json::Json;

/// SHA-256 of `third_party/crucible/SHA256SUMS`, which lists the digest of
/// every vendored file. Moving to another upstream release changes this.
const SUMS_SHA256: &str = "e42d4d5282f0c2e3d8f8e671c13475c32a2bddb1de65af67757d4c3143f3c61f";

const SHEET_MEMBER: &str = r#""extensions":{"tableprims":{"sheet":"Data"}}"#;
/// The policy the value, row and schema suites run under. Every exact number
/// is written as text, so no value is refused by the workbook profile.
const POLICY: &str = r#"{"format":"tabular-exchange.policy/0","exact_numbers":"text-always","extensions":{"tableprims":{"sheet":"Data"}}}"#;
/// The schema a policy is read beside when its fixture names none.
const SCHEMA: &str = r#"{"format":"tabular-exchange.schema/0","columns":[{"id":"a","label":"A","type":"string","nullable":true}]}"#;

/// One line of the account of every vector: a suite, a class within it, how
/// many the release holds, how many are run, and why the rest are not.
struct Line {
    suite: &'static str,
    class: &'static str,
    total: usize,
    run: usize,
    reason: &'static str,
}

const fn line(
    suite: &'static str,
    class: &'static str,
    total: usize,
    run: usize,
    reason: &'static str,
) -> Line {
    Line {
        suite,
        class,
        total,
        run,
        reason,
    }
}

const RESULTS_NOT_READ: &str =
    "a result document: this program writes results and does not read them";
const NO_MATCHING: &str = "matching a result to an output and to a request is done by a \
                           consumer: this program writes results and does not read them";

/// The account of every vector and fixture in the release. A class that is
/// not here, or a count that differs, fails
/// `every_vector_is_run_or_accounted_for`; each suite asserts its `run`.
const LEDGER: &[Line] = &[
    line("value-vectors", "integer", 43, 43, ""),
    line("value-vectors", "decimal", 45, 45, ""),
    line("value-vectors", "date", 28, 28, ""),
    line("value-vectors", "local-timestamp", 45, 45, ""),
    line("value-vectors", "offset-timestamp", 39, 39, ""),
    line("value-vectors", "cells", 25, 25, ""),
    line("value-vectors", "quantize", 77, 77, ""),
    line("row-vectors", "cases", 76, 76, ""),
    line("document-vectors", "table-schema", 31, 31, ""),
    line("fixtures", "table-schema", 36, 36, ""),
    line("fixtures", "conversion-policy", 32, 32, ""),
    line("fixtures", "conversion-result", 92, 0, RESULTS_NOT_READ),
    line("matching-vectors", "cases", 70, 0, NO_MATCHING),
];

fn ledger(suite: &str, class: &str) -> &'static Line {
    LEDGER
        .iter()
        .find(|line| line.suite == suite && line.class == class)
        .unwrap_or_else(|| panic!("{suite}/{class} is not in the ledger"))
}

fn vendored() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/crucible")
}

fn data() -> PathBuf {
    vendored().join("v0.5.1/config/data/tabular-exchange/v0.1.0")
}

fn load(name: &str) -> Json {
    let bytes = fs::read(data().join(name)).unwrap();
    vector_json::parse(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tablebolt-te-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ---------------------------------------------------------------------------
// The vendored bytes
// ---------------------------------------------------------------------------

fn files_under(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(root, &path, out);
        } else {
            let relative: Vec<String> = path
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|part| part.as_os_str().to_str().unwrap().to_owned())
                .collect();
            out.insert(relative.join("/"));
        }
    }
}

/// Compares the files under `root` with a list of `<sha256>  <path>` lines
/// and returns every difference: a file whose bytes differ, a listed file
/// that is absent and a file that is not listed.
fn verify_tree(root: &Path, sums: &str) -> Vec<String> {
    let mut found = BTreeSet::new();
    files_under(root, root, &mut found);
    let mut problems = Vec::new();
    let mut listed = BTreeSet::new();
    for line in sums.lines() {
        let Some((digest, path)) = line.split_once("  ") else {
            problems.push(format!("unreadable line: {line}"));
            continue;
        };
        if !listed.insert(path.to_owned()) {
            problems.push(format!("listed twice: {path}"));
        }
        match fs::read(root.join(path)) {
            Ok(bytes) if sha256_hex(&bytes) == digest => {}
            Ok(_) => problems.push(format!("changed: {path}")),
            Err(_) => problems.push(format!("missing: {path}")),
        }
    }
    for path in found.difference(&listed) {
        problems.push(format!("not listed: {path}"));
    }
    problems
}

#[test]
fn the_vendored_files_are_the_bytes_of_the_release() {
    let sums = fs::read(vendored().join("SHA256SUMS")).unwrap();
    assert_eq!(sha256_hex(&sums), SUMS_SHA256, "SHA256SUMS was changed");
    let sums = String::from_utf8(sums).unwrap();
    assert_eq!(sums.lines().count(), 173);
    let problems = verify_tree(&vendored().join("v0.5.1"), &sums);
    assert!(problems.is_empty(), "{problems:#?}");
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

/// Fault control: one changed byte, a removed file and an added file in a
/// copy of the vendored tree are each reported.
#[test]
fn a_changed_removed_or_added_vendored_file_is_reported() {
    let dir = scratch("tree");
    let copy = dir.join("v0.5.1");
    copy_tree(&vendored().join("v0.5.1"), &copy);
    let sums = fs::read_to_string(vendored().join("SHA256SUMS")).unwrap();
    assert!(verify_tree(&copy, &sums).is_empty());

    let vectors = copy.join("config/data/tabular-exchange/v0.1.0/value-vectors.json");
    let mut bytes = fs::read(&vectors).unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 1;
    fs::write(&vectors, &bytes).unwrap();
    fs::remove_file(copy.join("schemas/data/tabular-exchange/v0.1.0/README.md")).unwrap();
    fs::write(copy.join("schemas/extra.json"), b"{}\n").unwrap();
    assert_eq!(
        verify_tree(&copy, &sums),
        [
            "changed: config/data/tabular-exchange/v0.1.0/value-vectors.json",
            "missing: schemas/data/tabular-exchange/v0.1.0/README.md",
            "not listed: schemas/extra.json",
        ]
    );

    // A line ending rewritten on checkout is a changed file.
    let license = copy.join("LICENSE-CC0");
    let text = fs::read_to_string(&license).unwrap();
    fs::write(&license, text.replace('\n', "\r\n")).unwrap();
    assert!(verify_tree(&copy, &sums).contains(&"changed: LICENSE-CC0".to_owned()));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The account of every vector
// ---------------------------------------------------------------------------

/// Every vector and fixture in the release, as `(suite, class) -> count`.
fn census(
    contract: &Json,
    values: &Json,
    rows: &Json,
    documents: &Json,
    matching: &Json,
    manifest: &Json,
) -> Result<BTreeMap<(String, String), usize>, String> {
    let files: Vec<&str> = contract
        .at("vectors")
        .items()
        .iter()
        .map(Json::text)
        .collect();
    if files
        != [
            "value-vectors.json",
            "row-vectors.json",
            "document-vectors.json",
            "matching-vectors.json",
        ]
        || contract.at("fixtures").text() != "fixtures/manifest.json"
    {
        return Err(format!("the contract names other vector files: {files:?}"));
    }
    let mut count = BTreeMap::new();
    let mut add = |suite: &str, class: &str, n: usize| {
        *count
            .entry((suite.to_owned(), class.to_owned()))
            .or_insert(0) += n;
    };
    for (name, value) in values.members() {
        match value {
            Json::Array(items) => add("value-vectors", name, items.len()),
            Json::String { .. } => {}
            _ => return Err(format!("value-vectors: unexpected member {name}")),
        }
    }
    for (suite, file) in [("row-vectors", rows), ("matching-vectors", matching)] {
        for (name, value) in file.members() {
            match (name.as_str(), value) {
                ("cases", Json::Array(items)) => add(suite, "cases", items.len()),
                (_, Json::Array(_)) => return Err(format!("{suite}: unexpected list {name}")),
                _ => {}
            }
        }
    }
    for (name, value) in documents.members() {
        match (name.as_str(), value) {
            ("cases", Json::Array(items)) => {
                for case in items {
                    add("document-vectors", case.at("document").text(), 1);
                }
            }
            (_, Json::Array(_)) => return Err(format!("document-vectors: unexpected list {name}")),
            _ => {}
        }
    }
    for fixture in manifest.at("fixtures").items() {
        add("fixtures", fixture.at("document").text(), 1);
    }
    Ok(count)
}

/// Compares the census with the ledger; returns every difference.
fn audit(census: &BTreeMap<(String, String), usize>) -> Vec<String> {
    let mut problems = Vec::new();
    for ((suite, class), total) in census {
        match LEDGER
            .iter()
            .find(|line| line.suite == suite && line.class == class)
        {
            None => problems.push(format!(
                "{suite}/{class}: {total} vectors not in the ledger"
            )),
            Some(line) if line.total != *total => problems.push(format!(
                "{suite}/{class}: the ledger says {}, the release holds {total}",
                line.total
            )),
            Some(_) => {}
        }
    }
    for line in LEDGER {
        let key = (line.suite.to_owned(), line.class.to_owned());
        if !census.contains_key(&key) {
            problems.push(format!("{}/{}: not in the release", line.suite, line.class));
        }
        if line.run > line.total || (line.run < line.total && line.reason.is_empty()) {
            problems.push(format!(
                "{}/{}: {} of {} run and no reason is given",
                line.suite, line.class, line.run, line.total
            ));
        }
    }
    problems
}

fn fixture_files(kind: &str) -> BTreeSet<String> {
    let root = data();
    let mut all = BTreeSet::new();
    files_under(&root, &root.join("fixtures"), &mut all);
    all.into_iter()
        .filter(|path| path.contains(&format!("/{kind}/")))
        .collect()
}

#[test]
fn every_vector_is_run_or_accounted_for() {
    let manifest = load("fixtures/manifest.json");
    let count = census(
        &load("contract.json"),
        &load("value-vectors.json"),
        &load("row-vectors.json"),
        &load("document-vectors.json"),
        &load("matching-vectors.json"),
        &manifest,
    )
    .unwrap();
    let problems = audit(&count);
    assert!(problems.is_empty(), "{problems:#?}");
    assert_eq!(LEDGER.iter().map(|line| line.total).sum::<usize>(), 639);
    assert_eq!(LEDGER.iter().map(|line| line.run).sum::<usize>(), 477);

    // The manifest lists every fixture file, once.
    let mut listed = BTreeSet::new();
    for fixture in manifest.at("fixtures").items() {
        assert!(listed.insert(fixture.at("path").text().to_owned()));
    }
    let root = data();
    let mut present = BTreeSet::new();
    files_under(&root, &root.join("fixtures"), &mut present);
    present.remove("fixtures/manifest.json");
    assert_eq!(listed, present);
    assert_eq!(fixture_files("conversion-result").len(), 92);
}

/// Fault control: a vector the ledger does not count, a class it does not
/// name and a class left unrun without a reason are each reported.
#[test]
fn a_vector_outside_the_ledger_is_reported() {
    let contract = load("contract.json");
    let mut values = load("value-vectors.json");
    let rows = load("row-vectors.json");
    let documents = load("document-vectors.json");
    let matching = load("matching-vectors.json");
    let manifest = load("fixtures/manifest.json");

    let Json::Array(mut integers) = values.at("integer").clone() else {
        panic!()
    };
    integers.push(integers[0].clone());
    values.set("integer", Json::Array(integers));
    values.set("duration", Json::Array(vec![Json::Null]));
    let count = census(&contract, &values, &rows, &documents, &matching, &manifest).unwrap();
    assert_eq!(
        audit(&count),
        [
            "value-vectors/duration: 1 vectors not in the ledger",
            "value-vectors/integer: the ledger says 43, the release holds 44",
        ]
    );

    // A document vector of a kind no suite runs.
    let mut other = documents.clone();
    let Json::Array(mut cases) = other.at("cases").clone() else {
        panic!()
    };
    cases[0].set("document", Json::string("conversion-result"));
    other.set("cases", Json::Array(cases));
    let count = census(&contract, &values, &rows, &other, &matching, &manifest).unwrap();
    assert!(audit(&count)
        .contains(&"document-vectors/conversion-result: 1 vectors not in the ledger".to_owned()));

    // A vector file the contract names and no suite reads.
    let mut renamed = contract.clone();
    renamed.set("vectors", Json::Array(vec![Json::string("more.json")]));
    assert!(census(&renamed, &values, &rows, &documents, &matching, &manifest).is_err());
}

// ---------------------------------------------------------------------------
// Running the binary
// ---------------------------------------------------------------------------

/// The closed lists of the contract that a written result is checked
/// against, read from the vendored `contract.json` and result schema.
struct Contract {
    revision: String,
    result_format: String,
    result_members: Vec<String>,
    result_required: Vec<String>,
    statuses: BTreeMap<String, Vec<String>>,
    representations: Vec<String>,
    max_count: u64,
    max_rule_length: usize,
    /// Registered rule identifier to its category.
    rules: BTreeMap<String, String>,
    /// Registered rules that are located at a row and a column.
    cell_rules: BTreeSet<String>,
}

impl Contract {
    fn load() -> Self {
        let contract = load("contract.json");
        let texts = |value: &Json| -> Vec<String> {
            value
                .items()
                .iter()
                .map(|item| item.text().to_owned())
                .collect()
        };
        let schema =
            vector_json::parse(
                &fs::read(vendored().join(
                    "v0.5.1/schemas/data/tabular-exchange/v0.1.0/conversion-result.schema.json",
                ))
                .unwrap(),
            )
            .unwrap();
        let mut rules = BTreeMap::new();
        let mut cell_rules = BTreeSet::new();
        for rule in contract.at("rules").items() {
            let name = rule.at("rule").text().to_owned();
            if let Some(located) = rule.get("located_at") {
                if texts(located) == ["row", "column"] {
                    cell_rules.insert(name.clone());
                }
            }
            rules.insert(name, rule.at("category").text().to_owned());
        }
        let number = |value: &Json| -> u64 {
            match value {
                Json::Number(spelling) => spelling.parse().unwrap(),
                other => panic!("{}", other.compact()),
            }
        };
        Self {
            revision: contract.at("contract").text().to_owned(),
            result_format: contract
                .at("formats")
                .at("conversion-result")
                .text()
                .to_owned(),
            result_members: schema
                .at("properties")
                .members()
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            result_required: texts(schema.at("required")),
            statuses: contract
                .at("statuses")
                .members()
                .iter()
                .map(|(status, categories)| (status.clone(), texts(categories)))
                .collect(),
            representations: texts(contract.at("representations")),
            max_count: number(contract.at("bounds").at("max_count")),
            max_rule_length: usize::try_from(number(contract.at("bounds").at("max_rule_length")))
                .unwrap(),
            rules,
            cell_rules,
        }
    }
}

/// What the test holds about one run, independently of the result.
struct Request<'a> {
    run_id: &'a str,
    schema: &'a [u8],
    policy: &'a [u8],
    source: &'a [u8],
    /// The workbook at the `--output` name, when there is one.
    workbook: Option<&'a [u8]>,
}

fn is_rule_identifier(text: &str) -> bool {
    !text.is_empty()
        && text.split('-').enumerate().all(|(index, part)| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                && (index > 0 || part.as_bytes()[0].is_ascii_lowercase())
        })
}

fn is_sha256(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        text.len() == 64
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// Checks a result this program wrote against the rules of the standard that
/// can be applied with the facts the test holds: the members and closed
/// lists, which members go with which status, the category of a registered
/// rule, the identity of the run and of each input, the digest of the
/// workbook, and the arithmetic of the counts. Returns every rule broken.
///
/// This is not a reader of results: it does not judge bytes or grammar, the
/// order of reading, or the columns against a schema and a policy.
fn check_result(contract: &Contract, result: &Value, request: &Request) -> Vec<String> {
    let mut problems = Vec::new();
    let mut bad = |what: String| problems.push(what);
    let Some(top) = result.as_object() else {
        return vec!["the result is not an object".to_owned()];
    };
    for name in top.keys() {
        if !contract.result_members.contains(name) {
            bad(format!("member {name} is not defined"));
        }
    }
    for name in &contract.result_required {
        if !top.contains_key(name) {
            bad(format!("member {name} is absent"));
        }
    }
    if result["format"] != contract.result_format.as_str() {
        bad(format!("format {}", result["format"]));
    }
    if result["contract"] != contract.revision.as_str() {
        bad(format!("contract {}", result["contract"]));
    }
    if result["run_id"] != request.run_id {
        bad(format!("run_id {}", result["run_id"]));
    }
    let count = |value: &Value| value.as_u64().filter(|n| *n <= contract.max_count);

    let status = result["status"].as_str().unwrap_or("");
    let Some(categories) = contract.statuses.get(status) else {
        bad(format!("status {}", result["status"]));
        return problems;
    };
    let ok = status == "ok";
    for name in ["output", "counts"] {
        if top.contains_key(name) != ok {
            bad(format!("{name} with status {status}"));
        }
    }
    if top.contains_key("refusal") == ok {
        bad(format!("refusal with status {status}"));
    }
    if request.workbook.is_some() != ok {
        bad(format!("workbook present with status {status}"));
    }

    // Every input named is the file that was supplied.
    let held = [
        ("source", request.source),
        ("schema", request.schema),
        ("policy", request.policy),
    ];
    match result["inputs"].as_object() {
        None => bad("inputs is not an object".to_owned()),
        Some(inputs) => {
            for name in inputs.keys() {
                if !held.iter().any(|(known, _)| known == name) {
                    bad(format!("inputs.{name} is not defined"));
                }
            }
            for (name, bytes) in held {
                match inputs.get(name) {
                    None if ok => bad(format!("inputs.{name} is absent from an ok result")),
                    None => {}
                    Some(summary) => {
                        if !is_sha256(&summary["sha256"])
                            || summary["sha256"] != sha256_hex(bytes).as_str()
                            || count(&summary["bytes"]) != Some(bytes.len() as u64)
                            || summary.as_object().is_none_or(|m| m.len() != 2)
                        {
                            bad(format!("inputs.{name} is not the file supplied"));
                        }
                    }
                }
            }
        }
    }

    if let Some(refusal) = top.get("refusal") {
        let category = refusal["category"].as_str().unwrap_or("");
        let rule = refusal["rule"].as_str().unwrap_or("");
        if !categories.iter().any(|known| known == category) {
            bad(format!("category {category} with status {status}"));
        }
        if !is_rule_identifier(rule) || rule.len() > contract.max_rule_length {
            bad(format!("rule {rule:?} is not a rule identifier"));
        }
        if let Some(registered) = contract.rules.get(rule) {
            if registered != category {
                bad(format!(
                    "rule {rule} is registered under {registered}, not {category}"
                ));
            }
        }
        for (name, value) in refusal.as_object().into_iter().flatten() {
            let valid = match name.as_str() {
                "category" | "rule" => true,
                "row" => count(value).is_some(),
                "column" => value.as_str().is_some_and(|text| !text.is_empty()),
                _ => false,
            };
            if !valid {
                bad(format!("refusal.{name} is {value}"));
            }
        }
    }

    if let (Some(output), Some(workbook)) = (top.get("output"), request.workbook) {
        if output["sha256"] != sha256_hex(workbook).as_str()
            || count(&output["bytes"]) != Some(workbook.len() as u64)
            || output.as_object().is_none_or(|m| m.len() != 2)
        {
            bad("output is not the workbook".to_owned());
        }
    }

    if let Some(counts) = top.get("counts") {
        let rows = count(&counts["rows"]);
        let columns = counts["columns"].as_array().cloned().unwrap_or_default();
        if rows.is_none() || columns.is_empty() {
            bad("counts has no rows or no columns".to_owned());
        }
        let rows = rows.unwrap_or(0);
        let mut seen = BTreeSet::new();
        for column in &columns {
            let id = column["id"].as_str().unwrap_or("");
            if id.is_empty() || !seen.insert(id.to_owned()) {
                bad(format!("column id {id:?} is empty or repeated"));
            }
            let representation = column["representation"].as_str().unwrap_or("");
            if !contract
                .representations
                .iter()
                .any(|known| known == representation)
            {
                bad(format!("{id}: representation {representation:?}"));
            }
            let (Some(null), Some(empty)) = (count(&column["null"]), count(&column["empty"]))
            else {
                bad(format!("{id}: null or empty is not a count"));
                continue;
            };
            if null > rows || empty > rows - null {
                bad(format!("{id}: null and empty exceed the rows"));
                continue;
            }
            let five = [
                "numeric",
                "text_fallback",
                "text_forced",
                "quantized",
                "scale_changed",
            ];
            let have: Vec<Option<u64>> = five
                .iter()
                .filter(|name| column.get(**name).is_some())
                .map(|name| count(&column[*name]))
                .collect();
            if have.is_empty() {
                if representation == "text-fallback" || representation == "text-forced" {
                    bad(format!("{id}: {representation} without counters"));
                }
                continue;
            }
            if representation == "text" || have.len() != five.len() || have.contains(&None) {
                bad(format!("{id}: the five counters are not all present"));
                continue;
            }
            let have: Vec<u64> = have.into_iter().flatten().collect();
            let (numeric, fallback, forced, quantized, scale_changed) =
                (have[0], have[1], have[2], have[3], have[4]);
            let non_null = rows - null;
            if numeric
                .checked_add(fallback)
                .and_then(|n| n.checked_add(forced))
                != Some(non_null)
            {
                bad(format!(
                    "{id}: the counters do not sum to the non-null cells"
                ));
            }
            let own = match representation {
                "native" => numeric,
                "text-fallback" => fallback,
                _ => forced,
            };
            if own != non_null {
                bad(format!("{id}: cells counted outside {representation}"));
            }
            if (scale_changed != 0 && scale_changed != non_null) || quantized > scale_changed {
                bad(format!(
                    "{id}: quantized and scale_changed are inconsistent"
                ));
            }
        }
    }
    problems
}

/// What one run left behind.
struct Ran {
    exit: Option<i32>,
    result: Option<Value>,
}

/// A scratch directory in which cases run one after another. Every result
/// written is checked with [`check_result`]; the problems accumulate.
struct Bench {
    dir: PathBuf,
    next: Cell<usize>,
    contract: Contract,
    problems: RefCell<Vec<String>>,
    statuses: RefCell<BTreeMap<String, usize>>,
}

impl Bench {
    fn new(name: &str) -> Self {
        Self {
            dir: scratch(name),
            next: Cell::new(0),
            contract: Contract::load(),
            problems: RefCell::new(Vec::new()),
            statuses: RefCell::new(BTreeMap::new()),
        }
    }

    fn run(&self, label: &str, schema: &[u8], policy: &[u8], rows: &[u8]) -> Ran {
        let number = self.next.get() + 1;
        self.next.set(number);
        let dir = self.dir.join(format!("c{number:05}"));
        fs::create_dir(&dir).unwrap();
        let path = |name: &str| dir.join(name).to_str().unwrap().to_owned();
        fs::write(dir.join("schema.json"), schema).unwrap();
        fs::write(dir.join("policy.json"), policy).unwrap();
        fs::write(dir.join("rows.ndjson"), rows).unwrap();
        let run_id = format!("case-{number:05}");
        let output = Command::new(env!("CARGO_BIN_EXE_tablebolt"))
            .args(["xlsx", "write", "--input", &path("rows.ndjson")])
            .args(["--schema", &path("schema.json")])
            .args(["--policy", &path("policy.json")])
            .args(["--output", &path("out.xlsx")])
            .args(["--result", &path("result.json"), "--run-id", &run_id])
            .output()
            .expect("tablebolt binary runs");
        let workbook = fs::read(dir.join("out.xlsx")).ok();
        let result: Option<Value> = fs::read(dir.join("result.json"))
            .ok()
            .map(|bytes| serde_json::from_slice(&bytes).expect("the result is JSON"));
        if let Some(result) = &result {
            let request = Request {
                run_id: &run_id,
                schema,
                policy,
                source: rows,
                workbook: workbook.as_deref(),
            };
            for problem in check_result(&self.contract, result, &request) {
                self.problems
                    .borrow_mut()
                    .push(format!("{label}: {problem}"));
            }
            let status = result["status"].as_str().unwrap_or("?").to_owned();
            *self.statuses.borrow_mut().entry(status).or_insert(0) += 1;
        }
        let _ = fs::remove_dir_all(&dir);
        Ran {
            exit: output.status.code(),
            result,
        }
    }

    /// Asserts that no result written so far broke a rule, and that results
    /// were in fact checked.
    fn assert_results_hold(&self) {
        let problems = self.problems.borrow();
        assert!(problems.is_empty(), "{problems:#?}");
        assert_eq!(
            self.statuses.borrow().values().sum::<usize>(),
            self.next.get(),
            "a run wrote no result"
        );
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

// ---------------------------------------------------------------------------
// Outcomes
// ---------------------------------------------------------------------------

/// An outcome as named members; `None` is a member that is absent or null.
type Outcome = BTreeMap<&'static str, Option<String>>;

fn scalar(value: Option<&Json>) -> Option<String> {
    match value? {
        Json::Null => None,
        Json::Bool(flag) => Some(flag.to_string()),
        Json::Number(spelling) => Some(spelling.clone()),
        Json::String { text, .. } => Some(text.clone()),
        other => Some(other.compact()),
    }
}

fn written(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

/// The outcome a vector states. A refusal states its category and rule, and
/// with `located` its row and column; an accepted vector states the members
/// named in `accepted`, when it has them.
fn expected(entry: &Json, located: bool, accepted: &[&'static str]) -> Outcome {
    let mut outcome = Outcome::new();
    let verdict = entry.at("verdict").text();
    outcome.insert("verdict", Some(verdict.to_owned()));
    if verdict == "refuse" {
        outcome.insert("category", scalar(entry.get("category")));
        outcome.insert("rule", scalar(entry.get("rule")));
        if located {
            outcome.insert("row", scalar(entry.get("row")));
            outcome.insert("column", scalar(entry.get("column")));
        }
    } else {
        for name in accepted {
            if entry.get(name).is_some() {
                outcome.insert(name, scalar(entry.get(name)));
            }
        }
    }
    outcome
}

/// The outcome of a run, in the same members. A status other than `ok` and
/// `refused`, and a run that left no result, are verdicts no vector states.
fn actual(ran: &Ran, located: bool) -> Outcome {
    let mut outcome = Outcome::new();
    let Some(result) = &ran.result else {
        outcome.insert("verdict", Some(format!("no result, exit {:?}", ran.exit)));
        return outcome;
    };
    let verdict = match result["status"].as_str() {
        Some("ok") => "accept",
        Some("refused") => "refuse",
        other => other.unwrap_or("no status"),
    };
    outcome.insert("verdict", Some(verdict.to_owned()));
    if let Some(refusal) = result.get("refusal") {
        outcome.insert("category", written(refusal.get("category")));
        outcome.insert("rule", written(refusal.get("rule")));
        if located {
            outcome.insert("row", written(refusal.get("row")));
            outcome.insert("column", written(refusal.get("column")));
        }
    }
    outcome
}

fn refusal(category: &str, rule: &str) -> Outcome {
    Outcome::from([
        ("verdict", Some("refuse".to_owned())),
        ("category", Some(category.to_owned())),
        ("rule", Some(rule.to_owned())),
    ])
}

/// The cases of one suite that were run and how each compared.
#[derive(Default)]
struct Report {
    run: usize,
    agree: usize,
    disagree: Vec<String>,
}

impl Report {
    fn record(&mut self, label: &str, input: &str, expected: &Outcome, actual: &Outcome) {
        self.run += 1;
        if expected == actual {
            self.agree += 1;
        } else {
            self.disagree.push(format!(
                "{label}\n    input:    {input}\n    expected: {expected:?}\n    actual:   {actual:?}"
            ));
        }
    }

    fn assert_agrees(&self, run: usize) {
        assert!(
            self.disagree.is_empty(),
            "{} of {} disagree:\n{}",
            self.disagree.len(),
            self.run,
            self.disagree.join("\n")
        );
        assert_eq!((self.run, self.agree), (run, run));
    }
}

// ---------------------------------------------------------------------------
// Value vectors
// ---------------------------------------------------------------------------

fn schema_of(column: &str) -> String {
    format!(r#"{{"format":"tabular-exchange.schema/0","columns":[{column}]}}"#)
}

/// A refusal under a rule the contract locates at a row and a column must
/// name the only cell there is. A wrong location is added to the outcome,
/// where it disagrees with every expectation.
fn check_cell_location(bench: &Bench, ran: &Ran, outcome: &mut Outcome) {
    let Some(refusal) = ran.result.as_ref().and_then(|result| result.get("refusal")) else {
        return;
    };
    let rule = refusal["rule"].as_str().unwrap_or("");
    if bench.contract.cell_rules.contains(rule)
        && (refusal.get("row") != Some(&Value::from(0))
            || refusal.get("column") != Some(&Value::from("v")))
    {
        outcome.insert("location", Some(refusal.to_string()));
    }
}

/// `integer`, `decimal`, `date`, `local-timestamp` and `offset-timestamp`:
/// the text is the only element of the only row, in a column of that type
/// with the parameters the vector gives.
fn run_texts(bench: &Bench, kind: &str, entries: &[Json]) -> Report {
    let mut report = Report::default();
    for (index, entry) in entries.iter().enumerate() {
        let mut column = format!(r#"{{"id":"v","label":"V","type":"{kind}","nullable":false"#);
        for name in ["precision", "scale", "fraction"] {
            if let Some(value) = entry.get(name) {
                column.push_str(&format!(r#","{name}":{}"#, value.compact()));
            }
        }
        column.push('}');
        let rows = format!("[{}]\n", entry.at("text").compact());
        let label = format!("{kind}[{index}] {}", entry.at("text").compact());
        let ran = bench.run(
            &label,
            schema_of(&column).as_bytes(),
            POLICY.as_bytes(),
            rows.as_bytes(),
        );
        let mut outcome = actual(&ran, false);
        check_cell_location(bench, &ran, &mut outcome);
        report.record(
            &label,
            &format!("{column} <- {}", rows.trim_end()),
            &expected(entry, false, &[]),
            &outcome,
        );
    }
    report
}

/// `cells`: a JSON element of any kind, written as the vector spells it, in
/// the column the vector gives.
fn run_cells(bench: &Bench, entries: &[Json]) -> Report {
    let mut report = Report::default();
    for (index, entry) in entries.iter().enumerate() {
        let mut column = entry.at("column").clone();
        for (name, value) in [("id", "v"), ("label", "V")] {
            if column.get(name).is_none() {
                column.set(name, Json::string(value));
            }
        }
        let column = column.compact();
        let rows = format!("[{}]\n", entry.at("value").compact());
        let label = format!("cells[{index}]");
        let ran = bench.run(
            &label,
            schema_of(&column).as_bytes(),
            POLICY.as_bytes(),
            rows.as_bytes(),
        );
        let mut outcome = actual(&ran, false);
        check_cell_location(bench, &ran, &mut outcome);
        report.record(
            &label,
            &format!("{column} <- {}", rows.trim_end()),
            &expected(entry, false, &[]),
            &outcome,
        );
    }
    report
}

/// `quantize`: a decimal value under a policy that quantizes its column.
/// `value_changed` and `scale_changed` are read from the `quantized` and
/// `scale_changed` counts of the result, which hold 0 or 1 for one cell.
/// The quantized text is in the workbook, which this test target has no
/// reader for; `crates/tableprims-xlsx/tests/tabular_exchange.rs` compares
/// it.
fn run_quantize(bench: &Bench, entries: &[Json]) -> Report {
    let mut report = Report::default();
    for (index, entry) in entries.iter().enumerate() {
        let column = format!(
            r#"{{"id":"v","label":"V","type":"decimal","precision":{},"scale":{},"nullable":false}}"#,
            entry.at("precision").compact(),
            entry.at("scale").compact()
        );
        let policy = format!(
            r#"{{"format":"tabular-exchange.policy/0","exact_numbers":"text-always","quantize":[{{"column":"v","scale":{},"rounding":{}}}],{SHEET_MEMBER}}}"#,
            entry.at("target_scale").compact(),
            entry.at("rounding").compact()
        );
        let rows = format!("[{}]\n", entry.at("text").compact());
        let label = format!("quantize[{index}]");
        let ran = bench.run(
            &label,
            schema_of(&column).as_bytes(),
            policy.as_bytes(),
            rows.as_bytes(),
        );
        let mut outcome = actual(&ran, false);
        check_cell_location(bench, &ran, &mut outcome);
        if let Some(counts) = ran.result.as_ref().and_then(|result| result.get("counts")) {
            let column = &counts["columns"][0];
            for (member, counter) in [
                ("value_changed", "quantized"),
                ("scale_changed", "scale_changed"),
            ] {
                if entry.get(member).is_some() {
                    let flag = match column[counter].as_u64() {
                        Some(0) => "false".to_owned(),
                        Some(1) => "true".to_owned(),
                        other => format!("{other:?}"),
                    };
                    outcome.insert(member, Some(flag));
                }
            }
        }
        report.record(
            &label,
            &format!("{column} quantize {policy} <- {}", rows.trim_end()),
            &expected(entry, false, &["value_changed", "scale_changed"]),
            &outcome,
        );
    }
    report
}

#[test]
fn value_vectors_agree() {
    let vectors = load("value-vectors.json");
    let bench = Bench::new("values");
    for kind in [
        "integer",
        "decimal",
        "date",
        "local-timestamp",
        "offset-timestamp",
    ] {
        run_texts(&bench, kind, vectors.at(kind).items())
            .assert_agrees(ledger("value-vectors", kind).run);
    }
    run_cells(&bench, vectors.at("cells").items())
        .assert_agrees(ledger("value-vectors", "cells").run);
    bench.assert_results_hold();
}

#[test]
fn quantize_vectors_agree() {
    let vectors = load("value-vectors.json");
    let bench = Bench::new("quantize");
    let entries = vectors.at("quantize").items();
    run_quantize(&bench, entries).assert_agrees(ledger("value-vectors", "quantize").run);
    // Every accepted vector states both flags, so both were compared.
    let flagged = entries
        .iter()
        .filter(|entry| {
            entry.get("value_changed").is_some() && entry.get("scale_changed").is_some()
        })
        .count();
    assert_eq!(flagged, 75);
    bench.assert_results_hold();
}

/// Returns the first entry whose verdict is `verdict`.
fn first_with<'a>(entries: &'a [Json], verdict: &str) -> &'a Json {
    entries
        .iter()
        .find(|entry| entry.at("verdict").text() == verdict)
        .unwrap()
}

/// Fault control: an altered or removed expectation of a value vector
/// disagrees, and the unaltered vector agrees.
#[test]
fn an_altered_value_expectation_disagrees() {
    let vectors = load("value-vectors.json");
    let bench = Bench::new("values-fault");

    // A text the reader accepts, stated as refused; and one it refuses,
    // stated under another rule, under another category, and as accepted.
    let integers = vectors.at("integer").items();
    let accepted = first_with(integers, "accept");
    let refused = first_with(integers, "refuse");
    let mut faults = vec![
        accepted.clone(),
        refused.clone(),
        refused.clone(),
        refused.clone(),
    ];
    faults[0].set("verdict", Json::string("refuse"));
    faults[0].set("category", Json::string("invalid-input"));
    faults[0].set("rule", Json::string("exact-empty"));
    faults[1].set("rule", Json::string("exact-negative-zero"));
    faults[2].set("category", Json::string("schema-mismatch"));
    faults[3].set("verdict", Json::string("accept"));
    let mut removed = refused.clone();
    removed.remove("rule");
    faults.push(removed);
    assert_eq!(run_texts(&bench, "integer", &faults).disagree.len(), 5);
    run_texts(&bench, "integer", &[accepted.clone(), refused.clone()]).assert_agrees(2);

    let cells = vectors.at("cells").items();
    let mut fault = first_with(cells, "refuse").clone();
    fault.set("rule", Json::string("wrong-scalar-kind-of-thing"));
    let mut flipped = first_with(cells, "accept").clone();
    flipped.set("verdict", Json::string("refuse"));
    assert_eq!(run_cells(&bench, &[fault, flipped]).disagree.len(), 2);

    // The value itself altered under an unaltered expectation: the fault
    // goes in through the input the binary reads.
    let mut other = accepted.clone();
    other.set("text", Json::string("+1"));
    assert_eq!(run_texts(&bench, "integer", &[other]).disagree.len(), 1);
}

/// Fault control: an altered counter flag, verdict or rule of a quantize
/// vector disagrees.
#[test]
fn an_altered_quantize_expectation_disagrees() {
    let vectors = load("value-vectors.json");
    let bench = Bench::new("quantize-fault");
    let entries = vectors.at("quantize").items();
    let accepted = first_with(entries, "accept");
    let refused = first_with(entries, "refuse");
    let flip = |entry: &Json, member: &str| {
        let mut fault = entry.clone();
        let stated = matches!(entry.at(member), Json::Bool(true));
        fault.set(member, Json::Bool(!stated));
        fault
    };
    let mut rule = refused.clone();
    rule.set("rule", Json::string("quantize-scale-too-large-by-one"));
    let mut verdict = refused.clone();
    verdict.set("verdict", Json::string("accept"));
    let faults = [
        flip(accepted, "value_changed"),
        flip(accepted, "scale_changed"),
        rule,
        verdict,
    ];
    assert_eq!(run_quantize(&bench, &faults).disagree.len(), 4);
    run_quantize(&bench, &[accepted.clone(), refused.clone()]).assert_agrees(2);
}

// ---------------------------------------------------------------------------
// Row vectors
// ---------------------------------------------------------------------------

fn hex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0);
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap())
        .collect()
}

/// The bytes a case gives: `input_hex` when it has one, or `input` as UTF-8.
fn case_bytes(case: &Json) -> Vec<u8> {
    match case.get("input_hex") {
        Some(Json::String { text, .. }) => hex(text),
        _ => case.at("input").text().as_bytes().to_vec(),
    }
}

/// A case is an input in the rows encoding under the schema it carries, or
/// under the schema of the file. An accepted case states the number of
/// rows; a refusal states its row and column, or that it has none.
fn run_rows(bench: &Bench, schema: &Json, cases: &[Json]) -> Report {
    let mut report = Report::default();
    for case in cases {
        let schema = case.get("schema").unwrap_or(schema).compact();
        let input = case_bytes(case);
        let label = case.at("name").text();
        let ran = bench.run(label, schema.as_bytes(), POLICY.as_bytes(), &input);
        let mut outcome = actual(&ran, true);
        if let Some(counts) = ran.result.as_ref().and_then(|result| result.get("counts")) {
            outcome.insert("rows", written(counts.get("rows")));
        }
        report.record(
            label,
            &format!("{:?}", String::from_utf8_lossy(&input)),
            &expected(case, true, &["rows"]),
            &outcome,
        );
    }
    report
}

#[test]
fn row_vectors_agree() {
    let vectors = load("row-vectors.json");
    let bench = Bench::new("rows");
    let cases = vectors.at("cases").items();
    run_rows(&bench, vectors.at("schema"), cases).assert_agrees(ledger("row-vectors", "cases").run);
    // Every accepted case states its row count, so every count was compared.
    assert!(cases
        .iter()
        .all(|case| case.at("verdict").text() != "accept" || case.get("rows").is_some()));
    bench.assert_results_hold();
}

/// Fault control: an altered row count, row index, column or rule of a row
/// vector disagrees, and so does a removed location.
#[test]
fn an_altered_row_expectation_disagrees() {
    let vectors = load("row-vectors.json");
    let bench = Bench::new("rows-fault");
    let cases = vectors.at("cases").items();
    let accepted = cases
        .iter()
        .find(|case| case.at("name").text() == "CRLF terminators")
        .unwrap();
    let located = cases
        .iter()
        .find(|case| case.get("row").is_some() && case.get("column").is_some())
        .unwrap();

    let mut count = accepted.clone();
    count.set("rows", Json::Number("3".to_owned()));
    let mut row = located.clone();
    row.set("row", Json::Number("7".to_owned()));
    let mut column = located.clone();
    column.set("column", Json::string("id-other"));
    let mut unlocated = located.clone();
    unlocated.remove("row");
    unlocated.remove("column");
    let mut rule = located.clone();
    rule.set("rule", Json::string("malformed-row"));
    let faults = [count, row, column, unlocated, rule];
    let schema = vectors.at("schema");
    assert_eq!(run_rows(&bench, schema, &faults).disagree.len(), 5);
    run_rows(&bench, schema, &[accepted.clone(), located.clone()]).assert_agrees(2);

    // A row dropped from the input under an unaltered expectation.
    let mut dropped = accepted.clone();
    dropped.set("input", Json::string("[\"a\",\"1\"]\r\n"));
    assert_eq!(run_rows(&bench, schema, &[dropped]).disagree.len(), 1);
}

// ---------------------------------------------------------------------------
// Document vectors
// ---------------------------------------------------------------------------

/// A case is a table schema as exact bytes, read beside [`POLICY`] and an
/// empty input.
fn run_documents(bench: &Bench, cases: &[Json]) -> Report {
    let mut report = Report::default();
    for case in cases {
        let label = case.at("name").text();
        let kind = case.at("document").text();
        assert_eq!(kind, "table-schema", "{label}: no suite runs a {kind}");
        let input = case_bytes(case);
        let ran = bench.run(label, &input, POLICY.as_bytes(), b"");
        report.record(
            label,
            &format!("{:?}", String::from_utf8_lossy(&input)),
            &expected(case, false, &[]),
            &actual(&ran, false),
        );
    }
    report
}

#[test]
fn document_vectors_agree() {
    let vectors = load("document-vectors.json");
    let bench = Bench::new("documents");
    run_documents(&bench, vectors.at("cases").items())
        .assert_agrees(ledger("document-vectors", "table-schema").run);
    bench.assert_results_hold();
}

/// Fault control: an altered verdict or rule of a document vector disagrees.
#[test]
fn an_altered_document_expectation_disagrees() {
    let vectors = load("document-vectors.json");
    let bench = Bench::new("documents-fault");
    let cases = vectors.at("cases").items();
    let accepted = first_with(cases, "accept");
    let refused = first_with(cases, "refuse");
    let mut verdict = accepted.clone();
    verdict.set("verdict", Json::string("refuse"));
    verdict.set("category", Json::string("invalid-input"));
    verdict.set("rule", Json::string("schema-malformed"));
    let mut rule = refused.clone();
    rule.set("rule", Json::string("schema-not-an-object"));
    let mut removed = refused.clone();
    removed.remove("category");
    assert_eq!(
        run_documents(&bench, &[verdict, rule, removed])
            .disagree
            .len(),
        3
    );
    run_documents(&bench, &[accepted.clone(), refused.clone()]).assert_agrees(2);

    // The document altered under an unaltered expectation.
    let mut other = accepted.clone();
    other.set("input", Json::string("[]"));
    assert_eq!(run_documents(&bench, &[other]).disagree.len(), 1);
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn fixtures_of(manifest: &Json, kind: &str) -> Vec<Json> {
    manifest
        .at("fixtures")
        .items()
        .iter()
        .filter(|fixture| fixture.at("document").text() == kind)
        .cloned()
        .collect()
}

/// A table schema fixture is read beside [`POLICY`] and an empty input.
fn run_schema_fixtures(bench: &Bench, fixtures: &[Json]) -> Report {
    let mut report = Report::default();
    for fixture in fixtures {
        let path = fixture.at("path").text();
        let schema = fs::read(data().join(path)).unwrap();
        let ran = bench.run(path, &schema, POLICY.as_bytes(), b"");
        report.record(
            path,
            path,
            &expected(fixture, false, &[]),
            &actual(&ran, false),
        );
    }
    report
}

#[test]
fn table_schema_fixtures_agree() {
    let manifest = load("fixtures/manifest.json");
    let bench = Bench::new("schemas");
    let fixtures = fixtures_of(&manifest, "table-schema");
    run_schema_fixtures(&bench, &fixtures).assert_agrees(ledger("fixtures", "table-schema").run);
    bench.assert_results_hold();
}

/// How the policy fixtures compared.
#[derive(Default)]
struct PolicyReport {
    /// Each fixture exactly as the release has it.
    as_written: Report,
    /// Of those, valid policies that were read and then refused only for
    /// naming no worksheet.
    read_then_no_sheet: usize,
    /// Each fixture that could be given a worksheet name, with one.
    with_sheet: Report,
    /// Fixtures that could not be given a worksheet name, with the reason.
    not_given_sheet: Vec<(String, &'static str)>,
}

/// The policy with `extensions.tableprims.sheet` added as its last member:
/// the bytes of the fixture with the member inserted before the closing
/// brace, so nothing else about the document changes. `Err` says why a
/// fixture is left as it is.
fn with_sheet(policy: &[u8]) -> Result<Vec<u8>, &'static str> {
    let Ok(Json::Object(members)) = vector_json::parse(policy) else {
        return Err("not a JSON object, so there is no place for the member");
    };
    if members.iter().any(|(name, _)| name == "extensions") {
        return Err("carries its own extensions member, which is what the fixture tests");
    }
    let close = policy.iter().rposition(|byte| *byte == b'}').unwrap();
    let mut out = policy[..close].to_vec();
    if !members.is_empty() {
        out.push(b',');
    }
    out.extend_from_slice(SHEET_MEMBER.as_bytes());
    out.extend_from_slice(&policy[close..]);
    Ok(out)
}

/// A policy fixture is read beside the schema it names, or [`SCHEMA`], and
/// an empty input, twice.
///
/// As written: no policy the standard accepts names a worksheet, and this
/// command requires one, so none can complete. A valid policy that is
/// refused with `unsupported-representation`, `sheet-name-missing` was read
/// as a document and accepted, and then refused by the destination; that
/// agrees with a vector that says the document is accepted. Any other
/// outcome for a valid policy disagrees, and a policy the vector refuses must
/// be refused under the stated category and rule.
///
/// With a worksheet named: the same fixture with
/// `extensions.tableprims.sheet` added must have exactly the stated
/// outcome, and a valid policy must complete.
fn run_policy_fixtures(bench: &Bench, fixtures: &[Json]) -> PolicyReport {
    let mut report = PolicyReport::default();
    let no_sheet = refusal("unsupported-representation", "sheet-name-missing");
    for fixture in fixtures {
        let path = fixture.at("path").text();
        let policy = fs::read(data().join(path)).unwrap();
        let schema = match fixture.get("with_schema") {
            Some(name) => fs::read(data().join(name.text())).unwrap(),
            None => SCHEMA.as_bytes().to_vec(),
        };
        let stated = expected(fixture, false, &[]);
        let accept = stated["verdict"].as_deref() == Some("accept");
        let edited = with_sheet(&policy);

        let ran = bench.run(path, &schema, &policy, b"");
        let outcome = actual(&ran, false);
        if outcome == no_sheet && accept {
            report.read_then_no_sheet += 1;
            report.as_written.record(path, path, &no_sheet, &outcome);
        } else {
            report.as_written.record(path, path, &stated, &outcome);
        }

        match edited {
            Err(reason) => report.not_given_sheet.push((path.to_owned(), reason)),
            Ok(edited) => {
                let label = format!("{path} with a worksheet named");
                let ran = bench.run(&label, &schema, &edited, b"");
                report
                    .with_sheet
                    .record(&label, &label, &stated, &actual(&ran, false));
            }
        }
    }
    report
}

#[test]
fn conversion_policy_fixtures_agree() {
    let manifest = load("fixtures/manifest.json");
    let bench = Bench::new("policies");
    let fixtures = fixtures_of(&manifest, "conversion-policy");
    let report = run_policy_fixtures(&bench, &fixtures);
    report
        .as_written
        .assert_agrees(ledger("fixtures", "conversion-policy").run);
    // The four valid policies are read and stop at the missing worksheet;
    // with one named, each completes.
    assert_eq!(report.read_then_no_sheet, 4);
    report.with_sheet.assert_agrees(22);
    let reasons: BTreeMap<&str, usize> =
        report
            .not_given_sheet
            .iter()
            .fold(BTreeMap::new(), |mut count, (_, reason)| {
                *count.entry(*reason).or_insert(0) += 1;
                count
            });
    assert_eq!(
        reasons.into_iter().collect::<Vec<_>>(),
        [
            (
                "carries its own extensions member, which is what the fixture tests",
                9
            ),
            ("not a JSON object, so there is no place for the member", 1),
        ]
    );
    bench.assert_results_hold();
}

/// Fault control: an altered expectation of a schema fixture and of a policy
/// fixture disagrees, in the run as written and in the run with a worksheet
/// named.
#[test]
fn an_altered_fixture_expectation_disagrees() {
    let manifest = load("fixtures/manifest.json");
    let bench = Bench::new("fixtures-fault");

    let schemas = fixtures_of(&manifest, "table-schema");
    let accepted = first_with(&schemas, "accept");
    let refused = first_with(&schemas, "refuse");
    let mut verdict = accepted.clone();
    verdict.set("verdict", Json::string("refuse"));
    let mut rule = refused.clone();
    rule.set("rule", Json::string("schema-member-unknown-here"));
    let mut removed = refused.clone();
    removed.remove("rule");
    // A fixture file other than the one the expectation was written for.
    let mut swapped = accepted.clone();
    swapped.set("path", refused.at("path").clone());
    let faults = [verdict, rule, removed, swapped];
    assert_eq!(run_schema_fixtures(&bench, &faults).disagree.len(), 4);
    run_schema_fixtures(&bench, &[accepted.clone(), refused.clone()]).assert_agrees(2);

    let policies = fixtures_of(&manifest, "conversion-policy");
    let named = |name: &str| {
        policies
            .iter()
            .find(|fixture| fixture.at("path").text().ends_with(name))
            .unwrap()
            .clone()
    };
    // A valid policy stated as refused: both runs disagree.
    let mut valid = named("/valid/conversion-policy/quantize.json");
    valid.set("verdict", Json::string("refuse"));
    valid.set("category", Json::string("invalid-input"));
    valid.set("rule", Json::string("policy-member-invalid"));
    let report = run_policy_fixtures(&bench, &[valid]);
    assert_eq!(report.as_written.disagree.len(), 1);
    assert_eq!(report.with_sheet.disagree.len(), 1);
    // An invalid policy stated as accepted, and under another rule.
    let mut invalid = named("/invalid/conversion-policy/member-unknown.json");
    invalid.set("verdict", Json::string("accept"));
    let mut other = named("/invalid/conversion-policy/member-unknown.json");
    other.set("rule", Json::string("policy-member-missing"));
    let report = run_policy_fixtures(&bench, &[invalid, other]);
    assert_eq!(report.as_written.disagree.len(), 2);
    assert_eq!(report.with_sheet.disagree.len(), 2);
    // A rule of the policy against the schema, altered.
    let mut across = named("/semantic-invalid/conversion-policy/quantize-column-unknown.json");
    across.set("rule", Json::string("quantize-not-decimal"));
    let report = run_policy_fixtures(&bench, &[across]);
    assert_eq!(report.as_written.disagree.len(), 1);
    assert_eq!(report.with_sheet.disagree.len(), 1);
    // A valid policy that could not be read is not excused by the rule about
    // the worksheet: beside a schema that is refused, the outcome disagrees.
    let mut beside = named("/valid/conversion-policy/quantize.json");
    beside.set(
        "with_schema",
        Json::string("fixtures/invalid/table-schema/columns-empty.json"),
    );
    let report = run_policy_fixtures(&bench, &[beside]);
    assert_eq!(report.read_then_no_sheet, 0);
    assert_eq!(report.as_written.disagree.len(), 1);
}

// ---------------------------------------------------------------------------
// Results written during the runs
// ---------------------------------------------------------------------------

fn held<'a>(
    run_id: &'a str,
    schema: &'a str,
    policy: &'a str,
    source: &'a str,
    workbook: Option<&'a [u8]>,
) -> Request<'a> {
    Request {
        run_id,
        schema: schema.as_bytes(),
        policy: policy.as_bytes(),
        source: source.as_bytes(),
        workbook,
    }
}

/// Fault control: `check_result` passes the results of a completed, a refused
/// and a failed run, and reports each alteration made to them.
#[test]
fn an_altered_result_is_reported() {
    let contract = Contract::load();
    let dir = scratch("result-fault");
    let schema = r#"{"format":"tabular-exchange.schema/0","columns":[
        {"id":"name","label":"Name","type":"string","nullable":true},
        {"id":"amount","label":"Amount","type":"decimal","precision":6,"scale":2,"nullable":true}]}"#;
    let policy = format!(
        r#"{{"format":"tabular-exchange.policy/0","exact_numbers":"text-if-needed","quantize":[{{"column":"amount","scale":1,"rounding":"half-even"}}],{SHEET_MEMBER}}}"#
    );
    let run = |name: &str, rows: Option<&str>| -> (Value, Option<Vec<u8>>) {
        let path = |file: &str| {
            dir.join(format!("{name}-{file}"))
                .to_str()
                .unwrap()
                .to_owned()
        };
        fs::write(path("schema.json"), schema).unwrap();
        fs::write(path("policy.json"), &policy).unwrap();
        if let Some(rows) = rows {
            fs::write(path("rows.ndjson"), rows).unwrap();
        }
        Command::new(env!("CARGO_BIN_EXE_tablebolt"))
            .args(["xlsx", "write", "--input", &path("rows.ndjson")])
            .args(["--schema", &path("schema.json")])
            .args(["--policy", &path("policy.json")])
            .args(["--output", &path("out.xlsx")])
            .args(["--result", &path("result.json"), "--run-id", name])
            .output()
            .unwrap();
        (
            serde_json::from_slice(&fs::read(path("result.json")).unwrap()).unwrap(),
            fs::read(path("out.xlsx")).ok(),
        )
    };
    let rows = "[\"a\",\"1.25\"]\n[\"\",null]\n[null,\"2.00\"]\n";
    let (ok, workbook) = run("completed", Some(rows));
    let (refused, none) = run("refused", Some("[\"a\",\"1.2\"]\n"));
    assert!(workbook.is_some() && none.is_none());
    // A run whose input does not exist fails, and its result names no source.
    let (failed, none) = run("failed", None);
    assert!(none.is_none());
    assert_eq!(failed["status"], "failed");
    assert!(failed["inputs"].get("source").is_none());
    let workbook = workbook.unwrap();
    let request = |run_id, source, workbook| held(run_id, schema, &policy, source, workbook);
    let held_ok = request("completed", rows, Some(workbook.as_slice()));
    let held_refused = request("refused", "[\"a\",\"1.2\"]\n", None);
    assert_eq!(check_result(&contract, &ok, &held_ok), [] as [String; 0]);
    assert_eq!(
        check_result(&contract, &refused, &held_refused),
        [] as [String; 0]
    );
    assert_eq!(
        check_result(&contract, &failed, &request("failed", "", None)),
        [] as [String; 0]
    );
    assert_eq!(ok["counts"]["columns"][1]["scale_changed"], 2);

    type Fault = fn(&mut Value);
    let reported = |result: &Value, held: &Request, fault: Fault| -> usize {
        let mut altered = result.clone();
        fault(&mut altered);
        assert_ne!(&altered, result);
        check_result(&contract, &altered, held).len()
    };
    let completed: [Fault; 14] = [
        |r| r["format"] = "tabular-exchange.result/1".into(),
        |r| r["contract"] = "tabular-exchange/0.2.0".into(),
        |r| r["status"] = "done".into(),
        |r| r["status"] = "refused".into(),
        |r| r["run_id"] = "another".into(),
        |r| r["note"] = "x".into(),
        |r| r["inputs"]["source"]["bytes"] = 1.into(),
        |r| r["inputs"]["schema"]["sha256"] = "0".repeat(64).into(),
        |r| {
            r["inputs"].as_object_mut().unwrap().remove("policy");
        },
        |r| r["output"]["bytes"] = 1.into(),
        |r| r["counts"]["rows"] = 2.into(),
        |r| r["counts"]["columns"][1]["numeric"] = 1.into(),
        |r| r["counts"]["columns"][1]["scale_changed"] = 1.into(),
        |r| r["counts"]["columns"][1]["id"] = "name".into(),
    ];
    for (index, fault) in completed.into_iter().enumerate() {
        assert!(reported(&ok, &held_ok, fault) > 0, "fault {index}");
    }
    let refusals: [Fault; 6] = [
        |r| r["refusal"]["category"] = "io-failure".into(),
        |r| r["refusal"]["category"] = "invalid-input".into(),
        |r| r["refusal"]["rule"] = "Not_A_Rule".into(),
        |r| r["refusal"]["row"] = (-1).into(),
        |r| r["refusal"]["value"] = "1.2".into(),
        |r| r["counts"] = serde_json::json!({"rows": 0, "columns": []}),
    ];
    for (index, fault) in refusals.into_iter().enumerate() {
        assert!(
            reported(&refused, &held_refused, fault) > 0,
            "fault {index}"
        );
    }
    // A workbook other than the one the result names, and a workbook beside
    // a refusal.
    let other = request("completed", rows, Some(b"PK".as_slice()));
    assert_eq!(
        check_result(&contract, &ok, &other),
        ["output is not the workbook"]
    );
    let beside = request("refused", "[\"a\",\"1.2\"]\n", Some(b"PK".as_slice()));
    assert_eq!(
        check_result(&contract, &refused, &beside),
        ["workbook present with status refused"]
    );
    let _ = fs::remove_dir_all(&dir);
}
