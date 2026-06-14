//! Canaries for the P3 concrete-domain tableau clash (integer, string, float,
//! decimal, date, dateTime buckets): a node whose data constraints (`∃p.R` /
//! `≥n p.R` / `≤m p.S` / `∀p.U`) are jointly unsatisfiable by `card_sat`
//! becomes a clash, making the class unsatisfiable.
//!
//! NEGATIVES-FIRST: the FP-critical direction is a false clash on a SATISFIABLE
//! node → a spurious subsumption. Every `assert!(sat(...))` below is a
//! genuinely-satisfiable data node (one per lowering path: DataSome, qualified
//! DataMin/DataMax/DataExact, ∀+∃) that MUST stay SAT. The `assert!(!sat(...))`
//! cases verify the clash actually fires (utility): capacity (more distinct
//! values demanded than exist) and ≥n-vs-≤m conflict.
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

// ─── STRING BUCKET: capacity clash ────────────────────────────────────

/// String capacity: `≥3 p.{"a","b"}` demands 3 distinct strings but only 2
/// exist in the enumeration. UNSAT. (Previously this was a "not-yet-handled"
/// SAT canary; strings are now wired into the concrete-domain solver.)
#[test]
fn string_capacity_clash_unsat() {
    assert!(!sat(
        "  SubClassOf(:C DataMinCardinality(3 :p DataOneOf(\"a\" \"b\")))"
    ));
}

/// Exactly enough strings: `≥2 p.{"a","b"}` — 2 demanded, 2 available. SAT.
/// FP GUARD: must NOT clash.
#[test]
fn string_exactly_enough_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"a\" \"b\")))"
    ));
}

/// String `∃p.{"a","b"}` (DataSomeValuesFrom, ≥1). No cardinality constraint. SAT.
/// FP GUARD: must NOT clash.
#[test]
fn string_datasome_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataSomeValuesFrom(:p DataOneOf(\"a\" \"b\")))"
    ));
}

/// `≥1000 p.xsd:string` — bare string = Top = ∞ capacity. SAT.
/// FP GUARD: must NOT clash even with a very large demand.
#[test]
fn string_top_large_demand_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataMinCardinality(1000 :p xsd:string))"
    ));
}

/// `≥2 p.{"a","b"}` with `≤1 p.{"a","b"}` — min/max conflict on same set. UNSAT.
#[test]
fn string_min_max_conflict_unsat() {
    assert!(!sat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"a\" \"b\")))\n\
         \x20 SubClassOf(:C DataMaxCardinality(1 :p DataOneOf(\"a\" \"b\")))"
    ));
}

/// Plain class (no data cardinality). SAT. FP GUARD.
#[test]
fn no_data_cardinality_is_sat() {
    assert!(sat(""));
}

// ─── DENSE BUCKET CANARIES ────────────────────────────────────────────
//
// For each dense type (float, decimal, date, dateTime):
//   CLASH: `≥2 p.[v,v]` (single inclusive point, capacity 1 < 2). UNSAT.
//   SAT guards:
//     - `≥1 p.[v,v]` (1 fits in capacity 1). SAT.
//     - `≥1000 p.[0,100]` (real interval, ∞ capacity). SAT.
//     - `∃p.range` (DataSome, no cardinality). SAT.
//     - `≥2 p.(lo,hi)` (exclusive bounds, capacity ∞). SAT.

// ── FLOAT ─────────────────────────────────────────────────────────────

/// CLASH: `≥2 p.{1.5}` (single inclusive point, capacity 1). UNSAT.
#[test]
fn float_point_capacity_clash_unsat() {
    assert!(!sat("  SubClassOf(:C DataMinCardinality(2 :p \
         DatatypeRestriction(xsd:float xsd:minInclusive \"1.5\"^^xsd:float \
         xsd:maxInclusive \"1.5\"^^xsd:float)))"));
}

