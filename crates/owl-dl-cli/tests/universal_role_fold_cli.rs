//! #543 part 2 (S+F): the universal-role uses that need no fresh symbol are folded at
//! conversion. Each fold adds an axiom that holds in every model under the Direct
//! Semantics (`owl:topObjectProperty` is `Δ × Δ`, `Δ ≠ ∅`) and gives the use exactly its
//! universal-role meaning. Folded uses answer right and are no longer flagged
//! (`universal_role_ordinary: false`); every other use stays flagged (part 1).
//!
//! Oracle: `HermiT` for every case it accepts (`sweeps/wB/543f/canaries/oracle.out`).
//! It rejects top in `Self`-free characteristics, cardinality and disjointness as a
//! non-simple property, and answers `Self(top)` wrongly (consistent); those cases say
//! "adjudicated by hand from the Direct Semantics". Negative controls come first.
#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const T: &str = "http://ex.org/t#";
const THING: &str = "http://www.w3.org/2002/07/owl#Thing";

/// A process-unique fixture path (tests run as threads of one process).
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, body: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rustdl-universal-fold-{}-{n}-{tag}.ofn",
            std::process::id()
        ));
        let src = format!(
            "Prefix(:=<{T}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Ontology(<http://ex.org/t>\n\
             Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))\n\
             Declaration(Class(:Woman)) Declaration(Class(:Person))\n\
             Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
             Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n{body}\n)\n"
        );
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

