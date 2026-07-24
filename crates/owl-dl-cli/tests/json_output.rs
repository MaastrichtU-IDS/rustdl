//! End-to-end test for the `--json` output mode (schema v1).

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn rustdl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustdl"))
}

fn tiny_consistent() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/consistent_tiny.ofn"
    )
}

#[test]
fn classify_json_parses_and_reports_consistent() {
    let out = rustdl()
        .args(["classify", "--json", tiny_consistent()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["consistent"], true);
    assert!(v["direct_subsumptions"].is_array());
}
