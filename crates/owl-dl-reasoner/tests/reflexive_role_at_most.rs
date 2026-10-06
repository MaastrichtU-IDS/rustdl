#![allow(clippy::doc_markdown)]
//! A node is its own `r`-successor when `r` is reflexive, so it counts toward a
//! `≤n r` (functional included). The wedge encodes reflexivity by contracting
//! clause bodies and never built that self-loop, so `Reflexive(r)` +
//! `Functional(r)` + `A ⊑ ∃r.B` missed `A ⊑ B` with `incomplete: false`
//! (#192 gap 1). The `B`-witness must be `A` itself.
//!
//! Every positive below is confirmed by both HermiT and Konclude, and every
//! negative is a case where neither reports the relation.

use owl_dl_reasoner::classify;

fn ontology(axioms: &str) -> String {
    format!(
        "Prefix(:=<http://ex.org/#>)
Ontology(<http://ex.org/rm>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
{axioms}
)
"
    )
}

fn classified(axioms: &str) -> owl_dl_reasoner::Classification {
    let mut cur = std::io::Cursor::new(ontology(axioms));
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");
    classify(&onto).expect("classify")
}

fn sub(axioms: &str, a: &str, b: &str) -> bool {
    classified(axioms).is_subclass(
        &format!("http://ex.org/#{a}"),
        &format!("http://ex.org/#{b}"),
    )
}

const REFL_FUNC: &str = "ReflexiveObjectProperty(:r) FunctionalObjectProperty(:r)";

#[test]
fn functional_reflexive_role_makes_the_witness_the_node_itself() {
    let ax = format!("{REFL_FUNC} SubClassOf(:A ObjectSomeValuesFrom(:r :B))");
    assert!(sub(&ax, "A", "B"));
}

#[test]
fn functional_reflexive_role_refutes_a_witness_the_node_contradicts() {
    let ax = format!(
        "{REFL_FUNC} SubClassOf(:D ObjectIntersectionOf(ObjectSomeValuesFrom(:r :C) ObjectComplementOf(:C)))"
    );
    let c = classified(&ax);
    assert_eq!(c.unsatisfiable_classes(), ["http://ex.org/#D"]);
}

/// After the merge, `A` carries the witness's `∃s.C`, which its own `∀s.¬C`
/// refutes.
#[test]
fn merged_witness_edges_reach_the_node() {
    let ax = format!(
        "{REFL_FUNC} SubClassOf(:A ObjectSomeValuesFrom(:r :B)) \
         SubClassOf(:B ObjectSomeValuesFrom(:s :C)) \
         SubClassOf(:A ObjectAllValuesFrom(:s ObjectComplementOf(:C)))"
    );
    let c = classified(&ax);
    assert_eq!(c.unsatisfiable_classes(), ["http://ex.org/#A"]);
}

#[test]
fn unqualified_at_most_one_counts_the_self_loop() {
    let ax = "ReflexiveObjectProperty(:r) SubClassOf(:A ObjectIntersectionOf(\
              ObjectSomeValuesFrom(:r :B) ObjectMaxCardinality(1 :r)))";
    assert!(sub(ax, "A", "B"));
}

/// A qualified `≤1 r.C` counts the node only if the node is a `C`.
#[test]
fn qualified_at_most_counts_the_node_only_when_it_qualifies() {
    let body = "ReflexiveObjectProperty(:r) SubClassOf(:A ObjectIntersectionOf(\
                ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :C)) \
                ObjectMaxCardinality(1 :r :C)))";
    assert!(sub(&format!("{body} SubClassOf(:A :C)"), "A", "B"));
    assert!(!sub(body, "A", "B"), "A is not a C, so it does not count");
}

/// `≤2` with three forced-distinct candidates (the node, a `B` and a disjoint
/// `C`) must merge the node with one of them, and `A ⊑ ¬B` leaves only `C`.
#[test]
fn at_most_two_merges_the_node_nondeterministically() {
    let ax = "ReflexiveObjectProperty(:r) DisjointClasses(:B :C) \
              SubClassOf(:A ObjectComplementOf(:B)) \
              SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :B) \
              ObjectSomeValuesFrom(:r :C) ObjectMaxCardinality(2 :r)))";
    assert!(sub(ax, "A", "C"));
}

/// A super-role of a reflexive role is reflexive.
#[test]
fn functional_super_role_of_a_reflexive_role() {
    let ax = "ReflexiveObjectProperty(:s) SubObjectPropertyOf(:s :r) \
              FunctionalObjectProperty(:r) SubClassOf(:A ObjectSomeValuesFrom(:r :B))";
    assert!(sub(ax, "A", "B"));
}

#[test]
fn inverse_functional_reflexive_role() {
    let ax = "ReflexiveObjectProperty(:r) InverseFunctionalObjectProperty(:r) \
              SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:r) :B))";
    assert!(sub(ax, "A", "B"));
}

/// `s = r⁻` is reflexive, so `InverseFunctional(s)` (that is,
/// `Functional(r)`) forces the merge.
#[test]
fn inverse_functional_declared_inverse_of_a_reflexive_role() {
    let ax = "InverseObjectProperties(:r :s) ReflexiveObjectProperty(:r) \
              InverseFunctionalObjectProperty(:s) SubClassOf(:A ObjectSomeValuesFrom(:s :B))";
    assert!(sub(ax, "A", "B"));
}

/// The witness is an `r`-predecessor: it is functional on `r` and has both
/// itself and `A` as `r`-successors, so the two coincide.
#[test]
fn functional_reflexive_role_through_an_inverse_witness() {
    let ax = format!("{REFL_FUNC} SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:r) :B))");
    assert!(sub(&ax, "A", "B"));
}

/// FP guard: without reflexivity the witness is a separate node.
#[test]
fn functional_role_without_reflexivity_keeps_the_witness_separate() {
    assert!(!sub(
        "FunctionalObjectProperty(:r) SubClassOf(:A ObjectSomeValuesFrom(:r :B))",
        "A",
        "B"
    ));
}

/// FP guard: `≤2` leaves room for the node and one separate witness.
#[test]
fn at_most_two_leaves_room_for_a_separate_witness() {
    let ax = "ReflexiveObjectProperty(:r) SubClassOf(:A ObjectIntersectionOf(\
              ObjectSomeValuesFrom(:r :B) ObjectMaxCardinality(2 :r)))";
    assert!(!sub(ax, "A", "B"));
}

/// FP guard: a functional SUB-role of a reflexive role is not itself
/// reflexive, so its witness stays separate.
#[test]
fn functional_sub_role_of_a_reflexive_role_is_not_reflexive() {
    let ax = "ReflexiveObjectProperty(:r) SubObjectPropertyOf(:s :r) \
              FunctionalObjectProperty(:s) SubClassOf(:A ObjectSomeValuesFrom(:s :B))";
    assert!(!sub(ax, "A", "B"));
}
