//! #336 — D4 Pattern 2 (`≥n dp` with `≤m dp`, `n > m` ⟹ unsat) compared the counts
//! per `(class, dp)` and ignored the data ranges, so `≥2 p.xsd:integer` with
//! `≤1 p.xsd:string` came back UNSATISFIABLE, a false positive at default settings:
//! the limit counts no integer. Pattern 2 now pairs a demand with a limit only when
//! the demand's range provably lies inside the limit's (`bound_range_within`).
//!
//! The genuine cases that only Pattern 2 catches stay caught: a range inside
//! another of a different `card_sat` bucket (an integer enumeration inside an
//! integer interval, `xsd:integer` inside `xsd:decimal`, `xsd:string` inside
//! `rdf:PlainLiteral`).
//!
//! ORACLE: `HermiT` 1.4.5 agrees on every verdict below.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify_top_down_with_timeout, is_class_satisfiable};
use std::fmt::Write as _;
use std::io::Cursor;
use std::time::Duration;

const C: &str = "http://ex.org/C";

fn onto(body: &str) -> SetOntology<RcStr> {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)\n\
         Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:C)) Declaration(Class(:A)) Declaration(Class(:B))\n\
         Declaration(DataProperty(:p))\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// `C`'s satisfiability, asserted to agree between `classify` and the per-class
/// probe, which gate the tableau differently.
fn c_unsat(body: &str) -> bool {
    let o = onto(body);
    let by_classify = classify_top_down_with_timeout(&o, Duration::from_secs(10))
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&C);
    let by_sat = !is_class_satisfiable(&o, C).expect("sat");
    assert_eq!(by_classify, by_sat, "classify and sat disagree on: {body}");
    by_sat
}

fn min_max(n: u32, d1: &str, m: u32, d2: &str) -> String {
    format!(
        "SubClassOf(:C ObjectIntersectionOf(DataMinCardinality({n} :p {d1}) \
         DataMaxCardinality({m} :p {d2})))"
    )
}

const INT_1_3: &str = "DatatypeRestriction(xsd:integer xsd:minInclusive \"1\"^^xsd:integer \
                       xsd:maxInclusive \"3\"^^xsd:integer)";

/// The issue's false positives: the limit's range does not contain the demand's.
#[test]
fn a_limit_over_another_range_does_not_clash() {
    for (d1, d2) in [
        ("xsd:integer", "xsd:string"),
        ("DataOneOf(\"4\"^^xsd:integer \"5\"^^xsd:integer)", INT_1_3),
        ("DataOneOf(\"1\"^^xsd:integer \"5\"^^xsd:integer)", INT_1_3),
        ("xsd:float", "xsd:decimal"),
        ("xsd:decimal", "xsd:integer"),
        ("rdf:PlainLiteral", "xsd:string"),
        // Containment runs one way: `xsd:int` ⊆ `xsd:long`, not the reverse.
        ("xsd:long", "xsd:int"),
        // The whole domain as the DEMAND is inside no narrower limit.
        ("rdfs:Literal", "xsd:integer"),
    ] {
        assert!(!c_unsat(&min_max(2, d1, 1, d2)), "≥2 p.{d1} ⊓ ≤1 p.{d2}");
    }
    // As two axioms, and through the Max half of an exact cardinality.
    assert!(!c_unsat(
        "SubClassOf(:C DataMinCardinality(2 :p xsd:integer))\n\
         SubClassOf(:C DataMaxCardinality(1 :p xsd:string))"
    ));
    assert!(!c_unsat(
        "SubClassOf(:C ObjectIntersectionOf(DataExactCardinality(2 :p xsd:integer) \
         DataMaxCardinality(1 :p xsd:string)))"
    ));
}

/// A demand inside the limit's range clashes, including across `card_sat` buckets.
#[test]
fn a_limit_over_a_containing_range_clashes() {
    for (d1, d2) in [
        ("xsd:integer", "xsd:integer"),
        ("xsd:int", "xsd:integer"),
        (
            "DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)",
            "xsd:integer",
        ),
        ("xsd:integer", "xsd:decimal"),
        ("xsd:int", "xsd:decimal"),
        ("xsd:string", "rdf:PlainLiteral"),
        ("rdf:langString", "rdf:PlainLiteral"),
        ("xsd:string", "xsd:string"),
    ] {
        assert!(c_unsat(&min_max(2, d1, 1, d2)), "≥2 p.{d1} ⊓ ≤1 p.{d2}");
    }
    assert!(c_unsat(
        "SubClassOf(:C ObjectIntersectionOf(DataMinCardinality(2 :p) DataMaxCardinality(1 :p)))"
    ));
}

