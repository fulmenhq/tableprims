// SPDX-License-Identifier: MIT OR Apache-2.0

//! Process-run telemetry of `tablebolt xlsx write`: the card and the event
//! stream, checked against the process-run v0 schemas vendored from 3leaps
//! Crucible with an independent JSON Schema validator.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA: &str = r#"{"format":"tabular-exchange.schema/0","columns":[
 {"id":"invoice","label":"Invoice","type":"string","nullable":false},
 {"id":"qty","label":"Qty","type":"integer","nullable":false}]}"#;
const POLICY: &str = r#"{"format":"tabular-exchange.policy/0","exact_numbers":"require-exact",
 "extensions":{"tableprims":{"sheet":"Data"}}}"#;
const ROWS: &str = "[\"INV-1\",\"3\"]\n[\"INV-2\",\"7\"]\n";

/// SHA-256 of `third_party/3leaps-crucible/SHA256SUMS`.
const SUMS_SHA256: &str = "388cc8377fa3ad5036ef1878b6e8216e97389189e0743d260eb87aea2c30696c";

fn vendor() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/3leaps-crucible")
}

fn schema(name: &str) -> jsonschema::Validator {
    let path = vendor().join("v0.2.12/schemas/process-run/v0").join(name);
    let schema: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    jsonschema::draft202012::options()
        .should_validate_formats(true)
        .build(&schema)
        .unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A scratch directory with the inputs and a runtime directory, removed
/// when dropped.
struct Case {
    dir: PathBuf,
}

impl Case {
    fn new(name: &str, rows: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("tablebolt-pr-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("schema.json"), SCHEMA).unwrap();
        fs::write(dir.join("policy.json"), POLICY).unwrap();
        fs::write(dir.join("rows.ndjson"), rows).unwrap();
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn command(&self, extra: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tablebolt"));
        command
            .args(["xlsx", "write", "--input"])
            .arg(self.path("rows.ndjson"))
            .arg("--schema")
            .arg(self.path("schema.json"))
            .arg("--policy")
            .arg(self.path("policy.json"))
            .arg("--output")
            .arg(self.path("out.xlsx"))
            .arg("--result")
            .arg(self.path("result.json"))
            .args(extra)
            .env_remove("TABLEBOLT_RUNTIME_DIR");
        command
    }

    fn run(&self, extra: &[&str]) -> Output {
        self.command(extra).output().unwrap()
    }

    fn runtime(&self) -> String {
        self.path("rt").to_str().unwrap().to_owned()
    }

    fn stream(&self, run_id: &str) -> PathBuf {
        self.path("rt")
            .join("events")
            .join(format!("{run_id}.ndjson"))
    }

    fn card(&self, run_id: &str) -> PathBuf {
        self.path("rt").join("proc").join(run_id).join("card.json")
    }

    fn result(&self) -> Value {
        serde_json::from_slice(&fs::read(self.path("result.json")).unwrap()).unwrap()
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Reads a stream and checks what every stream must satisfy: each line is
/// a valid event of this run, `seq` counts from 0 without a gap, the first
/// event is `started` with the producer, and at most one terminal event
/// appears, last. Returns the events.
fn events(path: &Path, run_id: &str) -> Vec<Value> {
    let validator = schema("process-event.schema.json");
    let text = fs::read_to_string(path).unwrap();
    assert!(text.ends_with('\n'), "{text}");
    let events: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for (seq, event) in events.iter().enumerate() {
        assert!(validator.is_valid(event), "{event}");
        assert_eq!(event["seq"], seq, "{event}");
        assert_eq!(event["run_id"], run_id);
        assert!(event["ts"].as_str().unwrap().ends_with('Z'));
    }
    assert_eq!(events[0]["event"], "started");
    assert_eq!(events[0]["producer"]["name"], "tablebolt");
    assert_eq!(
        events[0]["producer"]["profile"],
        "tableprims.xlsx-write-run/v0"
    );
    let terminal = ["completed", "canceled", "failed"];
    let ends: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| terminal.contains(&e["event"].as_str().unwrap()))
        .map(|(i, _)| i)
        .collect();
    assert!(
        ends.is_empty() || ends == [events.len() - 1],
        "terminal events at {ends:?} of {}",
        events.len()
    );
    events
}

fn last(events: &[Value]) -> &Value {
    events.last().unwrap()
}

#[test]
fn the_vendored_schemas_are_as_released() {
    let sums = fs::read(vendor().join("SHA256SUMS")).unwrap();
    assert_eq!(sha256_hex(&sums), SUMS_SHA256, "SHA256SUMS was changed");
    let listed: Vec<(String, String)> = String::from_utf8(sums)
        .unwrap()
        .lines()
        .map(|line| {
            let (digest, path) = line.split_once("  ").unwrap();
            (path.to_owned(), digest.to_owned())
        })
        .collect();
    let mut found = Vec::new();
    let mut pending = vec![vendor().join("v0.2.12")];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(vendor().join("v0.2.12")).unwrap();
                found.push(relative.to_str().unwrap().replace('\\', "/"));
            }
        }
    }
    found.sort();
    let mut names: Vec<&String> = listed.iter().map(|(path, _)| path).collect();
    names.sort();
    assert_eq!(found.iter().collect::<Vec<_>>(), names);
    for (path, digest) in &listed {
        let bytes = fs::read(vendor().join("v0.2.12").join(path)).unwrap();
        assert_eq!(&sha256_hex(&bytes), digest, "{path}");
    }
    // The goldens are valid under the schemas they accompany.
    let golden = vendor().join("v0.2.12/schemas/process-run/v0/examples");
    let card: Value =
        serde_json::from_slice(&fs::read(golden.join("process-card.example.json")).unwrap())
            .unwrap();
    assert!(schema("process-card.schema.json").is_valid(&card));
    let validator = schema("process-event.schema.json");
    for line in fs::read_to_string(golden.join("process-events.example.ndjson"))
        .unwrap()
        .lines()
    {
        assert!(validator.is_valid(&serde_json::from_str(line).unwrap()));
    }
}

/// Without a runtime directory nothing is written beyond the workbook and
/// the result.
#[test]
fn telemetry_is_off_unless_asked() {
    let case = Case::new("off", ROWS);
    assert_eq!(case.run(&[]).status.code(), Some(0));
    assert!(!case.path("rt").exists());
    // A blank value is unset: no directory named by it appears where the
    // run is started.
    for blank in ["", "  "] {
        let case = Case::new("blank", ROWS);
        let output = case
            .command(&[])
            .env("TABLEBOLT_RUNTIME_DIR", blank)
            .current_dir(&case.dir)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "{blank:?}");
        let mut names: Vec<String> = fs::read_dir(&case.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "out.xlsx",
                "policy.json",
                "result.json",
                "rows.ndjson",
                "schema.json"
            ],
            "{blank:?}"
        );
    }
}

