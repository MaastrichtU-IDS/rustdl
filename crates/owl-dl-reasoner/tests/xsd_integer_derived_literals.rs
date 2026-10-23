//! #323 — a LITERAL of an integer-derived datatype (`"5"^^xsd:nonNegativeInteger`)
//! must be read as an integer, not dropped.
//!
//! #121 made the derived types work as data RANGES; every literal parser still
//! matched the exact `xsd:integer` IRI, so `DataHasValue`, `DataOneOf` members and
//! `DataPropertyAssertion`s carrying one dropped (`ore_ont_10391` alone dropped
//! 1,893 `"…"^^xsd:nonNegativeInteger` assertions). They now share one parser,
//! `integer_family_literal_value`.
//!
//! **DIRECTION OF RISK IS A FALSE POSITIVE**, since the fix adds facts. Two rules
//! carry it: the derived types share the integer value space, so `"5"^^xsd:int`
//! and `"5"^^xsd:integer` are the SAME value (counting them as two would be an
//! over-count, the safe direction, but wrong); and a lexical form outside its
//! type's range is ill-formed and keeps dropping rather than being clamped.
//!
//! ORACLE: `HermiT` 1.4.5 agrees on every probe below that rustdl decides (6
//! positives, 6 controls). On the two that still drop (`unsignedLong`, and the
//! ill-formed `"-1"^^xsd:nonNegativeInteger`) `HermiT` calls `C` unsatisfiable:
//! those are visible misses.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    Classification, classify_top_down_with_timeout, dropped_axioms, is_consistent,
};
use std::io::Cursor;
use std::time::Duration;

fn onto(body: &str) -> SetOntology<RcStr> {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))\n\
         Declaration(NamedIndividual(:a))\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn classify(body: &str) -> Classification {
    classify_top_down_with_timeout(&onto(body), Duration::from_secs(10)).expect("classify")
}

fn c_unsat(body: &str) -> bool {
    classify(body)
        .unsatisfiable_classes()
        .contains(&"http://ex.org/C")
}

fn drops(body: &str) -> bool {
    !dropped_axioms(&onto(body))
        .expect("dropped_axioms")
        .is_empty()
}

/// The issue's reproducer, and its control with the value inside the enumeration.
#[test]
fn a_derived_literal_value_clashes_with_an_enumeration() {
    let body = |v: &str| {
        format!(
            "SubClassOf(:C DataHasValue(:p \"{v}\"^^xsd:nonNegativeInteger))\n\
             SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"7\"^^xsd:integer)))"
        )
    };
    assert!(!drops(&body("5")));
    assert!(c_unsat(&body("5")), "5 is not 7");
    assert!(!c_unsat(&body("7")), "7^^nonNegativeInteger is 7^^integer");
}

/// The literal's value is compared against a facet range like an integer's.
#[test]
fn a_derived_literal_value_is_checked_against_an_integer_range() {
    let body = |facet: &str| {
        format!(
            "SubClassOf(:C DataHasValue(:p \"100\"^^xsd:byte))\n\
             SubClassOf(:C DataAllValuesFrom(:p DatatypeRestriction(xsd:integer {facet})))"
        )
    };
    assert!(c_unsat(&body("xsd:minInclusive \"200\"^^xsd:integer")));
    assert!(!c_unsat(&body("xsd:maxInclusive \"150\"^^xsd:integer")));
}

/// `DataOneOf` members of derived types form an integer enumeration.
#[test]
fn an_enumeration_of_derived_literals_is_read() {
    let body = |v: &str| {
        format!(
            "SubClassOf(:C DataHasValue(:p \"{v}\"^^xsd:integer))\n\
             SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"1\"^^xsd:int \"2\"^^xsd:short)))"
        )
    };
    assert!(c_unsat(&body("5")));
    assert!(!c_unsat(&body("2")));
}

/// A derived literal is a member of a derived range that contains it, and not of one
/// that does not.
#[test]
fn a_derived_literal_falls_under_the_ranges_that_contain_it() {
    let body = |range: &str| {
        format!(
            "EquivalentClasses(:D DataSomeValuesFrom(:p xsd:{range}))\n\
             SubClassOf(:C DataHasValue(:p \"5\"^^xsd:unsignedByte))"
        )
    };
    assert!(classify(&body("positiveInteger")).is_subclass("http://ex.org/C", "http://ex.org/D"));
    assert!(!classify(&body("negativeInteger")).is_subclass("http://ex.org/C", "http://ex.org/D"));
}

