//! #509: a chosen `⊔` disjunct whose inline `≤1` merge is refused (the two
//! successors are `≠`) must refute its branch. Before the fix the wedge dropped
//! that clash: the search went on with the `≤1` violated, a debug build tripped
//! the `find_open_at_most` assertion (`≤1 violation reached find_open_at_most
//! under inverse_func_merge`), and a release build skipped it until the #137 leaf
//! net re-found the clash.
//!
//! Fixtures are the two R7 generated cases (generator `bd63a61a6921`, seed
//! 20261103) minimised by axiom deletion. Neither states a `≤1`: it comes from
//! the NNF of `¬(≥2 …)` on the `≥2 … → C` side of an equivalence.
//! - `le1_disjunct_00125_509.ofn`: a data `≤1 d0`;
//! - `le1_disjunct_00801_509.ofn`: a qualified `≤1 r0.C0`.
//!
//! The expected closures are `HermiT`'s (robot `reason`, 2026-11-11).
//!
//! **This test discriminates only in a debug build.** With the fix reverted the
//! release answers are the same (the net recovers them), so a release run passes
//! either way; under `cargo test` the assertion is live and a revert panics. The
//! white-box `le1_*` tests in `owl-dl-tableau/src/hyper.rs` pin the verdict in
//! both builds.
#![allow(clippy::unwrap_used)]

use std::process::Command;

const NS: &str = "http://gen.rustdl/o#";

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn iri(local: &str) -> String {
    format!("{NS}{local}")
}

/// `rustdl <args…>` with no inherited `RUSTDL_*` setting, so the defaults run.
fn run(args: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd.args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

/// `classify --json`: exit 0 (no panic), consistent, no deadline, and exactly
/// `expected` as the direct subsumptions with nothing unsatisfiable.
fn assert_classify(file: &str, expected: &[(&str, &str)]) {
    let f = fixture(file);
    let (code, stdout, stderr) = run(&["classify", "--json", &f]);
    assert_eq!(code, 0, "[{file}] stderr {stderr}");
    let j: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(j["consistent"], true, "[{file}]");
    assert_eq!(j["incomplete"], false, "[{file}]");
    assert_eq!(j["unsatisfiable"], serde_json::json!([]), "[{file}]");
    let mut got: Vec<(String, String)> = j["direct_subsumptions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p[0].as_str().unwrap().to_owned(),
                p[1].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    got.sort();
    let mut want: Vec<(String, String)> = expected.iter().map(|(a, b)| (iri(a), iri(b))).collect();
    want.sort();
    assert_eq!(got, want, "[{file}]");
}

fn assert_subclass(file: &str, sub: &str, sup: &str, expected: &str) {
    let f = fixture(file);
    let (code, stdout, stderr) = run(&["subclass", &f, &iri(sub), &iri(sup)]);
    assert_eq!(code, 0, "[{file} {sub} {sup}] stderr {stderr}");
    assert_eq!(stdout.trim(), expected, "[{file} {sub} {sup}] {stderr}");
}

#[test]
fn data_le1_disjunct_00125_classifies_without_a_violation() {
    assert_classify("le1_disjunct_00125_509.ofn", &[("C2", "C4"), ("C3", "C2")]);
    // The R7 call that tripped the assertion on the full case.
    assert_subclass("le1_disjunct_00125_509.ofn", "C3", "C4", "yes");
}

#[test]
fn qualified_le1_disjunct_00801_classifies_without_a_violation() {
    assert_classify("le1_disjunct_00801_509.ofn", &[]);
    // The R7 calls that tripped the assertion on the full case.
    for sub in ["C0", "C1", "C2", "C3", "C6"] {
        assert_subclass("le1_disjunct_00801_509.ofn", sub, "C7", "no");
    }
}
