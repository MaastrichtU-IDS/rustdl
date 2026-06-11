//! Canaries for the P3 concrete-domain tableau clash (integer bucket): a node
//! whose integer data constraints (`∃p.R` / `≥n p.R` / `≤m p.S` / `∀p.U`) are
//! jointly unsatisfiable by `card_sat` becomes a clash, making the class
//! unsatisfiable.
//!
//! NEGATIVES-FIRST: the FP-critical direction is a false clash on a SATISFIABLE
//! node → a spurious subsumption. Every `assert!(sat(...))` below is a
//! genuinely-satisfiable data node (one per lowering path: DataSome, qualified
//! DataMin/DataMax/DataExact, ∀+∃) that MUST stay SAT. The `assert!(!sat(...))`
//! cases verify the clash actually fires (utility): capacity (more distinct
//! integers demanded than exist) and ≥n-vs-≤m conflict.
//!
//! `is_class_satisfiable` runs the main tableau (not the classify wedge), so the
//! clash is exercised directly. Run:
//! `cargo test -p owl-dl-reasoner --test concrete_domain_clash`.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_class_satisfiable;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";

/// Is `:C` satisfiable given `body` (the ontology axioms)?
fn sat(body: &str) -> bool {
    let src = format!(
        "{PFX}Ontology(<http://t/o>\n  Declaration(Class(:C)) Declaration(DataProperty(:p))\n{body}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    is_class_satisfiable(&onto, "http://t/C").expect("is_class_satisfiable")
}

/// `≥n p` over `xsd:integer` in `[lo,hi]`.
fn min_int(n: u32, lo: i64, hi: i64) -> String {
    format!(
        "  SubClassOf(:C DataMinCardinality({n} :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"{lo}\"^^xsd:integer xsd:maxInclusive \"{hi}\"^^xsd:integer)))"
    )
}
fn max_int(n: u32, lo: i64, hi: i64) -> String {
    format!(
        "  SubClassOf(:C DataMaxCardinality({n} :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"{lo}\"^^xsd:integer xsd:maxInclusive \"{hi}\"^^xsd:integer)))"
    )
}

// ─── UTILITY: the clash must fire (class unsatisfiable) ──────────────

/// Capacity: `≥3 p.[0,1]` demands 3 distinct integers but only 2 exist. UNSAT.
#[test]
fn capacity_clash_makes_class_unsat() {
    assert!(!sat(&min_int(3, 0, 1)));
}

/// Conflict: `≥3 p.[0,100]` with `≤2 p.[0,100]`. UNSAT.
#[test]
fn min_max_conflict_makes_class_unsat() {
    assert!(!sat(&format!(
        "{}\n{}",
        min_int(3, 0, 100),
        max_int(2, 0, 100)
    )));
}

/// `≥2 p.[0,1]` with `≤1 p.[0,1]` — 2 demanded, 1 allowed. UNSAT.
#[test]
fn exact_over_subset_conflict_unsat() {
    assert!(!sat(&format!("{}\n{}", min_int(2, 0, 1), max_int(1, 0, 1))));
}

// ─── FP GATE: satisfiable data nodes MUST stay SAT ───────────────────

/// DataSome path: `∃p.[0,10]` (≥1, 11 ints). SAT.
#[test]
fn datasome_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))"
    ));
}

/// Tight-but-feasible capacity: `≥2 p.[0,1]` — exactly 2 ints. SAT.
#[test]
fn exactly_enough_integers_is_sat() {
    assert!(sat(&min_int(2, 0, 1)));
}

/// DataMax alone is always feasible (pick ≤n values). SAT.
#[test]
fn datamax_alone_is_sat() {
    assert!(sat(&max_int(1, 0, 10)));
}

/// `≥2 p.[0,10]` with `≤5 p.[0,10]` — room to spare. SAT.
#[test]
fn min_under_max_is_sat() {
    assert!(sat(&format!(
        "{}\n{}",
        min_int(2, 0, 10),
        max_int(5, 0, 10)
    )));
}

/// Exact `=2 p.[0,10]` (≥2 ⊓ ≤2, 11 ints). SAT.
#[test]
fn exact_cardinality_feasible_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataExactCardinality(2 :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))"
    ));
}

/// `∀p.[5,20]` ⊓ `∃p.[0,10]` — the existential value can sit in [5,10]. SAT.
/// (Guards against a false clash from over-tightening via the ∀ filter.)
#[test]
fn forall_compatible_with_exists_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataAllValuesFrom(:p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"5\"^^xsd:integer xsd:maxInclusive \"20\"^^xsd:integer)))\n\
         \x20 SubClassOf(:C DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))"
    ));
}

/// Non-integer-bucket cardinality is NOT handled (dropped) — must NOT clash
/// (sound under-approximation). `≥3 p.{a,b}` (string oneOf) stays SAT.
#[test]
fn noninteger_cardinality_not_clashed() {
    assert!(sat(
        "  SubClassOf(:C DataMinCardinality(3 :p DataOneOf(\"a\" \"b\")))"
    ));
}