/// Interval containment is checked by the bounds, not by the datatype.
#[test]
fn integer_interval_containment_is_exact() {
    let int_0_10 = "DatatypeRestriction(xsd:integer xsd:minInclusive \"0\"^^xsd:integer \
                    xsd:maxInclusive \"10\"^^xsd:integer)";
    let int_1_5 = "DatatypeRestriction(xsd:integer xsd:minInclusive \"1\"^^xsd:integer \
                   xsd:maxInclusive \"5\"^^xsd:integer)";
    assert!(c_unsat(&min_max(3, INT_1_3, 2, int_0_10)));
    assert!(!c_unsat(&min_max(2, int_1_5, 1, INT_1_3)));
}

/// Bounds inherited through `C ≡ A ⊓ B` are compared the same way.
#[test]
fn inherited_bounds_compare_ranges() {
    let body = |d2: &str| {
        format!(
            "EquivalentClasses(:C ObjectIntersectionOf(:A :B))\n\
             SubClassOf(:A DataMinCardinality(2 :p xsd:integer))\n\
             SubClassOf(:B DataMaxCardinality(1 :p {d2}))"
        )
    };
    assert!(!c_unsat(&body("xsd:string")));
    assert!(c_unsat(&body("xsd:decimal")));
}

/// Exact cardinality on both sides, a zero limit, and an explicit `rdfs:Literal`
/// limit.
#[test]
fn exact_zero_and_literal_bounds() {
    let exact = |d2: &str| {
        format!(
            "SubClassOf(:C ObjectIntersectionOf(DataExactCardinality(2 :p xsd:integer) \
             DataExactCardinality(1 :p {d2})))"
        )
    };
    assert!(!c_unsat(&exact("xsd:string")));
    assert!(c_unsat(&exact("xsd:decimal")));
    assert!(c_unsat(&min_max(1, "xsd:integer", 0, "xsd:decimal")));
    assert!(!c_unsat(&min_max(1, "xsd:integer", 0, "xsd:string")));
    assert!(c_unsat(&min_max(2, "xsd:string", 1, "rdfs:Literal")));
}

/// A bound inherited through a deep chain of definitions keeps one entry per
/// range (the tightest), so the inheritance fixpoint stays cheap; the verdict
/// at the bottom is still found.
#[test]
fn a_bound_inherited_through_a_long_chain() {
    let mut body = String::new();
    let depth = 200;
    for i in 0..depth {
        write!(
            body,
            "Declaration(Class(:K{i})) Declaration(Class(:D{i}))\n\
             EquivalentClasses(:K{i} ObjectIntersectionOf(:K{j} :D{i}))\n\
             SubClassOf(:D{i} DataMinCardinality({n} :p xsd:int))\n",
            j = i + 1,
            n = i + 1
        )
        .unwrap();
    }
    write!(
        body,
        "Declaration(Class(:K{depth}))\n\
         SubClassOf(:K{depth} DataMaxCardinality(1 :p xsd:decimal))\n\
         SubClassOf(:C :K0)"
    )
    .unwrap();
    let start = std::time::Instant::now();
    assert!(c_unsat(&body));
    assert!(
        start.elapsed() < Duration::from_secs(60),
        "{:?}",
        start.elapsed()
    );
}

/// Two inherited bounds over the same range: only the tightest one clashes, so it
/// must be the one kept (the largest demand, the smallest limit).
#[test]
fn the_tightest_inherited_bound_is_kept() {
    assert!(c_unsat(
        "EquivalentClasses(:C ObjectIntersectionOf(:A :B))\n\
         SubClassOf(:A DataMinCardinality(1 :p xsd:integer))\n\
         SubClassOf(:B DataMinCardinality(2 :p xsd:integer))\n\
         SubClassOf(:C DataMaxCardinality(1 :p xsd:decimal))"
    ));
    assert!(c_unsat(
        "EquivalentClasses(:C ObjectIntersectionOf(:A :B))\n\
         SubClassOf(:A DataMaxCardinality(2 :p xsd:decimal))\n\
         SubClassOf(:B DataMaxCardinality(1 :p xsd:decimal))\n\
         SubClassOf(:C DataMinCardinality(2 :p xsd:integer))"
    ));
}
