//! One `+`/`-` pair per [`owl_dl_saturation::unlowered`] site.
//!
//! `+` is a shape that reaches the site today. Every site but `top-lhs-nonatomic` is
//! unreachable from EL input on main, so its `+` is the nearest non-EL shape that takes
//! the same branch. `-` is the nearest EL shape the same code lowers fully; it must
//! record nothing at all.
//!
//! `rhs-exists-dropped`'s `+` needs `RUSTDL_EL_BOT_FILLER=0` and lives in its own test
//! binary (`unlowered_bot_filler_off.rs`), so the env change cannot race these tests.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::InternalOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_saturation::{unlowered_el_constructs, unlowered_realize_abox};
use std::collections::BTreeSet;
use std::io::Cursor;

fn internal(axioms: &str) -> InternalOntology {
    let src = format!(
        "Prefix(:=<http://rustdl.test/>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://rustdl.test/o>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
         Declaration(Class(:D)) Declaration(ObjectProperty(:r)) \
         Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:t)) \
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         {axioms}\n)"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut Cursor::new(src), ParserConfiguration::default()).expect("ofn parses");
    convert_ontology(&onto).expect("conversion")
}

fn sites(axioms: &str) -> BTreeSet<&'static str> {
    unlowered_el_constructs(&internal(axioms))
        .into_iter()
        .map(|u| u.site)
        .collect()
}

fn realize_sites(axioms: &str) -> BTreeSet<&'static str> {
    unlowered_realize_abox(&internal(axioms))
        .into_iter()
        .map(|u| u.site)
        .collect()
}

#[track_caller]
fn records(site: &str, axioms: &str) {
    let got = sites(axioms);
    assert!(
        got.contains(site),
        "{axioms}: expected `{site}`, got {got:?}"
    );
}

#[track_caller]
fn clean(axioms: &str) {
    let got = sites(axioms);
    assert!(got.is_empty(), "{axioms}: expected no records, got {got:?}");
}

#[test]
fn rhs_exists_weakened() {
    records(
        "rhs-exists-weakened",
        "SubClassOf(:A ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C)))",
    );
    clean(
        "SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B \
         ObjectSomeValuesFrom(:s owl:Thing))))",
    );
}

#[test]
fn rhs_exists_weakened_inside_a_conjunction() {
    records(
        "rhs-exists-weakened",
        "SubClassOf(:A ObjectIntersectionOf(:D ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C))))",
    );
    clean("SubClassOf(:A ObjectIntersectionOf(:D ObjectSomeValuesFrom(:r :B)))");
}

/// With the D10 chokepoint on (the default) a `⊥` filler lowers to the bot key: no record.
#[test]
fn rhs_exists_dropped_negative() {
    clean("SubClassOf(:A ObjectSomeValuesFrom(:r owl:Nothing))");
}

#[test]
fn lhs_and_unsalvageable() {
    records(
        "lhs-and-unsalvageable",
        "SubClassOf(ObjectIntersectionOf(:A ObjectAllValuesFrom(:r ObjectUnionOf(:B :C))) :D)",
    );
    clean(
        "SubClassOf(ObjectIntersectionOf(:A ObjectSomeValuesFrom(:r :B) \
         ObjectSomeValuesFrom(:s owl:Thing)) :D)",
    );
}

#[test]
fn and_lhs_rhs_exists_dropped() {
    records(
        "and-lhs-rhs-exists-dropped",
        "SubClassOf(ObjectIntersectionOf(:A :B) ObjectSomeValuesFrom(:r ObjectUnionOf(:C :D)))",
    );
    clean(
        "SubClassOf(ObjectIntersectionOf(:A :B) ObjectSomeValuesFrom(:r \
         ObjectIntersectionOf(:C ObjectSomeValuesFrom(:t owl:Thing))))",
    );
}

#[test]
fn some_lhs_body() {
    records(
        "some-lhs-body",
        "SubClassOf(ObjectSomeValuesFrom(:r ObjectComplementOf(:B)) :D)",
    );
    clean(
        "SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B \
         ObjectSomeValuesFrom(:s owl:Thing))) :D)",
    );
}

