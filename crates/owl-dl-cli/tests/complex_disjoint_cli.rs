//! #600: the wedge clausifier used to defer a `DisjointClasses` / `DisjointUnion`
//! member it could not encode as a body (`∀`, `≤n`, `≥n`, …) and a `DisjointUnion`
//! covering with any complex member. Both wedges trust their `Sat`, so `consistent`
//! and `classify` missed inconsistencies, unsatisfiable classes and subsumptions the
//! main tableau (`sat`, `subclass`) finds. Every expected answer below was checked
//! against HermiT (and Konclude for the issue's repros).
//!
//! Negatives first: each case runs through the command that showed the miss, and
//! with `RUSTDL_CLAUSIFY_COMPLEX_DISJOINT=0` it must show the old miss (attribution).
//! Then non-entailment controls (a covering with a disjunct missing would be an
//! unsound positive), and justify's minimality, which the fix moves.
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use std::path::PathBuf;
use std::process::Command;

const NS: &str = "http://ex/#";
const OFF: (&str, &str) = ("RUSTDL_CLAUSIFY_COMPLEX_DISJOINT", "0");

struct Fixture(PathBuf);

impl Fixture {
    fn new(body: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-complex-disjoint-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(
            &path,
            format!(
                "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
                 Ontology(<http://ex/cd>\n\
                 Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
                 Declaration(Class(:D)) Declaration(Class(:E))\n\
                 Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
                 Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:c))\n\
                 {body}\n)"
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

fn run(args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn json(args: &[&str], env: &[(&str, &str)]) -> serde_json::Value {
    let out = run(args, env);
    assert!(out.status.success(), "{args:?} {env:?}: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

/// `consistent --json`'s verdict. Exit 3 (a give-up, reported as `consistent:
/// true`, its sound direction) is accepted: on a loaded host the debug build's wedge
/// budget can run out, and the FP control below only needs "not inconsistent".
fn consistent(f: &Fixture, env: &[(&str, &str)]) -> bool {
    let out = run(&["consistent", "--json", f.path()], env);
    assert!(matches!(out.status.code(), Some(0 | 3)), "{env:?}: {out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    v["consistent"].as_bool().unwrap()
}

fn classify(f: &Fixture, env: &[(&str, &str)]) -> serde_json::Value {
    json(&["classify", "--json", f.path()], env)
}

fn unsat(v: &serde_json::Value, class: &str) -> bool {
    v["unsatisfiable"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c == &format!("{NS}{class}"))
}

fn has_row(v: &serde_json::Value, sub: &str, sup: &str) -> bool {
    let want = serde_json::json!([format!("{NS}{sub}"), format!("{NS}{sup}")]);
    v["direct_subsumptions"].as_array().unwrap().contains(&want)
}

/// `disjoint-union-covering` (ABox) and `disjoint-antecedent` (ABox): inconsistent.
#[test]
fn inconsistent_abox_forms_are_found() {
    for (tag, body) in [
        (
            "du",
            "DisjointUnion(:C :A ObjectSomeValuesFrom(:r :B))\n\
             ClassAssertion(:C :c) ClassAssertion(ObjectComplementOf(:A) :c)\n\
             ClassAssertion(ObjectAllValuesFrom(:r ObjectComplementOf(:B)) :c)",
        ),
        (
            "dc",
            "DisjointClasses(:A ObjectAllValuesFrom(:r :B))\n\
             ClassAssertion(:A :a) ClassAssertion(ObjectAllValuesFrom(:r :B) :a)",
        ),
    ] {
        let f = Fixture::new(body, &format!("incons-{tag}"));
        assert!(!consistent(&f, &[]), "{tag}: consistent");
        assert_eq!(classify(&f, &[])["consistent"], false, "{tag}: classify");
        assert!(consistent(&f, &[OFF]), "{tag}: flag off shows the old miss");
    }
}

/// The TBox forms, and the shape diversity: each `D` is unsatisfiable (HermiT).
#[test]
fn unsatisfiable_classes_are_found() {
    for (tag, body) in [
        (
            "du-covering",
            "DisjointUnion(:C :A ObjectSomeValuesFrom(:r :B))\n\
             SubClassOf(:D :C) SubClassOf(:D ObjectComplementOf(:A))\n\
             SubClassOf(:D ObjectAllValuesFrom(:r ObjectComplementOf(:B)))",
        ),
        (
            "dc-forall",
            "DisjointClasses(:A ObjectAllValuesFrom(:r :B))\n\
             SubClassOf(:D :A) SubClassOf(:D ObjectAllValuesFrom(:r :B))",
        ),
        (
            "du-two-hard-members",
            "DisjointUnion(:C :A ObjectAllValuesFrom(:r :B) ObjectMaxCardinality(1 :s :B))\n\
             SubClassOf(:D ObjectAllValuesFrom(:r :B)) SubClassOf(:D ObjectMaxCardinality(1 :s :B))",
        ),
        (
            "dc-soft-trigger",
            "DisjointClasses(:E ObjectIntersectionOf(:A ObjectAllValuesFrom(:r :B)))\n\
             SubClassOf(:D :E) SubClassOf(:D :A) SubClassOf(:D ObjectAllValuesFrom(:r :B))",
        ),
        (
            "dc-min",
            "DisjointClasses(:A ObjectMinCardinality(2 :r :B))\n\
             SubClassOf(:D :A) SubClassOf(:D ObjectMinCardinality(2 :r :B))",
        ),
        (
            "dc-or-member",
            "DisjointClasses(:E ObjectUnionOf(:B ObjectAllValuesFrom(:r :B)))\n\
             SubClassOf(:D :E) SubClassOf(:D ObjectAllValuesFrom(:r :B))",
        ),
    ] {
        let f = Fixture::new(body, &format!("unsat-{tag}"));
        assert!(
            unsat(&classify(&f, &[]), "D"),
            "{tag}: classify misses D ⊑ ⊥"
        );
        assert!(
            !unsat(&classify(&f, &[OFF]), "D"),
            "{tag}: flag off shows the old miss"
        );
        // Control through the main tableau: `sat` decides it either way.
        let out = run(&["sat", f.path(), &format!("{NS}D")], &[]);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "unsat",
            "{tag}"
        );
    }
}

/// `disjoint-union-member`: `D ⊑ ∀r.B` and `∀r.B` a member of `C`'s union, so `D ⊑ C`.
#[test]
fn a_complex_member_is_a_subclass_of_the_union() {
    let f = Fixture::new(
        "DisjointUnion(:C :A ObjectAllValuesFrom(:r :B))\n\
         SubClassOf(:D ObjectAllValuesFrom(:r :B))",
        "member",
    );
    assert!(
        has_row(&classify(&f, &[]), "D", "C"),
        "classify misses D ⊑ C"
    );
    assert!(
        !has_row(&classify(&f, &[OFF]), "D", "C"),
        "flag off: the old miss"
    );
}

/// Non-entailment controls: the covering must be the whole union (a missing
/// disjunct would make `C ⊑ A`), and disjointness must not reach a class that is in
/// only one member.
#[test]
fn nothing_is_entailed_beyond_the_axioms() {
    let f = Fixture::new("DisjointUnion(:C :A ObjectSomeValuesFrom(:r :B))", "fp-du");
    let v = classify(&f, &[]);
    assert!(!has_row(&v, "C", "A"), "C ⋢ A: {v}");
    assert!(has_row(&v, "A", "C"), "A ⊑ C: {v}");
    assert!(!unsat(&v, "C"), "C satisfiable: {v}");
    let f = Fixture::new(
        "DisjointClasses(:A ObjectAllValuesFrom(:r :B)) SubClassOf(:D :A)",
        "fp-dc",
    );
    let v = classify(&f, &[]);
    assert!(
        !unsat(&v, "A") && !unsat(&v, "D"),
        "A and D satisfiable: {v}"
    );
    assert!(consistent(
        &Fixture::new(
            "DisjointClasses(:A ObjectAllValuesFrom(:r :B))\n\
             ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :c)\n\
             ClassAssertion(ObjectComplementOf(:B) :c)",
            "fp-dc-abox",
        ),
        &[]
    ));
}

/// The clausification now leaves nothing deferred on `DisjointClasses(A, ∀r.B)`, so
/// the classify wedge's clause shape is Horn (`A → ∃r.Q`), and `analyze_fragment`
/// reads `Horn` instead of `OutOfFragment`. Horn is a clause-shape label, not a
/// completeness claim (#124), but justify's `minimal_guaranteed` admits it: pinned so
/// the shift is visible.
#[test]
fn the_horn_shift_reaches_justify() {
    let f = Fixture::new(
        "DisjointClasses(:A ObjectAllValuesFrom(:r :B)) SubClassOf(:D :A)",
        "horn",
    );
    let q = ["justify", "--json", f.path(), "subclass"];
    let (d, a) = (format!("{NS}D"), format!("{NS}A"));
    let args = [q[0], q[1], q[2], q[3], d.as_str(), a.as_str()];
    assert_eq!(json(&args, &[])["minimal"], true);
    assert_eq!(json(&args, &[OFF])["minimal"], false);
    let fragment = |env: &[(&str, &str)]| {
        let out = run(&["classify", f.path()], env);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        text.lines()
            .find(|l| l.starts_with("# fragment:"))
            .map(str::to_owned)
            .unwrap_or_default()
    };
    assert!(fragment(&[]).contains("Horn"), "{}", fragment(&[]));
    assert!(
        fragment(&[OFF]).contains("out-of-EL"),
        "{}",
        fragment(&[OFF])
    );
}
