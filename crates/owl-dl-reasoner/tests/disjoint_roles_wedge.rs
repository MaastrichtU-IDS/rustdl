#![allow(clippy::doc_markdown)]
//! `DisjointObjectProperties` was dropped from the wedge's clause theory (#198): it
//! fell into `clausify_axiom`'s catch-all, uncounted. It now clausifies pairwise to
//! `R(X,y) ∧ S(X,y) → ⊥`. Each fixture carries a `∀` so it leaves the EL fast path.

use owl_dl_reasoner::classify;

fn unsat(axioms: &str) -> Vec<String> {
    let src = format!(
        "Prefix(:=<http://ex.org/#>)
Ontology(<http://ex.org/dj>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:E))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q)) Declaration(ObjectProperty(:t))
SubClassOf(:E ObjectAllValuesFrom(:t :C))
DisjointObjectProperties(:p :q)
{axioms}
)
"
    );
    let mut cur = std::io::Cursor::new(src);
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

/// `p` and `q` edges to the same named-by-self successor: `∃p.Self ⊓ ∃q.Self`.
#[test]
fn disjoint_roles_refute_a_shared_self_loop() {
    assert_eq!(
        unsat("SubClassOf(:A ObjectIntersectionOf(ObjectHasSelf(:p) ObjectHasSelf(:q)))"),
        ["A"]
    );
}

/// A functional super-role forces the `p` and `q` successors to merge into one node,
/// which is then reached by both roles.
#[test]
fn disjoint_roles_refute_a_merged_successor() {
    assert_eq!(
        unsat(
            "SubObjectPropertyOf(:p :t)\nSubObjectPropertyOf(:q :t)\nFunctionalObjectProperty(:t)\n\
             SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:p :B) ObjectSomeValuesFrom(:q :B)))"
        ),
        ["A"]
    );
}

/// FP guard: separate `p` and `q` successors are fine.
#[test]
fn disjoint_roles_allow_separate_successors() {
    assert!(
        unsat("SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:p :B) ObjectSomeValuesFrom(:q :B)))")
            .is_empty()
    );
}

/// FP guard: an edge one way on `p` and the other way on `q` is not a shared pair.
#[test]
fn disjoint_roles_allow_opposite_directions() {
    assert!(
        unsat("SubClassOf(:A ObjectSomeValuesFrom(:p ObjectHasSelf(:t)))\nSubClassOf(:B ObjectSomeValuesFrom(:q ObjectSomeValuesFrom(ObjectInverseOf(:p) :A)))")
            .is_empty()
    );
}
