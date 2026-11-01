//! #403: `ClassAssertion` classes reach the tableau in NNF.
//!
//! A raw `¬∃r.B` label (a user `ObjectComplementOf`, or the lowered
//! `NegativeDataPropertyAssertion` `¬∃dp.DKey(v)`) was an opaque literal: no
//! ∀-rule fired and no clash was found, so the main tableau answered
//! "consistent" for inconsistent `ABox`es and missed instance entailments. Debug
//! builds panicked on `add_label`'s NNF `debug_assert!` instead.
//!
//! The expected answers are `HermiT`'s. `RUSTDL_WEDGE_CONSISTENCY=0` and
//! `RUSTDL_ABOX_CHECK=0` make `consistent` reach the main tableau (by default
//! the consistency wedge, which NNFs its own copy, answers first). Each case runs the binary in its own
//! process.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

// r(a,b), B(b), ¬∃r.B(a): inconsistent.
const OBJ: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/obj>
  Declaration(Class(:B)) Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  ObjectPropertyAssertion(:r :a :b)
  ClassAssertion(:B :b)
  ClassAssertion(ObjectComplementOf(ObjectSomeValuesFrom(:r :B)) :a))
";

// r(a,b), ¬∃r.B(a), Q ≡ ¬B: consistent, and Q(b) is entailed.
const OBJ2: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/obj2>
  Declaration(Class(:B)) Declaration(Class(:Q)) Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  ObjectPropertyAssertion(:r :a :b)
  ClassAssertion(ObjectComplementOf(ObjectSomeValuesFrom(:r :B)) :a)
  EquivalentClasses(:Q ObjectComplementOf(:B)))
";

// The issue's repro: sub(a,0) ⟹ sup(a,0), against ¬sup(a,0). Inconsistent.
const NEG_DATA: &str = r#"Prefix(:=<http://example.org/gd#>) Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://example.org/gd>
  Declaration(DataProperty(:sub)) Declaration(DataProperty(:sup))
  Declaration(NamedIndividual(:a))
  SubClassOf(DataHasValue(:sub "0"^^xsd:integer) DataHasValue(:sup "0"^^xsd:integer))
  DataPropertyAssertion(:sub :a "0"^^xsd:integer)
  NegativeDataPropertyAssertion(:sup :a "0"^^xsd:integer))
"#;

// ¬¬A(a), B(a), A ⊓ B ⊑ ⊥: inconsistent only once ¬¬A is read as A.
const DOUBLE_NEG: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/dneg>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:a))
  ClassAssertion(ObjectComplementOf(ObjectComplementOf(:A)) :a)
  ClassAssertion(:B :a)
  DisjointClasses(:A :B))
";

/// Writes a fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(src: &str, tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("rustdl-abox-nnf-{}-{tag}.ofn", std::process::id()));
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

/// Runs the binary with no inherited `RUSTDL_*` settings (so "default" means
/// the shipped defaults) plus `env`; returns stdout, failing the test on a
/// non-zero exit.
fn run(env: &[(&str, &str)], args: &[&str]) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd.args(args).envs(env.iter().copied()).output().unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "{args:?}: {stderr}");
    String::from_utf8(out.stdout).unwrap()
}

// The wedge and the told-`ABox` pre-check (`¬¬A` is atomic `A` once NNF'd,
// so its told-disjointness rule would answer first) both stand aside.
const WEDGE_OFF: &[(&str, &str)] = &[
    ("RUSTDL_WEDGE_CONSISTENCY", "0"),
    ("RUSTDL_ABOX_CHECK", "0"),
];

fn consistent_on_main_tableau(src: &str, tag: &str) -> String {
    let f = Fixture::new(src, tag);
    run(WEDGE_OFF, &["consistent", f.path()]).trim().to_owned()
}

#[test]
fn a_negated_existential_assertion_clashes_on_the_main_tableau() {
    assert_eq!(consistent_on_main_tableau(OBJ, "obj"), "inconsistent");
}

#[test]
fn a_negative_data_property_assertion_clashes_on_the_main_tableau() {
    assert_eq!(
        consistent_on_main_tableau(NEG_DATA, "negdata"),
        "inconsistent"
    );
}

#[test]
fn a_double_negation_assertion_clashes_on_the_main_tableau() {
    assert_eq!(
        consistent_on_main_tableau(DOUBLE_NEG, "dneg"),
        "inconsistent"
    );
}

#[test]
fn a_negated_existential_assertion_alone_stays_consistent() {
    assert_eq!(consistent_on_main_tableau(OBJ2, "obj2"), "consistent");
}

#[test]
fn a_negated_existential_assertion_propagates_to_the_successor() {
    // ¬∃r.B(a) ≡ ∀r.¬B(a), and r(a,b), so ¬B(b), i.e. Q(b).
    let f = Fixture::new(OBJ2, "instance");
    let out = run(&[], &["instance", f.path(), "http://ex/#Q", "http://ex/#b"]);
    assert_eq!(out.trim(), "yes");
}

#[test]
fn realize_reports_the_propagated_type() {
    let f = Fixture::new(OBJ2, "realize");
    let out = run(&[], &["realize", f.path()]);
    let rows: Vec<&str> = out.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(rows, ["http://ex/#b\thttp://ex/#Q"], "{out}");
}
