//! #162: with `RUSTDL_HARD_GLOBAL_DEADLINE=1`, `classify` stops waiting on a
//! parse that outlasts the global budget (`RUSTDL_PARSE_DEADLINE`, default ON).

#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output};

/// The 3,000-rung EL ladder of `json_output.rs`: every `A_i ⊑ C_i` is
/// entailed, and parsing it takes far longer than 1 ms.
fn ladder(tag: &str) -> PathBuf {
    let mut src = String::from("Prefix(:=<http://ex#>)\nOntology(\n");
    for i in 0..3000 {
        writeln!(src, "SubClassOf(:A{i} ObjectSomeValuesFrom(:r :B{i}))").unwrap();
        writeln!(src, "SubClassOf(ObjectSomeValuesFrom(:r :B{i}) :C{i})").unwrap();
    }
    src.push(')');
    let path =
        std::env::temp_dir().join(format!("rustdl-parse-cut-{}-{tag}.ofn", std::process::id()));
    std::fs::write(&path, src).unwrap();
    path
}

fn classify(path: &PathBuf, budget_ms: &str, json: bool, env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    cmd.arg("classify");
    if json {
        cmd.arg("--json");
    }
    cmd.args(["--global-timeout-ms", budget_ms]).arg(path);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn json_of(out: &Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn derives_a0_c0(v: &serde_json::Value) -> bool {
    v["direct_subsumptions"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!(["http://ex#A0", "http://ex#C0"]))
}

const HARD: (&str, &str) = ("RUSTDL_HARD_GLOBAL_DEADLINE", "1");

/// A 1 ms budget ends during parsing: the answer is empty and every cut flag
/// is set.
#[test]
fn a_parse_past_the_deadline_is_abandoned() {
    let path = ladder("abandon");
    let out = classify(&path, "1", true, &[HARD]);
    std::fs::remove_file(&path).ok();
    let v = json_of(&out);
    assert_eq!(v["parse_timed_out"], true);
    assert_eq!(
        v["conversion_timed_out"], true,
        "a parse cut is a conversion cut"
    );
    assert_eq!(v["prep_timed_out"], true);
    assert_eq!(v["incomplete"], true);
    assert_eq!(v["consistency_undetermined"], true);
    assert_eq!(v["completeness_guaranteed"], false);
    assert_eq!(v["direct_subsumptions"], serde_json::json!([]));
    assert_eq!(v["unsatisfiable"], serde_json::json!([]));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("while parsing"), "{err}");
}

/// Without `--json` the abandoned run prints no hierarchy, only the warning.
#[test]
fn an_abandoned_parse_prints_nothing_but_the_warning() {
    let path = ladder("text");
    let out = classify(&path, "1", false, &[HARD]);
    std::fs::remove_file(&path).ok();
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("while parsing"));
}

/// `RUSTDL_PARSE_DEADLINE=0` waits for the parse; the cut then lands in
/// conversion, as before #162's parse half.
#[test]
fn the_flag_off_waits_for_the_parse() {
    let path = ladder("off");
    let out = classify(&path, "1", true, &[HARD, ("RUSTDL_PARSE_DEADLINE", "0")]);
    std::fs::remove_file(&path).ok();
    let v = json_of(&out);
    assert_eq!(v["parse_timed_out"], false);
    assert_eq!(v["conversion_timed_out"], true);
}

/// Without hard mode the parse is never abandoned and the answer is complete.
#[test]
fn the_default_mode_never_abandons_a_parse() {
    let path = ladder("soft");
    let out = classify(&path, "1", true, &[]);
    std::fs::remove_file(&path).ok();
    let v = json_of(&out);
    assert_eq!(v["parse_timed_out"], false);
    assert_eq!(v["conversion_timed_out"], false);
    assert!(derives_a0_c0(&v));
}

/// A budget the parse fits in: the worker thread classifies exactly as the
/// main thread would, and nothing is flagged.
#[test]
fn a_parse_inside_the_budget_classifies_normally() {
    let path = ladder("fits");
    let hard = json_of(&classify(&path, "600000", true, &[HARD]));
    let soft = json_of(&classify(&path, "600000", true, &[]));
    std::fs::remove_file(&path).ok();
    assert_eq!(hard["parse_timed_out"], false);
    assert_eq!(hard["incomplete"], false);
    assert!(derives_a0_c0(&hard));
    assert_eq!(
        hard, soft,
        "the worker thread changes nothing but where it runs"
    );
}

/// A parse ERROR under hard mode is still reported as an error, not as a
/// timeout.
#[test]
fn a_parse_error_is_still_an_error() {
    let path = std::env::temp_dir().join(format!("rustdl-parse-bad-{}.ofn", std::process::id()));
    std::fs::write(&path, "Prefix(:=<http://ex#>)\nOntology(\nSubClassOf(:A\n").unwrap();
    let out = classify(&path, "600000", true, &[HARD]);
    std::fs::remove_file(&path).ok();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("parsing"), "{err}");
    assert!(!err.contains("while parsing"), "{err}");
}
