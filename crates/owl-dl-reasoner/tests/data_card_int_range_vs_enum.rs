//! #334 — an integer RANGE and an integer ENUMERATION on one data property were
//! counted in separate `card_sat` buckets, so `≥2 p.xsd:integer` with `∀p.{1}`
//! came back satisfiable (nothing dropped) where `HermiT` says unsatisfiable.
//! `concrete_domain_clash` now also counts the two together over `IntUnion`, a
//! normalised union of integer intervals, for a property with items in both.
//! `DataHasValue` lowers to a range point, so `≤1 p.{1,2}` with two `DataHasValue`
//! witnesses is caught too.
//!
//! **Demands that meet a `≤m` are written as `DataSomeValuesFrom` / `DataHasValue`,
//! never `DataMinCardinality`,** so that a clash comes from this check and not from
//! the D4 `≥n`/`≤m` preprocessing pattern (#336).
//!
//! ORACLE: `HermiT` 1.4.5 agrees on every verdict below except the pinned misses.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify_top_down_with_timeout, dropped_axioms, is_class_satisfiable};
use std::io::Cursor;
use std::time::Duration;

const C: &str = "http://ex.org/C";

fn onto(body: &str) -> SetOntology<RcStr> {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:C)) Declaration(Class(:G))\n\
         Declaration(DataProperty(:p)) Declaration(DataProperty(:q))\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// `C`'s satisfiability, asserted to agree between `classify` and the per-class
/// probe, and with nothing dropped.
fn c_unsat(body: &str) -> bool {
    let o = onto(body);
    assert!(
        dropped_axioms(&o).expect("dropped").is_empty(),
        "nothing may drop: {body}"
    );
    let by_classify = classify_top_down_with_timeout(&o, Duration::from_secs(10))
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&C);
    let by_sat = !is_class_satisfiable(&o, C).expect("sat");
    assert_eq!(by_classify, by_sat, "classify and sat disagree on: {body}");
    by_sat
}

fn on_c(parts: &str) -> String {
    format!("SubClassOf(:C ObjectIntersectionOf({parts}))")
}

const ONE: &str = "DataOneOf(\"1\"^^xsd:integer)";
const ONE_TWO: &str = "DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)";
const INT_1_5: &str = "DatatypeRestriction(xsd:integer xsd:minInclusive \"1\"^^xsd:integer \
                       xsd:maxInclusive \"5\"^^xsd:integer)";

/// The issue: a range demand against an enumeration filter, in one axiom or two,
/// and through a data range.
#[test]
fn a_range_demand_meets_an_enumeration_filter() {
    assert!(c_unsat(&on_c(&format!(
        "DataMinCardinality(2 :p xsd:integer) DataAllValuesFrom(:p {ONE})"
    ))));
    assert!(c_unsat(&format!(
        "SubClassOf(:C DataMinCardinality(2 :p xsd:integer))\n\
         SubClassOf(:C DataAllValuesFrom(:p {ONE}))"
    )));
    assert!(c_unsat(&format!(
        "DataPropertyRange(:p {ONE})\n\
         SubClassOf(:C DataMinCardinality(2 :p xsd:integer))"
    )));
    // Derived types share the integer value space.
    assert!(c_unsat(&on_c(
        "DataMinCardinality(2 :p xsd:int) DataAllValuesFrom(:p DataOneOf(\"1\"^^xsd:short))"
    )));
    // Controls: two allowed values; a filter on another property.
    assert!(!c_unsat(&on_c(&format!(
        "DataMinCardinality(2 :p xsd:integer) DataAllValuesFrom(:p {ONE_TWO})"
    ))));
    assert!(!c_unsat(&on_c(&format!(
        "DataMinCardinality(2 :p xsd:integer) DataAllValuesFrom(:q {ONE})"
    ))));
}

/// The filter narrows a bounded range demand to the enumeration's members inside it.
#[test]
fn an_enumeration_filter_narrows_an_interval_demand() {
    let seven = "DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer \"7\"^^xsd:integer)";
    assert!(c_unsat(&on_c(&format!(
        "DataMinCardinality(3 :p {INT_1_5}) DataAllValuesFrom(:p {seven})"
    ))));
    assert!(!c_unsat(&on_c(&format!(
        "DataMinCardinality(2 :p {INT_1_5}) DataAllValuesFrom(:p {seven})"
    ))));
}

