//! #343 — a union of `xsd:integer` intervals is counted under data cardinality.
//!
//! #42 lowered `∀p.([0,5] ⊔ [10,15])` to an interval-set (`iset:`) key but gave it no
//! counting entry, so `≥3 p.xsd:integer ⊓ ∀p.({1} ⊔ {5})` was satisfiable with nothing
//! dropped; as a cardinality QUALIFIER the union dropped the axiom. Now the key decodes
//! to an exact `IntUnion` and is counted with the other integer buckets, and the
//! qualifier lowers to the same key.
//!
//! A union whose pieces touch (`{0} ⊔ {1}`) normalises to one interval and was counted
//! already; those cases are controls that must not move.
//!
//! ORACLES split, so the OWL 2 semantics (count the distinct integers) decide every
//! verdict. `HermiT` agrees on every `≥n` case (slowly on large `n`) but is inverted on
//! `≤n` over a union against asserted values; Konclude does not count interval capacity
//! at all but is right on those `≤n` cases.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    classify_top_down_with_timeout, dropped_axioms, is_class_satisfiable, is_consistent,
};
use std::io::Cursor;
use std::time::Duration;

const C: &str = "http://ex.org/C";

fn onto(body: &str) -> SetOntology<RcStr> {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:C)) Declaration(Class(:G))\n\
         Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// `C`'s satisfiability, asserted to agree between `classify` and the per-class
/// probe, with nothing dropped.
fn c_unsat(body: &str) -> bool {
    let o = onto(body);
    assert!(
        dropped_axioms(&o).expect("dropped").is_empty(),
        "nothing may drop: {body}"
    );
    let by_classify = classify_top_down_with_timeout(&o, Duration::from_secs(30))
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&C);
    let by_sat = !is_class_satisfiable(&o, C).expect("sat");
    assert_eq!(by_classify, by_sat, "classify and sat disagree on: {body}");
    by_sat
}

/// `[lo,hi]` as an `xsd:integer` restriction; `None` leaves that end open.
fn r(lo: Option<i64>, hi: Option<i64>) -> String {
    let min = lo.map_or_else(String::new, |v| {
        format!(" xsd:minInclusive \"{v}\"^^xsd:integer")
    });
    let max = hi.map_or_else(String::new, |v| {
        format!(" xsd:maxInclusive \"{v}\"^^xsd:integer")
    });
    format!("DatatypeRestriction(xsd:integer{min}{max})")
}

fn union(pieces: &[(Option<i64>, Option<i64>)]) -> String {
    let members: Vec<String> = pieces.iter().map(|&(lo, hi)| r(lo, hi)).collect();
    format!("DataUnionOf({})", members.join(" "))
}

fn on_c(parts: &str) -> String {
    format!("SubClassOf(:C ObjectIntersectionOf({parts}))")
}

/// `{1} ⊔ {5}` and `[0,5] ⊔ [10,15]`.
fn u15() -> String {
    union(&[(Some(1), Some(1)), (Some(5), Some(5))])
}
fn u0510() -> String {
    union(&[(Some(0), Some(5)), (Some(10), Some(15))])
}

/// The union as a `∀` filter, in one axiom, two axioms, and a data range.
#[test]
fn a_union_filter_bounds_the_count() {
    let u = u15();
    assert!(c_unsat(&on_c(&format!(
        "DataMinCardinality(3 :p xsd:integer) DataAllValuesFrom(:p {u})"
    ))));
    assert!(!c_unsat(&on_c(&format!(
        "DataMinCardinality(2 :p xsd:integer) DataAllValuesFrom(:p {u})"
    ))));
    assert!(c_unsat(&format!(
        "SubClassOf(:C DataMinCardinality(3 :p xsd:integer))\n\
         SubClassOf(:C DataAllValuesFrom(:p {u}))"
    )));
    assert!(c_unsat(&format!(
        "DataPropertyRange(:p {u})\nSubClassOf(:C DataMinCardinality(3 :p xsd:integer))"
    )));
    // [0,5] ⊔ [10,15] holds 12 integers.
    let w = u0510();
    assert!(c_unsat(&on_c(&format!(
        "DataMinCardinality(13 :p xsd:integer) DataAllValuesFrom(:p {w})"
    ))));
    assert!(!c_unsat(&on_c(&format!(
        "DataMinCardinality(12 :p xsd:integer) DataAllValuesFrom(:p {w})"
    ))));
}

