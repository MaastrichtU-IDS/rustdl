//! #625: no surface claims a minimality it cannot guarantee.
//! - The `report` headings say "(minimal justification)" only when
//!   `minimal_guaranteed`: #584's `H2` (inconsistent, found on the saturation-only
//!   oracle) reads "minimality not guaranteed"; its control, the full oracle
//!   (`RUSTDL_JUSTIFY_SAT_ORACLE=0`), finds the 5-axiom minimal set and says minimal.
//! - `justify --all --json`'s `enumeration_complete` also needs every justification
//!   minimal: out of the EL/Horn fragment a "not entailed" may have pruned a branch.
//!   Control: the same entailment in EL.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const H2: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/o>\n\
    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
    Declaration(Class(:D)) Declaration(Class(:E))\n\
    Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:q))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
    ClassAssertion(:A :a)\n\
    SubClassOf(:A ObjectIntersectionOf(:D ObjectAllValuesFrom(:r :B)))\n\
    ObjectPropertyAssertion(:r :a :b)\n\
    ObjectPropertyRange(:r :C)\n\
    DisjointClasses(:B :C)\n\
    SubClassOf(:D ObjectHasValue(:q :b))\n\
    ObjectPropertyRange(:q :B)\n)\n";

/// `A ⊑ B ⊔ C`, `B ⊑ D`, `C ⊑ D`: `A ⊑ D` holds only through the disjunction, so the
/// ontology is out of the EL/Horn fragment.
const DISJUNCTIVE: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/d>\n\
    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))\n\
    SubClassOf(:A ObjectUnionOf(:B :C))\n\
    SubClassOf(:B :D)\n\
    SubClassOf(:C :D)\n)\n";

/// `A ⊑ B`, `B ⊑ D`: the EL control.
const EL: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/e>\n\
    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :D)\n)\n";

struct Fixture(PathBuf);

impl Fixture {
    fn new(src: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-minimality-claims-{}-{tag}.ofn",
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

fn run(args: &[&str], env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (var, _) in std::env::vars_os() {
        if var.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(var);
        }
    }
    let out = cmd.envs(env.iter().copied()).args(args).output().unwrap();
    assert!(out.status.success(), "{args:?} {env:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

const FULL_ORACLE: (&str, &str) = ("RUSTDL_JUSTIFY_SAT_ORACLE", "0");

#[test]
fn the_report_claims_a_minimal_justification_only_when_guaranteed() {
    let f = Fixture::new(H2, "report-h2");
    let html = run(&["report", f.path()], &[]);
    assert!(
        html.contains("Why it is inconsistent (a justification; minimality not guaranteed)"),
        "{html}"
    );
    assert!(!html.contains("(minimal justification)"), "{html}");
    let html = run(&["report", f.path()], &[FULL_ORACLE]);
    assert!(
        html.contains("Why it is inconsistent (minimal justification)"),
        "full oracle: {html}"
    );
}

#[test]
fn enumeration_complete_needs_every_justification_minimal() {
    let q = |f: &Fixture| -> serde_json::Value {
        let out = run(
            &[
                "justify",
                "--all",
                "--json",
                f.path(),
                "subclass",
                "http://ex.org/A",
                "http://ex.org/D",
            ],
            &[],
        );
        serde_json::from_str(&out).unwrap()
    };
    let d = Fixture::new(DISJUNCTIVE, "disj");
    let v = q(&d);
    assert_eq!(v["status"], "entailed", "{v}");
    assert_eq!(v["minimal"], false, "precondition: out of fragment: {v}");
    assert_eq!(v["incomplete"], false, "not a give-up: {v}");
    assert_eq!(v["enumeration_complete"], false, "{v}");
    let e = Fixture::new(EL, "el");
    let v = q(&e);
    assert_eq!(v["minimal"], true, "{v}");
    assert_eq!(v["enumeration_complete"], true, "EL control: {v}");
}