/// Enumeration demands against a range limit, and `DataHasValue` (a range point)
/// against an enumeration limit.
#[test]
fn enumeration_demands_meet_a_range_limit_and_vice_versa() {
    assert!(c_unsat(&on_c(
        "DataMaxCardinality(1 :p xsd:integer) \
         DataSomeValuesFrom(:p DataOneOf(\"1\"^^xsd:integer)) \
         DataSomeValuesFrom(:p DataOneOf(\"2\"^^xsd:integer))"
    )));
    assert!(!c_unsat(&on_c(&format!(
        "DataMaxCardinality(1 :p xsd:integer) \
         DataSomeValuesFrom(:p DataOneOf(\"1\"^^xsd:integer)) DataSomeValuesFrom(:p {ONE_TWO})"
    ))));
    assert!(c_unsat(&on_c(&format!(
        "DataMaxCardinality(1 :p {ONE_TWO}) \
         DataHasValue(:p \"1\"^^xsd:integer) DataHasValue(:p \"2\"^^xsd:integer)"
    ))));
    assert!(c_unsat(&on_c(&format!(
        "DataMaxCardinality(0 :p {ONE}) DataHasValue(:p \"1\"^^xsd:integer)"
    ))));
    assert!(!c_unsat(&on_c(&format!(
        "DataMaxCardinality(0 :p {ONE}) DataHasValue(:p \"2\"^^xsd:integer)"
    ))));
}

/// `[1,2] ⊆ {1,2}` holds only because adjacent points merge into one interval.
/// FP guards: `{1,5}` is not inside `[1,3]`, and `[2,6]` (which overlaps `{1,2}`)
/// is not inside it either.
#[test]
fn containment_follows_the_values() {
    let r12 = "DatatypeRestriction(xsd:integer xsd:minInclusive \"1\"^^xsd:integer \
               xsd:maxInclusive \"2\"^^xsd:integer)";
    assert!(c_unsat(&on_c(&format!(
        "DataSomeValuesFrom(:p {r12}) DataMaxCardinality(0 :p {ONE_TWO})"
    ))));
    let r13 = "DatatypeRestriction(xsd:integer xsd:minInclusive \"1\"^^xsd:integer \
               xsd:maxInclusive \"3\"^^xsd:integer)";
    assert!(!c_unsat(&on_c(&format!(
        "DataSomeValuesFrom(:p DataOneOf(\"1\"^^xsd:integer \"5\"^^xsd:integer)) \
         DataMaxCardinality(0 :p {r13})"
    ))));
    let r26 = "DatatypeRestriction(xsd:integer xsd:minInclusive \"2\"^^xsd:integer \
               xsd:maxInclusive \"6\"^^xsd:integer)";
    assert!(!c_unsat(&on_c(&format!(
        "DataMinCardinality(2 :p {r26}) DataMaxCardinality(1 :p {ONE_TWO})"
    ))));
}

/// Backjumping: `G ≡ ∀p.{1}` puts the filter on `C` only in one branch of the `⊒`
/// half; that branch clashes and the search must backjump to the other, which
/// needs the clash's deps to include the filter's.
#[test]
fn a_filter_reached_by_branching_does_not_clash() {
    assert!(!c_unsat(&format!(
        "EquivalentClasses(:G DataAllValuesFrom(:p {ONE}))\n\
         SubClassOf(:C DataMinCardinality(2 :p xsd:integer))"
    )));
}

/// KNOWN MISSES, sound. An enumeration mixing datatypes is dropped whole (visibly);
/// an integer value counted against an `xsd:decimal` limit is a different bucket
/// (integers are decimals: one-way containment, separate reasoning). `HermiT` says
/// unsat on both. FLIP when handled.
#[test]
fn mixed_and_cross_datatype_cases_are_known_misses() {
    let mixed = onto(&on_c(
        "DataMinCardinality(2 :p xsd:integer) \
         DataAllValuesFrom(:p DataOneOf(\"1\"^^xsd:integer \"a\"))",
    ));
    assert!(!dropped_axioms(&mixed).expect("dropped").is_empty());
    assert!(is_class_satisfiable(&mixed, C).expect("sat"));
    assert!(!c_unsat(&on_c(
        "DataMaxCardinality(1 :p xsd:decimal) \
         DataHasValue(:p \"1\"^^xsd:integer) DataHasValue(:p \"2\"^^xsd:integer)"
    )));
}
