//! #325 — qualified data cardinality over `xsd:boolean` (and `xsd:anyURI`
//! enumerations) was dropped as an unsupported data range, so `≥3 p.xsd:boolean`
//! came back satisfiable.
//!
//! Both datatypes got `DKey` buckets in #321; this adds the cardinality arms in
//! conversion and teaches the tableau's concrete-domain check to count them, each
//! in its own `CardRange` variant. A boolean is never counted as a string, nor a
//! URI as either: the FP guards below put one of each under a single `≤1`.
//!
//! Every verdict below matches `HermiT` 1.4.5 on the same ontology.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify_top_down_with_timeout, dropped_axioms, is_class_satisfiable};
use std::io::Cursor;
use std::time::Duration;

const C: &str = "http://example.org/C";
const T: &str = "\"true\"^^xsd:boolean";
const F: &str = "\"false\"^^xsd:boolean";
const UA: &str = "\"http://a\"^^xsd:anyURI";
const UB: &str = "\"http://b\"^^xsd:anyURI";

fn onto(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://example.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://example.org/onto>\n\
         Declaration(DataProperty(:p))\n\
         Declaration(Class(:C))\n\
         {axioms}\n\
         )\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse ofn")
        .0
}

/// `C`'s satisfiability through both `classify` and `sat`, insisting nothing
/// was dropped and that the two surfaces agree.
fn c_satisfiable(axioms: &str) -> bool {
    let o = onto(axioms);
    let dropped = dropped_axioms(&o).unwrap();
    assert!(dropped.is_empty(), "axioms dropped: {dropped:?}");
    let classify_unsat = classify_top_down_with_timeout(&o, Duration::from_secs(10))
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&C);
    let sat = is_class_satisfiable(&o, C).unwrap();
    assert_eq!(!classify_unsat, sat, "classify and sat disagree: {axioms}");
    sat
}

// ── xsd:boolean ──────────────────────────────────────────────────────────

/// The issue: `xsd:boolean` has two values.
#[test]
fn at_least_three_booleans_is_unsat() {
    assert!(!c_satisfiable(
        "SubClassOf(:C DataMinCardinality(3 :p xsd:boolean))"
    ));
}

/// Control: two booleans fit.
#[test]
fn at_least_two_booleans_is_sat() {
    assert!(c_satisfiable(
        "SubClassOf(:C DataMinCardinality(2 :p xsd:boolean))"
    ));
}

#[test]
fn at_least_two_of_one_boolean_is_unsat() {
    assert!(!c_satisfiable(&format!(
        "SubClassOf(:C DataMinCardinality(2 :p DataOneOf({T})))"
    )));
}

/// `true` and `false` are two values, over a limit of one.
#[test]
fn at_most_one_boolean_with_both_values_is_unsat() {
    assert!(!c_satisfiable(&format!(
        "SubClassOf(:C ObjectIntersectionOf(DataMaxCardinality(1 :p xsd:boolean) \
         DataHasValue(:p {T}) DataHasValue(:p {F})))"
    )));
}

/// The same with `=1`. It used to stay satisfiable: `=1` adds a `≥1
/// p.{false,true}` demand, and `card_sat`'s packing took that broad demand
/// first, which blocked both `{true}` and `{false}`. Fixed by #326.
#[test]
fn exactly_one_boolean_with_both_values_is_unsat() {
    assert!(!c_satisfiable(&format!(
        "SubClassOf(:C ObjectIntersectionOf(DataExactCardinality(1 :p xsd:boolean) \
         DataHasValue(:p {T}) DataHasValue(:p {F})))"
    )));
}

/// At least two booleans, all of them `true`: only one value is left.
#[test]
fn at_least_two_booleans_all_true_is_unsat() {
    assert!(!c_satisfiable(&format!(
        "SubClassOf(:C ObjectIntersectionOf(DataMinCardinality(2 :p xsd:boolean) \
         DataAllValuesFrom(:p DataOneOf({T}))))"
    )));
}

/// FP guard: the string `"true"` is not a boolean, so it does not count
/// against `≤1 p.xsd:boolean`. One shared bucket with `str:` would say unsat.
#[test]
fn a_string_does_not_count_as_a_boolean() {
    assert!(c_satisfiable(&format!(
        "SubClassOf(:C ObjectIntersectionOf(DataMaxCardinality(1 :p xsd:boolean) \
         DataHasValue(:p {T}) DataHasValue(:p \"true\"^^xsd:string)))"
    )));
}

/// FP guard, the other way: a boolean does not count against `≤1 p.xsd:string`.
#[test]
fn a_boolean_does_not_count_as_a_string() {
    assert!(c_satisfiable(&format!(
        "SubClassOf(:C ObjectIntersectionOf(DataMaxCardinality(1 :p xsd:string) \
         DataHasValue(:p \"a\") DataHasValue(:p {T})))"
    )));
}

// ── xsd:anyURI ───────────────────────────────────────────────────────────

#[test]
fn at_least_three_of_two_uris_is_unsat() {
    assert!(!c_satisfiable(&format!(
        "SubClassOf(:C DataMinCardinality(3 :p DataOneOf({UA} {UB})))"
    )));
}

/// Controls: two of two fit, and bare `xsd:anyURI` is infinite.
#[test]
fn uri_counts_that_fit_are_sat() {
    assert!(c_satisfiable(&format!(
        "SubClassOf(:C DataMinCardinality(2 :p DataOneOf({UA} {UB})))"
    )));
    assert!(c_satisfiable(
        "SubClassOf(:C DataMinCardinality(3 :p xsd:anyURI))"
    ));
}

#[test]
fn at_most_one_uri_with_two_values_is_unsat() {
    assert!(!c_satisfiable(&format!(
        "SubClassOf(:C ObjectIntersectionOf(DataMaxCardinality(1 :p xsd:anyURI) \
         DataHasValue(:p {UA}) DataHasValue(:p {UB})))"
    )));
}

/// FP guard: a URI-shaped string is not an `anyURI` value.
#[test]
fn a_string_does_not_count_as_a_uri() {
    assert!(c_satisfiable(&format!(
        "SubClassOf(:C ObjectIntersectionOf(DataMaxCardinality(1 :p xsd:anyURI) \
         DataHasValue(:p {UA}) DataHasValue(:p \"http://b\"^^xsd:string)))"
    )));
}
