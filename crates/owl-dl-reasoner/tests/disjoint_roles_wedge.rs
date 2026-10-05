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

/// FP guard: a `p`-edge one way and a `q`-edge the other way share no ordered pair.
/// `{o} ⊑ ∃p.{o2}`, `{o2} ⊑ ∃q.{o}` is consistent (HermiT).
#[test]
fn disjoint_roles_allow_opposite_directions() {
    assert!(
        unsat(
            "Declaration(NamedIndividual(:o)) Declaration(NamedIndividual(:o2))\n\
             SubClassOf(ObjectOneOf(:o) ObjectSomeValuesFrom(:p ObjectOneOf(:o2)))\n\
             SubClassOf(ObjectOneOf(:o2) ObjectSomeValuesFrom(:q ObjectOneOf(:o)))\n\
             SubClassOf(:A ObjectSomeValuesFrom(:p :B))"
        )
        .is_empty()
    );
}

/// `Disjoint(p, t⁻)`: a `t` self-loop is also a `t⁻` self-loop, so it clashes with a
/// `p` self-loop on the same node. Exercises the inverse-polarity edge check.
#[test]
fn disjointness_with_an_inverse_role_matches_through_polarity() {
    assert_eq!(
        unsat(
            "DisjointObjectProperties(:p ObjectInverseOf(:t))\n\
             SubClassOf(:A ObjectIntersectionOf(ObjectHasSelf(:p) ObjectHasSelf(:t)))"
        ),
        ["A"]
    );
}

/// A repeated operand is vacuous (OWL 2 operands are a set): `Disjoint(t t)` must not
/// empty `t`. HermiT and the main tableau agree.
#[test]
fn a_repeated_operand_is_vacuous() {
    assert!(
        unsat("DisjointObjectProperties(:t :t)\nSubClassOf(:A ObjectSomeValuesFrom(:t :B))")
            .is_empty()
    );
}

/// ...but disjointness between a role and the declared inverse of its inverse is
/// genuine: `InverseObjectProperties(t s)` makes `s ≡ t⁻`, so `Disjoint(s, t⁻)`
/// empties `s` (HermiT, Konclude). The duplicate check must run before inverse
/// canonicalisation.
#[test]
fn disjointness_of_a_role_with_its_own_equivalent_is_not_skipped() {
    assert_eq!(
        unsat(
            "Declaration(ObjectProperty(:s))\nInverseObjectProperties(:t :s)\n\
             DisjointObjectProperties(:s ObjectInverseOf(:t))\n\
             SubClassOf(:A ObjectSomeValuesFrom(:s :B))"
        ),
        ["A"]
    );
}
