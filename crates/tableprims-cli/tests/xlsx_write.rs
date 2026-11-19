// SPDX-License-Identifier: MIT OR Apache-2.0

//! Runs `tablebolt xlsx write` and checks the exit code, the result file
//! and what is left on disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA: &str = r#"{"format":"tableprims.schema/0","columns":[
 {"id":"invoice","label":"Invoice","type":"string","nullable":false},
 {"id":"qty","label":"Qty","type":"integer","nullable":false},
 {"id":"amount","label":"Amount","type":"decimal","precision":12,"scale":4,"nullable":true},
 {"id":"paid","label":"Paid","type":"boolean","nullable":true}]}"#;
const POLICY: &str = r#"{"format":"tableprims.policy/0","exact_numbers":"require-exact","sheet":"Data",
 "quantize":[{"column":"amount","scale":2,"rounding":"half-even"}]}"#;
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
        serde_json::from_slice(&fs::read(self.dir.join("result.json")).unwrap()).unwrap()
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
    assert_eq!(result["format"], "tableprims.result/0");
    assert_eq!(result["status"], "ok");
    assert_eq!(result["run_id"], "case-0001");
    assert!(result.get("refusal").is_none());
    assert_eq!(result["output"], sum(&case.dir.join("out.xlsx")));
    assert!(fs::read(case.dir.join("out.xlsx"))
        .unwrap()
        .starts_with(b"PK"));
    assert_eq!(result["inputs"]["rows"], sum(&case.dir.join("rows.ndjson")));
    assert_eq!(
        result["inputs"]["schema"],
        sum(&case.dir.join("schema.json"))
    );
    assert_eq!(
        result["inputs"]["policy"],
        sum(&case.dir.join("policy.json"))
    );
    assert_eq!(result["tool"]["name"], "tablebolt");
    assert_eq!(result["backend"]["version"], "0.102.0");
    assert_eq!(result["profile"]["id"], "xlsx-write/r1");
    assert_eq!(result["policy_digest"].as_str().unwrap().len(), 64);
    assert_eq!(
        result["limits"],
        serde_json::json!({"max_rows": 1_048_575, "max_line_bytes": 1_048_576, "max_empty_ranges": 65_536})
    );
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
        assert!(result["inputs"].get("rows").is_none(), "{name}");
        assert_eq!(
            result["policy_digest"].as_str().unwrap().len(),
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
        assert!(result["inputs"]
            .get(missing.split('.').next().unwrap())
            .is_none());
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

/// Without a signal handler a kill ends the process where it stands. The
/// workbook is only ever published under its name when complete, so nothing
/// may be at the destination. What can remain is this program's temporary
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
