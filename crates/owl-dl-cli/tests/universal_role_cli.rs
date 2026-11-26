//! #543: `owl:topObjectProperty` is read as an ordinary property, which is sound for
//! positives but loses entailments of the universal role `Δ × Δ`. Where that can change
//! an answer, every surface now says so: `completeness_guaranteed` is false
//! (condition 9), the JSON outputs carry `universal_role_ordinary: true`, the text
//! outputs print one stderr note, and the exit codes are unchanged. The answers
//! themselves are unchanged; part 2 of #543 implements the semantics.
//!
//! Oracle: `HermiT` for every case it accepts (∃/∀ on top, `top ⊑ r`, chains). It
//! rejects top in `Functional`, cardinality and disjointness ("non-simple property")
//! and `owl:topDataProperty` outside the super-property position; those cases say
//! "adjudicated by hand from the Direct Semantics". Konclude under-reports
//! universal-role `ABox`es (#539), so it is not used. Negative controls come first.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const T: &str = "http://ex.org/t#";
const NOTE: &str = "owl:topObjectProperty is used here as the universal role";

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, body: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-universal-role-{}-{tag}.ofn",
            std::process::id()
        ));
        let src = format!(
            "Prefix(:=<{T}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\n\
             Ontology(<http://ex.org/t>\n\
             Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))\n\
             Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
             Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
             Declaration(NamedIndividual(:c))\n{body}\n)\n"
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

/// (exit code, stdout, stderr).
fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap()
}

fn iri(local: &str) -> String {
    format!("{T}{local}")
}

