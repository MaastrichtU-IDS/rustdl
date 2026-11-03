//! #435 / #436: a ⊤ filler (`∃r.⊤`) in the EL saturator, in every position the
//! pure-EL gate admits it. On main each POSITIVE case below was a missed entailment
//! reported with `completeness_guaranteed: true` (the D10 shape); the reproducers are
//! the generated net's (`el_guaranteed`, seed 20261106), Konclude and `HermiT` agreeing.
//!
//! Every positive case asserts the subsumption on BOTH surfaces (classify, which
//! takes the pure-EL saturation path, and the per-pair `is_subclass_of`), and that
//! the file is pure EL with the guarantee on, so a pass is a pass under the claim.
//! The negative cases are FP controls; they already hold on main and are kept as
//! controls (a sabotage that over-derives turns them red).
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::FragmentClassification;
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/top>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
         Declaration(Class(:D)) Declaration(Class(:F)) Declaration(Class(:T)) \
         Declaration(Class(:W)) Declaration(Class(:X)) \
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) \
         Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:p))\n{body}\n)"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

fn iri(name: &str) -> String {
    format!("http://ex/#{name}")
}

/// `sub ⊑ sup` is derived on both surfaces, under a pure-EL completeness claim.
fn derives(body: &str, sub: &str, sup: &str) {
    let o = onto(body);
    let c = owl_dl_reasoner::classify(&o).unwrap();
    let stats = c.stats();
    assert_eq!(stats.fragment, FragmentClassification::PureEl, "{body}");
    assert!(stats.completeness_guaranteed(), "{body}: {stats:?}");
    assert!(
        c.is_subclass(&iri(sub), &iri(sup)),
        "classify misses {sub} ⊑ {sup}:\n{body}"
    );
    assert!(
        owl_dl_reasoner::is_subclass_of(&o, &iri(sub), &iri(sup)).unwrap(),
        "subclass misses {sub} ⊑ {sup}:\n{body}"
    );
}

/// `sub ⊑ sup` is NOT derived on either surface (an FP control).
fn does_not_derive(body: &str, sub: &str, sup: &str) {
    let o = onto(body);
    let c = owl_dl_reasoner::classify(&o).unwrap();
    assert!(
        !c.is_subclass(&iri(sub), &iri(sup)),
        "classify over-derives {sub} ⊑ {sup}:\n{body}"
    );
    assert!(
        !owl_dl_reasoner::is_subclass_of(&o, &iri(sub), &iri(sup)).unwrap(),
        "subclass over-derives {sub} ⊑ {sup}:\n{body}"
    );
}

// ---- #435: the top-level ⊤-witness carries its ranges and ⊤ ⊑ C -----------------

/// The issue's 3-axiom reproducer (`el/repro/topwitness-range-3ax.ofn`).
#[test]
fn top_witness_carries_its_range() {
    derives(
        "SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing))
         ObjectPropertyRange(:r :F)
         SubClassOf(ObjectSomeValuesFrom(:r :F) :W)",
        "X",
        "W",
    );
}

/// A range on a super-role reaches a sub-role's witness (`effective_ranges`).
#[test]
fn top_witness_carries_a_super_roles_range() {
    derives(
        "SubObjectPropertyOf(:r :t) ObjectPropertyRange(:t :F)
         SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing))
         SubClassOf(ObjectSomeValuesFrom(:r :F) :W)",
        "X",
        "W",
    );
}

/// `⊤ ⊑ T`: every element, the witness included, is a `T`.
#[test]
fn top_witness_carries_top_subsumers() {
    derives(
        "SubClassOf(owl:Thing :T)
         SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing))
         SubClassOf(ObjectSomeValuesFrom(:r :T) :W)",
        "X",
        "W",
    );
}

/// The domain inference the witness exists for still fires.
#[test]
fn the_domain_inference_still_fires() {
    derives(
        "SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing)) ObjectPropertyRange(:r :F)
         ObjectPropertyDomain(:r :D)",
        "X",
        "D",
    );
}

/// FP controls. A range constrains the successor, not the subject; and a range of
/// one role never reaches another role's witness.
#[test]
fn ranges_stay_on_their_own_role_and_side() {
    does_not_derive(
        "SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing)) ObjectPropertyRange(:r :F)",
        "X",
        "F",
    );
    does_not_derive(
        "SubClassOf(:X ObjectSomeValuesFrom(:s owl:Thing)) ObjectPropertyRange(:r :F)
         SubClassOf(ObjectSomeValuesFrom(:s :F) :W)",
        "X",
        "W",
    );
}

