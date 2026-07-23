//! Correctness gate for the Phase-2 disjoint-clash indexing
//! (perf/abox-disjoint-index): the type-driven Rule 8 (`disjoint_of` adjacency)
//! must detect exactly the same clashes as the old pair×individual scan.
//!
//! Rules 7b/8 write only `result.clash` (no types/edges), so the observable is
//! the consistency verdict. These are deterministic verdict checks; the
//! indexed-vs-brute A/B across family + the 79 ORE `ABox` onts is the broad net.
//! (Rule 7b — the functional existential-marker clash — is exercised by
//! `family.ofn`'s `∃hasSex.Male ⊓ ∃hasSex.Female` inconsistency, guarded by the
//! issue-#35 tests + the A/B sweep.)

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::io::Cursor;

fn consistent_ofn(src: &str) -> bool {
    let mut reader = Cursor::new(src.to_owned());
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("verdict, not error")
}

#[test]
fn disjoint_pair_on_one_individual_is_inconsistent() {
    // x : A, x : B, DisjointClasses(A, B) — Rule 8 clash.
    assert!(!consistent_ofn(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))"
    ));
}

#[test]
fn disjoint_pair_across_distinct_individuals_is_consistent() {
    // x : A, y : B — no single individual carries both ⇒ no clash.
    assert!(consistent_ofn(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :y))"
    ));
}

#[test]
fn three_way_disjoint_partial_overlap_is_inconsistent() {
    // DisjointClasses(A,B,C) expands pairwise; x : A, x : C must clash — exercises
    // that the symmetric `disjoint_of` adjacency finds the (A,C) pair regardless
    // of which type the individual is iterated from first.
    assert!(!consistent_ofn(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
            Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B :C) ClassAssertion(:A :x) ClassAssertion(:C :x))"
    ));
}

#[test]
fn no_disjointness_is_consistent() {
    // Guard the empty-`disjoint_of` (advisor B1) path: an ABox with individuals
    // but no DisjointClasses must stay consistent (and the guarded Rule 8 does
    // no work).
    assert!(consistent_ofn(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            Declaration(NamedIndividual(:x))
            ClassAssertion(:A :x) ClassAssertion(:B :x))"
    ));
}
