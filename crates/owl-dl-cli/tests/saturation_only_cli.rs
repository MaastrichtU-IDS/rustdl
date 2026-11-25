//! #539: `--saturation-only` is a declared sound under-approximation, and its answers now
//! say so. On `r(a,b)`, `¬r(a,b)` (inconsistent: `HermiT` and Konclude agree) `realize
//! --saturation-only` printed `types: []` for every individual with exit 0, nothing on
//! stderr and every JSON flag `false`, and `classify --saturation-only --json` read
//! `consistent: true` with `consistency_undetermined: false`. On `C ⊑ ∀r.D`, `C ⊑ ∃r.E`,
//! `∃r.(D ⊓ E) ⊑ F`, `C(a)` the closure misses `C ⊑ F` and `F(a)` (`HermiT` derives
//! `C ⊑ F`, as does Konclude on the `TBox`), unflagged. Negative controls first: where the
//! closure is the full answer, or proves what it says, nothing new is printed or set.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const T: &str = "http://ex.org/t#";

/// Pure EL, no `ABox`: the closure is complete, so nothing is flagged.
const PURE_EL: &str = "Declaration(Class(:C)) Declaration(Class(:F)) SubClassOf(:C :F)";
/// `r(a,b)`, `¬r(a,b)`: inconsistent, and the closure does not see it.
const NPA_CLASH: &str = "Declaration(Class(:D)) Declaration(ObjectProperty(:r))\n\
                         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
                         ObjectPropertyAssertion(:r :a :b)\n\
                         NegativeObjectPropertyAssertion(:r :a :b)";
/// `a : ⊥` with an edge: inconsistent, and classify's closure (no `ABox` seed) does not see it.
const NOTHING_ASSERTION: &str = "Declaration(ObjectProperty(:r))\n\
                                 Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
                                 ObjectPropertyAssertion(:r :a :b)\n\
                                 ClassAssertion(owl:Nothing :a)";
/// Not pure EL (a union), inconsistent (`⊤ ⊑ D ⊑ ⊥`), and the closure proves it.
const CLOSURE_CLASH: &str = "Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))\n\
                             SubClassOf(owl:Thing :D) SubClassOf(:D owl:Nothing)\n\
                             SubClassOf(:C ObjectUnionOf(:D :E))";
/// `C ⊑ F` and `F(a)` are entailed; the closure misses both.
const FORALL_MISS: &str = "Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))\n\
                           Declaration(Class(:F)) Declaration(ObjectProperty(:r))\n\
                           Declaration(NamedIndividual(:a))\n\
                           SubClassOf(:C ObjectAllValuesFrom(:r :D))\n\
                           SubClassOf(:C ObjectSomeValuesFrom(:r :E))\n\
                           SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:D :E)) :F)\n\
                           ClassAssertion(:C :a)";

