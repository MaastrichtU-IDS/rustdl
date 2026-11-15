//! #536: realize / instance's saturation fast route certified files with
//! `NegativeObjectPropertyAssertion` and then dropped them, so an `ABox` made inconsistent
//! by one got a definite wrong `no` (`realize` returned empty types), unflagged. The
//! fast route now seeds `¬r(a, b)` as `{a} ⊓ ∃r.{b} ⊑ ⊥` over its stand-ins.
//!
//! The completeness claim is that on the realize-certified fragment an entailed `r(a, b)`
//! comes only from asserted edges composed through sub-roles, role equivalence,
//! transitivity and chains, with the top role handled only as the NPA's own role or a
//! sub-role of it. (The top role inside a chain, e.g. `s ∘ top ⊑ r`, is not handled: the
//! #539 family.) So there is one inconsistent canary per derivation route,
//! each in a named and an inverse-assertion form, plus `owl:topObjectProperty`, which
//! the IR treats as an ordinary role.
//!
//! Negatives first: every consistent control keeps its exact types, and every fixture
//! stays realize-certified (the fast path is not lost). Every expectation was
//! adjudicated by `HermiT` and Konclude.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    ReasonError, certified_unlowered, instances_of, is_consistent, is_instance_of, realize,
    realize_saturation_only,
};

const NS: &str = "http://ex/#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/o>\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))\n\
         Declaration(ObjectProperty(:v))\n\
         Declaration(Class(:C)) Declaration(Class(:D))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         Declaration(NamedIndividual(:c)) Declaration(NamedIndividual(:d))\n\
         Declaration(NamedIndividual(:e))\n\
         ClassAssertion(:C :a)\n{body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

fn iri(local: &str) -> String {
    format!("{NS}{local}")
}

/// The fix lives on the fast route only, so every fixture must still reach it.
#[track_caller]
fn assert_certified(case: &str, o: &SetOntology<RcStr>) {
    assert!(
        certified_unlowered(o).unwrap().realize_certified,
        "{case}: no longer realize-certified (the fast path would be lost)"
    );
}

/// Consistent: realize answers, `a` keeps its told type `C`, `b` gets none.
#[track_caller]
fn assert_consistent(case: &str, body: &str) {
    let o = onto(body);
    assert_certified(case, &o);
    let r = realize(&o).unwrap_or_else(|e| panic!("{case}: realize failed: {e}"));
    assert_eq!(
        r.entailed_types(&iri("a")),
        [iri("C")],
        "{case}: types of a"
    );
    assert!(r.entailed_types(&iri("b")).is_empty(), "{case}: types of b");
    assert!(
        is_instance_of(&o, &iri("C"), &iri("a")).unwrap(),
        "{case}: a : C"
    );
    assert!(
        !is_instance_of(&o, &iri("D"), &iri("b")).unwrap(),
        "{case}: b : D"
    );
    assert_eq!(
        instances_of(&o, &iri("C")).unwrap(),
        [iri("a")],
        "{case}: C"
    );
}

/// Inconsistent: every certified surface refuses instead of answering a definite `no`.
#[track_caller]
fn assert_inconsistent(case: &str, body: &str) {
    let o = onto(body);
    assert_certified(case, &o);
    assert!(
        matches!(realize(&o), Err(ReasonError::Inconsistent)),
        "{case}: realize must refuse"
    );
    assert!(
        matches!(
            is_instance_of(&o, &iri("D"), &iri("b")),
            Err(ReasonError::Inconsistent)
        ),
        "{case}: is_instance_of must refuse"
    );
    assert!(
        matches!(instances_of(&o, &iri("D")), Err(ReasonError::Inconsistent)),
        "{case}: instances_of must refuse"
    );
}

