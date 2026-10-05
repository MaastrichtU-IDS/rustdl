#![allow(clippy::doc_markdown)]
//! `IrreflexiveObjectProperty` and `AsymmetricObjectProperty` were dropped from
//! the wedge's clause theory (#192 gap 2): both fell into `clausify_axiom`'s
//! catch-all, uncounted. `classify` then reported the classes below
//! satisfiable with `incomplete: false`, while Konclude and rustdl's own `sat`
//! call them unsatisfiable. Each fixture carries a `∀` so it leaves the EL
//! fast path and reaches the wedge.

use owl_dl_reasoner::classify;

fn unsat(src: &str) -> Vec<String> {
    let mut cur = std::io::Cursor::new(src.to_owned());
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");
    let mut v: Vec<String> = classify(&onto)
        .expect("classify")
        .unsatisfiable_classes()
        .into_iter()
        .map(|c| c.trim_start_matches("http://ex.org/#").to_owned())
        .collect();
    v.sort();
    v
}

fn onto(axioms: &str) -> String {
    format!(
        "Prefix(:=<http://ex.org/#>)
Ontology(<http://ex.org/ra>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:E))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
SubClassOf(:E ObjectAllValuesFrom(:p :C))
{axioms}
)
"
    )
}

#[test]
fn irreflexive_role_refutes_a_self_restriction() {
    let src = onto("IrreflexiveObjectProperty(:p)\nSubClassOf(:A ObjectHasSelf(:p))");
    assert_eq!(unsat(&src), ["A"]);
}

/// Negative control: without the characteristic `A` is satisfiable.
#[test]
fn without_irreflexivity_a_self_restriction_is_satisfiable() {
    let src = onto("SubClassOf(:A ObjectHasSelf(:p))");
    assert!(unsat(&src).is_empty());
}

/// `q` is both asymmetric and symmetric, so any `q`-edge has its reverse and
/// `B ⊑ ∃q.C` is unsatisfiable.
#[test]
fn asymmetric_symmetric_role_refutes_any_edge() {
    let src = onto(
        "AsymmetricObjectProperty(:q)\nSymmetricObjectProperty(:q)\nSubClassOf(:B ObjectSomeValuesFrom(:q :C))",
    );
    assert_eq!(unsat(&src), ["B"]);
}

/// Asymmetry entails irreflexivity, so `∃q.Self` is refuted too.
#[test]
fn asymmetric_role_refutes_a_self_restriction() {
    let src = onto("AsymmetricObjectProperty(:q)\nSubClassOf(:A ObjectHasSelf(:q))");
    assert_eq!(unsat(&src), ["A"]);
}

/// FP guard: an asymmetric role with a one-way edge is fine.
#[test]
fn asymmetric_role_allows_a_one_way_edge() {
    let src = onto("AsymmetricObjectProperty(:q)\nSubClassOf(:B ObjectSomeValuesFrom(:q :C))");
    assert!(unsat(&src).is_empty());
}

/// FP guard: an inverse-path edge back is only a clash when it is the same
/// role. `B ⊑ ∃q.(∃p.B)` with `q` asymmetric has no `q` cycle.
#[test]
fn asymmetric_role_ignores_a_different_back_edge() {
    let src = onto(
        "AsymmetricObjectProperty(:q)\nSubClassOf(:B ObjectSomeValuesFrom(:q ObjectSomeValuesFrom(:p :B)))",
    );
    assert!(unsat(&src).is_empty());
}