const NOTE: &str = "--saturation-only answers from the EL saturation closure alone";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, body: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-saturation-only-{}-{tag}.ofn",
            std::process::id()
        ));
        let src = format!(
            "Prefix(:=<{T}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Ontology(<http://ex.org/t>\n{body}\n)\n"
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

#[test]
fn pure_el_saturation_only_classify_is_unflagged() {
    let f = Fixture::new("pure-el", PURE_EL);
    let (code, out, err) = run(&["classify", "--saturation-only", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["consistent"], true);
    assert_eq!(
        v["consistency_undetermined"], false,
        "the saturator decides pure EL"
    );
    assert_eq!(v["completeness_guaranteed"], true);
    assert!(!err.contains(NOTE), "no note on a guaranteed answer: {err}");
    let (_, _, err) = run(&["classify", "--saturation-only", f.path()]);
    assert!(!err.contains(NOTE), "{err}");
}

#[test]
fn a_proved_positive_carries_no_note() {
    let f = Fixture::new("yes", PURE_EL);
    let (code, out, err) = run(&[
        "subclass",
        "--saturation-only",
        f.path(),
        &iri("C"),
        &iri("F"),
    ]);
    assert_eq!((code, out.trim()), (0, "yes"), "{err}");
    assert!(!err.contains(NOTE), "{err}");
}

#[test]
fn plain_realize_reports_saturation_only_false() {
    let f = Fixture::new("plain", FORALL_MISS);
    let (code, out, err) = run(&["realize", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["saturation_only"], false);
    assert!(!err.contains(NOTE), "{err}");
    // The control for the miss below: the full realize derives F(a).
    let types = v["individuals"][0]["types"].as_array().unwrap();
    assert!(types.contains(&iri("F").into()), "{v}");
}

#[test]
fn a_clash_the_closure_proves_is_not_undetermined() {
    let f = Fixture::new("closure-clash", CLOSURE_CLASH);
    let (code, out, err) = run(&["classify", "--saturation-only", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["consistent"], false, "{v}");
    assert_eq!(v["consistency_undetermined"], false, "{v}");
}

#[test]
fn an_undetected_inconsistency_is_flagged() {
    for (tag, body) in [
        ("npa-clash", NPA_CLASH),
        ("nothing-assertion", NOTHING_ASSERTION),
    ] {
        let f = Fixture::new(tag, body);
        let (code, out, err) = run(&["classify", "--saturation-only", "--json", f.path()]);
        assert_eq!(code, 0, "{tag}: {err}");
        let v = json(&out);
        // The closure cannot see the clash; the answer must not read as a verdict.
        assert_eq!(v["consistency_undetermined"], true, "{tag}: {v}");
        assert_eq!(v["completeness_guaranteed"], false, "{tag}: {v}");
        assert!(err.contains(NOTE), "{tag}: {err}");
        let (_, out, _) = run(&["classify", "--saturation-only", f.path()]);
        assert!(out.contains("# consistency: undetermined"), "{tag}: {out}");

        let (code, out, err) = run(&["realize", "--saturation-only", "--json", f.path()]);
        assert_eq!(code, 0, "{tag}: {err}");
        assert_eq!(json(&out)["saturation_only"], true, "{tag}");
        assert!(err.contains(NOTE), "{tag}: {err}");
        let (code, _, err) = run(&["realize", "--saturation-only", f.path()]);
        assert_eq!(code, 0, "{tag}: {err}");
        assert!(err.contains(NOTE), "{tag}: text realize: {err}");
    }
}

#[test]
fn a_missed_entailment_is_flagged_on_every_surface() {
    let f = Fixture::new("forall-miss", FORALL_MISS);
    let (c, a, fc) = (iri("C"), iri("a"), iri("F"));

    let (code, out, err) = run(&["subclass", "--saturation-only", f.path(), &c, &fc]);
    assert_eq!((code, out.trim()), (0, "no"), "{err}");
    assert!(err.contains(NOTE), "subclass: {err}");
    let (_, out, _) = run(&["subclass", f.path(), &c, &fc]);
    assert_eq!(out.trim(), "yes", "control: the entailment holds");

    let (code, out, err) = run(&["instance", "--saturation-only", f.path(), &fc, &a]);
    assert_eq!((code, out.trim()), (0, "no"), "{err}");
    assert!(err.contains(NOTE), "instance: {err}");

    let (code, out, err) = run(&["instances", "--saturation-only", f.path(), &fc]);
    assert_eq!((code, out.trim()), (0, ""), "{err}");
    assert!(err.contains(NOTE), "instances: {err}");

    let (code, out, err) = run(&["realize", "--saturation-only", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["saturation_only"], true);
    assert_eq!(
        v["incomplete"], false,
        "no deadline fired; the flag is the signal"
    );
    let types = v["individuals"][0]["types"].as_array().unwrap();
    assert!(
        !types.contains(&fc.clone().into()),
        "the closure misses F(a): {v}"
    );
    assert!(err.contains(NOTE), "realize: {err}");
}
