//! Canaries for the `ABox`-seeded WEDGE consistency route
//! (`RUSTDL_WEDGE_CONSISTENCY`, default on).
//!
//! Two directions, both load-bearing:
//! - INCONSISTENT (wedge `Unsat`): a real clause clash on the
//!   asserted-only `ABox` seed ⟹ `is_consistent → false`.
//! - CONSISTENT (the catastrophic-guard): a satisfiable `ABox` must NOT
//!   flip to inconsistent. A spurious `Unsat` here = false-inconsistent
//!   = the worst unsoundness in the system, so these are the tests that
//!   matter most.
//!
//! Plus a previously-hanging out-of-EL `ABox` that must now TERMINATE
//! quickly (the whole point of the route: kill the `decide(Top)` hang).

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::io::Cursor;
use std::path::Path;
use std::time::{Duration, Instant};

const PREFIX: &str = "Prefix(:=<http://t/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";

fn consistent(body: &str) -> bool {
    let src = format!("{PREFIX}Ontology(<http://t/o>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent succeeds")
}

// ─────────────────────── INCONSISTENT (wedge Unsat) ───────────────────

#[test]
fn disjoint_class_assertion_is_inconsistent() {
    // ClassAssertion(C,a) + ClassAssertion(D,a) + DisjointClasses(C,D).
    let body = "\
        Declaration(Class(:C)) Declaration(Class(:D))\n\
        Declaration(NamedIndividual(:a))\n\
        DisjointClasses(:C :D)\n\
        ClassAssertion(:C :a)\n\
        ClassAssertion(:D :a)\n";
    assert!(
        !consistent(body),
        "two disjoint types on one individual should be inconsistent"
    );
}

#[test]
fn different_then_same_is_inconsistent() {
    // DifferentIndividuals(a,b) + SameIndividual(a,b): the SameIndividual
    // merge brings both nominals onto one node, where the `{a}⊓{b}⊑⊥`
    // disjointness clause clashes.
    let body = "\
        Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
        DifferentIndividuals(:a :b)\n\
        SameIndividual(:a :b)\n";
    assert!(
        !consistent(body),
        "Different(a,b) + Same(a,b) should be inconsistent"
    );
}

#[test]
fn max_cardinality_clash_is_inconsistent() {
    // ClassAssertion(a, ≤1 r.⊤) + r(a,b) + r(a,c) + Different(b,c):
    // the two r-edges off `a` are forced distinct by the `{b}⊓{c}⊑⊥`
    // disjointness clause, so `≤1 r` clashes. Exercises the exact
    // cardinality path `push_different_individuals_disjoint` documents.
    let body = "\
        Declaration(ObjectProperty(:r))\n\
        Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
        Declaration(NamedIndividual(:c))\n\
        ClassAssertion(ObjectMaxCardinality(1 :r owl:Thing) :a)\n\
        ObjectPropertyAssertion(:r :a :b)\n\
        ObjectPropertyAssertion(:r :a :c)\n\
        DifferentIndividuals(:b :c)\n";
    assert!(
        !consistent(body),
        "≤1 r with two distinct r-successors should be inconsistent"
    );
}

// ─────────────── CONSISTENT (catastrophic-guard: must NOT flip) ───────

#[test]
fn plain_abox_stays_consistent() {
    let body = "\
        Declaration(Class(:Person))\n\
        Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
        ClassAssertion(:Person :a)\n\
        ClassAssertion(:Person :b)\n";
    assert!(
        consistent(body),
        "a plain satisfiable ABox stays consistent"
    );
}

#[test]
fn non_disjoint_two_types_stays_consistent() {
    let body = "\
        Declaration(Class(:C)) Declaration(Class(:D))\n\
        Declaration(NamedIndividual(:a))\n\
        ClassAssertion(:C :a)\n\
        ClassAssertion(:D :a)\n";
    assert!(
        consistent(body),
        "two NON-disjoint types on one individual stays consistent"
    );
}

#[test]
fn same_individual_compatible_stays_consistent() {
    // SameIndividual of two individuals with compatible (non-disjoint)
    // types must stay consistent — no spurious merge clash.
    let body = "\
        Declaration(Class(:C)) Declaration(Class(:D))\n\
        Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
        ClassAssertion(:C :a)\n\
        ClassAssertion(:D :b)\n\
        SameIndividual(:a :b)\n";
    assert!(
        consistent(body),
        "SameIndividual of compatible individuals stays consistent"
    );
}