#[test]
fn some_lhs_rhs_exists_dropped() {
    records(
        "some-lhs-rhs-exists-dropped",
        "SubClassOf(ObjectSomeValuesFrom(:r :B) ObjectSomeValuesFrom(:s ObjectUnionOf(:C :D)))",
    );
    clean(
        "SubClassOf(ObjectSomeValuesFrom(:r :B) ObjectSomeValuesFrom(:s \
         ObjectIntersectionOf(:C ObjectSomeValuesFrom(:t owl:Thing))))",
    );
}

#[test]
fn domain_heads_lossy() {
    records(
        "domain-heads-lossy",
        "SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) ObjectSomeValuesFrom(:s ObjectUnionOf(:C :D)))",
    );
    clean(
        "SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) ObjectIntersectionOf(:A \
         ObjectSomeValuesFrom(:s :B)))",
    );
}

/// The live drop (issue draft `top-lhs-existential`): `⊤ ⊑ ∃r.B` loses the `∃`.
#[test]
fn top_lhs_nonatomic() {
    records(
        "top-lhs-nonatomic",
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B))",
    );
    records(
        "top-lhs-nonatomic",
        "EquivalentClasses(owl:Thing ObjectSomeValuesFrom(:r :B))",
    );
    clean("SubClassOf(owl:Thing ObjectIntersectionOf(:A :B))");
}

#[test]
fn sub_kind_unhandled() {
    records(
        "sub-kind-unhandled",
        "SubClassOf(ObjectComplementOf(:B) :D)",
    );
    clean("SubClassOf(owl:Nothing :D)");
}

#[test]
fn disjoint_nonatomic_member() {
    records(
        "disjoint-nonatomic-member",
        "DisjointClasses(:A ObjectSomeValuesFrom(:r :B))",
    );
    clean("DisjointClasses(:A :B)");
}

#[test]
fn domain_range_nonatomic() {
    records(
        "domain-range-nonatomic",
        "ObjectPropertyRange(:r ObjectSomeValuesFrom(:s :B))",
    );
    clean("ObjectPropertyRange(:r :A) ObjectPropertyDomain(:r owl:Thing)");
}

#[test]
fn abox_class_nonatomic() {
    let got = realize_sites("ClassAssertion(ObjectSomeValuesFrom(:r :B) :a)");
    assert!(got.contains("abox-class-nonatomic"), "{got:?}");
    let got = realize_sites("ClassAssertion(ObjectIntersectionOf(:A :B) :a)");
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn abox_inverse_edge() {
    let got = realize_sites("ObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)");
    assert!(got.contains("abox-inverse-edge"), "{got:?}");
    let got = realize_sites("ObjectPropertyAssertion(:r :a :b)");
    assert!(got.is_empty(), "{got:?}");
}

/// Vacuous axioms need no rule: nothing records, however unlowerable the other side.
#[test]
fn vacuous_axioms_record_nothing() {
    clean(
        "SubClassOf(ObjectIntersectionOf(:A ObjectAllValuesFrom(:r ObjectUnionOf(:B :C))) owl:Thing)",
    );
    clean("SubClassOf(ObjectSomeValuesFrom(:r ObjectComplementOf(:B)) owl:Thing)");
    clean("SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) owl:Thing)");
}

/// Records point at the axiom being lowered.
#[test]
fn records_carry_the_axiom_index() {
    let i =
        internal("SubClassOf(:A :B) SubClassOf(:C ObjectSomeValuesFrom(:r ObjectUnionOf(:B :D)))");
    let recs = unlowered_el_constructs(&i);
    assert_eq!(recs.len(), 1, "{recs:?}");
    assert!(
        matches!(
            i.axioms[recs[0].axiom],
            owl_dl_core::Axiom::SubClassOf { .. }
        ),
        "{:?}",
        i.axioms[recs[0].axiom]
    );
    let owl_dl_core::Axiom::SubClassOf { sub, .. } = &i.axioms[recs[0].axiom] else {
        unreachable!()
    };
    assert!(
        matches!(i.concepts.get(*sub), owl_dl_core::ConceptExpr::Atomic(c) if i.vocabulary.class_iri(*c).ends_with('C')),
        "the record points at `C ⊑ ∃r.(B ⊔ D)`"
    );
}