/// The polarity-normalised inverse form of an inconsistent case: `¬r(x, y)` is written
/// `¬r⁻(y, x)`.
#[track_caller]
fn assert_inconsistent_both(case: &str, edges: &str, (r, x, y): (&str, &str, &str)) {
    assert_inconsistent(
        &format!("{case} (named NPA)"),
        &format!("{edges} NegativeObjectPropertyAssertion(:{r} :{x} :{y})"),
    );
    assert_inconsistent(
        &format!("{case} (inverse NPA)"),
        &format!("{edges} NegativeObjectPropertyAssertion(ObjectInverseOf(:{r}) :{y} :{x})"),
    );
}

#[test]
fn consistent_controls_keep_their_types_and_the_fast_path() {
    assert_consistent(
        "∃r.D on a, ¬r(a,b)",
        "SubClassOf(:C ObjectSomeValuesFrom(:r :D)) NegativeObjectPropertyAssertion(:r :a :b)",
    );
    assert_consistent(
        "¬r(a,b), r(b,a)",
        "ObjectPropertyAssertion(:r :b :a) NegativeObjectPropertyAssertion(:r :a :b)",
    );
    assert_consistent(
        "¬r⁻(a,b), r(a,b)",
        "ObjectPropertyAssertion(:r :a :b) \
         NegativeObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)",
    );
    assert_consistent(
        "r not transitive: r(a,b), r(b,c), ¬r(a,c)",
        "ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :b :c) \
         NegativeObjectPropertyAssertion(:r :a :c)",
    );
    assert_consistent(
        "one chain leg only: r∘s ⊑ t, r(a,b), ¬t(a,c)",
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) ObjectPropertyAssertion(:r :a :b) \
         NegativeObjectPropertyAssertion(:t :a :c)",
    );
    // Pins the `N_a` conjunct: another subject's edge to `b` is no clash.
    assert_consistent(
        "r(c,b), ¬r(a,b)",
        "ObjectPropertyAssertion(:r :c :b) NegativeObjectPropertyAssertion(:r :a :b)",
    );
    // Pins the hierarchy direction: an `r`-edge is not an `s`-edge when `s ⊑ r`.
    assert_consistent(
        "s ⊑ r, r(a,b), ¬s(a,b)",
        "SubObjectPropertyOf(:s :r) ObjectPropertyAssertion(:r :a :b) \
         NegativeObjectPropertyAssertion(:s :a :b)",
    );
    // Pin the direction of the top-role check: a sub-role of top, or a top-role edge,
    // makes no other NPA contradictory.
    assert_consistent(
        "r ⊑ top, ¬r(a,b)",
        "SubObjectPropertyOf(:r owl:topObjectProperty) NegativeObjectPropertyAssertion(:r :a :b)",
    );
    assert_consistent(
        "top(a,c), ¬r(a,b)",
        "ObjectPropertyAssertion(owl:topObjectProperty :a :c) \
         NegativeObjectPropertyAssertion(:r :a :b)",
    );
    assert_consistent(
        "two NPAs sharing an object",
        "ObjectPropertyAssertion(:r :a :b) NegativeObjectPropertyAssertion(:r :a :c) \
         NegativeObjectPropertyAssertion(:r :b :c)",
    );
}

// One canary per derivation route of an entailed `r(a, b)` on the certified fragment,
// each its own test so a sabotage that drops the seeding turns every route red.

#[test]
fn route_the_told_edge_contradicts_the_npa() {
    assert_inconsistent_both("told", "ObjectPropertyAssertion(:r :a :b)", ("r", "a", "b"));
}

#[test]
fn route_a_told_self_edge_contradicts_the_npa() {
    assert_inconsistent_both(
        "self edge",
        "ObjectPropertyAssertion(:r :a :a)",
        ("r", "a", "a"),
    );
}

#[test]
fn route_a_sub_role_edge_contradicts_the_npa() {
    assert_inconsistent_both(
        "sub-role",
        "SubObjectPropertyOf(:s :r) ObjectPropertyAssertion(:s :a :b)",
        ("r", "a", "b"),
    );
}