/// FP GUARD: `≥1 p.{1.5}` — 1 value fits in a point. SAT.
#[test]
fn float_point_ge1_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(1 :p \
         DatatypeRestriction(xsd:float xsd:minInclusive \"1.5\"^^xsd:float \
         xsd:maxInclusive \"1.5\"^^xsd:float)))"));
}

/// FP GUARD: `≥1000 p.[0.0,100.0]` — dense interval, ∞ capacity. SAT.
#[test]
fn float_interval_large_demand_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(1000 :p \
         DatatypeRestriction(xsd:float xsd:minInclusive \"0.0\"^^xsd:float \
         xsd:maxInclusive \"100.0\"^^xsd:float)))"));
}

/// FP GUARD: `∃p.[0.0,1.0]` (DataSome). No cardinality count. SAT.
#[test]
fn float_datasome_is_sat() {
    assert!(sat("  SubClassOf(:C DataSomeValuesFrom(:p \
         DatatypeRestriction(xsd:float xsd:minInclusive \"0.0\"^^xsd:float \
         xsd:maxInclusive \"1.0\"^^xsd:float)))"));
}

/// FP GUARD: `≥2 p.(0.0,1.0)` — exclusive bounds, capacity ∞. SAT.
#[test]
fn float_exclusive_bounds_large_demand_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(2 :p \
         DatatypeRestriction(xsd:float xsd:minExclusive \"0.0\"^^xsd:float \
         xsd:maxExclusive \"1.0\"^^xsd:float)))"));
}

/// FP GUARD (signed-zero landmine): `≤1 p.[-1,1]` + `≥1 p.[-1,-0.0]` +
/// `≥1 p.[0.0,1]`. The two demands share `0.0` (`-0.0 == +0.0` in IEEE),
/// so a single filler satisfies both under the `≤1` limit ⟹ SAT. Without
/// `OrdF64::new`'s signed-zero normalization, `total_cmp` orders
/// `-0.0 < +0.0`, the demands look disjoint, and `1+1 > 1` fires a spurious
/// false-unsat = FP. End-to-end regression for that exact bug.
#[test]
fn float_signed_zero_split_demands_is_sat() {
    assert!(sat("  SubClassOf(:C DataMaxCardinality(1 :p \
           DatatypeRestriction(xsd:float xsd:minInclusive \"-1.0\"^^xsd:float \
           xsd:maxInclusive \"1.0\"^^xsd:float)))\n\
         SubClassOf(:C DataMinCardinality(1 :p \
           DatatypeRestriction(xsd:float xsd:minInclusive \"-1.0\"^^xsd:float \
           xsd:maxInclusive \"-0.0\"^^xsd:float)))\n\
         SubClassOf(:C DataMinCardinality(1 :p \
           DatatypeRestriction(xsd:float xsd:minInclusive \"0.0\"^^xsd:float \
           xsd:maxInclusive \"1.0\"^^xsd:float)))"));
}

// ── DECIMAL ───────────────────────────────────────────────────────────

/// CLASH: `≥2 p.{1.5}` decimal point. UNSAT.
#[test]
fn decimal_point_capacity_clash_unsat() {
    assert!(!sat("  SubClassOf(:C DataMinCardinality(2 :p \
         DatatypeRestriction(xsd:decimal xsd:minInclusive \"1.5\"^^xsd:decimal \
         xsd:maxInclusive \"1.5\"^^xsd:decimal)))"));
}

/// FP GUARD: `≥1 p.{1.5}` decimal — 1 value fits. SAT.
#[test]
fn decimal_point_ge1_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(1 :p \
         DatatypeRestriction(xsd:decimal xsd:minInclusive \"1.5\"^^xsd:decimal \
         xsd:maxInclusive \"1.5\"^^xsd:decimal)))"));
}

/// FP GUARD: `≥1000 p.[0,100]` decimal — dense interval, ∞ capacity. SAT.
#[test]
fn decimal_interval_large_demand_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(1000 :p \
         DatatypeRestriction(xsd:decimal xsd:minInclusive \"0\"^^xsd:decimal \
         xsd:maxInclusive \"100\"^^xsd:decimal)))"));
}

