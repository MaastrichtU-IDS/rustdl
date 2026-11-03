//! #428: `sat`, `subclass` and `explain` run the inconsistency guard. On an
//! inconsistent ontology they exit 1 with "ontology is inconsistent" (on main
//! they answered the vacuous unsat / yes, or, on `family.ofn`, did not finish).
//! `prove` reports the entailment with `kb_inconsistent`.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const X: &str = "http://ex/#X";
const Z: &str = "http://ex/#Z";

const INCONSISTENT: &str = "Prefix(:=<http://ex/#>)
Ontology(<http://ex/o>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) Declaration(Class(:Z))
  Declaration(NamedIndividual(:a))
  DisjointClasses(:A :B) ClassAssertion(:A :a) ClassAssertion(:B :a)
)
";

const CONSISTENT: &str = "Prefix(:=<http://ex/#>)
Ontology(<http://ex/c>
  Declaration(Class(:X)) Declaration(Class(:Z)) Declaration(NamedIndividual(:a))
  SubClassOf(:X :Z) ClassAssertion(:X :a)
)
";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, src: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-class-query-guard-{}-{tag}.ofn",
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

/// Runs the binary with no inherited `RUSTDL_*` settings: (exit code, stdout,
/// stderr).
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

#[test]
fn sat_subclass_and_explain_error_on_an_inconsistent_ontology() {
    let f = Fixture::new("incons", INCONSISTENT);
    for args in [
        vec!["sat", f.path(), X],
        vec!["subclass", f.path(), X, Z],
        vec!["explain", f.path(), X, Z],
    ] {
        let (code, stdout, stderr) = run(&args);
        assert_eq!(code, 1, "{args:?}\nstdout: {stdout}\nstderr: {stderr}");
        assert!(stderr.contains("ontology is inconsistent"), "{args:?}: {stderr}");
    }
}

#[test]
fn prove_json_reports_the_inconsistent_kb_and_its_core() {
    let f = Fixture::new("prove-incons", INCONSISTENT);
    let (code, stdout, stderr) = run(&["prove", "--json", f.path(), X, Z]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let j: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(j["entailed"], true);
    assert_eq!(j["kb_inconsistent"], true);
    let fallback = j["justification_fallback"].as_str().unwrap();
    for axiom in ["DisjointClasses", "ClassAssertion(:A :a)", "ClassAssertion(:B :a)"] {
        assert!(fallback.contains(axiom), "{axiom} missing from {fallback}");
    }
    // Text mode points at `justify … inconsistent`.
    let (code, _, stderr) = run(&["prove", f.path(), X, Z]);
    assert_eq!(code, 0);
    assert!(stderr.contains("justify <file> inconsistent"), "{stderr}");
    // A consistent ontology: a step proof, no flag.
    let c = Fixture::new("prove-cons", CONSISTENT);
    let (code, stdout, _) = run(&["prove", "--json", c.path(), X, Z]);
    assert_eq!(code, 0);
    let j: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(j["kb_inconsistent"], false);
    assert_eq!(j["has_proof"], true);
}