/// The union as a cardinality qualifier: Min, Exact, and a mixed enumeration +
/// interval union (`{1} ⊔ [5,6]` holds 3).
#[test]
fn a_union_qualifier_is_counted() {
    let u = u15();
    assert!(c_unsat(&format!(
        "SubClassOf(:C DataMinCardinality(3 :p {u}))"
    )));
    assert!(!c_unsat(&format!(
        "SubClassOf(:C DataMinCardinality(2 :p {u}))"
    )));
    assert!(c_unsat(&format!(
        "SubClassOf(:C DataExactCardinality(3 :p {u}))"
    )));
    let mixed = format!(
        "DataUnionOf(DataOneOf(\"1\"^^xsd:integer) {})",
        r(Some(5), Some(6))
    );
    assert!(c_unsat(&format!(
        "SubClassOf(:C DataMinCardinality(4 :p {mixed}))"
    )));
    assert!(!c_unsat(&format!(
        "SubClassOf(:C DataMinCardinality(3 :p {mixed}))"
    )));
    // A derived-type piece keeps its own value space: byte ≥ 120 is [120,127].
    let derived = format!(
        "DataUnionOf(DatatypeRestriction(xsd:byte xsd:minInclusive \"120\"^^xsd:integer) {})",
        r(Some(200), Some(200))
    );
    assert!(c_unsat(&format!(
        "SubClassOf(:C DataMinCardinality(10 :p {derived}))"
    )));
    assert!(!c_unsat(&format!(
        "SubClassOf(:C DataMinCardinality(9 :p {derived}))"
    )));
}

/// Limits over a union against values, and two unions against each other.
#[test]
fn a_union_limit_counts_the_values_inside_it() {
    let w = u0510();
    let has = |a: i64, b: i64| {
        format!("DataHasValue(:p \"{a}\"^^xsd:integer) DataHasValue(:p \"{b}\"^^xsd:integer)")
    };
    assert!(c_unsat(&on_c(&format!(
        "DataMaxCardinality(1 :p {w}) {}",
        has(2, 12)
    ))));
    assert!(!c_unsat(&on_c(&format!(
        "DataMaxCardinality(1 :p {w}) {}",
        has(7, 8)
    ))));
    assert!(c_unsat(&on_c(&format!(
        "DataMaxCardinality(0 :p {w}) DataHasValue(:p \"2\"^^xsd:integer)"
    ))));
    assert!(!c_unsat(&on_c(&format!(
        "DataMaxCardinality(0 :p {w}) DataHasValue(:p \"7\"^^xsd:integer)"
    ))));
    let two = union(&[(Some(0), Some(0)), (Some(10), Some(10))]);
    assert!(c_unsat(&on_c(&format!(
        "DataMinCardinality(3 :p {w}) DataAllValuesFrom(:p {two})"
    ))));
    assert!(!c_unsat(&on_c(&format!(
        "DataMinCardinality(2 :p {w}) DataAllValuesFrom(:p {two})"
    ))));
}

/// Capacity is exact: an unbounded piece is infinite, and against a bounded filter
/// only the part inside it counts.
#[test]
fn an_unbounded_piece_is_infinite() {
    let open = union(&[(None, Some(-5)), (Some(3), Some(3))]);
    assert!(!c_unsat(&format!(
        "SubClassOf(:C DataMinCardinality(5 :p {open}))"
    )));
    assert!(c_unsat(&on_c(&format!(
        "DataMinCardinality(2 :p {open}) DataAllValuesFrom(:p {})",
        r(Some(3), Some(10))
    ))));
}