/// FP GUARD: `≥2 p.(0,1)` decimal exclusive bounds, capacity ∞. SAT.
#[test]
fn decimal_exclusive_bounds_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(2 :p \
         DatatypeRestriction(xsd:decimal xsd:minExclusive \"0\"^^xsd:decimal \
         xsd:maxExclusive \"1\"^^xsd:decimal)))"));
}

// ── DATE ──────────────────────────────────────────────────────────────

/// CLASH: `≥2 p.{2020-01-01}` single date point. UNSAT.
#[test]
fn date_point_capacity_clash_unsat() {
    assert!(!sat("  SubClassOf(:C DataMinCardinality(2 :p \
         DatatypeRestriction(xsd:date xsd:minInclusive \"2020-01-01\"^^xsd:date \
         xsd:maxInclusive \"2020-01-01\"^^xsd:date)))"));
}

/// FP GUARD: `≥1 p.{2020-01-01}` — 1 value fits. SAT.
#[test]
fn date_point_ge1_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(1 :p \
         DatatypeRestriction(xsd:date xsd:minInclusive \"2020-01-01\"^^xsd:date \
         xsd:maxInclusive \"2020-01-01\"^^xsd:date)))"));
}

/// FP GUARD: `≥1000 p.[2020-01-01,2021-12-31]` — dense range, ∞ capacity. SAT.
#[test]
fn date_interval_large_demand_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(1000 :p \
         DatatypeRestriction(xsd:date xsd:minInclusive \"2020-01-01\"^^xsd:date \
         xsd:maxInclusive \"2021-12-31\"^^xsd:date)))"));
}

/// FP GUARD: `∃p.xsd:date` (DataSome bare date). No count. SAT.
#[test]
fn date_datasome_is_sat() {
    assert!(sat("  SubClassOf(:C DataSomeValuesFrom(:p xsd:date))"));
}

// ── DATETIME ──────────────────────────────────────────────────────────

/// CLASH: `≥2 p.{2020-01-01T00:00:00}` single datetime point. UNSAT.
#[test]
fn datetime_point_capacity_clash_unsat() {
    assert!(!sat("  SubClassOf(:C DataMinCardinality(2 :p \
         DatatypeRestriction(xsd:dateTime xsd:minInclusive \
         \"2020-01-01T00:00:00\"^^xsd:dateTime \
         xsd:maxInclusive \"2020-01-01T00:00:00\"^^xsd:dateTime)))"));
}

/// FP GUARD: `≥1 p.{2020-01-01T00:00:00}` — 1 value fits. SAT.
#[test]
fn datetime_point_ge1_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(1 :p \
         DatatypeRestriction(xsd:dateTime xsd:minInclusive \
         \"2020-01-01T00:00:00\"^^xsd:dateTime \
         xsd:maxInclusive \"2020-01-01T00:00:00\"^^xsd:dateTime)))"));
}

/// FP GUARD: `≥1000 p.[2020-01-01T00:00:00,2021-01-01T00:00:00]` — dense range, ∞. SAT.
#[test]
fn datetime_interval_large_demand_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(1000 :p \
         DatatypeRestriction(xsd:dateTime xsd:minInclusive \
         \"2020-01-01T00:00:00\"^^xsd:dateTime \
         xsd:maxInclusive \"2021-01-01T00:00:00\"^^xsd:dateTime)))"));
}

/// FP GUARD: `∃p.xsd:dateTime` (DataSome bare dateTime). No count. SAT.
#[test]
fn datetime_datasome_is_sat() {
    assert!(sat("  SubClassOf(:C DataSomeValuesFrom(:p xsd:dateTime))"));
}