/// FP guard: the same value spelled in two types is ONE value. Under a functional
/// property, `"5"^^xsd:int` and `"5"^^xsd:integer` are consistent; 5 and 6 are not.
#[test]
fn the_same_value_in_two_integer_types_is_one_value() {
    let body = |v: &str| {
        format!(
            "FunctionalDataProperty(:p) ClassAssertion(:C :a)\n\
             DataPropertyAssertion(:p :a \"5\"^^xsd:int)\n\
             DataPropertyAssertion(:p :a \"{v}\"^^xsd:integer)"
        )
    };
    assert!(is_consistent(&onto(&body("5"))).expect("consistent"));
    assert!(!is_consistent(&onto(&body("6"))).expect("consistent"));
}

/// The `ABox` pre-checks key a derived literal by value end to end: a shared value
/// under `DisjointDataProperties` clashes, and two distinct in-range values break a
/// `≤1` over a derived qualifier, while a value outside the qualifier does not count.
#[test]
fn abox_checks_read_derived_literals_by_value() {
    let disjoint = |v: &str| {
        format!(
            "DisjointDataProperties(:p :q)\n\
             DataPropertyAssertion(:p :a \"5\"^^xsd:int)\n\
             DataPropertyAssertion(:q :a \"{v})"
        )
    };
    assert!(!is_consistent(&onto(&disjoint("5\"^^xsd:long"))).expect("consistent"));
    assert!(is_consistent(&onto(&disjoint("6\"^^xsd:int"))).expect("consistent"));
    let at_most = |v: &str| {
        format!(
            "SubClassOf(:C DataMaxCardinality(1 :p xsd:nonNegativeInteger)) ClassAssertion(:C :a)\n\
             DataPropertyAssertion(:p :a \"5\"^^xsd:int)\n\
             DataPropertyAssertion(:p :a \"{v})"
        )
    };
    assert!(!is_consistent(&onto(&at_most("6\"^^xsd:unsignedByte"))).expect("consistent"));
    assert!(is_consistent(&onto(&at_most("-3\"^^xsd:int"))).expect("consistent"));
}

/// `NegativeDataPropertyAssertion` on `"5"^^xsd:int` contradicts an assertion of
/// `"5"^^xsd:integer`, the same value. Konclude agrees; `HermiT` 1.4.5 calls this
/// consistent (while calling the identical-literal case inconsistent), so it is not
/// the oracle here.
#[test]
fn a_negative_assertion_on_a_derived_literal_denies_the_same_value() {
    let body = |v: &str| {
        format!(
            "NegativeDataPropertyAssertion(:p :a \"5\"^^xsd:int)\n\
             DataPropertyAssertion(:p :a \"{v}\"^^xsd:integer)"
        )
    };
    assert!(!is_consistent(&onto(&body("5"))).expect("consistent"));
    assert!(is_consistent(&onto(&body("6"))).expect("consistent"));
}

/// FP guard for counting: three spellings of 5 (`int`, `integer`, `"05"^^long`) are
/// one value, so `≥2` over them is unsatisfiable; 5 and 6 give two.
#[test]
fn qualified_cardinality_counts_derived_literals_by_value() {
    assert!(c_unsat(
        "SubClassOf(:C DataMinCardinality(2 :p \
         DataOneOf(\"5\"^^xsd:int \"5\"^^xsd:integer \"05\"^^xsd:long)))"
    ));
    assert!(!c_unsat(
        "SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"5\"^^xsd:int \"6\"^^xsd:integer)))"
    ));
}

/// An ill-formed lexical form (outside its type's range) is not clamped into range,
/// and `xsd:unsignedLong` (not representable, #121) is not read either: both keep
/// dropping VISIBLY. FLIP the `unsignedLong` half if `IntegerRange` widens to `i128`.
#[test]
fn ill_formed_and_unrepresentable_literals_still_drop_visibly() {
    for lit in ["\"-1\"^^xsd:nonNegativeInteger", "\"5\"^^xsd:unsignedLong"] {
        let body = format!(
            "SubClassOf(:C DataHasValue(:p {lit}))\n\
             SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"7\"^^xsd:integer)))"
        );
        assert!(drops(&body), "{lit} must drop visibly");
        assert!(!c_unsat(&body), "{lit} must not be read");
    }
}