/// `classify --json` of `body`: (`universal_role_ordinary`, `completeness_guaranteed`).
fn classify_flags(f: &Fixture) -> (serde_json::Value, serde_json::Value, String) {
    let (code, out, err) = run(&["classify", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    (
        v["universal_role_ordinary"].clone(),
        v["completeness_guaranteed"].clone(),
        err,
    )
}

// ── Negative controls ──

/// The only use on ORE (`r ⊑ top`) and the other inert positions change nothing:
/// the guarantee stands, no field, no note. HermiT: `C ⊑ ∃r.D` stays as is.
#[test]
fn inert_uses_leave_the_guarantee_and_print_nothing() {
    for (tag, body) in [
        (
            "sub",
            "SubObjectPropertyOf(:r owl:topObjectProperty) \
             SubClassOf(:C ObjectSomeValuesFrom(:r :D))",
        ),
        (
            "chars",
            "TransitiveObjectProperty(owl:topObjectProperty) \
             ReflexiveObjectProperty(owl:topObjectProperty) SubClassOf(:C :D)",
        ),
    ] {
        let f = Fixture::new(tag, body);
        let (uro, guaranteed, err) = classify_flags(&f);
        assert_eq!(uro, false, "{tag}");
        assert_eq!(guaranteed, true, "{tag}: pure EL, nothing read as ordinary");
        assert!(!err.contains(NOTE), "{tag}: {err}");
        let (_, _, err) = run(&["subclass", f.path(), &iri("C"), &iri("E")]);
        assert!(!err.contains(NOTE), "{tag}: {err}");
    }
}

/// #539's A3 (`top ⊑ r ⊑ bottom`) and A4 (`¬top(a, b)`) are inconsistent and
/// answered right; they must not be flagged. HermiT: both inconsistent.
#[test]
fn the_539_contradictions_are_not_flagged() {
    for (tag, body) in [
        (
            "a3",
            "SubObjectPropertyOf(owl:topObjectProperty :r) \
             SubObjectPropertyOf(:r owl:bottomObjectProperty)",
        ),
        (
            "a4",
            "NegativeObjectPropertyAssertion(owl:topObjectProperty :a :b)",
        ),
    ] {
        let f = Fixture::new(tag, body);
        let (code, out, err) = run(&["consistent", "--json", f.path()]);
        assert_eq!(code, 0, "{tag}: {err}");
        let v = json(&out);
        assert_eq!(v["consistent"], false, "{tag}");
        assert_eq!(v["universal_role_ordinary"], false, "{tag}");
        assert!(!err.contains(NOTE), "{tag}: {err}");
    }
}

/// A positive top assertion is inert: `top(a, b)` holds in `Δ × Δ`.
#[test]
fn a_positive_top_assertion_is_inert() {
    let f = Fixture::new(
        "assert",
        "ObjectPropertyAssertion(owl:topObjectProperty :a :b) ClassAssertion(:C :a)",
    );
    let (code, out, err) = run(&["realize", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(json(&out)["universal_role_ordinary"], false);
    assert!(!err.contains(NOTE), "{err}");
}

// ── Positives ──

/// Fable's #578 case: `⊤ ⊑ ∃r.D`, `∃top.D ⊑ ⊥`. HermiT: inconsistent. rustdl says
/// consistent (the semantics is part 2); it may no longer say so with clean flags.
#[test]
fn an_unflagged_false_consistent_is_now_flagged() {
    let f = Fixture::new(
        "fab",
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :D)) \
         SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) owl:Nothing)",
    );
    let (code, out, err) = run(&["consistent", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["universal_role_ordinary"], true, "{v}");
    assert!(err.contains(NOTE), "{err}");
    let (code, out, err) = run(&["consistent", f.path()]);
    assert_eq!(code, 0, "exit code unchanged: {err}");
    assert_eq!(out.trim(), "consistent", "the answer is unchanged (part 2)");
    assert!(err.contains(NOTE), "text: {err}");
    let (uro, guaranteed, err) = classify_flags(&f);
    assert_eq!((uro, guaranteed), (true.into(), false.into()));
    assert!(err.contains(NOTE), "{err}");
}

/// `C ⊑ D`, `∃top.D ⊑ E` entail `C ⊑ E` (top is reflexive). HermiT derives it;
/// rustdl misses it, and every surface now says the "no" may be wrong.
#[test]
fn a_missed_subsumption_is_flagged_on_every_class_surface() {
    let f = Fixture::new(
        "sub",
        "SubClassOf(:C :D) SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) :E)",
    );
    let (uro, guaranteed, err) = classify_flags(&f);
    assert_eq!((uro, guaranteed), (true.into(), false.into()));
    assert!(err.contains(NOTE), "classify --json: {err}");
    let (_, _, err) = run(&["classify", f.path()]);
    assert!(err.contains(NOTE), "classify: {err}");

    let (code, out, err) = run(&["subclass", f.path(), &iri("C"), &iri("E")]);
    assert_eq!((code, out.trim()), (0, "no"));
    assert!(err.contains(NOTE), "subclass: {err}");

    let (code, out, err) = run(&["explain", f.path(), &iri("C"), &iri("E")]);
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains("closure is complete)"), "explain: {out}");
    assert!(err.contains(NOTE), "explain: {err}");

    let (code, out, err) = run(&[
        "subclass-expr",
        "--json",
        f.path(),
        ":C",
        "owl:topObjectProperty some :D",
    ]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["universal_role_ordinary"], true, "{v}");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");

    let (code, out, _) = run(&["verify-el", "--json", f.path()]);
    assert_ne!(code, 0, "verify-el cannot verify a weaker reading");
    let v = json(&out);
    assert_eq!(v["universal_role_ordinary"], true, "{v}");
    assert!(
        out.contains("UniversalRoleReadAsOrdinary") || v["verdict"] == "unresolved",
        "{out}"
    );
}