/// (exit code, stdout, stderr), with extra environment.
fn run_env(args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn run(args: &[&str]) -> (i32, String, String) {
    run_env(args, &[])
}

fn iri(local: &str) -> String {
    format!("{T}{local}")
}

fn classify_json(f: &Fixture, env: &[(&str, &str)]) -> serde_json::Value {
    let (code, out, err) = run_env(&["classify", "--json", f.path()], env);
    assert_eq!(code, 0, "{err}");
    serde_json::from_str(&out).unwrap()
}

fn subclass(f: &Fixture, sub: &str, sup: &str) -> String {
    run(&["subclass", f.path(), sub, sup]).1.trim().to_owned()
}

// ── Negative controls: no fold, the flag stays ──

/// `∃top.D ⊑ E` with D possibly empty: `⊤ ⊑ E` is NOT entailed (HermiT: only
/// `D ⊑ E`). No fold; flagged.
#[test]
fn an_existential_over_a_possibly_empty_class_is_not_folded() {
    let f = Fixture::new(
        "noinst",
        "SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) :E)",
    );
    assert_eq!(subclass(&f, THING, &iri("E")), "no");
    assert_eq!(classify_json(&f, &[])["universal_role_ordinary"], true);
}

/// `Functional(top)` with `a ≠ b` is inconsistent (`|Δ| = 1`; adjudicated by hand):
/// not folded (F10 was dropped), so still consistent and flagged.
#[test]
fn functional_top_is_not_folded() {
    let f = Fixture::new(
        "func",
        "FunctionalObjectProperty(owl:topObjectProperty) DifferentIndividuals(:a :b)",
    );
    let v = classify_json(&f, &[]);
    assert_eq!(v["consistent"], true);
    assert_eq!(v["universal_role_ordinary"], true);
}

/// One folded and one unfolded use: the folded entailment holds and the flag stays.
#[test]
fn a_mixed_ontology_folds_one_use_and_flags_the_other() {
    let f = Fixture::new(
        "mixed",
        "ClassAssertion(:D :a) SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) :E) \
         SubClassOf(:C ObjectAllValuesFrom(owl:topObjectProperty :D))",
    );
    assert_eq!(subclass(&f, THING, &iri("E")), "yes");
    assert_eq!(classify_json(&f, &[])["universal_role_ordinary"], true);
}

/// The rollback: with `RUSTDL_UNIVERSAL_ROLE_FOLD=0` a foldable use is read as part 1
/// read it (flagged, the entailment missed).
#[test]
fn the_rollback_flag_restores_part_1() {
    let f = Fixture::new(
        "rollback",
        "ClassAssertion(:D :a) SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) :E)",
    );
    let v = classify_json(&f, &[("RUSTDL_UNIVERSAL_ROLE_FOLD", "0")]);
    assert_eq!(v["universal_role_ordinary"], true, "{v}");
    let v = classify_json(&f, &[]);
    assert_eq!(v["universal_role_ordinary"], false, "{v}");
}

// ── Positives: folded, right, unflagged ──

/// Class-level folds, HermiT-adjudicated: `⊤ ⊑ E` for F1/F2/F3 (also through a told
/// subclass and a role above top), `C` unsat for F4/F5, `⊤ ⊑ D` for F7.
#[test]
fn class_level_folds_answer_right_without_the_flag() {
    for (tag, body, sub, sup) in [
        (
            "f1",
            "SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty owl:Thing) :E)",
            THING.to_owned(),
            iri("E"),
        ),
        (
            "f2",
            "SubClassOf(ObjectHasValue(owl:topObjectProperty :a) :E)",
            THING.to_owned(),
            iri("E"),
        ),
        (
            "f3",
            "ClassAssertion(:D :a) ClassAssertion(:C :b) \
             SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) :E)",
            THING.to_owned(),
            iri("E"),
        ),
        (
            "f3-told-sub",
            "ClassAssertion(:Woman :a) SubClassOf(:Woman :Person) \
             SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :Person) :E)",
            THING.to_owned(),
            iri("E"),
        ),
        (
            "f3-super",
            "SubObjectPropertyOf(owl:topObjectProperty :r) ClassAssertion(:D :a) \
             SubClassOf(ObjectSomeValuesFrom(:r :D) :E)",
            THING.to_owned(),
            iri("E"),
        ),
        (
            "f7-domain",
            "ObjectPropertyDomain(owl:topObjectProperty :D)",
            THING.to_owned(),
            iri("D"),
        ),
        (
            "f7-range",
            "ObjectPropertyRange(owl:topObjectProperty :D)",
            THING.to_owned(),
            iri("D"),
        ),
    ] {
        let f = Fixture::new(tag, body);
        assert_eq!(subclass(&f, &sub, &sup), "yes", "{tag}");
        let v = classify_json(&f, &[]);
        assert_eq!(v["universal_role_ordinary"], false, "{tag}: {v}");
        // The main tableau too.
        let (code, out, err) = run_env(
            &["classify", "--json", "--pair-timeout-ms", "1000", f.path()],
            &[("RUSTDL_HYPERTABLEAU", "0")],
        );
        assert_eq!(code, 0, "{tag}: {err}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["universal_role_ordinary"], false, "{tag}");
        assert_eq!(v["incomplete"], false, "{tag}: main tableau, uncut");
    }
    for (tag, body) in [
        (
            "f4",
            "SubClassOf(:C ObjectAllValuesFrom(owl:topObjectProperty owl:Nothing))",
        ),
        (
            "f5",
            "ClassAssertion(:D :a) \
             SubClassOf(:C ObjectAllValuesFrom(owl:topObjectProperty ObjectComplementOf(:D)))",
        ),
    ] {
        let f = Fixture::new(tag, body);
        assert_eq!(
            run(&["sat", f.path(), &iri("C")]).1.trim(),
            "unsat",
            "{tag}"
        );
        let v = classify_json(&f, &[]);
        assert_eq!(
            v["unsatisfiable"],
            serde_json::json!([iri("C")]),
            "{tag}: {v}"
        );
        assert_eq!(v["universal_role_ordinary"], false, "{tag}");
    }
    // `instance E b` (F3), the ABox surface.
    let f = Fixture::new(
        "f3-instance",
        "ClassAssertion(:D :a) ClassAssertion(:C :b) \
         SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) :E)",
    );
    assert_eq!(
        run(&["instance", f.path(), &iri("E"), &iri("b")]).1.trim(),
        "yes"
    );
}

/// Folds HermiT cannot adjudicate (it rejects top as non-simple, or answers `Self(top)`
/// wrongly), adjudicated by hand from the Direct Semantics: each is inconsistent.
/// `Self(top) ⊑ D` with `¬D(a)` (F6): `(a, a) ∈ top`. `Irreflexive(top)`,
/// `Asymmetric(top)` (F8): `(a, a) ∈ top`. `Disjoint(top, r)` with `r(a, b)` (F9):
/// `(a, b) ∈ top`. A control for F9 without the edge stays consistent.
#[test]
fn hand_adjudicated_folds_are_inconsistent() {
    for (tag, body, consistent) in [
        (
            "f6",
            "SubClassOf(ObjectHasSelf(owl:topObjectProperty) :D) \
             ClassAssertion(ObjectComplementOf(:D) :a)",
            false,
        ),
        (
            "f8",
            "IrreflexiveObjectProperty(owl:topObjectProperty) ClassAssertion(:C :a)",
            false,
        ),
        (
            "f8a",
            "AsymmetricObjectProperty(owl:topObjectProperty) ClassAssertion(:C :a)",
            false,
        ),
        (
            "f9",
            "DisjointObjectProperties(owl:topObjectProperty :r) ObjectPropertyAssertion(:r :a :b)",
            false,
        ),
        (
            "f9-control",
            "DisjointObjectProperties(owl:topObjectProperty :r) ClassAssertion(:C :a)",
            true,
        ),
    ] {
        let f = Fixture::new(tag, body);
        let expect = if consistent {
            "consistent"
        } else {
            "inconsistent"
        };
        assert_eq!(run(&["consistent", f.path()]).1.trim(), expect, "{tag}");
        let (_, out, _) = run_env(
            &["consistent", f.path()],
            &[("RUSTDL_WEDGE_CONSISTENCY", "0")],
        );
        assert_eq!(out.trim(), expect, "{tag}: main tableau");
    }
}

/// F9 folds the class level only: `property-hierarchy` does not list `r` as empty, so
/// the role-level flag stays set (Fable's S2).
#[test]
fn a_folded_property_disjointness_keeps_the_role_level_flag() {
    let f = Fixture::new(
        "f9-roles",
        "DisjointObjectProperties(owl:topObjectProperty :r) ClassAssertion(:C :a)",
    );
    assert_eq!(classify_json(&f, &[])["universal_role_ordinary"], false);
    let (code, out, err) = run(&["property-hierarchy", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["universal_role_ordinary"], true, "{v}");
}

/// A fold that rests on a source axiom is cited with it: `prove` lists the
/// `ClassAssertion` that makes `∃top.D` hold everywhere, not "OWL semantics" alone.
#[test]
fn prove_cites_the_licensing_source_axiom() {
    let f = Fixture::new(
        "cite",
        "ClassAssertion(:D :a) SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) :E)",
    );
    let (code, out, err) = run(&["prove", "--json", f.path(), &iri("C"), &iri("E")]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("ClassAssertion(:D :a)"), "{out}");
    let (_, out, _) = run(&["prove", f.path(), &iri("C"), &iri("E")]);
    assert!(out.contains("built-in, licensed by source axiom"), "{out}");
}

/// Known limitation (#611): `verify-el` reports a false "violated" on `⊤ ⊑ ∃u.⊤` with
/// `∃u.⊤ ⊑ E`, the shape F1 adds, so the F1 canary stays off `verify-el`. Flip this
/// when #611 is fixed.
#[test]
#[ignore = "#611: verify-el's model leaves the ⊤-witness unlabelled"]
fn verify_el_accepts_the_f1_fold() {
    let f = Fixture::new(
        "f1-verify",
        "SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty owl:Thing) :E)",
    );
    let (code, out, _) = run(&["verify-el", f.path()]);
    assert_eq!(code, 0, "{out}");
}
