//! #552: `instances-expr`'s completeness guarantee needs the KB's consistency to
//! have been checked. The realize fast path decides it eagerly
//! (`instance_query_inconsistent_fast`) unless `RUSTDL_QUERY_INCONSISTENCY_GUARD=0`,
//! and with the guard off it used to answer an inconsistent KB with a short list
//! and `completeness_guaranteed: true` (on an inconsistent KB every individual is
//! an instance).
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);

impl Fixture {
    fn new(body: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-expr-consistency-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(
            &path,
            format!(
                "Prefix(:=<http://e#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
                 Ontology(\nDeclaration(Class(:A)) Declaration(Class(:B))\n\
                 Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n{body}\n)"
            ),
        )
        .unwrap();
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

fn instances(file: &str, guard_off: bool) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    cmd.args(["instances-expr", "--json", file, ":B"]);
    if guard_off {
        cmd.env("RUSTDL_QUERY_INCONSISTENCY_GUARD", "0");
    }
    cmd.output().unwrap()
}

fn json(out: &std::process::Output) -> serde_json::Value {
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

/// `a : A ⊑ ⊥` makes the KB inconsistent; `b : B` is the only told member of `B`.
const INCONSISTENT: &str = "ClassAssertion(:A :a) SubClassOf(:A owl:Nothing) ClassAssertion(:B :b)";
const CONSISTENT: &str = "ClassAssertion(:A :a) ClassAssertion(:B :b)";

#[test]
fn the_guard_off_fast_path_is_not_guaranteed() {
    let bad = Fixture::new(INCONSISTENT, "bad");
    // Default: the eager check finds the clash, and the query does not answer.
    assert!(
        !instances(bad.path(), false).status.success(),
        "precondition: the KB is inconsistent"
    );
    // Guard off: no consistency check ran, so the guarantee must not hold.
    let v = json(&instances(bad.path(), true));
    assert_eq!(v["completeness_guaranteed"], false, "{v}");

    // Control: a consistent KB on the fast path keeps the guarantee with the guard
    // on; with it off, it is not claimed either (nothing checked consistency).
    let good = Fixture::new(CONSISTENT, "good");
    let v = json(&instances(good.path(), false));
    assert_eq!(v["instances"], serde_json::json!(["http://e#b"]), "{v}");
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
    let v = json(&instances(good.path(), true));
    assert_eq!(
        v["completeness_guaranteed"], false,
        "control, guard off: {v}"
    );
}

/// #596 review: `explain owl:Thing owl:Nothing` is the consistency query, whose
/// `pure_el_mode` follows the saturator's wider fast-path gate. On this input
/// (functional role: not pure EL, but saturation-complete) it printed "input is pure
/// EL; closure is complete".
#[test]
fn explain_consistency_does_not_claim_pure_el() {
    let f = Fixture::new(
        "Declaration(ObjectProperty(:r))\nFunctionalObjectProperty(:r)\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r :B))",
        "explain-top",
    );
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args([
            "explain",
            f.path(),
            "http://www.w3.org/2002/07/owl#Thing",
            "http://www.w3.org/2002/07/owl#Nothing",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(": no "),
        "precondition: consistent: {stdout}"
    );
    assert!(
        !stdout.contains("input is pure EL"),
        "the consistency query must not claim pure EL: {stdout}"
    );
}
