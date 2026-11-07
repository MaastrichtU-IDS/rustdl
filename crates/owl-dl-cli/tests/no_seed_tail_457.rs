//! #457 residual 2 (option A″, `RUSTDL_QUERY_NO_SEED_TAIL`, default on): on a KB
//! with no individual, the consistency wedge has no seed, so a positive answer
//! used to get no consistency signal at all. Now a query the main tableau answered
//! (the prepared route) runs the bounded main-tableau consistency check: a proof
//! of inconsistency errors `Inconsistent`, and a cut check flags
//! `consistency_undetermined`.
//!
//! A told, saturation or query-wedge answer (the light route) still runs no check,
//! by decision: `docs/known-limitations/tbox-only-inconsistency-light-route.md`.
//! The `…_light_route_gap…` tests pin that gap; a fix flips them.
//!
//! Fixtures: `query_wedge_hang_457.ofn` (shrunk from generated case 438;
//! inconsistent per Konclude and `HermiT`), `query_wedge_proves_inconsistent_457.ofn`
//! (inconsistent by construction: `⊤ ⊑ P ⊔ Q` with both empty) and its consistent
//! original `query_wedge_proves_457.ofn`.
#![allow(clippy::unwrap_used)]

use std::process::Command;

const C0: &str = "http://gen.rustdl/o#C0";
const C1: &str = "http://gen.rustdl/o#C1";
const C2: &str = "http://gen.rustdl/o#C2";
const C6: &str = "http://gen.rustdl/o#C6";
const A: &str = "http://example.org/w457#A";
const D: &str = "http://example.org/w457#D";
/// Unsatisfiable on both `w457` fixtures, and only the tableau refutes it (the
/// prepared route): `A` has two `r`-successors in `B`.
const TABLEAU_UNSAT: &str = ":A and (:r only (not :B))";
const UNDETERMINED: &str = "consistency could not be determined";
const INCONSISTENT: &str = "ontology is inconsistent";

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// `rustdl <args…>` with no inherited `RUSTDL_*` setting, plus `env`.
fn run(env: &[(&str, &str)], args: &[&str]) -> Out {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd.envs(env.iter().copied()).args(args).output().unwrap();
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

fn assert_inconsistent(o: &Out, what: &str) {
    assert_eq!(
        o.code, 1,
        "[{what}] stdout {:?}, stderr {}",
        o.stdout, o.stderr
    );
    assert!(o.stderr.contains(INCONSISTENT), "[{what}] {}", o.stderr);
}

/// Exit 0 with `expected` on stdout and no consistency warning.
fn assert_silent(o: &Out, expected: &str, what: &str) {
    assert_eq!(o.code, 0, "[{what}] stderr {}", o.stderr);
    assert_eq!(o.stdout.trim(), expected, "[{what}] {}", o.stderr);
    assert!(!o.stderr.contains(UNDETERMINED), "[{what}] {}", o.stderr);
    assert!(!o.stderr.contains(INCONSISTENT), "[{what}] {}", o.stderr);
}

#[test]
fn a_tableau_answered_query_finds_the_tbox_inconsistency() {
    let f = fixture("query_wedge_hang_457.ofn");
    for (what, args) in [
        ("sat C0", vec!["sat", &f, C0]),
        ("sat C2", vec!["sat", &f, C2]),
        ("subclass C0 C1", vec!["subclass", &f, C0, C1]),
        ("explain C0 C1", vec!["explain", &f, C0, C1]),
    ] {
        assert_inconsistent(&run(&[], &args), what);
    }
    let g = fixture("query_wedge_proves_inconsistent_457.ofn");
    assert_inconsistent(&run(&[], &["sat-expr", &g, TABLEAU_UNSAT]), "sat-expr");
}

/// The off arm pins the behaviour before #457: the same queries answer with no
/// signal.
#[test]
fn flag_off_restores_the_silent_answer() {
    let f = fixture("query_wedge_hang_457.ofn");
    let off = [("RUSTDL_QUERY_NO_SEED_TAIL", "0")];
    assert_silent(&run(&off, &["sat", &f, C0]), "unsat", "sat C0");
    assert_silent(
        &run(&off, &["subclass", &f, C0, C1]),
        "yes",
        "subclass C0 C1",
    );
}

/// `prove` maps the inconsistency to the vacuous entailment (#456).
#[test]
fn prove_reports_the_kb_inconsistent() {
    let f = fixture("query_wedge_hang_457.ofn");
    let o = run(&[], &["prove", "--json", &f, C0, C1]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let j: serde_json::Value = serde_json::from_str(&o.stdout).unwrap();
    assert_eq!(j["entailed"], true, "{}", o.stdout);
    assert_eq!(j["kb_inconsistent"], true, "{}", o.stdout);
}

/// The documented gap: `C0 ⊑ C6` is told, so `subclass` answers on the light
/// route, where no consistency check runs.
#[test]
fn a_told_answer_is_silent_light_route_gap() {
    let f = fixture("query_wedge_hang_457.ofn");
    assert_silent(
        &run(&[], &["subclass", &f, C0, C6]),
        "yes",
        "subclass C0 C6",
    );
}

/// The documented gap, for a query-wedge proof: `A ⊑ D` is outside the EL
/// closure and the H4 query wedge proves it (the light route).
#[test]
fn a_query_wedge_answer_is_silent_light_route_gap() {
    let g = fixture("query_wedge_proves_inconsistent_457.ofn");
    let o = run(&[("RUSTDL_TRACE", "1")], &["subclass", &g, A, D]);
    assert_silent(&o, "yes", "subclass A D");
    assert!(
        o.stderr.contains("query wedge (#457): Subsumed after"),
        "precondition: the H4 query wedge must answer this pair. stderr: {}",
        o.stderr
    );
}

/// A check that is cut keeps the answer and flags it.
/// `RUSTDL_CONSISTENCY_FALLBACK_MS=0` is a deadline already passed. The KB is
/// consistent, so with the default budget the check decides and nothing is
/// flagged.
#[test]
fn a_cut_check_flags_consistency_undetermined() {
    let f = fixture("query_wedge_proves_457.ofn");
    let undetermined = |env: &[(&str, &str)]| {
        let o = run(env, &["sat-expr", "--json", &f, TABLEAU_UNSAT]);
        assert_eq!(o.code, 0, "{}", o.stderr);
        let j: serde_json::Value = serde_json::from_str(&o.stdout).unwrap();
        assert_eq!(j["satisfiable"], false, "{}", o.stdout);
        j["consistency_undetermined"].as_bool().unwrap()
    };
    assert!(undetermined(&[("RUSTDL_CONSISTENCY_FALLBACK_MS", "0")]));
    assert!(
        !undetermined(&[]),
        "the default budget decides this consistent KB"
    );
    assert!(
        !undetermined(&[
            ("RUSTDL_CONSISTENCY_FALLBACK_MS", "0"),
            ("RUSTDL_QUERY_NO_SEED_TAIL", "0"),
        ]),
        "with the check off, a KB with no individual is not flagged (as before #457)"
    );
}