// ─── NUMERIC DATEONEOF BUCKETS ────────────────────────────────────────
//
// For each of int/float/decimal/date/dateTime DataOneOf:
//   CLASH:   `≥3 p.DataOneOf(v1 v2)` (capacity 2 < 3). UNSAT.
//   SAT:     `≥2 p.DataOneOf(v1 v2)` (exactly enough). SAT. (FP GUARD)
//   SAT:     `∃p.DataOneOf(v1 v2)` (DataSome, no count). SAT. (FP GUARD)
//   FP GUARD: special dedup probes (signed-zero, decimal normalization, etc.)
//   CROSS-DATATYPE DROP: mixed-type DataOneOf → drops, SAT. (FP GUARD)

// ── INTEGER ONEOF ─────────────────────────────────────────────────────

/// CLASH: `≥3 p.DataOneOf(1 2)` — capacity 2 < 3. UNSAT.
#[test]
fn int_oneof_capacity_clash_unsat() {
    assert!(!sat(
        "  SubClassOf(:C DataMinCardinality(3 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))"
    ));
}

/// FP GUARD: `≥2 p.DataOneOf(1 2)` — exactly enough (capacity 2 ≥ 2). SAT.
#[test]
fn int_oneof_exactly_enough_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))"
    ));
}

/// FP GUARD: `∃p.DataOneOf(1 2)` (DataSome). No cardinality count. SAT.
#[test]
fn int_oneof_datasome_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataSomeValuesFrom(:p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))"
    ));
}

/// FP GUARD: `≥1000 p.DataOneOf(1 2)` should be FAST UNSAT (capacity 2 < 1M).
#[test]
fn int_oneof_large_demand_fast_unsat() {
    assert!(!sat(
        "  SubClassOf(:C DataMinCardinality(1000000 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))"
    ));
}

/// FP GUARD: two DISTINCT integer values give capacity 2 (no over-conflation). SAT.
#[test]
fn int_oneof_distinct_values_no_overconflation_sat() {
    assert!(sat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"42\"^^xsd:integer \"43\"^^xsd:integer)))"
    ));
}

// ── FLOAT ONEOF ───────────────────────────────────────────────────────

/// CLASH: `≥3 p.DataOneOf(1.5 2.5)` — capacity 2 < 3. UNSAT.
#[test]
fn float_oneof_capacity_clash_unsat() {
    assert!(!sat(
        "  SubClassOf(:C DataMinCardinality(3 :p DataOneOf(\"1.5\"^^xsd:float \"2.5\"^^xsd:float)))"
    ));
}

/// FP GUARD: `≥2 p.DataOneOf(1.5 2.5)` — exactly enough. SAT.
#[test]
fn float_oneof_exactly_enough_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"1.5\"^^xsd:float \"2.5\"^^xsd:float)))"
    ));
}

/// FP GUARD: `∃p.DataOneOf(1.5 2.5)` (DataSome). No count. SAT.
#[test]
fn float_oneof_datasome_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataSomeValuesFrom(:p DataOneOf(\"1.5\"^^xsd:float \"2.5\"^^xsd:float)))"
    ));
}

/// FP GUARD (signed-zero dedup): `≥2 p.DataOneOf(-0.0 +0.0)` must be UNSAT
/// because -0.0 == +0.0 in IEEE-754 → capacity 1, not 2.
/// Without OrdF64::new's signed-zero normalization this would
/// be wrongly SAT (two distinct bit patterns → capacity 2 → no clash → FP).
#[test]
fn float_oneof_signed_zero_dedup_unsat() {
    assert!(!sat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"-0.0\"^^xsd:float \"0.0\"^^xsd:float)))"
    ));
}

// ── DECIMAL ONEOF ─────────────────────────────────────────────────────

/// CLASH: `≥3 p.DataOneOf(1.5 2.5)` decimal — capacity 2 < 3. UNSAT.
#[test]
fn decimal_oneof_capacity_clash_unsat() {
    assert!(!sat(
        "  SubClassOf(:C DataMinCardinality(3 :p DataOneOf(\"1.5\"^^xsd:decimal \"2.5\"^^xsd:decimal)))"
    ));
}

