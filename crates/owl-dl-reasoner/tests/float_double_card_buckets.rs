//! #332 — `xsd:float` and `xsd:double` values were counted in ONE `card_sat`
//! bucket, so a `≤n` limit over doubles constrained float values too:
//! `≤0 p.xsd:double` with a float value came back unsat, a FALSE POSITIVE.
//! The two value spaces are disjoint in OWL 2.
//!
//! `xsd:double` now has its own `CardRange::Double` / `DoubleSet` buckets. The
//! FP guards below put values of one type under a limit over the other; the
//! controls check that each bucket still finds its own clashes.
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
const F1: &str = "\"1.0\"^^xsd:float";
const F2: &str = "\"2.0\"^^xsd:float";
const D1: &str = "\"1.0\"^^xsd:double";
const D2: &str = "\"2.0\"^^xsd:double";

/// `C ⊑ ⊓ conjuncts`, through both `classify` and `sat`, which must agree.
fn c_satisfiable(conjuncts: &str) -> bool {
    let src = format!(
        "Prefix(:=<http://example.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://example.org/onto>\n\
         Declaration(DataProperty(:p))\n\
         Declaration(Class(:C))\n\
         SubClassOf(:C ObjectIntersectionOf({conjuncts}))\n\
         )\n"
    );
    let o: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse ofn")
        .0;
    let dropped = dropped_axioms(&o).unwrap();
    assert!(dropped.is_empty(), "axioms dropped: {dropped:?}");
    let classify_unsat = classify_top_down_with_timeout(&o, Duration::from_secs(10))
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&C);
    let sat = is_class_satisfiable(&o, C).unwrap();
    assert_eq!(
        !classify_unsat, sat,
        "classify and sat disagree: {conjuncts}"
    );
    sat
}

// ── FP guards: a value of one type never counts under the other's limit ──

/// The issue.
#[test]
fn a_float_value_does_not_count_against_at_most_zero_doubles() {
    assert!(c_satisfiable(&format!(
        "DataMaxCardinality(0 :p xsd:double) DataHasValue(:p {F1})"
    )));
}

#[test]
fn two_float_values_do_not_count_against_at_most_one_double() {
    assert!(c_satisfiable(&format!(
        "DataMaxCardinality(1 :p xsd:double) DataHasValue(:p {F1}) DataHasValue(:p {F2})"
    )));
}

/// The enumeration buckets (`fo:`/`dbo:`), both ways.
#[test]
fn enumerations_of_one_type_do_not_count_under_the_other() {
    assert!(c_satisfiable(&format!(
        "DataMaxCardinality(0 :p DataOneOf({D1})) DataSomeValuesFrom(:p DataOneOf({F1}))"
    )));
    assert!(c_satisfiable(&format!(
        "DataMaxCardinality(1 :p DataOneOf({F1} {F2})) \
         DataSomeValuesFrom(:p DataOneOf({D1})) DataSomeValuesFrom(:p DataOneOf({D2}))"
    )));
}

/// The reverse direction: a double does not count against a float limit.
#[test]
fn a_double_value_does_not_count_against_a_float_limit() {
    assert!(c_satisfiable(&format!(
        "DataMaxCardinality(1 :p xsd:float) DataHasValue(:p {F1}) DataHasValue(:p {D1})"
    )));
}

// ── Controls: each bucket still finds its own clashes ──────────────────

#[test]
fn doubles_still_count_against_a_double_limit() {
    assert!(!c_satisfiable(&format!(
        "DataMaxCardinality(0 :p xsd:double) DataHasValue(:p {D1})"
    )));
    assert!(!c_satisfiable(&format!(
        "DataMaxCardinality(1 :p xsd:double) DataHasValue(:p {D1}) DataHasValue(:p {D2})"
    )));
}

#[test]
fn floats_still_count_against_a_float_limit() {
    assert!(!c_satisfiable(&format!(
        "DataMaxCardinality(1 :p xsd:float) DataHasValue(:p {F1}) DataHasValue(:p {F2})"
    )));
}

#[test]
fn double_enumerations_still_count() {
    assert!(!c_satisfiable(&format!(
        "DataMaxCardinality(1 :p DataOneOf({D1} {D2})) \
         DataSomeValuesFrom(:p DataOneOf({D1})) DataSomeValuesFrom(:p DataOneOf({D2}))"
    )));
}