/// `D(a)`, `C(b)`, `∃top.D ⊑ E` entail `E(a)` and `E(b)`. HermiT derives both.
#[test]
fn a_missed_type_is_flagged_on_every_instance_surface() {
    let f = Fixture::new(
        "abox",
        "ClassAssertion(:D :a) ClassAssertion(:C :b) \
         SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) :E)",
    );
    let (code, out, err) = run(&["instance", f.path(), &iri("E"), &iri("b")]);
    assert_eq!((code, out.trim()), (0, "no"), "{err}");
    assert!(err.contains(NOTE), "instance: {err}");
    let (code, _, err) = run(&["instances", f.path(), &iri("E")]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains(NOTE), "instances: {err}");
    let (code, out, err) = run(&["realize", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(json(&out)["universal_role_ordinary"], true);
    assert!(err.contains(NOTE), "realize: {err}");
    let (code, out, err) = run(&["instances-expr", "--json", f.path(), ":E"]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["universal_role_ordinary"], true, "{v}");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");
}

/// `C ⊑ ∀top.⊥`: `C` is unsatisfiable (the universal role reaches `C`'s own element).
/// HermiT: C unsat. rustdl: satisfiable, flagged.
#[test]
fn a_missed_unsatisfiable_class_is_flagged() {
    let f = Fixture::new(
        "all",
        "SubClassOf(:C ObjectAllValuesFrom(owl:topObjectProperty owl:Nothing))",
    );
    let (code, out, err) = run(&["sat", f.path(), &iri("C")]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), "sat", "unchanged (part 2)");
    assert!(err.contains(NOTE), "sat: {err}");
    let (code, out, err) = run(&["sat-expr", "--json", f.path(), ":C"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(json(&out)["universal_role_ordinary"], true);
}

/// `top ∘ s ⊑ r`, `r ⊑ bottom`, `s(a, b)`: HermiT says inconsistent (`(a, a) ∈ top`,
/// so `r(a, b)`, which is empty). rustdl says consistent; part 2 must derive it.
#[test]
fn n7edge_is_a_flagged_positive_for_part_2() {
    let f = Fixture::new(
        "n7edge",
        "SubObjectPropertyOf(ObjectPropertyChain(owl:topObjectProperty :s) :r) \
         SubObjectPropertyOf(:r owl:bottomObjectProperty) ObjectPropertyAssertion(:s :a :b)",
    );
    let (code, out, err) = run(&["consistent", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["consistent"], true, "today's answer; part 2 flips it");
    assert_eq!(v["universal_role_ordinary"], true, "{v}");
}

/// A use inside the query expression only is caught: the expression queries convert
/// the probed ontology.
#[test]
fn a_use_in_the_query_only_is_flagged() {
    let f = Fixture::new("qonly", "ClassAssertion(:D :a)");
    let (code, out, err) = run(&[
        "instances-expr",
        "--json",
        f.path(),
        "owl:topObjectProperty some :D",
    ]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["universal_role_ordinary"], true, "{v}");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");
}

/// Shapes HermiT rejects as non-simple, adjudicated by hand from the Direct
/// Semantics: `Functional(top)` makes the domain a singleton, so with `a ≠ b` it is
/// inconsistent, and `owl:topDataProperty` restricted in a class is outside OWL 2 DL.
/// Each must carry the flag.
#[test]
fn shapes_hermit_rejects_are_flagged() {
    for (tag, body) in [
        (
            "func",
            "FunctionalObjectProperty(owl:topObjectProperty) DifferentIndividuals(:a :b)",
        ),
        (
            "tdata",
            "SubClassOf(:C DataSomeValuesFrom(owl:topDataProperty rdfs:Literal))",
        ),
    ] {
        let f = Fixture::new(tag, body);
        let (code, out, err) = run(&["consistent", "--json", f.path()]);
        assert_eq!(code, 0, "{tag}: {err}");
        assert_eq!(json(&out)["universal_role_ordinary"], true, "{tag}");
    }
}

/// Role level: `top ⊑ r` is inert for classes, but `r` relates every pair, so
/// `r(a, b)` is entailed (HermiT: with `¬r(a, b)` it is inconsistent) and
/// `property-values` does not list it. Its flag is role-level; `classify`'s is not.
#[test]
fn a_role_above_top_flags_the_role_level_surfaces_only() {
    let f = Fixture::new(
        "topsub",
        "SubObjectPropertyOf(owl:topObjectProperty :r) ObjectPropertyAssertion(:s :a :b)",
    );
    let (code, out, err) = run(&["property-values", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(json(&out)["universal_role_ordinary"], true);
    assert!(err.contains(NOTE), "property-values: {err}");
    let (uro, _, err) = classify_flags(&f);
    assert_eq!(uro, false, "class level: inert");
    assert!(!err.contains(NOTE), "{err}");
}