/// FP GUARD: `≥2 p.DataOneOf(1.5 2.5)` decimal — exactly enough. SAT.
#[test]
fn decimal_oneof_exactly_enough_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"1.5\"^^xsd:decimal \"2.5\"^^xsd:decimal)))"
    ));
}

/// FP GUARD (decimal normalization): `≥2 p.DataOneOf(1.5 1.50)` must be UNSAT
/// because "1.5" and "1.50" are the same decimal value → capacity 1, not 2.
#[test]
fn decimal_oneof_normalized_dedup_unsat() {
    assert!(!sat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"1.5\"^^xsd:decimal \"1.50\"^^xsd:decimal)))"
    ));
}

/// FP GUARD: two truly distinct decimal values give capacity 2 (no over-conflation). SAT.
#[test]
fn decimal_oneof_distinct_values_no_overconflation_sat() {
    assert!(sat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"1.5\"^^xsd:decimal \"1.6\"^^xsd:decimal)))"
    ));
}

// ── DATE ONEOF ────────────────────────────────────────────────────────

/// CLASH: `≥3 p.DataOneOf(2020-01-01 2020-01-02)` — capacity 2 < 3. UNSAT.
#[test]
fn date_oneof_capacity_clash_unsat() {
    assert!(!sat(
        "  SubClassOf(:C DataMinCardinality(3 :p DataOneOf(\"2020-01-01\"^^xsd:date \"2020-01-02\"^^xsd:date)))"
    ));
}

/// FP GUARD: `≥2 p.DataOneOf(2020-01-01 2020-01-02)` — exactly enough. SAT.
#[test]
fn date_oneof_exactly_enough_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"2020-01-01\"^^xsd:date \"2020-01-02\"^^xsd:date)))"
    ));
}

/// FP GUARD: `∃p.DataOneOf(2020-01-01 2020-01-02)` (DataSome). No count. SAT.
#[test]
fn date_oneof_datasome_is_sat() {
    assert!(sat(
        "  SubClassOf(:C DataSomeValuesFrom(:p DataOneOf(\"2020-01-01\"^^xsd:date \"2020-01-02\"^^xsd:date)))"
    ));
}

// ── DATETIME ONEOF ────────────────────────────────────────────────────

/// CLASH: `≥3 p.DataOneOf(2020-01-01T00:00:00 2020-01-01T01:00:00)` — capacity 2 < 3.
#[test]
fn datetime_oneof_capacity_clash_unsat() {
    assert!(!sat("  SubClassOf(:C DataMinCardinality(3 :p DataOneOf(\
         \"2020-01-01T00:00:00\"^^xsd:dateTime \
         \"2020-01-01T01:00:00\"^^xsd:dateTime)))"));
}

/// FP GUARD: `≥2 p.DataOneOf(2020-01-01T00:00:00 2020-01-01T01:00:00)` — exactly enough.
#[test]
fn datetime_oneof_exactly_enough_is_sat() {
    assert!(sat("  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\
         \"2020-01-01T00:00:00\"^^xsd:dateTime \
         \"2020-01-01T01:00:00\"^^xsd:dateTime)))"));
}

/// FP GUARD: `∃p.DataOneOf(...)` dateTime (DataSome). No count. SAT.
#[test]
fn datetime_oneof_datasome_is_sat() {
    assert!(sat("  SubClassOf(:C DataSomeValuesFrom(:p DataOneOf(\
         \"2020-01-01T00:00:00\"^^xsd:dateTime \
         \"2020-01-01T01:00:00\"^^xsd:dateTime)))"));
}

// ── CROSS-DATATYPE DROP ────────────────────────────────────────────────

/// FP GUARD: mixed-type DataOneOf (integer + float) → DROPS entire range → SAT.
/// A `≥5` demand over a 2-member mixed set should NOT clash (range dropped,
/// so no counting constraint at all).
#[test]
fn cross_datatype_oneof_drops_sat() {
    assert!(sat(
        "  SubClassOf(:C DataMinCardinality(5 :p DataOneOf(\"1\"^^xsd:integer \"2.5\"^^xsd:float)))"
    ));
}