// ---- #436: a nested ⊤ filler ---------------------------------------------------

/// Reproducer 1: the same `∃r.∃s.⊤` as a fact and as a trigger.
#[test]
fn nested_top_as_fact_and_trigger() {
    derives(
        "SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Thing)))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Thing)) :W)",
        "X",
        "W",
    );
}

/// Reproducer 2: a nested `∃s.⊤` only in the trigger. It must accept ANY
/// `s`-successor (here a `B`), which only the domain-driven ⊤-marker does.
#[test]
fn nested_top_trigger_accepts_any_successor() {
    derives(
        "SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Thing)) :W)",
        "X",
        "W",
    );
}

/// Reproducer 3 (`el/repro/nested-top-3-domainhead.ofn`): a domain-GCI head with a
/// `∃·.⊤` conjunct no longer drops the whole head.
#[test]
fn domain_head_with_a_top_conjunct() {
    derives(
        "SubClassOf(:C ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :C)))
         SubClassOf(ObjectSomeValuesFrom(:r owl:Thing)
                    ObjectIntersectionOf(:D ObjectSomeValuesFrom(:r owl:Thing)))",
        "C",
        "D",
    );
}

/// FACT position, discriminating the split: the nested witness must carry
/// `Range(s)`. Lowering a fact's ⊤ the trigger way (to `T_s`, no witness typing)
/// misses this.
#[test]
fn nested_top_fact_carries_the_inner_range() {
    derives(
        "SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Thing)))
         ObjectPropertyRange(:s :F)
         SubClassOf(ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :F)) :W)",
        "X",
        "W",
    );
}

/// FACT position with a chain: `r ∘ s ⊑ t`, `Range(t, F)` types the nested
/// `s`-successor under the `r` edge (#84's chain range).
#[test]
fn nested_top_fact_carries_the_chain_range() {
    derives(
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) ObjectPropertyRange(:t :F)
         SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Thing)))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :F)) :W)",
        "X",
        "W",
    );
}

/// An And-LHS operand `∃r.⊤` used to drop the whole conjunctive trigger.
#[test]
fn and_lhs_top_operand() {
    derives(
        "SubClassOf(:X :A) SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         SubClassOf(ObjectIntersectionOf(:A ObjectSomeValuesFrom(:r owl:Thing)) :W)",
        "X",
        "W",
    );
}

/// An And-LHS operand with a NESTED ⊤ (`∃r.∃s.⊤`).
#[test]
fn and_lhs_nested_top_operand() {
    derives(
        "SubClassOf(:X :A)
         SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)))
         SubClassOf(ObjectIntersectionOf(:A
                    ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Thing))) :W)",
        "X",
        "W",
    );
}

/// The consequent of `∃r.B ⊑ ∃s.⊤` used to be dropped whole.
#[test]
fn top_consequent_of_an_existential_trigger() {
    derives(
        "SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         SubClassOf(ObjectSomeValuesFrom(:r :B) ObjectSomeValuesFrom(:s owl:Thing))
         ObjectPropertyDomain(:s :D)",
        "X",
        "D",
    );
}

/// The consequent `∃r.B ⊑ ∃s.C` now carries `Range(s)` (the reroute through
/// `atomic_existential_rhs`).
#[test]
fn existential_consequent_carries_its_range() {
    derives(
        "SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         SubClassOf(ObjectSomeValuesFrom(:r :B) ObjectSomeValuesFrom(:s :C))
         ObjectPropertyRange(:s :F)
         SubClassOf(ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C :F)) :W)",
        "X",
        "W",
    );
}

/// FP controls for the trigger marker: `∃r.∃s.⊤` needs an `s`-edge under the `r`
/// edge, and an `s`-edge elsewhere does not count.
#[test]
fn nested_top_trigger_needs_the_inner_edge() {
    does_not_derive(
        "SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Thing)) :W)",
        "X",
        "W",
    );
    does_not_derive(
        "SubClassOf(:X ObjectSomeValuesFrom(:r :B)) SubClassOf(:X ObjectSomeValuesFrom(:s :C))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Thing)) :W)",
        "X",
        "W",
    );
}

/// FP control for the witness/marker separation (`T_s` ≠ `TW_s`): `∃s.∃s.⊤ ⊑ W`
/// must not fire on a single `s`-edge.
#[test]
fn a_single_edge_is_not_a_double_edge() {
    does_not_derive(
        "SubClassOf(:X ObjectSomeValuesFrom(:s :B))
         SubClassOf(ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:s owl:Thing)) :W)",
        "X",
        "W",
    );
}