#[test]
fn typed_individual_with_edges_stays_consistent() {
    let body = "\
        Declaration(Class(:Person)) Declaration(ObjectProperty(:knows))\n\
        Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
        ClassAssertion(:Person :a)\n\
        ClassAssertion(:Person :b)\n\
        ObjectPropertyAssertion(:knows :a :b)\n\
        DifferentIndividuals(:a :b)\n";
    assert!(
        consistent(body),
        "typed individuals with asserted edges + distinctness stay consistent"
    );
}

#[test]
fn complex_under_seeded_class_assertion_stays_consistent() {
    // A complex ClassAssertion (∃r.C ⊓ D) that is actually satisfiable:
    // proves the `{a}⊑(complex)` encoding does not false-fire. ≤1 r is
    // satisfiable with exactly one r-successor.
    let body = "\
        Declaration(Class(:C)) Declaration(Class(:D)) Declaration(ObjectProperty(:r))\n\
        Declaration(NamedIndividual(:a))\n\
        ClassAssertion(\
            ObjectIntersectionOf(ObjectSomeValuesFrom(:r :C) ObjectMaxCardinality(1 :r owl:Thing)) \
            :a)\n";
    assert!(
        consistent(body),
        "a satisfiable complex ClassAssertion must NOT false-fire"
    );
}

#[test]
fn max_cardinality_one_successor_stays_consistent() {
    // ≤1 r with a SINGLE r-successor is satisfiable (no two distinct
    // successors) — guards against an over-eager cardinality clash.
    let body = "\
        Declaration(ObjectProperty(:r))\n\
        Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
        ClassAssertion(ObjectMaxCardinality(1 :r owl:Thing) :a)\n\
        ObjectPropertyAssertion(:r :a :b)\n";
    assert!(consistent(body), "≤1 r with one successor stays consistent");
}

// ──── wine: the false-`Unsat` regression guard (NN-merge backjump hole) ────

#[test]
#[ignore = "needs ontologies/real/wine.ofn; ~seconds. Catastrophic-guard \
            regression: the ABox-seeded wedge with nominals false-`Unsat`ed \
            wine (consistent per Konclude/HermiT, 764 SubClassOf, 0 Nothing) \
            via an NN-merge backjump-dep hole. The `nn_tainted` fix makes the \
            clash report `DepSet::ALL`. This test pins it WONT-regress."]
fn wine_stays_consistent() {
    let path = Path::new("../../ontologies/real/wine.ofn");
    let alt = Path::new("ontologies/real/wine.ofn");
    let p = if path.exists() {
        path
    } else if alt.exists() {
        alt
    } else {
        eprintln!("SKIP: wine.ofn fixture missing");
        return;
    };
    let src = std::fs::read_to_string(p).expect("read wine.ofn");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse wine.ofn");
    assert!(
        is_consistent(&onto).expect("is_consistent"),
        "wine is consistent (Konclude/HermiT); a false-Unsat = catastrophic"
    );
}

// ───────────── previously-hanging out-of-EL ABox now terminates ───────

#[test]
fn out_of_el_abox_terminates_quickly() {
    // An out-of-EL (∀ + disjunction + cardinality) ABox that is
    // CONSISTENT but whose `decide(Top)` main-tableau path could spin.
    // The wedge route must return a verdict within a few seconds.
    let body = "\
        Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
        Declaration(ObjectProperty(:r))\n\
        Declaration(NamedIndividual(:x))\n\
        SubClassOf(:A ObjectUnionOf(:B :C))\n\
        SubClassOf(:A ObjectAllValuesFrom(:r :B))\n\
        SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :A))\n\
        ClassAssertion(:A :x)\n\
        ObjectPropertyAssertion(:r :x :x)\n";
    let start = Instant::now();
    let verdict = consistent(body);
    let elapsed = start.elapsed();
    assert!(verdict, "this out-of-EL ABox is consistent");
    assert!(
        elapsed < Duration::from_secs(20),
        "wedge consistency must terminate quickly, took {elapsed:?}"
    );
}