#[test]
fn route_an_equivalent_role_edge_contradicts_the_npa() {
    assert_inconsistent_both(
        "equivalent roles",
        "EquivalentObjectProperties(:r :s) ObjectPropertyAssertion(:s :a :b)",
        ("r", "a", "b"),
    );
}

#[test]
fn route_a_transitive_edge_over_2_hops_contradicts_the_npa() {
    assert_inconsistent_both(
        "transitive, 2 hops",
        "TransitiveObjectProperty(:r) ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :b :c)",
        ("r", "a", "c"),
    );
}

#[test]
fn route_a_transitive_edge_over_4_hops_contradicts_the_npa() {
    assert_inconsistent_both(
        "transitive, 4 hops",
        "TransitiveObjectProperty(:r) ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :b :c) ObjectPropertyAssertion(:r :c :d) ObjectPropertyAssertion(:r :d :e)",
        ("r", "a", "e"),
    );
}

#[test]
fn route_a_chain_of_length_2_contradicts_the_npa() {
    assert_inconsistent_both(
        "chain of length 2",
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :c)",
        ("t", "a", "c"),
    );
}

#[test]
fn route_a_chain_of_length_3_contradicts_the_npa() {
    assert_inconsistent_both(
        "chain of length 3",
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s :u) :t) ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :c) ObjectPropertyAssertion(:u :c :d)",
        ("t", "a", "d"),
    );
}

#[test]
fn route_a_super_role_of_a_chain_result_contradicts_the_npa() {
    assert_inconsistent_both(
        "NPA on a super-role of a chain result",
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) SubObjectPropertyOf(:t :v) ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :c)",
        ("v", "a", "c"),
    );
}

/// `owl:topObjectProperty` is an ordinary role in the IR: no ground edge ever fires the
/// trigger, so an NPA on it (or on a super-role of it) is contradicted outright.
#[test]
fn an_npa_on_the_top_role_is_contradicted_outright() {
    assert_inconsistent(
        "¬top(a,b)",
        "NegativeObjectPropertyAssertion(owl:topObjectProperty :a :b)",
    );
    assert_inconsistent(
        "¬top⁻(b,a)",
        "NegativeObjectPropertyAssertion(ObjectInverseOf(owl:topObjectProperty) :b :a)",
    );
}

#[test]
fn an_npa_on_a_super_role_of_the_top_role_is_contradicted_outright() {
    assert_inconsistent(
        "top ⊑ r, ¬r(a,b)",
        "SubObjectPropertyOf(owl:topObjectProperty :r) NegativeObjectPropertyAssertion(:r :a :b)",
    );
}

/// `realize --saturation-only` never uses the fast route's seeding. It is a documented
/// sound under-approximation whose `no` may be a missed positive, and its output does not
/// say so (#539). Pinned as it is, so a change to it is visible.
#[test]
fn saturation_only_realize_is_an_unflagged_under_approximation() {
    let o = onto("ObjectPropertyAssertion(:r :a :b) NegativeObjectPropertyAssertion(:r :a :b)");
    let r = realize_saturation_only(&o).unwrap();
    assert_eq!(r.entailed_types(&iri("a")), [iri("C")]);
    assert!(
        !r.incomplete(),
        "no under-approximation signal today (#539)"
    );
}

/// SENTINEL, not a desired answer. After this fix `realize` refuses the transitive shape
/// as inconsistent, while `is_consistent` still says consistent: the consistency wedge
/// never sees negative assertions (#532). Flip this when #532 is fixed.
#[test]
fn sentinel_consistent_still_misses_the_transitive_shape_532() {
    let o = onto(
        "TransitiveObjectProperty(:r) ObjectPropertyAssertion(:r :a :b) \
         ObjectPropertyAssertion(:r :b :c) NegativeObjectPropertyAssertion(:r :a :c)",
    );
    assert!(matches!(realize(&o), Err(ReasonError::Inconsistent)));
    assert!(
        is_consistent(&o).unwrap(),
        "#532 fixed? is_consistent now refuses the transitive NPA shape: flip this sentinel"
    );
}
