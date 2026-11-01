//! #402: `prove` must not read a reasoner give-up as "NOT entailed".
//!
//! A top-level give-up prints `unknown` and exits 3 (`prove --json`:
//! `entailed: false`, `status: "unknown"`, `incomplete: true`). A give-up in the
//! justification fallback keeps the (sound) justification and sets `incomplete`.
//! `prove` holds the single-query node cap (`RUSTDL_QUERY_MAX_NODES`), like
//! `subclass` / `explain`.
//!
//! Each case runs the binary in its own process, so the `OnceLock`-cached caps
//! are read fresh every time. The control arm disables the cap.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

// The #363 A2 fixture: C and D are satisfiable and unrelated, but each generates
// an `:r`-successor, so the tableau probe for C ⊑ D needs a node beyond the root
// and trips a cap of 1. `∀s.E` takes it out of EL, so the closure cannot answer.
const TOP_LEVEL: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/provegiveup>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
)
";

// C ⊑ A, C ⊑ B, A ⊓ B ⊑ ⊥: C is unsatisfiable in the EL closure, so C ⊑ D holds
// without a saturator proof of C ⊑ D, and prove falls back to a justification.
// The justification's QuickXplain probes the subsets without the disjointness;
// `C ⊑ ∃r.C` and `C ⊑ ∀r.E` send those to the tableau, which trips a cap of 1.
const FALLBACK: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/provefallback>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(Class(:E)) Declaration(ObjectProperty(:r))
  SubClassOf(:C :A)
  SubClassOf(:C :B)
  DisjointClasses(:A :B)
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:C ObjectAllValuesFrom(:r :E))
)
";

/// Writes a fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(src: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-prove-give-up-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, src).unwrap();
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| panic!("{e}: {}", self.stdout))
    }
}

/// Runs the binary with exactly the given cap variables set (the others removed).
fn run(env: &[(&str, &str)], args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .env_remove("RUSTDL_MAX_NODES")
        .env_remove("RUSTDL_QUERY_MAX_NODES")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

const C: &str = "http://ex/#C";
const D: &str = "http://ex/#D";
const CAP1: &[(&str, &str)] = &[("RUSTDL_MAX_NODES", "1")];
const UNCAPPED: &[(&str, &str)] = &[("RUSTDL_MAX_NODES", "0")];

#[test]
fn prove_prints_unknown_when_the_check_gives_up() {
    let f = Fixture::new(TOP_LEVEL, "top-text");
    let capped = run(CAP1, &["prove", f.path(), C, D]);
    assert!(capped.stdout.starts_with("unknown"), "{}", capped.stdout);
    assert!(!capped.stdout.contains("NOT entailed"), "{}", capped.stdout);
    assert_eq!(capped.code, 3, "stderr: {}", capped.stderr);
    assert!(
        capped.stderr.contains("not a non-entailment"),
        "{}",
        capped.stderr
    );
    let control = run(UNCAPPED, &["prove", f.path(), C, D]);
    assert!(
        control.stdout.starts_with("NOT entailed"),
        "{}",
        control.stdout
    );
    assert_eq!(control.code, 0);
}

#[test]
fn prove_json_reports_unknown_when_the_check_gives_up() {
    let f = Fixture::new(TOP_LEVEL, "top-json");
    let args = ["prove", f.path(), "--json", C, D];
    let capped = run(CAP1, &args);
    let j = capped.json();
    // `entailed` stays a bool in its sound direction; `status` and `incomplete`
    // say it is a give-up (the `consistent --json` shape).
    assert_eq!(j["entailed"], false, "{j}");
    assert_eq!(j["status"], "unknown", "{j}");
    assert_eq!(j["incomplete"], true, "{j}");
    assert_eq!(capped.code, 3);
    let control = run(UNCAPPED, &args);
    let j = control.json();
    assert_eq!(j["entailed"], false, "{j}");
    assert_eq!(j["status"], "not-entailed", "{j}");
    assert_eq!(j["incomplete"], false, "{j}");
    assert_eq!(control.code, 0);
}

#[test]
fn prove_flags_a_give_up_in_the_justification_fallback() {
    let f = Fixture::new(FALLBACK, "fallback");
    let args = ["prove", f.path(), "--json", C, D];
    let capped = run(CAP1, &args);
    let j = capped.json();
    // The entailment was witnessed; the fallback's justification still entails.
    assert_eq!(j["entailed"], true, "{j}");
    assert_eq!(j["status"], "entailed", "{j}");
    assert_eq!(j["has_proof"], false, "{j}");
    assert!(j["justification_fallback"].is_string(), "{j}");
    assert_eq!(j["incomplete"], true, "{j}");
    assert_eq!(capped.code, 0);
    assert!(
        capped.stderr.contains("justification fallback"),
        "{}",
        capped.stderr
    );
    let control = run(UNCAPPED, &args);
    let j = control.json();
    assert_eq!(j["entailed"], true, "{j}");
    assert_eq!(j["incomplete"], false, "{j}");
    assert!(control.stderr.is_empty(), "{}", control.stderr);
}

#[test]
fn prove_runs_under_the_single_query_cap() {
    // `RUSTDL_QUERY_MAX_NODES` (with `RUSTDL_MAX_NODES` unset) reaches only the
    // commands that hold `single_query_node_cap()`: prove gives up under it,
    // while justify, which keeps the classify-wide cap, still decides.
    let f = Fixture::new(TOP_LEVEL, "scope");
    let query_cap = [("RUSTDL_QUERY_MAX_NODES", "1")];
    let prove = run(&query_cap, &["prove", f.path(), C, D]);
    assert!(prove.stdout.starts_with("unknown"), "{}", prove.stdout);
    assert_eq!(prove.code, 3);
    let justify = run(&query_cap, &["justify", f.path(), "subclass", C, D]);
    assert_eq!(
        justify.stdout.trim(),
        "not entailed (no justification)",
        "stderr: {}",
        justify.stderr
    );
    assert_eq!(justify.code, 0);
}
