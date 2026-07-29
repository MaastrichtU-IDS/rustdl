//! Canaries for `X ⊓ Y ⊑ ⊥` (the lowered-`⊥` disjointness GCI) in the EL saturator.
//!
//! Lever 1b (commit 3e3a731) admitted this form to the fragment gate, but the
//! saturator's rule collector derived heads only from an atomic or existential
//! RHS — with `sup = Bot` both are empty, so the axiom was SILENTLY DROPPED while
//! the gate certified the closure complete (the D10 unsound-completeness class).
//!
//! Run: `cargo test -p owl-dl-reasoner --test conjunctive_unsat`

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

/// Classify `body` and return the sorted list of unsatisfiable class IRIs.
fn unsat_of(body: &str) -> Vec<String> {
    let onto = parse(body);
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    let mut v: Vec<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();
    v.sort();
    v
}

const DECLS: &str = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
";

/// THE BUG REPRODUCER. `C ⊑ A`, `C ⊑ B`, `A ⊓ B ⊑ ⊥` ⟹ `C` unsatisfiable.
/// Before the fix this returns an EMPTY unsat set while printing
/// "pure-EL — saturator alone is complete".
#[test]
fn conjunctive_bot_derives_unsat() {
    let body = format!(
        "{DECLS}    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    assert_eq!(
        unsat_of(&body),
        vec!["http://t/C".to_string()],
        "C ⊑ A, C ⊑ B, A ⊓ B ⊑ ⊥ entails C ⊑ ⊥"
    );
}

/// SPELLING DIFFERENTIAL — the direct gate for the bug. The same ontology
/// written `A ⊓ B ⊑ ⊥` and `DisjointClasses(A B)` must classify identically.
#[test]
fn conjunctive_bot_matches_disjoint_classes_spelling() {
    let and_bot = format!(
        "{DECLS}    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    let disjoint = format!(
        "{DECLS}    DisjointClasses(:A :B)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    assert_eq!(
        unsat_of(&and_bot),
        unsat_of(&disjoint),
        "the two spellings of disjointness must produce the same closure"
    );
}

/// FP GUARD (negatives-first). A class with only ONE of the two conjuncts must
/// stay satisfiable. Guards against a rule that fires on a partial body match.
#[test]
fn conjunctive_bot_does_not_over_fire() {
    let body = format!(
        "{DECLS}    Declaration(Class(:D))
    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)
    SubClassOf(:D :A)"
    );
    assert_eq!(
        unsat_of(&body),
        vec!["http://t/C".to_string()],
        "D has only A, so D must remain satisfiable"
    );
}
