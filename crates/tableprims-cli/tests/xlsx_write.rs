// SPDX-License-Identifier: MIT OR Apache-2.0

//! Runs `tablebolt xlsx write` and checks the exit code, the result file
//! and what is left on disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA: &str = r#"{"format":"tabular-exchange.schema/0","columns":[
 {"id":"invoice","label":"Invoice","type":"string","nullable":false},
 {"id":"qty","label":"Qty","type":"integer","nullable":false},
 {"id":"amount","label":"Amount","type":"decimal","precision":12,"scale":4,"nullable":true},
 {"id":"paid","label":"Paid","type":"boolean","nullable":true}]}"#;
const POLICY: &str = r#"{"format":"tabular-exchange.policy/0","exact_numbers":"require-exact",
 "quantize":[{"column":"amount","scale":2,"rounding":"half-even"}],
 "extensions":{"tableprims":{"sheet":"Data"}}}"#;
const ROWS: &str = concat!(
    "[\"INV-1\",\"3\",\"12.3450\",true]\n",
    "[\"\",\"7\",\"123.4500\",null]\n",
    "[\"INV-3\",\"1\",null,false]\n",
);

/// A scratch directory holding the three inputs, removed when dropped.
struct Case {
    dir: PathBuf,
}

impl Case {
    fn new(name: &str, schema: &str, policy: &str, rows: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("tablebolt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("schema.json"), schema).unwrap();
        fs::write(dir.join("policy.json"), policy).unwrap();
        fs::write(dir.join("rows.ndjson"), rows).unwrap();
        Self { dir }
    }

    fn path(&self, name: &str) -> String {
        self.dir.join(name).to_str().unwrap().to_owned()
    }

    fn run(&self, extra: &[&str]) -> Output {
        let mut args = vec![
            "xlsx".to_owned(),
            "write".to_owned(),
            "--input".to_owned(),
            self.path("rows.ndjson"),
            "--schema".to_owned(),
            self.path("schema.json"),
            "--policy".to_owned(),
            self.path("policy.json"),
        ];
        // An argument given by the test replaces the default.
        for (flag, name) in [("--output", "out.xlsx"), ("--result", "result.json")] {
            if !extra.contains(&flag) {
                args.extend([flag.to_owned(), self.path(name)]);
            }
        }
        args.extend(extra.iter().map(|arg| (*arg).to_owned()));
        Command::new(env!("CARGO_BIN_EXE_tablebolt"))
            .args(&args)
            .output()
            .expect("tablebolt binary runs")
    }

    fn result(&self) -> Value {
        let result =
            serde_json::from_slice(&fs::read(self.dir.join("result.json")).unwrap()).unwrap();
        check_result_envelope(&result);
        result
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn sum(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap();
    let hex: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    serde_json::json!({ "sha256": hex, "bytes": bytes.len() })
}

const INPUTS: [&str; 3] = ["policy.json", "rows.ndjson", "schema.json"];

/// The members a result may have at its top level.
const RESULT_MEMBERS: [&str; 10] = [
    "format",
    "contract",
    "status",
    "run_id",
    "profile",
    "inputs",
    "output",
    "counts",
    "refusal",
    "extensions",
];

/// Counts the values under `value` and checks every string, member name and
/// number against the bounds on `extensions`. Returns the deepest nesting,
/// counting `value` itself as `depth`.
fn measure(value: &Value, depth: usize, values: &mut usize) -> usize {
    match value {
        Value::String(text) => {
            assert!(text.chars().count() <= 256);
            depth - 1
        }
        Value::Number(number) => {
            assert!(number.as_u64().unwrap() <= 9_007_199_254_740_991);
            depth - 1
        }
        Value::Array(items) => {
            *values += items.len();
            items
                .iter()
                .map(|item| measure(item, depth + 1, values))
                .max()
                .unwrap_or(depth)
                .max(depth)
        }
        Value::Object(members) => {
            *values += members.len();
            members
                .iter()
                .map(|(name, member)| {
                    assert!(name.chars().count() <= 256);
                    measure(member, depth + 1, values)
                })
                .max()
                .unwrap_or(depth)
                .max(depth)
        }
        _ => depth - 1,
    }
}

/// Checks what every result has, whatever its status: the format and the
/// contract revision, no member the contract does not define, and
/// implementation members only under this program's extension namespace
/// and within the bounds on `extensions`.
fn check_result_envelope(result: &Value) {
    assert_eq!(result["format"], "tabular-exchange.result/0");
    assert_eq!(result["contract"], "tabular-exchange/0.1.0");
    for name in result.as_object().unwrap().keys() {
        assert!(RESULT_MEMBERS.contains(&name.as_str()), "{name}");
    }
    for name in result["inputs"].as_object().unwrap().keys() {
        assert!(
            ["source", "schema", "policy"].contains(&name.as_str()),
            "{name}"
        );
    }
    let ok = result["status"] == "ok";
    assert_eq!(result.get("output").is_some(), ok);
    assert_eq!(result.get("counts").is_some(), ok);
    assert_eq!(result.get("refusal").is_some(), !ok);
    let extensions = result["extensions"].as_object().unwrap();
    assert_eq!(extensions.keys().collect::<Vec<_>>(), ["tableprims"]);
    let ours = extensions["tableprims"].as_object().unwrap();
    for name in ours.keys() {
        assert!(
            ["tool", "backend", "limits", "policy_digest"].contains(&name.as_str()),
            "{name}"
        );
    }
    assert_eq!(ours["tool"]["name"], "tablebolt");
    assert_eq!(ours["backend"]["name"], "rust_xlsxwriter");
    // The namespaces are at depth 3 of the document; `extensions` at 2.
    let mut values = 0;
    let deepest = measure(&result["extensions"], 2, &mut values);
    assert!(deepest <= 6, "{deepest}");
    assert!(values <= 256, "{values}");
    // Every count and length is one a reader of any JSON library reads
    // exactly.
    let mut all = 0;
    measure(&result["inputs"], 2, &mut all);
    if ok {
        measure(&result["output"], 2, &mut all);
        measure(&result["counts"], 2, &mut all);
    } else {
        measure(&result["refusal"], 2, &mut all);
    }
}

#[test]
fn a_completed_run_publishes_the_workbook_and_a_matching_result() {
    let case = Case::new("ok", SCHEMA, POLICY, ROWS);
    let output = case.run(&["--run-id", "case-0001"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert_eq!(
        case.names(),
        [
            "out.xlsx",
            "policy.json",
            "result.json",
            "rows.ndjson",
            "schema.json"
        ]
    );
    let result = case.result();
    assert_eq!(result["format"], "tabular-exchange.result/0");
    assert_eq!(result["contract"], "tabular-exchange/0.1.0");
    assert_eq!(result["status"], "ok");
    assert_eq!(result["run_id"], "case-0001");
    assert!(result.get("refusal").is_none());
    assert_eq!(result["output"], sum(&case.dir.join("out.xlsx")));
    assert!(fs::read(case.dir.join("out.xlsx"))
        .unwrap()
        .starts_with(b"PK"));
    assert_eq!(
        result["inputs"]["source"],
        sum(&case.dir.join("rows.ndjson"))
    );
    assert!(result["inputs"].get("rows").is_none());
    assert_eq!(
        result["inputs"]["schema"],
        sum(&case.dir.join("schema.json"))
    );
    assert_eq!(
        result["inputs"]["policy"],
        sum(&case.dir.join("policy.json"))
    );
    // Implementation members are under the extension namespace and
    // nowhere else.
    for moved in ["tool", "backend", "limits", "policy_digest"] {
        assert!(result.get(moved).is_none(), "{moved}");
    }
    let ours = &result["extensions"]["tableprims"];
    assert_eq!(ours["tool"]["name"], "tablebolt");
    assert_eq!(ours["tool"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(ours["tool"]["library"], env!("CARGO_PKG_VERSION"));
    assert_eq!(ours["backend"]["version"], "0.102.0");
    assert_eq!(result["profile"]["id"], "xlsx-write/r1");
    assert_eq!(result["profile"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(ours["policy_digest"].as_str().unwrap().len(), 64);
    assert_eq!(
        ours["limits"],
        serde_json::json!({"max_rows": 1_048_575, "max_line_bytes": 1_048_576, "max_empty_ranges": 65_536})
    );
    // The namespace object, four members, and the eight values they hold.
    let mut values = 0;
    assert_eq!(measure(&result["extensions"], 2, &mut values), 4);
    assert_eq!(values, 13);
    assert_eq!(
        result["counts"],
        serde_json::json!({"rows": 3, "columns": [
            {"id": "invoice", "representation": "text", "null": 0, "empty": 1},
            {"id": "qty", "representation": "native", "null": 0, "empty": 0,
             "numeric": 3, "text_fallback": 0, "text_forced": 0, "quantized": 0, "scale_changed": 0},
            {"id": "amount", "representation": "native", "null": 1, "empty": 0,
             "numeric": 2, "text_fallback": 0, "text_forced": 0, "quantized": 1, "scale_changed": 2},
            {"id": "paid", "representation": "native", "null": 1, "empty": 0}
        ]})
    );
}

#[test]
fn a_run_without_an_identifier_generates_one() {
    let case = Case::new("auto", SCHEMA, POLICY, ROWS);
    assert_eq!(case.run(&[]).status.code(), Some(0));
    let result = case.result();
    let id = result["run_id"].as_str().unwrap();
    let hex = id.strip_prefix("auto-").unwrap();
    assert_eq!(hex.len(), 32);
    assert!(hex
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
}

#[test]
fn existing_files_are_never_replaced() {
    let case = Case::new("exists-output", SCHEMA, POLICY, ROWS);
    fs::write(case.dir.join("out.xlsx"), b"theirs").unwrap();
    let output = case.run(&[]);
    assert_eq!(output.status.code(), Some(54));
    assert_eq!(fs::read(case.dir.join("out.xlsx")).unwrap(), b"theirs");
    let result = case.result();
    assert_eq!(result["status"], "failed");
    assert_eq!(
        result["refusal"],
        serde_json::json!({"category": "io-failure", "rule": "output-exists"})
    );
    assert!(result.get("output").is_none() && result.get("counts").is_none());
    assert_eq!(result["inputs"], serde_json::json!({}));

    let case = Case::new("exists-result", SCHEMA, POLICY, ROWS);
    fs::write(case.dir.join("result.json"), b"theirs").unwrap();
    let output = case.run(&[]);
    assert_eq!(output.status.code(), Some(54));
    assert_eq!(fs::read(case.dir.join("result.json")).unwrap(), b"theirs");
    assert_eq!(
        case.names(),
        ["policy.json", "result.json", "rows.ndjson", "schema.json"]
    );
}

#[test]
fn usage_errors_write_nothing() {
    let case = Case::new("usage", SCHEMA, POLICY, ROWS);
    assert_eq!(case.run(&["--run-id", "not valid"]).status.code(), Some(64));
    let same = case.path("out.xlsx");
    assert_eq!(case.run(&["--result", &same]).status.code(), Some(64));
    assert_eq!(case.names(), INPUTS);
}

/// Runs a case expected not to complete and checks that no workbook and no
/// temporary file is left.
fn refused(
    name: &str,
    schema: &str,
    policy: &str,
    rows: &str,
    exit: i32,
    status: &str,
    refusal: Value,
) -> Value {
    let case = Case::new(name, schema, policy, rows);
    let output = case.run(&[]);
    assert_eq!(output.status.code(), Some(exit), "{name}");
    assert!(output.stdout.is_empty(), "{name}");
    assert_eq!(
        case.names(),
        ["policy.json", "result.json", "rows.ndjson", "schema.json"],
        "{name}"
    );
    let result = case.result();
    assert_eq!(result["status"], status, "{name}");
    assert_eq!(result["refusal"], refusal, "{name}");
    assert!(
        result.get("output").is_none() && result.get("counts").is_none(),
        "{name}"
    );
    assert_eq!(
        result["inputs"]["schema"],
        sum(&case.dir.join("schema.json")),
        "{name}"
    );
    result
}

#[test]
fn refusals_carry_a_category_a_rule_and_a_location() {
    use serde_json::json;
    let rows = |row: &str| format!("[\"INV-1\",\"3\",\"12.3450\",true]\n{row}\n");
    for (name, row, exit, refusal) in [
        (
            "numeric-literal",
            "[\"INV-2\",7,null,null]",
            61,
            json!({"category": "invalid-input", "rule": "numeric-literal", "row": 1}),
        ),
        (
            "leading-zero",
            "[\"INV-2\",\"007\",null,null]",
            61,
            json!({"category": "invalid-input", "rule": "exact-leading-zero", "row": 1, "column": "qty"}),
        ),
        (
            "null",
            "[null,\"7\",null,null]",
            60,
            json!({"category": "schema-mismatch", "rule": "null-not-allowed", "row": 1, "column": "invoice"}),
        ),
        (
            "scale",
            "[\"INV-2\",\"7\",\"1.5\",null]",
            60,
            json!({"category": "schema-mismatch", "rule": "exact-scale-mismatch", "row": 1, "column": "amount"}),
        ),
        (
            "digits",
            "[\"INV-2\",\"9007199254740993\",null,null]",
            62,
            json!({"category": "unsupported-representation", "rule": "too-many-significant-digits", "row": 1, "column": "qty"}),
        ),
    ] {
        let result = refused(name, SCHEMA, POLICY, &rows(row), exit, "refused", refusal);
        // The rows were not read to the end, so no digest is claimed.
        assert!(result["inputs"].get("source").is_none(), "{name}");
        assert_eq!(
            result["extensions"]["tableprims"]["policy_digest"]
                .as_str()
                .unwrap()
                .len(),
            64,
            "{name}"
        );
    }
    let capped = POLICY.replace("\"sheet\"", "\"limits\":{\"max_rows\":2},\"sheet\"");
    refused(
        "rows-limit",
        SCHEMA,
        &capped,
        ROWS,
        60,
        "refused",
        json!({"category": "resource-limit", "rule": "too-many-rows", "row": 2}),
    );
    let over = POLICY.replace("\"sheet\"", "\"limits\":{\"max_rows\":1048576},\"sheet\"");
    refused(
        "ceiling",
        SCHEMA,
        &over,
        ROWS,
        60,
        "refused",
        json!({"category": "resource-limit", "rule": "limit-above-profile"}),
    );
    refused(
        "sheet",
        SCHEMA,
        &POLICY.replace("\"Data\"", "\"a/b\""),
        ROWS,
        62,
        "refused",
        json!({"category": "unsupported-representation", "rule": "sheet-name-unsupported"}),
    );
    refused(
        "quantize",
        SCHEMA,
        &POLICY.replace("\"amount\"", "\"missing\""),
        ROWS,
        60,
        "refused",
        json!({"category": "schema-mismatch", "rule": "quantize-unknown-column"}),
    );
    refused(
        "policy-json",
        SCHEMA,
        "{",
        ROWS,
        61,
        "refused",
        json!({"category": "invalid-input", "rule": "policy-malformed"}),
    );
    refused(
        "policy-member",
        SCHEMA,
        &POLICY.replace("\"sheet\"", "\"extra\":1,\"sheet\""),
        ROWS,
        61,
        "refused",
        json!({"category": "invalid-input", "rule": "policy-member-unknown"}),
    );
    refused(
        "policy-member-top",
        SCHEMA,
        &POLICY.replace("\"extensions\"", "\"extra\":1,\"extensions\""),
        ROWS,
        61,
        "refused",
        json!({"category": "invalid-input", "rule": "policy-member-unknown"}),
    );
}

/// A policy with `extensions` as its extensions member.
fn policy_extended(extensions: &str) -> String {
    format!(
        r#"{{"format":"tabular-exchange.policy/0","exact_numbers":"require-exact","extensions":{extensions}}}"#
    )
}

#[test]
fn earlier_format_identifiers_are_refused() {
    use serde_json::json;
    let case = Case::new(
        "old-schema",
        &SCHEMA.replace("tabular-exchange.schema/0", "tableprims.schema/0"),
        POLICY,
        ROWS,
    );
    assert_eq!(case.run(&[]).status.code(), Some(61));
    let result = case.result();
    assert_eq!(result["status"], "refused");
    assert_eq!(
        result["refusal"],
        json!({"category": "invalid-input", "rule": "schema-format-unsupported"})
    );
    for (name, policy) in [
        (
            "old-policy",
            POLICY.replace("tabular-exchange.policy/0", "tableprims.policy/0"),
        ),
        (
            "old-policy-shape",
            String::from(
                r#"{"format":"tableprims.policy/0","exact_numbers":"require-exact","sheet":"Data"}"#,
            ),
        ),
        (
            "result-format-as-policy",
            POLICY.replace("tabular-exchange.policy/0", "tabular-exchange.result/0"),
        ),
        (
            "later-policy",
            POLICY.replace("tabular-exchange.policy/0", "tabular-exchange.policy/1"),
        ),
    ] {
        refused(
            name,
            SCHEMA,
            &policy,
            ROWS,
            61,
            "refused",
            json!({"category": "invalid-input", "rule": "policy-format-unsupported"}),
        );
    }
}

#[test]
fn policy_extensions_are_bounded_and_named() {
    use serde_json::json;
    let invalid = |rule: &str| json!({"category": "invalid-input", "rule": rule});
    let nulls = |count: usize| vec!["null"; count].join(",");
    let cases: Vec<(&str, String, &str)> = vec![
        // Settings of the destination outside the extension namespace.
        (
            "top-sheet",
            String::from(
                r#"{"format":"tabular-exchange.policy/0","exact_numbers":"require-exact","sheet":"Data"}"#,
            ),
            "policy-member-unknown",
        ),
        (
            "top-limits",
            POLICY.replace(
                "\"extensions\"",
                "\"limits\":{\"max_rows\":2},\"extensions\"",
            ),
            "policy-member-unknown",
        ),
        // A namespace this program does not implement.
        (
            "other-namespace",
            policy_extended(r#"{"example":{"sheet":"Data"}}"#),
            "policy-extension-unsupported",
        ),
        (
            "second-namespace",
            policy_extended(r#"{"tableprims":{"sheet":"Data"},"example-tool":{}}"#),
            "policy-extension-unsupported",
        ),
        // The bounds, each just past its limit; they are judged before the
        // namespace is looked up.
        (
            "namespaces",
            policy_extended(
                r#"{"tableprims":{"sheet":"Data"},"b":{},"c":{},"d":{},"e":{},"f":{},"g":{},"h":{},"i":{}}"#,
            ),
            "policy-member-invalid",
        ),
        (
            "values",
            policy_extended(&format!(r#"{{"example":{{"a":[{}]}}}}"#, nulls(255))),
            "policy-member-invalid",
        ),
        (
            "string",
            policy_extended(&format!(
                r#"{{"tableprims":{{"sheet":"{}"}}}}"#,
                "s".repeat(257)
            )),
            "policy-member-invalid",
        ),
        (
            "name",
            policy_extended(&format!(r#"{{"example":{{"{}":null}}}}"#, "k".repeat(257))),
            "policy-member-invalid",
        ),
        (
            "number",
            policy_extended(
                r#"{"tableprims":{"sheet":"Data","limits":{"max_rows":9007199254740992}}}"#,
            ),
            "policy-member-invalid",
        ),
        (
            "huge-number",
            policy_extended(
                r#"{"example":{"a":123456789012345678901234567890123456789012345678901234567890}}"#,
            ),
            "policy-member-invalid",
        ),
        (
            "namespace-name",
            policy_extended(r#"{"Tableprims":{"sheet":"Data"}}"#),
            "policy-member-invalid",
        ),
        (
            "namespace-value",
            policy_extended(r#"{"tableprims":"Data"}"#),
            "policy-not-an-object",
        ),
        (
            "extensions-value",
            policy_extended(r#"["tableprims"]"#),
            "policy-not-an-object",
        ),
        (
            "depth",
            policy_extended(r#"{"example":{"a":{"b":{"c":{"d":{}}}}}}"#),
            "policy-malformed",
        ),
        // Content of this program's namespace.
        (
            "namespace-member",
            policy_extended(r#"{"tableprims":{"sheet":"Data","colour":"red"}}"#),
            "policy-member-unknown",
        ),
        (
            "sheet-type",
            policy_extended(r#"{"tableprims":{"sheet":7}}"#),
            "policy-member-invalid",
        ),
        (
            "sheet-reserved",
            policy_extended(r#"{"tableprims":{"sheet":"_TablePrims"}}"#),
            "policy-member-invalid",
        ),
    ];
    for (name, policy, rule) in cases {
        let result = refused(name, SCHEMA, &policy, ROWS, 61, "refused", invalid(rule));
        // The policy was read to its end, and was not resolved.
        assert_eq!(result["inputs"]["policy"]["bytes"], policy.len(), "{name}");
        let ours = &result["extensions"]["tableprims"];
        assert!(
            ours.get("policy_digest").is_none() && ours.get("limits").is_none(),
            "{name}"
        );
    }
    // Each bound at its limit is read: the namespace is then looked up, or
    // the policy runs.
    for (name, policy, rule) in [
        (
            "namespaces-at",
            policy_extended(
                r#"{"tableprims":{"sheet":"Data"},"b":{},"c":{},"d":{},"e":{},"f":{},"g":{},"h":{}}"#,
            ),
            "policy-extension-unsupported",
        ),
        (
            "values-at",
            policy_extended(&format!(r#"{{"example":{{"a":[{}]}}}}"#, nulls(254))),
            "policy-extension-unsupported",
        ),
        (
            "name-at",
            policy_extended(&format!(
                r#"{{"example":{{"{}":"{}"}}}}"#,
                "k".repeat(256),
                "v".repeat(256)
            )),
            "policy-extension-unsupported",
        ),
        (
            "number-at",
            policy_extended(r#"{"example":{"a":9007199254740991}}"#),
            "policy-extension-unsupported",
        ),
        (
            "depth-at",
            policy_extended(r#"{"example":{"a":{"b":{"c":{}}}}}"#),
            "policy-extension-unsupported",
        ),
    ] {
        refused(name, SCHEMA, &policy, ROWS, 61, "refused", invalid(rule));
    }
    // A limit at the largest number a document may carry is read, and is
    // then above what the profile supports.
    refused(
        "number-at-ours",
        SCHEMA,
        &policy_extended(
            r#"{"tableprims":{"sheet":"Data","limits":{"max_rows":9007199254740991}}}"#,
        ),
        ROWS,
        60,
        "refused",
        json!({"category": "resource-limit", "rule": "limit-above-profile"}),
    );
}

#[test]
fn a_policy_that_names_no_sheet_is_refused_by_the_destination() {
    use serde_json::json;
    for (name, policy) in [
        (
            "no-extensions",
            String::from(
                r#"{"format":"tabular-exchange.policy/0","exact_numbers":"require-exact"}"#,
            ),
        ),
        ("empty-extensions", policy_extended("{}")),
        ("empty-namespace", policy_extended(r#"{"tableprims":{}}"#)),
        (
            "limits-only",
            policy_extended(r#"{"tableprims":{"limits":{"max_rows":2}}}"#),
        ),
    ] {
        let result = refused(
            name,
            SCHEMA,
            &policy,
            ROWS,
            62,
            "refused",
            json!({"category": "unsupported-representation", "rule": "sheet-name-missing"}),
        );
        assert_eq!(result["inputs"]["policy"]["bytes"], policy.len(), "{name}");
    }
}

#[test]
fn numbers_in_documents_and_rows_are_classified() {
    use serde_json::json;
    // A quantize scale no decimal can have is an invalid member; one that
    // only this column cannot have does not fit the schema.
    for scale in ["39", "255", "256", "18446744073709551616"] {
        refused(
            &format!("scale-{scale}"),
            SCHEMA,
            &POLICY.replace("\"scale\":2", &format!("\"scale\":{scale}")),
            ROWS,
            61,
            "refused",
            json!({"category": "invalid-input", "rule": "policy-member-invalid"}),
        );
    }
    for scale in ["13", "38"] {
        refused(
            &format!("scale-{scale}"),
            SCHEMA,
            &POLICY.replace("\"scale\":2", &format!("\"scale\":{scale}")),
            ROWS,
            60,
            "refused",
            json!({"category": "schema-mismatch", "rule": "quantize-scale-too-large"}),
        );
    }
    // An integer too large for its member is an invalid member, however
    // large; a number outside the document grammar is malformed.
    for (name, precision, rule) in [
        ("precision-39", "39", "schema-member-invalid"),
        (
            "precision-u64",
            "18446744073709551615",
            "schema-member-invalid",
        ),
        (
            "precision-past-u64",
            "18446744073709551616",
            "schema-member-invalid",
        ),
        (
            "precision-huge",
            "1000000000000000000000000000000000000000000000000",
            "schema-member-invalid",
        ),
        ("precision-fraction", "12.0", "schema-malformed"),
        ("precision-exponent", "1e1", "schema-malformed"),
        ("precision-negative", "-12", "schema-malformed"),
        ("precision-leading-zero", "012", "schema-malformed"),
    ] {
        let case = Case::new(
            name,
            &SCHEMA.replace("\"precision\":12", &format!("\"precision\":{precision}")),
            POLICY,
            ROWS,
        );
        assert_eq!(case.run(&[]).status.code(), Some(61), "{name}");
        assert_eq!(
            case.result()["refusal"],
            json!({"category": "invalid-input", "rule": rule}),
            "{name}"
        );
    }
    // In a row, a JSON number is forbidden; a token that only begins like a
    // number is not JSON.
    let rows = |element: &str| {
        format!("[\"INV-1\",\"3\",\"12.3450\",true]\n[\"INV-2\",{element},null,null]\n")
    };
    for (at, (element, rule)) in [
        ("7", "numeric-literal"),
        ("-0", "numeric-literal"),
        ("1.5", "numeric-literal"),
        ("1e3", "numeric-literal"),
        ("-", "malformed-row"),
        ("01", "malformed-row"),
        ("1.", "malformed-row"),
        ("1e", "malformed-row"),
        ("1x", "malformed-row"),
        ("+1", "malformed-row"),
        (".5", "malformed-row"),
    ]
    .into_iter()
    .enumerate()
    {
        refused(
            &format!("element-{at}"),
            SCHEMA,
            POLICY,
            &rows(element),
            61,
            "refused",
            json!({"category": "invalid-input", "rule": rule, "row": 1}),
        );
    }
}

#[test]
fn timestamps_are_judged_well_formed_before_they_are_compared_with_the_column() {
    use serde_json::json;
    let schema = r#"{"format":"tabular-exchange.schema/0","columns":[
 {"id":"seen","label":"Seen","type":"local-timestamp","fraction":0,"nullable":true},
 {"id":"sent","label":"Sent","type":"offset-timestamp","fraction":3,"nullable":true}]}"#;
    let policy = policy_extended(r#"{"tableprims":{"sheet":"Data"}}"#);
    for (name, row, exit, category, rule, column) in [
        (
            "local-with-offset",
            r#"["2026-11-19T08:30:00Z",null]"#,
            60,
            "schema-mismatch",
            "temporal-offset-mismatch",
            "seen",
        ),
        (
            "offset-without-offset",
            r#"[null,"2026-11-19T08:30:00.123"]"#,
            60,
            "schema-mismatch",
            "temporal-offset-mismatch",
            "sent",
        ),
        (
            "point-without-digit",
            r#"["2026-11-19T08:30:00.",null]"#,
            61,
            "invalid-input",
            "temporal-malformed",
            "seen",
        ),
        (
            "bad-offset-and-fraction",
            r#"[null,"2026-11-19T08:30:00.12+99:99"]"#,
            61,
            "invalid-input",
            "temporal-invalid-offset",
            "sent",
        ),
        (
            "negative-zero-and-fraction",
            r#"[null,"2026-11-19T08:30:00.12-00:00"]"#,
            61,
            "invalid-input",
            "temporal-negative-zero-offset",
            "sent",
        ),
        (
            "local-bad-offset",
            r#"["2026-11-19T08:30:00+24:00",null]"#,
            61,
            "invalid-input",
            "temporal-invalid-offset",
            "seen",
        ),
        (
            "local-negative-zero",
            r#"["2026-11-19T08:30:00-00:00",null]"#,
            61,
            "invalid-input",
            "temporal-negative-zero-offset",
            "seen",
        ),
        (
            "fraction-before-offset",
            r#"[null,"2026-11-19T08:30:00"]"#,
            60,
            "schema-mismatch",
            "temporal-fraction-mismatch",
            "sent",
        ),
    ] {
        refused(
            name,
            schema,
            &policy,
            &format!("{row}\n"),
            exit,
            "refused",
            json!({"category": category, "rule": rule, "row": 0, "column": column}),
        );
    }
}

#[test]
fn document_and_file_failures_are_classified() {
    use serde_json::json;
    let case = Case::new(
        "schema-type",
        &SCHEMA.replace("\"boolean\"", "\"uuid\""),
        POLICY,
        ROWS,
    );
    assert_eq!(case.run(&[]).status.code(), Some(62));
    let result = case.result();
    assert_eq!(
        result["refusal"],
        json!({"category": "unsupported-type", "rule": "schema-type-unsupported"})
    );
    // Nothing was resolved, so the extension carries the tool and the
    // workbook writer only.
    let ours = result["extensions"]["tableprims"].as_object().unwrap();
    assert_eq!(ours.keys().collect::<Vec<_>>(), ["backend", "tool"]);
    assert!(result.get("policy_digest").is_none() && result.get("limits").is_none());

    let case = Case::new("schema-utf8", SCHEMA, POLICY, ROWS);
    fs::write(case.dir.join("schema.json"), [0xff_u8, 0xfe]).unwrap();
    assert_eq!(case.run(&[]).status.code(), Some(61));
    assert_eq!(case.result()["refusal"]["rule"], "schema-not-utf8");

    for (missing, rule) in [
        ("rows.ndjson", "rows-not-found"),
        ("schema.json", "schema-not-found"),
        ("policy.json", "policy-not-found"),
    ] {
        let case = Case::new(&format!("missing-{rule}"), SCHEMA, POLICY, ROWS);
        fs::remove_file(case.dir.join(missing)).unwrap();
        assert_eq!(case.run(&[]).status.code(), Some(51), "{missing}");
        let result = case.result();
        assert_eq!(result["status"], "failed");
        assert_eq!(
            result["refusal"],
            json!({"category": "io-failure", "rule": rule})
        );
        let input = match missing {
            "rows.ndjson" => "source",
            other => other.split('.').next().unwrap(),
        };
        assert!(result["inputs"].get(input).is_none());
        assert!(!case.dir.join("out.xlsx").exists());
    }
}

#[test]
fn a_missing_output_directory_fails_without_staging_elsewhere() {
    let case = Case::new("no-directory", SCHEMA, POLICY, ROWS);
    let output = case.path("absent/out.xlsx");
    let run = case.run(&["--output", &output]);
    assert_eq!(run.status.code(), Some(54));
    assert_eq!(
        case.result()["refusal"],
        serde_json::json!({"category": "io-failure", "rule": "output-create-failed"})
    );
    assert_eq!(
        case.names(),
        ["policy.json", "result.json", "rows.ndjson", "schema.json"]
    );
}

#[test]
fn capabilities_list_the_conversion() {
    let output = Command::new(env!("CARGO_BIN_EXE_tablebolt"))
        .args(["capabilities", "--json"])
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["conversions"],
        serde_json::json!([{"source": "ndjson", "destination": "xlsx"}])
    );
}

fn spawn_long(case: &Case) -> std::process::Child {
    spawn_named(case, "out.xlsx", "result.json")
}

fn spawn_named(case: &Case, output: &str, result: &str) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_tablebolt"))
        .args(["xlsx", "write", "--input"])
        .arg(case.path("rows.ndjson"))
        .arg("--schema")
        .arg(case.path("schema.json"))
        .arg("--policy")
        .arg(case.path("policy.json"))
        .arg("--output")
        .arg(case.path(output))
        .arg("--result")
        .arg(case.path(result))
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

/// Waits for runs started together and returns their exit codes. A run
/// here ends with 0 or, having found a name taken, 54; any other code is a
/// failure of the host, such as exhausted storage, and fails the test.
fn finish(children: Vec<std::process::Child>) -> Vec<Option<i32>> {
    let codes: Vec<Option<i32>> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap().status.code())
        .collect();
    for code in &codes {
        assert!(matches!(code, Some(0 | 54)), "{codes:?}");
    }
    codes
}

fn completed(codes: &[Option<i32>]) -> usize {
    codes.iter().filter(|code| **code == Some(0)).count()
}

fn read_result(case: &Case, name: &str) -> Value {
    serde_json::from_slice(&fs::read(case.dir.join(name)).unwrap()).unwrap()
}

/// Waits until the run has started writing its workbook.
fn wait_for_workbook_temporary(case: &Case, child: &mut std::process::Child) {
    let workbook =
        |name: &String| name.starts_with(".tablebolt-") && name.ends_with(".workbook.tmp");
    let started = std::time::Instant::now();
    while !case.names().iter().any(workbook) {
        assert!(
            child.try_wait().unwrap().is_none(),
            "the run ended before its workbook was started"
        );
        assert!(
            started.elapsed().as_secs() < 60,
            "no temporary file appeared"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn long_rows(count: usize) -> String {
    "[\"INV-1\",\"3\",\"12.3450\",true]\n".repeat(count)
}

/// A kill (SIGKILL on Unix) cannot be handled: it ends the process where it
/// stands. The workbook is only ever published under its name when
/// complete, so nothing may be at the destination. What can remain is this program's temporary
/// files and the workbook writer's scratch files.
#[test]
fn a_killed_run_leaves_no_workbook_and_no_result() {
    let case = Case::new("killed", SCHEMA, POLICY, &long_rows(400_000));
    let mut child = spawn_long(&case);
    wait_for_workbook_temporary(&case, &mut child);
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    let names = case.names();
    assert!(!names.contains(&"out.xlsx".to_owned()), "{names:?}");
    assert!(!names.contains(&"result.json".to_owned()), "{names:?}");
    for name in names {
        let ours = name.starts_with(".tablebolt-") && name.ends_with(".tmp");
        let scratch = name.starts_with(".tmp");
        assert!(INPUTS.contains(&name.as_str()) || ours || scratch, "{name}");
    }
}

/// Sends `signal` (`INT` or `TERM`) to a running child.
#[cfg(unix)]
fn send(child: &std::process::Child, signal: &str) {
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(status.success(), "kill -{signal}");
}

/// Waits for a child for at most a minute, killing it and failing the test
/// if it has not ended by then.
#[cfg(unix)]
fn wait_bounded(mut child: std::process::Child) -> Output {
    let started = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed().as_secs() >= 60 {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the run did not end within a minute");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
}

/// How a child ended: its exit code, or the signal that ended it.
#[cfg(unix)]
fn ending(output: &Output) -> (Option<i32>, Option<i32>) {
    use std::os::unix::process::ExitStatusExt;
    (output.status.code(), output.status.signal())
}

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

/// Checks a run stopped by `signal`: it published a `cancelled` result,
/// left nothing else, and then ended by that signal, which a shell reports
/// as 128 plus the signal number (130, 143).
#[cfg(unix)]
fn check_cancelled(case: &Case, output: &Output, signal: i32) -> Value {
    assert_eq!(ending(output), (None, Some(signal)));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "tablebolt: cancelled: cancelled: cancelled\n"
    );
    // The workbook's temporary and the writer's scratch files are gone.
    assert_eq!(
        case.names(),
        ["policy.json", "result.json", "rows.ndjson", "schema.json"]
    );
    let result = case.result();
    assert_eq!(result["status"], "cancelled");
    assert_eq!(
        result["refusal"],
        serde_json::json!({"category": "cancelled", "rule": "cancelled"})
    );
    result
}

/// SIGINT or SIGTERM while the workbook is written stops the run: no
/// workbook, a result that says `cancelled`, and death by that signal.
#[cfg(unix)]
#[test]
fn a_signal_while_writing_stops_the_run_and_says_so() {
    for (signal, number) in [("INT", SIGINT), ("TERM", SIGTERM)] {
        let case = Case::new(
            &format!("signal-{signal}"),
            SCHEMA,
            POLICY,
            &long_rows(400_000),
        );
        let mut child = spawn_long(&case);
        wait_for_workbook_temporary(&case, &mut child);
        send(&child, signal);
        let output = wait_bounded(child);
        let result = check_cancelled(&case, &output, number);
        // The first read was complete, so the rows are bound.
        assert_eq!(
            result["inputs"]["source"],
            sum(&case.dir.join("rows.ndjson"))
        );
    }
}

/// A case whose rows are a named pipe, and a run reading it. Returns once
/// the run has opened the pipe, which it does after reading the schema and
/// the policy and installing its handlers, with the writing end.
#[cfg(unix)]
fn run_on_pipe(name: &str) -> (Case, std::process::Child, fs::File) {
    let case = Case::new(name, SCHEMA, POLICY, "");
    let rows = case.dir.join("rows.ndjson");
    fs::remove_file(&rows).unwrap();
    assert!(Command::new("mkfifo")
        .arg(&rows)
        .status()
        .unwrap()
        .success());
    let child = spawn_long(&case);
    // Opening blocks until the run opens the other end.
    let writer = fs::OpenOptions::new().write(true).open(&rows).unwrap();
    (case, child, writer)
}

/// Replaces the pipe with an empty file so the directory lists as usual.
#[cfg(unix)]
fn unpipe(case: &Case) {
    let rows = case.dir.join("rows.ndjson");
    let _ = fs::remove_file(&rows);
    fs::write(&rows, "").unwrap();
}

/// A signal during the first read. The run is waiting for the next row
/// when the signal comes, and checks the flag before reading it.
#[cfg(unix)]
#[test]
fn a_signal_while_reading_stops_the_run_and_says_so() {
    use std::io::Write;
    let (case, child, mut writer) = run_on_pipe("signal-read");
    writer
        .write_all(b"[\"INV-1\",\"3\",\"12.3450\",true]\n")
        .unwrap();
    send(&child, "INT");
    writer.write_all(b"[\"INV-2\",\"4\",null,null]\n").unwrap();
    drop(writer);
    let output = wait_bounded(child);
    unpipe(&case);
    let result = check_cancelled(&case, &output, SIGINT);
    // The rows were not read to their end, so they are not bound.
    assert!(result["inputs"].get("source").is_none(), "{result}");
    assert_eq!(
        result["inputs"]["schema"],
        sum(&case.dir.join("schema.json"))
    );
}

/// A second signal ends the process at once by its default action. Both
/// come while the run waits for a row, so no result has been published.
#[cfg(unix)]
#[test]
fn a_second_signal_ends_the_run_at_once() {
    for (first, second, number) in [("INT", "TERM", SIGTERM), ("TERM", "INT", SIGINT)] {
        let (case, child, writer) = run_on_pipe(&format!("signal-twice-{first}"));
        send(&child, first);
        // Let the first be handled before the second is sent; signals
        // pending together are not delivered in a promised order.
        std::thread::sleep(std::time::Duration::from_millis(200));
        send(&child, second);
        let output = wait_bounded(child);
        drop(writer);
        unpipe(&case);
        assert_eq!(ending(&output), (None, Some(number)), "{first} {second}");
        assert!(output.stderr.is_empty(), "{first} {second}");
        let names = case.names();
        assert!(!names.contains(&"out.xlsx".to_owned()), "{names:?}");
        assert!(!names.contains(&"result.json".to_owned()), "{names:?}");
    }
}

#[test]
fn paths_that_name_one_file_are_a_usage_error() {
    let case = Case::new("alias", SCHEMA, POLICY, ROWS);
    let spelled = case.dir.join(".").join("out.xlsx");
    let run = case.run(&["--result", spelled.to_str().unwrap()]);
    assert_eq!(run.status.code(), Some(64));
    assert_eq!(case.names(), INPUTS);
}

/// A result that cannot be created stops the run before a workbook exists.
#[cfg(unix)]
#[test]
fn an_unwritable_result_directory_fails_before_the_workbook() {
    use std::os::unix::fs::PermissionsExt;
    let case = Case::new("result-directory", SCHEMA, POLICY, ROWS);
    let locked = case.dir.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    if fs::write(locked.join("probe"), b"").is_ok() {
        // Running with privileges that ignore the mode: nothing to test.
        return;
    }
    let run = case.run(&["--result", locked.join("result.json").to_str().unwrap()]);
    assert_eq!(run.status.code(), Some(50));
    assert_eq!(
        case.names(),
        ["locked", "policy.json", "rows.ndjson", "schema.json"]
    );
    assert_eq!(fs::read_dir(&locked).unwrap().count(), 0);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A result name taken while the run is under way: the run fails and
/// replaces nothing. The workbook it had published stays, without a result,
/// and the run says so.
#[test]
fn a_workbook_whose_result_cannot_be_published_is_reported_incomplete() {
    let case = Case::new("orphan", SCHEMA, POLICY, &long_rows(100_000));
    let mut child = spawn_long(&case);
    wait_for_workbook_temporary(&case, &mut child);
    fs::write(case.dir.join("result.json"), b"theirs").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(54));
    assert!(String::from_utf8_lossy(&output.stderr).contains("has no result and is incomplete"));
    assert_eq!(fs::read(case.dir.join("result.json")).unwrap(), b"theirs");
    assert_eq!(
        case.names(),
        [
            "out.xlsx",
            "policy.json",
            "result.json",
            "rows.ndjson",
            "schema.json"
        ]
    );
}

/// Runs started together for one destination. At most one completes, and a
/// result with status `ok` always describes the workbook that is there.
#[test]
fn runs_racing_for_one_destination_never_pair_a_result_with_another_workbook() {
    let (mut won, mut lost) = (0, 0);
    // Either outcome is about as likely in a round. Rounds continue until
    // both have been seen, within a bound no healthy host reaches.
    for round in 0..200 {
        if won > 0 && lost > 0 && round >= 4 {
            break;
        }
        let case = Case::new("racing", SCHEMA, POLICY, &long_rows(2_000));
        let codes = finish((0..6).map(|_| spawn_long(&case)).collect());
        let names = case.names();
        // Every run ended on its own, so no temporary file remains.
        for name in &names {
            assert!(
                INPUTS.contains(&name.as_str()) || name == "out.xlsx" || name == "result.json",
                "{names:?}"
            );
        }
        // Every run opens its result before converting, and the first to
        // finish finds the name free, so a result is published.
        let result = case.result();
        let workbook = case.dir.join("out.xlsx");
        if result["status"] == "ok" {
            won += 1;
            assert_eq!(completed(&codes), 1, "{codes:?}");
            assert_eq!(result["output"], sum(&workbook));
        } else {
            lost += 1;
            assert_eq!(completed(&codes), 0, "{codes:?}");
            assert_eq!(result["refusal"]["rule"], "output-exists");
            // The run that published the workbook lost the result name.
            // Its workbook stays, without a result that describes it.
            assert!(workbook.exists(), "{names:?}");
        }
    }
    assert!(
        won > 0 && lost > 0,
        "completed {won}, none completed {lost}"
    );
}

/// Runs sharing a workbook name, each with its own result: exactly one
/// completes and the others report the name as taken.
#[test]
fn runs_racing_for_one_workbook_name_complete_once() {
    let case = Case::new("racing-output", SCHEMA, POLICY, &long_rows(2_000));
    let children = (0..6)
        .map(|run| spawn_named(&case, "out.xlsx", &format!("result-{run}.json")))
        .collect();
    let codes = finish(children);
    assert_eq!(completed(&codes), 1, "{codes:?}");
    for (run, code) in codes.iter().enumerate() {
        let result = read_result(&case, &format!("result-{run}.json"));
        if *code == Some(0) {
            assert_eq!(result["status"], "ok");
            assert_eq!(result["output"], sum(&case.dir.join("out.xlsx")));
        } else {
            assert_eq!(result["refusal"]["rule"], "output-exists");
        }
    }
    assert_eq!(case.names().len(), INPUTS.len() + 7);
}

/// Runs sharing a result name, each with its own workbook: exactly one
/// completes, and the result describes a workbook that is there.
#[test]
fn runs_racing_for_one_result_name_complete_once() {
    let case = Case::new("racing-result", SCHEMA, POLICY, &long_rows(2_000));
    let children = (0..6)
        .map(|run| spawn_named(&case, &format!("out-{run}.xlsx"), "result.json"))
        .collect();
    let codes = finish(children);
    assert_eq!(completed(&codes), 1, "{codes:?}");
    let result = case.result();
    assert_eq!(result["status"], "ok");
    let winner = codes.iter().position(|code| *code == Some(0)).unwrap();
    assert_eq!(
        result["output"],
        sum(&case.dir.join(format!("out-{winner}.xlsx")))
    );
    for name in case.names() {
        let workbook = name.starts_with("out-") && name.ends_with(".xlsx");
        assert!(
            INPUTS.contains(&name.as_str()) || name == "result.json" || workbook,
            "{name}"
        );
    }
}

/// A destination directory that stops accepting new names while the run is
/// under way: the cause is reported as a permission failure and nothing is
/// published.
#[cfg(unix)]
#[test]
fn a_directory_made_unwritable_during_the_run_is_a_permission_failure() {
    use std::os::unix::fs::PermissionsExt;
    let case = Case::new("locked-late", SCHEMA, POLICY, &long_rows(100_000));
    let inputs = Case::new("locked-late-inputs", SCHEMA, POLICY, &long_rows(100_000));
    let mut child = Command::new(env!("CARGO_BIN_EXE_tablebolt"))
        .args(["xlsx", "write", "--input"])
        .arg(inputs.path("rows.ndjson"))
        .arg("--schema")
        .arg(inputs.path("schema.json"))
        .arg("--policy")
        .arg(inputs.path("policy.json"))
        .arg("--output")
        .arg(case.path("out.xlsx"))
        .arg("--result")
        .arg(case.path("result.json"))
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_workbook_temporary(&case, &mut child);
    fs::set_permissions(&case.dir, fs::Permissions::from_mode(0o555)).unwrap();
    let privileged = fs::write(case.dir.join("probe"), b"").is_ok();
    let output = child.wait_with_output().unwrap();
    fs::set_permissions(&case.dir, fs::Permissions::from_mode(0o755)).unwrap();
    if privileged {
        // Running with privileges that ignore the mode: nothing to test.
        return;
    }
    assert_eq!(output.status.code(), Some(50), "{output:?}");
    let names = case.names();
    assert!(!names.contains(&"out.xlsx".to_owned()), "{names:?}");
    assert!(!names.contains(&"result.json".to_owned()), "{names:?}");
}

#[test]
fn a_long_destination_name_is_accepted() {
    let case = Case::new("long-name", SCHEMA, POLICY, ROWS);
    let output = format!("{}.xlsx", "w".repeat(250));
    let result = format!("{}.json", "r".repeat(250));
    let run = case.run(&[
        "--output",
        &case.path(&output),
        "--result",
        &case.path(&result),
    ]);
    assert_eq!(run.status.code(), Some(0));
    assert!(case.dir.join(&output).exists() && case.dir.join(&result).exists());
    assert_eq!(case.names().len(), 5);
}

// Fault controls for matching a result to a workbook and to a request, as
// a consumer does (see "Matching a result" in docs/xlsx-write.md).

/// What a caller records before it runs a conversion: the run identifier it
/// passes and the length and SHA-256 of each input file.
struct Request {
    run_id: &'static str,
    source: Value,
    schema: Value,
    policy: Value,
}

impl Request {
    fn record(case: &Case, run_id: &'static str) -> Self {
        Self {
            run_id,
            source: sum(&case.dir.join("rows.ndjson")),
            schema: sum(&case.dir.join("schema.json")),
            policy: sum(&case.dir.join("policy.json")),
        }
    }
}

/// The verdicts `output-agreement` and `requested-run-completed` for a
/// result file's bytes, if there is one, and a candidate workbook's bytes.
fn verdicts(result: Option<&[u8]>, workbook: &[u8], request: &Request) -> (bool, bool) {
    let Some(result) = result.and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok()) else {
        return (false, false);
    };
    let hex: String = Sha256::digest(workbook)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let output_agreement = result["format"] == "tabular-exchange.result/0"
        && result["contract"] == "tabular-exchange/0.1.0"
        && result["status"] == "ok"
        && result["output"] == serde_json::json!({"sha256": hex, "bytes": workbook.len()});
    let requested = output_agreement
        && result["run_id"] == request.run_id
        && result["inputs"]["source"] == request.source
        && result["inputs"]["schema"] == request.schema
        && result["inputs"]["policy"] == request.policy;
    (output_agreement, requested)
}

fn read_pair(case: &Case) -> (Vec<u8>, Vec<u8>) {
    (
        fs::read(case.dir.join("result.json")).unwrap(),
        fs::read(case.dir.join("out.xlsx")).unwrap(),
    )
}

/// Control: a completed run reaches both verdicts, so a verdict lost below
/// comes from the fault. A workbook without a result reaches neither.
#[test]
fn a_completed_run_matches_its_workbook_and_its_request() {
    let case = Case::new("match-ok", SCHEMA, POLICY, ROWS);
    let request = Request::record(&case, "case-0001");
    assert_eq!(case.run(&["--run-id", "case-0001"]).status.code(), Some(0));
    let (result, workbook) = read_pair(&case);
    assert_eq!(verdicts(Some(&result), &workbook, &request), (true, true));
    assert_eq!(verdicts(None, &workbook, &request), (false, false));
}

/// A workbook changed after it was published no longer matches its result:
/// cut short, as an interrupted copy leaves it, or one byte changed at the
/// same length.
#[test]
fn a_changed_or_truncated_workbook_matches_nothing() {
    let case = Case::new("match-truncated", SCHEMA, POLICY, ROWS);
    let request = Request::record(&case, "case-0001");
    assert_eq!(case.run(&["--run-id", "case-0001"]).status.code(), Some(0));
    let (result, workbook) = read_pair(&case);
    for keep in [0, workbook.len() / 2, workbook.len() - 1] {
        assert_eq!(
            verdicts(Some(&result), &workbook[..keep], &request),
            (false, false),
            "{keep}"
        );
    }
    for at in [0, workbook.len() / 2, workbook.len() - 1] {
        let mut changed = workbook.clone();
        changed[at] ^= 0x01;
        assert_eq!(
            verdicts(Some(&result), &changed, &request),
            (false, false),
            "{at}"
        );
    }
}

/// A result that is not this request's: another run identifier, or other
/// schema or policy bytes than the caller recorded. The result still agrees
/// with its workbook; only the match with the request fails.
#[test]
fn a_result_for_another_request_fails_only_the_request_match() {
    let case = Case::new("match-other", SCHEMA, POLICY, ROWS);
    let request = Request::record(&case, "case-0001");
    assert_eq!(case.run(&["--run-id", "case-0001"]).status.code(), Some(0));
    let (result, workbook) = read_pair(&case);
    let other = serde_json::json!({"sha256": "0".repeat(64), "bytes": 1});
    let requests = [
        Request {
            run_id: "case-0002",
            ..Request::record(&case, "")
        },
        Request {
            schema: other.clone(),
            ..Request::record(&case, "case-0001")
        },
        Request {
            policy: other,
            ..Request::record(&case, "case-0001")
        },
    ];
    for (index, other) in requests.iter().enumerate() {
        assert_eq!(
            verdicts(Some(&result), &workbook, other),
            (true, false),
            "{index}"
        );
    }
    assert_eq!(verdicts(Some(&result), &workbook, &request), (true, true));
}

/// A result whose wire format, contract revision or status is not the one a
/// consumer reads as complete establishes neither verdict, though its output
/// digest and length are the workbook's.
#[test]
fn a_result_that_is_not_ok_under_this_contract_matches_nothing() {
    let case = Case::new("match-envelope", SCHEMA, POLICY, ROWS);
    let request = Request::record(&case, "case-0001");
    assert_eq!(case.run(&["--run-id", "case-0001"]).status.code(), Some(0));
    let (result, workbook) = read_pair(&case);
    let result: Value = serde_json::from_slice(&result).unwrap();
    for (member, value) in [
        ("format", "tabular-exchange.result/1"),
        ("contract", "tabular-exchange/0.2.0"),
        ("status", "failed"),
    ] {
        let mut altered = result.clone();
        altered[member] = Value::from(value);
        let bytes = serde_json::to_vec(&altered).unwrap();
        assert_eq!(
            verdicts(Some(&bytes), &workbook, &request),
            (false, false),
            "{member}"
        );
    }
}

/// Rows cut short at the end of a line after the caller recorded them. The
/// rows that remain are well formed, so the run completes and its result
/// agrees with its workbook: only the match with the request shows that a
/// row, or the final line terminator, is missing.
#[test]
fn rows_cut_at_a_line_end_fail_only_the_request_match() {
    let full = ROWS.len();
    let last_line = ROWS[..full - 1].rfind('\n').unwrap() + 1;
    for (keep, rows) in [(last_line, 2), (full - 1, 3)] {
        let case = Case::new(&format!("match-cut-{keep}"), SCHEMA, POLICY, ROWS);
        let request = Request::record(&case, "case-0001");
        fs::write(case.dir.join("rows.ndjson"), &ROWS[..keep]).unwrap();
        assert_eq!(case.run(&["--run-id", "case-0001"]).status.code(), Some(0));
        assert_eq!(case.result()["counts"]["rows"], rows);
        let (result, workbook) = read_pair(&case);
        assert_eq!(
            verdicts(Some(&result), &workbook, &request),
            (true, false),
            "{keep}"
        );
    }
}

/// Rows cut inside the last line are refused as malformed, and no workbook
/// is published, so there is nothing to match.
#[test]
fn rows_cut_inside_a_line_are_refused() {
    let full = ROWS.len();
    let last_line = ROWS[..full - 1].rfind('\n').unwrap() + 1;
    for keep in last_line + 1..full - 1 {
        refused(
            &format!("cut-{keep}"),
            SCHEMA,
            POLICY,
            &ROWS[..keep],
            61,
            "refused",
            serde_json::json!({"category": "invalid-input", "rule": "malformed-row", "row": 2}),
        );
    }
}

/// Rows cut inside a character of the last line are refused as invalid
/// UTF-8 before they are read as JSON.
#[test]
fn rows_cut_inside_a_character_are_refused() {
    let rows = format!("{ROWS}[\"INV-\u{e9}\",\"2\",null,null]\n");
    let cut = rows.find('\u{e9}').unwrap() + 1;
    let case = Case::new("cut-utf8", SCHEMA, POLICY, "");
    fs::write(case.dir.join("rows.ndjson"), &rows.as_bytes()[..cut]).unwrap();
    assert_eq!(case.run(&[]).status.code(), Some(61));
    assert_eq!(
        case.names(),
        ["policy.json", "result.json", "rows.ndjson", "schema.json"]
    );
    let result = case.result();
    assert_eq!(result["status"], "refused");
    assert_eq!(
        result["refusal"],
        serde_json::json!({"category": "invalid-input", "rule": "invalid-utf8", "row": 3})
    );
}
