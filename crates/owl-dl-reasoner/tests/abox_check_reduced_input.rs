//! Canaries for the reduced-input `abox_check` change.
//!
//! The change passes `abox_check` the same eight values it reads today, so every
//! verdict must be identical. These tests pin that from the outside, via the
//! classification the verdict drives: an ABox-inconsistent ontology classifies
//! every class unsatisfiable; a consistent one does not.
//!
//! Run: `cargo test -p owl-dl-reasoner --test abox_check_reduced_input`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn unsat_count(body: &str) -> usize {
    let c = owl_dl_reasoner::classify(&parse(body)).expect("classify");
    c.unsatisfiable_classes().len()
}

/// P2-shaped ABox clash (disjoint types on one individual) on an ontology that
/// takes the FAST path — the only path this change touches. Every class must be
/// reported unsatisfiable, which is `classify_inconsistent`'s behaviour and is
/// reachable only if the ABox verdict still fires.
#[test]
fn fastpath_abox_clash_still_marks_all_classes_unsat() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(NamedIndividual(:i))
    DisjointClasses(:A :B)
    ClassAssertion(:A :i)
    ClassAssertion(:B :i)
    SubClassOf(:C :A)";
    assert!(
        unsat_count(body) >= 3,
        "an ABox clash must make every class unsatisfiable on the fast path"
    );
}

/// NEGATIVE control: a consistent ABox on the fast path must NOT mark anything
/// unsatisfiable. Guards against the change turning the verdict into a blanket
/// `Inconsistent`.
#[test]
fn fastpath_consistent_abox_marks_nothing_unsat() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(NamedIndividual(:i))
    ClassAssertion(:A :i)
    SubClassOf(:A :B)";
    assert_eq!(
        unsat_count(body), 0,
        "a consistent ABox must not make any class unsatisfiable"
    );
}

/// ABox-free fast-path ontology: entirely inert — `has_abox_axioms` short-circuits
/// before any of this code runs.
#[test]
fn fastpath_no_abox_is_inert() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    SubClassOf(:A :B)";
    assert_eq!(unsat_count(body), 0);
}
