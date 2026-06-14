//! Canaries for classify-level concrete-domain VERIFY: a class
//! unsatisfiable only by an integer or string counting clash (`≥3 p.[0,1]`
//! capacity, `≥3 ⊓ ≤2` conflict, `≥3 p.{"a","b"}` string capacity) must
//! appear unsatisfiable via `classify` — not just via `is_class_satisfiable`.
//! Before this feature, classify trusted the wedge's `Sat` (the wedge has no
//! `card_sat`) and missed these.
//!
//! NEGATIVES-FIRST: the FP-critical direction is a satisfiable class wrongly
//! reported unsatisfiable. Every `assert!(!c_unsat(...))` is a genuinely
//! satisfiable data node that MUST stay satisfiable.
//!
//! Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain`.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";

/// Classify `body` and return true iff `:C` (`http://t/C`) is unsatisfiable.
fn c_unsat(body: &str) -> bool {
    let src = format!(
        "{PFX}Ontology(<http://t/o>\n  Declaration(Class(:C)) Declaration(DataProperty(:p))\n{body}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    classify(&onto)
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&"http://t/C")
}

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

/// Capacity: `≥3 p.[0,1]` demands 3 distinct integers, only 2 exist. UNSAT.
#[test]
fn capacity_clash_unsat_via_classify() {
    assert!(c_unsat(&min_int(3, 0, 1)));
}

/// Conflict: `≥3 p.[0,100]` with `≤2 p.[0,100]`. UNSAT via classify.
#[test]
fn min_max_conflict_unsat_via_classify() {
    assert!(c_unsat(&format!(
        "{}\n{}",
        min_int(3, 0, 100),
        max_int(2, 0, 100)
    )));
}

/// Inheritance: `D` carries `≥3 p.[0,1]`, `C ⊑ D`. Both unsat via classify
/// (exercises the saturation-subsumer downward check in the probe).
#[test]
fn inherited_counting_clash_unsat_via_classify() {
    let src = format!(
        "{PFX}Ontology(<http://t/o>\n  \
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))\n  \
         SubClassOf(:D DataMinCardinality(3 :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"1\"^^xsd:integer)))\n  \
         SubClassOf(:C :D)\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    let unsat = classify(&onto).expect("classify");
    let unsat = unsat.unsatisfiable_classes();
    assert!(unsat.contains(&"http://t/D"), "D unsat; got {unsat:?}");
    assert!(
        unsat.contains(&"http://t/C"),
        "C (⊑ D) unsat; got {unsat:?}"
    );
}

// ─── FP GATE: satisfiable data nodes MUST stay satisfiable via classify ───

/// `∃p.[0,10]` (≥1, 11 ints). SAT.
#[test]
fn datasome_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))"
    ));
}

/// Tight-but-feasible: `≥2 p.[0,1]` — exactly 2 ints. SAT.
#[test]
fn exactly_enough_sat_via_classify() {
    assert!(!c_unsat(&min_int(2, 0, 1)));
}

/// `≥2 p.[0,10]` with `≤5 p.[0,10]` — room to spare. SAT.
#[test]
fn min_under_max_sat_via_classify() {
    assert!(!c_unsat(&format!(
        "{}\n{}",
        min_int(2, 0, 10),
        max_int(5, 0, 10)
    )));
}

/// `≤1 p.[0,10]` alone — always feasible. SAT.
#[test]
fn datamax_alone_sat_via_classify() {
    assert!(!c_unsat(&max_int(1, 0, 10)));
}

// ─── STRING BUCKET (classify level) ───────────────────────────────────────

/// String capacity clash via classify: `≥3 p.{"a","b"}` — 2-element
/// enumeration, 3 demanded. UNSAT. (Previously asserted SAT when strings were
/// unhandled; now strings are wired into the concrete-domain solver.)
#[test]
fn string_capacity_clash_unsat_via_classify() {
    assert!(c_unsat(
        "  SubClassOf(:C DataMinCardinality(3 :p DataOneOf(\"a\" \"b\")))"
    ));
}

/// Exactly enough strings: `≥2 p.{"a","b"}` — 2 demanded, 2 available. SAT.
/// FP GUARD: must NOT be reported unsatisfiable.
#[test]
fn string_exactly_enough_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"a\" \"b\")))"
    ));
}

/// String `∃p.{"a","b"}` (DataSomeValuesFrom). No cardinality constraint. SAT.
/// FP GUARD.
#[test]
fn string_datasome_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataSomeValuesFrom(:p DataOneOf(\"a\" \"b\")))"
    ));
}

/// `≥1000 p.xsd:string` — bare string = Top = ∞ capacity. SAT.
/// FP GUARD: large demand over an infinite domain must never clash.
#[test]
fn string_top_large_demand_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(1000 :p xsd:string))"
    ));
}

/// D11b probe (spec test gate): `∃p.{5} ⊓ ∀p.[0,3]`, 5 ∉ [0,3] ⟹ C unsat.
/// This is a *membership* clash (DKey disjointness), NOT counting — the
/// spec predicts the WEDGE already catches it in classify, so
/// `data_counting_classes` stays counting-only. If this FAILS, widen the
/// predicate to include ∀-over-DKey classes (see the spec).
#[test]
fn forall_exists_membership_clash_unsat_via_classify() {
    assert!(c_unsat(
        "  SubClassOf(:C DataHasValue(:p \"5\"^^xsd:integer))\n  \
         SubClassOf(:C DataAllValuesFrom(:p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"3\"^^xsd:integer)))"
    ));
}

/// Inheritance + feasible: `D` carries `≥2 p.[0,1]` (exactly 2 ints — SAT),
/// `C ⊑ D`. The override fires for both (D is counting-constrained, C
/// inherits via subsumers) but the main-tableau verify finds both SAT —
/// proving the inheritance trigger does not over-fire into an FP.
#[test]
fn inherited_feasible_counting_sat_via_classify() {
    let src = format!(
        "{PFX}Ontology(<http://t/o>\n  \
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))\n  \
         SubClassOf(:D DataMinCardinality(2 :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"1\"^^xsd:integer)))\n  \
         SubClassOf(:C :D)\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    let unsat = classify(&onto).expect("classify");
    let unsat = unsat.unsatisfiable_classes();
    assert!(
        !unsat.contains(&"http://t/D"),
        "D (≥2 over 2 ints) must be SAT; got {unsat:?}"
    );
    assert!(
        !unsat.contains(&"http://t/C"),
        "C (⊑ D) must be SAT; got {unsat:?}"
    );
}