/// Universals from all three integer buckets at once: `∀p.[0,20]` (range),
/// `∀p.{0,1,2,3,10,11,12,30}` (enumeration) and `∀p.([0,5] ⊔ [10,15])` (union)
/// leave `{0,1,2,3,10,11,12}`, seven integers.
#[test]
fn universals_from_all_three_buckets_intersect() {
    let body = |n: u32| {
        on_c(&format!(
            "DataMinCardinality({n} :p xsd:integer)\n\
             DataAllValuesFrom(:p {})\n\
             DataAllValuesFrom(:p DataOneOf(\"0\"^^xsd:integer \"1\"^^xsd:integer \
             \"2\"^^xsd:integer \"3\"^^xsd:integer \"10\"^^xsd:integer \"11\"^^xsd:integer \
             \"12\"^^xsd:integer \"30\"^^xsd:integer))\n\
             DataAllValuesFrom(:p {})",
            r(Some(0), Some(20)),
            u0510()
        ))
    };
    assert!(!c_unsat(&body(7)));
    assert!(c_unsat(&body(8)));
}

/// Controls that must not move: a union whose pieces touch or overlap is one
/// interval and was counted before #343.
#[test]
fn a_union_that_normalises_to_one_interval_is_unchanged() {
    let touch = union(&[(Some(0), Some(0)), (Some(1), Some(1))]);
    assert!(c_unsat(&on_c(&format!(
        "DataMinCardinality(3 :p xsd:integer) DataAllValuesFrom(:p {touch})"
    ))));
    assert!(!c_unsat(&on_c(&format!(
        "DataMinCardinality(2 :p xsd:integer) DataAllValuesFrom(:p {touch})"
    ))));
    let overlap = union(&[(Some(0), Some(5)), (Some(3), Some(8))]);
    assert!(c_unsat(&on_c(&format!(
        "DataMinCardinality(10 :p xsd:integer) DataAllValuesFrom(:p {overlap})"
    ))));
    assert!(!c_unsat(&on_c(&format!(
        "DataMinCardinality(9 :p xsd:integer) DataAllValuesFrom(:p {overlap})"
    ))));
}

/// Backjumping: the filter or the demand reached by branching (the `⊒` half of an
/// `EquivalentClasses`) must not clash on the branch that avoids it; told, it clashes.
#[test]
fn a_union_reached_by_branching_does_not_clash() {
    let u = u15();
    let filter_g = format!(
        "EquivalentClasses(:G DataAllValuesFrom(:p {u}))\n\
         SubClassOf(:C DataMinCardinality(3 :p xsd:integer))"
    );
    assert!(!c_unsat(&filter_g));
    assert!(c_unsat(&format!("{filter_g}\nSubClassOf(:C :G)")));
    let demand_g = format!(
        "EquivalentClasses(:G DataMinCardinality(3 :p xsd:integer))\n\
         SubClassOf(:C DataAllValuesFrom(:p {u}))"
    );
    assert!(!c_unsat(&demand_g));
    assert!(c_unsat(&format!("{demand_g}\nSubClassOf(:C :G)")));
}

/// `ABox`: two asserted values inside a `≤1` union are inconsistent; outside it, not.
/// Konclude agrees on both; `HermiT` is inverted here.
#[test]
fn asserted_values_are_counted_against_a_union_limit() {
    let body = |a: i64, b: i64| {
        format!(
            "SubClassOf(:C DataMaxCardinality(1 :p {})) ClassAssertion(:C :a)\n\
             DataPropertyAssertion(:p :a \"{a}\"^^xsd:integer)\n\
             DataPropertyAssertion(:p :a \"{b}\"^^xsd:integer)",
            u0510()
        )
    };
    assert!(!is_consistent(&onto(&body(2, 12))).expect("consistent"));
    assert!(is_consistent(&onto(&body(7, 8))).expect("consistent"));
}

/// A union with a member of another datatype still drops whole, visibly.
#[test]
fn a_union_with_a_non_integer_member_still_drops() {
    let o = onto(&format!(
        "SubClassOf(:C DataMinCardinality(3 :p DataUnionOf({} \
         DatatypeRestriction(xsd:decimal xsd:minInclusive \"0.5\"^^xsd:decimal \
         xsd:maxInclusive \"1.5\"^^xsd:decimal))))",
        r(Some(1), Some(1))
    ));
    assert!(!dropped_axioms(&o).expect("dropped").is_empty());
    assert!(is_class_satisfiable(&o, C).expect("sat"));
}
