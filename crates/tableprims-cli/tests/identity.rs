// SPDX-License-Identifier: MIT OR Apache-2.0

//! Checks that the compiled identity agrees with `.fulmen/app.yaml`.

use std::path::Path;
use std::process::Command;

/// Returns the value of a `key: value` line nested under `app:`.
fn app_field(document: &str, key: &str) -> String {
    let prefix = format!("  {key}: ");
    let mut in_app = false;
    for line in document.lines() {
        if !line.starts_with(' ') && !line.starts_with('#') {
            in_app = line.trim_end() == "app:";
            continue;
        }
        if in_app {
            if let Some(value) = line.strip_prefix(&prefix) {
                return value.trim().to_owned();
            }
        }
    }
    panic!("app.{key} missing from .fulmen/app.yaml");
}

#[test]
fn compiled_identity_matches_app_yaml() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.fulmen/app.yaml");
    let document = std::fs::read_to_string(&path).expect("read .fulmen/app.yaml");

    let output = Command::new(env!("CARGO_BIN_EXE_tablebolt"))
        .args(["version", "--json"])
        .output()
        .expect("tablebolt binary runs");
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is one JSON document");

    assert_eq!(value["name"], app_field(&document, "binary_name"));
    assert_eq!(value["vendor"], app_field(&document, "vendor"));
    assert_eq!(value["env_prefix"], app_field(&document, "env_prefix"));

    let help = Command::new(env!("CARGO_BIN_EXE_tablebolt"))
        .arg("--help")
        .output()
        .expect("tablebolt binary runs");
    let help = String::from_utf8(help.stdout).expect("utf-8 stdout");
    assert!(help.contains(&app_field(&document, "description")));
}
