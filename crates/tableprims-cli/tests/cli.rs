// SPDX-License-Identifier: MIT OR Apache-2.0

//! Runs the built `tablebolt` binary and checks its resolved behaviour.

#[cfg(unix)]
use std::process::Stdio;
use std::process::{Command, Output};

use serde_json::Value;

fn tablebolt(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tablebolt"))
        .args(args)
        .output()
        .expect("tablebolt binary runs")
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout is one JSON document")
}

#[test]
fn version_json_reports_versions_and_identity() {
    let output = tablebolt(&["version", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());

    let value = stdout_json(&output);
    assert_eq!(value["name"], "tablebolt");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["tableprims_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["vendor"], "fulmenhq");
    assert_eq!(value["env_prefix"], "TABLEBOLT_");
}

#[test]
fn version_text_names_binary_and_library() {
    let output = tablebolt(&["version"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        text,
        format!("tablebolt {version} (tableprims {version})\n")
    );
}

#[test]
fn capabilities_json_lists_the_conversions() {
    let output = tablebolt(&["capabilities", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let value = stdout_json(&output);
    assert_eq!(value["conversions"].as_array().map(Vec::len), Some(1));
}

#[test]
fn capabilities_text_lists_the_conversions() {
    let output = tablebolt(&["capabilities"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"ndjson -> xlsx\n");
}

#[test]
fn help_succeeds_on_stdout() {
    let output = tablebolt(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).expect("utf-8 stdout");
    assert!(text.contains("capabilities"));
    assert!(text.contains("version"));
}

#[test]
fn usage_errors_exit_64_with_empty_stdout() {
    // An unimplemented conversion command is a usage error, never a success.
    let cases: [&[&str]; 4] = [
        &[],
        &["xlsx", "write"],
        &["version", "--unknown-flag"],
        &["--version"],
    ];
    for args in cases {
        let output = tablebolt(args);
        assert_eq!(output.status.code(), Some(64), "args: {args:?}");
        assert!(output.stdout.is_empty(), "args: {args:?}");
        assert!(!output.stderr.is_empty(), "args: {args:?}");
    }
}

/// `/dev/full` accepts the open and fails every write with ENOSPC.
#[cfg(target_os = "linux")]
#[test]
fn unwritable_stdout_exits_54() {
    let cases: [&[&str]; 3] = [&["version"], &["capabilities", "--json"], &["--help"]];
    for args in cases {
        let full = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .expect("open /dev/full");
        let output = Command::new(env!("CARGO_BIN_EXE_tablebolt"))
            .args(args)
            .stdout(Stdio::from(full))
            .output()
            .expect("tablebolt binary runs");
        assert_eq!(output.status.code(), Some(54), "args: {args:?}");
        assert!(!output.stderr.is_empty(), "args: {args:?}");
    }
}

#[cfg(unix)]
#[test]
fn closed_reader_is_not_an_error() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tablebolt"))
        .arg("--help")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("tablebolt binary runs");
    drop(child.stdout.take());
    let status = child.wait().expect("tablebolt exits");
    // Either the write completed before the reader closed or it hit a broken
    // pipe; both are success.
    assert_eq!(status.code(), Some(0));
}

/// With stderr unwritable as well, the diagnostic is lost but the exit code
/// still reports the outcome.
#[cfg(target_os = "linux")]
#[test]
fn unwritable_stdout_and_stderr_still_exit_by_outcome() {
    let full = || {
        std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .expect("open /dev/full")
    };
    let cases: [(&[&str], i32); 5] = [
        (&["version"], 54),
        (&["capabilities", "--json"], 54),
        (&["--help"], 54),
        (&[], 64),
        (&["xlsx", "write"], 64),
    ];
    for (args, expected) in cases {
        let status = Command::new(env!("CARGO_BIN_EXE_tablebolt"))
            .args(args)
            .stdout(Stdio::from(full()))
            .stderr(Stdio::from(full()))
            .status()
            .expect("tablebolt binary runs");
        assert_eq!(status.code(), Some(expected), "args: {args:?}");
    }
}