#[test]
fn a_completed_run_reports_its_output_and_removes_its_card() {
    let case = Case::new("ok", ROWS);
    let runtime = case.runtime();
    let output = case.run(&["--run-id", "case-1", "--runtime-dir", &runtime]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let events = events(&case.stream("case-1"), "case-1");
    let end = last(&events);
    assert_eq!(end["event"], "completed");
    assert_eq!(end["severity"], "INFO");
    let result = case.result();
    assert_eq!(end["data"]["status"], "ok");
    assert_eq!(end["data"]["rows"], 2);
    assert_eq!(end["data"]["output"], result["output"]);
    assert_eq!(events[0]["data"]["source_bytes"], ROWS.len());
    // The card is gone and so is its directory; the stream stays.
    assert!(!case.card("case-1").exists());
    assert!(!case.path("rt/proc/case-1").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&case.stream("case-1")), 0o600);
        for dir in ["rt", "rt/proc", "rt/events"] {
            assert_eq!(mode(&case.path(dir)), 0o700, "{dir}");
        }
    }
}

#[test]
fn the_environment_names_the_runtime_directory_when_the_flag_does_not() {
    let case = Case::new("env", ROWS);
    let output = case
        .command(&["--run-id", "case-1"])
        .env("TABLEBOLT_RUNTIME_DIR", case.runtime())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        last(&events(&case.stream("case-1"), "case-1"))["event"],
        "completed"
    );
    // The flag wins over the environment.
    let other = Case::new("env-flag", ROWS);
    let flag = other.runtime();
    let output = other
        .command(&["--run-id", "case-1", "--runtime-dir", &flag])
        .env("TABLEBOLT_RUNTIME_DIR", case.path("unused"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(other.stream("case-1").exists());
    assert!(!case.path("unused").exists());
}

#[test]
fn a_refused_run_reports_its_category_and_rule() {
    let case = Case::new("refused", "[\"INV-1\",3]\n");
    let runtime = case.runtime();
    let output = case.run(&["--run-id", "case-1", "--runtime-dir", &runtime]);
    assert_eq!(output.status.code(), Some(61));
    let events = events(&case.stream("case-1"), "case-1");
    let end = last(&events);
    assert_eq!(end["event"], "failed");
    assert_eq!(end["severity"], "ERROR");
    assert_eq!(
        end["data"],
        serde_json::json!({"status": "refused", "reason": "invalid-input/numeric-literal"})
    );
    assert_eq!(case.result()["status"], "refused");
    assert!(!case.card("case-1").exists());
}

/// Run identifiers that cannot name a file are refused before anything is
/// written, when a runtime directory is in use, and accepted otherwise.
#[test]
fn run_ids_that_cannot_name_telemetry_files_are_a_usage_error() {
    for id in [".", "..", "CON", "nul.txt", "com1", "LPT9"] {
        let case = Case::new(&format!("id-{}", id.replace('.', "dot")), ROWS);
        let runtime = case.runtime();
        let output = case.run(&["--run-id", id, "--runtime-dir", &runtime]);
        assert_eq!(output.status.code(), Some(64), "{id}");
        assert!(
            !case.path("rt").exists() && !case.path("result.json").exists(),
            "{id}"
        );
    }
    let case = Case::new("id-plain", ROWS);
    assert_eq!(case.run(&["--run-id", "CON"]).status.code(), Some(0));
}

/// Telemetry failures change nothing: a runtime directory that is a file, a
/// stream that already exists (left untouched), and on Unix a directory
/// that cannot be written.
#[test]
fn telemetry_failures_do_not_affect_the_run() {
    let case = Case::new("open-file", ROWS);
    fs::write(case.path("rt"), b"not a directory").unwrap();
    let runtime = case.runtime();
    let output = case.run(&["--run-id", "case-1", "--runtime-dir", &runtime]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(case.result()["status"], "ok");
    assert_eq!(fs::read(case.path("rt")).unwrap(), b"not a directory");

    let case = Case::new("open-existing", ROWS);
    fs::create_dir_all(case.path("rt/events")).unwrap();
    fs::write(case.stream("case-1"), b"theirs\n").unwrap();
    let runtime = case.runtime();
    let output = case.run(&["--run-id", "case-1", "--runtime-dir", &runtime]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read(case.stream("case-1")).unwrap(), b"theirs\n");
    assert!(!case.path("rt/proc/case-1").exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let case = Case::new("open-locked", ROWS);
        fs::create_dir(case.path("rt")).unwrap();
        fs::set_permissions(case.path("rt"), fs::Permissions::from_mode(0o500)).unwrap();
        let runtime = case.runtime();
        let output = case.run(&["--run-id", "case-1", "--runtime-dir", &runtime]);
        fs::set_permissions(case.path("rt"), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        assert_eq!(case.result()["status"], "ok");
    }
}

/// The run-id rule applies when the environment names the directory.
#[test]
fn the_run_id_rule_applies_through_the_environment() {
    let case = Case::new("id-env", ROWS);
    let output = case
        .command(&["--run-id", "a."])
        .env("TABLEBOLT_RUNTIME_DIR", case.runtime())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(64));
    assert!(!case.path("rt").exists() && !case.path("result.json").exists());
}

/// A relative runtime directory is taken from where the run starts, and the
/// card still names the stream by absolute path (the card would otherwise
/// fail its check and telemetry would be off).
#[test]
fn a_relative_runtime_directory_is_made_absolute() {
    let case = Case::new("relative", ROWS);
    let output = case
        .command(&["--run-id", "case-1", "--runtime-dir", "rt"])
        .current_dir(&case.dir)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        last(&events(&case.stream("case-1"), "case-1"))["event"],
        "completed"
    );
}

/// A card directory that already exists for the run id is not reused: the
/// run has no telemetry and the directory is left as it was.
#[test]
fn an_existing_card_directory_turns_telemetry_off() {
    let case = Case::new("card-dir", ROWS);
    fs::create_dir_all(case.path("rt/proc/case-1")).unwrap();
    fs::write(case.path("rt/proc/case-1/theirs"), b"x").unwrap();
    let runtime = case.runtime();
    let output = case.run(&["--run-id", "case-1", "--runtime-dir", &runtime]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(!case.stream("case-1").exists());
    assert_eq!(fs::read(case.path("rt/proc/case-1/theirs")).unwrap(), b"x");
    assert!(!case.card("case-1").exists());
}

/// A run that converted but could not publish its result ends `failed`
/// with the result-write rule, never `completed`.
#[test]
fn a_converted_run_without_a_result_ends_failed() {
    let rows = "[\"INV-1\",\"3\"]\n".repeat(200_000);
    let case = Case::new("orphan", &rows);
    let runtime = case.runtime();
    let mut child = case
        .command(&["--run-id", "case-1", "--runtime-dir", &runtime])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Take the result name once the workbook is being written.
    let started = std::time::Instant::now();
    let writing = |name: &str| name.starts_with(".tablebolt-") && name.ends_with(".workbook.tmp");
    while !fs::read_dir(&case.dir)
        .unwrap()
        .any(|e| writing(&e.unwrap().file_name().to_string_lossy()))
    {
        assert!(child.try_wait().unwrap().is_none(), "ended before writing");
        assert!(started.elapsed().as_secs() < 60, "no workbook temporary");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    fs::write(case.path("result.json"), b"theirs").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(54));
    let events = events(&case.stream("case-1"), "case-1");
    assert_eq!(
        last(&events)["data"],
        serde_json::json!({"status": "failed", "reason": "io-failure/result-write-failed"})
    );
    assert!(!case.card("case-1").exists());
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt;

    /// A child that is killed if the test ends before it does, so a failed
    /// assertion never leaves a run waiting on its pipe.
    struct Running(Option<std::process::Child>);

    impl Running {
        fn child(&self) -> &std::process::Child {
            self.0.as_ref().unwrap()
        }

        fn finish(mut self) -> Output {
            wait_bounded(self.0.take().unwrap())
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn send(child: &std::process::Child, signal: &str) {
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("kill -{signal} {}", child.id()))
            .status()
            .unwrap();
        assert!(status.success());
    }

    /// A run whose rows come through a pipe, started with telemetry, and
    /// the writing end of the pipe once the run has opened it.
    fn run_on_pipe(case: &Case, extra: &[&str]) -> (Running, fs::File) {
        let rows = case.path("rows.ndjson");
        fs::remove_file(&rows).unwrap();
        assert!(Command::new("mkfifo")
            .arg(&rows)
            .status()
            .unwrap()
            .success());
        let runtime = case.runtime();
        let mut args = vec!["--run-id", "case-1", "--runtime-dir", &runtime];
        args.extend_from_slice(extra);
        let child = case
            .command(&args)
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let child = Running(Some(child));
        let writer = fs::OpenOptions::new().write(true).open(&rows).unwrap();
        (child, writer)
    }

    fn unpipe(case: &Case) {
        let rows = case.path("rows.ndjson");
        let _ = fs::remove_file(&rows);
        fs::write(&rows, "").unwrap();
    }

    /// Waits until `ready` holds, for at most a minute.
    fn until(mut ready: impl FnMut() -> bool) {
        let started = std::time::Instant::now();
        while !ready() {
            assert!(started.elapsed().as_secs() < 60, "timed out");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

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

    fn feed(writer: &mut fs::File, row: &[u8]) {
        match writer.write_all(row) {
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {}
            other => other.unwrap(),
        }
    }

    /// While the run waits on a pipe: its card is published and valid,
    /// names this process and the stream by absolute path, and heartbeats
    /// show it alive with no total. SIGINT then ends it with `canceled`
    /// and removes the card.
    #[test]
    fn a_waiting_run_publishes_a_valid_card_and_heartbeats() {
        let case = Case::new("pipe", "");
        let (child, mut writer) = run_on_pipe(&case, &[]);
        writer.write_all(b"[\"INV-1\",\"3\"]\n").unwrap();
        until(|| case.card("case-1").exists());
        let card: Value = serde_json::from_slice(&fs::read(case.card("case-1")).unwrap()).unwrap();
        assert!(schema("process-card.schema.json").is_valid(&card), "{card}");
        assert_eq!(card["pid"], child.child().id());
        assert_eq!(card["run_id"], "case-1");
        assert_eq!(
            card["capabilities"],
            serde_json::json!(["contract: process-run/v0"])
        );
        assert!(card.get("control").is_none());
        let stream = Path::new(card["telemetry"]["path"].as_str().unwrap());
        assert!(stream.is_absolute());
        assert_eq!(
            fs::canonicalize(stream).unwrap(),
            fs::canonicalize(case.stream("case-1")).unwrap()
        );
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(case.card("case-1"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        until(|| {
            fs::read_to_string(case.stream("case-1"))
                .is_ok_and(|text| text.contains("\"heartbeat\""))
        });
        send(child.child(), "INT");
        feed(&mut writer, b"[\"INV-2\",\"4\"]\n");
        drop(writer);
        let output = child.finish();
        unpipe(&case);
        assert_eq!(output.status.signal(), Some(2));
        let events = events(&case.stream("case-1"), "case-1");
        let progress: Vec<&Value> = events.iter().filter(|e| e["event"] == "progress").collect();
        assert!(progress.iter().all(|e| e["data"].get("total").is_none()));
        let end = last(&events);
        assert_eq!(end["event"], "canceled");
        assert_eq!(end["severity"], "WARN");
        assert_eq!(
            end["data"],
            serde_json::json!({"status": "cancelled", "reason": "SIGINT"})
        );
        assert!(!case.card("case-1").exists());
    }

    /// A stopped run whose result name is taken publishes no result, so its
    /// stream ends `failed` with the result-write rule, not `canceled`.
    #[test]
    fn a_stopped_run_without_a_result_ends_failed() {
        let case = Case::new("taken", "");
        let (child, mut writer) = run_on_pipe(&case, &[]);
        writer.write_all(b"[\"INV-1\",\"3\"]\n").unwrap();
        fs::write(case.path("result.json"), b"theirs").unwrap();
        send(child.child(), "TERM");
        feed(&mut writer, b"[\"INV-2\",\"4\"]\n");
        drop(writer);
        let output = child.finish();
        unpipe(&case);
        assert_eq!(output.status.code(), Some(54));
        let events = events(&case.stream("case-1"), "case-1");
        assert_eq!(
            last(&events)["data"],
            serde_json::json!({"status": "failed", "reason": "io-failure/result-write-failed"})
        );
        assert!(!case.card("case-1").exists());
    }

    /// A kill leaves the card behind, stale, and a stream with no terminal
    /// event, which the contract treats as normal.
    #[test]
    fn a_killed_run_leaves_a_stale_card_and_no_terminal_event() {
        let case = Case::new("killed", "");
        let (child, mut writer) = run_on_pipe(&case, &[]);
        writer.write_all(b"[\"INV-1\",\"3\"]\n").unwrap();
        // A published card always points at a stream that begins with
        // `started`.
        until(|| case.card("case-1").exists());
        assert!(fs::read_to_string(case.stream("case-1"))
            .unwrap()
            .contains("\"started\""));
        drop(child);
        drop(writer);
        unpipe(&case);
        assert!(case.card("case-1").exists());
        let events = events(&case.stream("case-1"), "case-1");
        assert!(!["completed", "canceled", "failed"]
            .contains(&last(&events)["event"].as_str().unwrap()));
    }

    /// SIGTERM ends a waiting run with `canceled` and reason `SIGTERM`.
    #[test]
    fn sigterm_is_reported_as_the_reason() {
        let case = Case::new("term", "");
        let (child, mut writer) = run_on_pipe(&case, &[]);
        writer.write_all(b"[\"INV-1\",\"3\"]\n").unwrap();
        until(|| case.card("case-1").exists());
        send(child.child(), "TERM");
        feed(&mut writer, b"[\"INV-2\",\"4\"]\n");
        drop(writer);
        let output = child.finish();
        unpipe(&case);
        assert_eq!(output.status.signal(), Some(15));
        let events = events(&case.stream("case-1"), "case-1");
        assert_eq!(
            last(&events)["data"],
            serde_json::json!({"status": "cancelled", "reason": "SIGTERM"})
        );
    }
}
