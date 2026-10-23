//! #326 — `card_sat`'s disjoint-packing step took demands in input order, so a
//! broad demand listed first could block the narrow disjoint ones it overlaps.
//! `=1 p.{1,2}` with `∃p.{1}` and `∃p.{2}` came back satisfiable, where `HermiT`
//! says unsat. A second pass now packs smallest capacity first.
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

fn c_satisfiable(axioms: &str) -> bool {
    let src = format!(
        "Prefix(:=<http://example.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://example.org/onto>\n\
         Declaration(DataProperty(:p))\n\
         Declaration(Class(:C))\n\
         {axioms}\n\
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
    assert_eq!(!classify_unsat, sat, "classify and sat disagree: {axioms}");
    sat
}

/// The issue's reproducer.
#[test]
fn exactly_one_of_two_integers_with_both_is_unsat() {
    assert!(!c_satisfiable(
        "SubClassOf(:C ObjectIntersectionOf(\
           DataExactCardinality(1 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)) \
           DataSomeValuesFrom(:p DataOneOf(\"1\"^^xsd:integer)) \
           DataSomeValuesFrom(:p DataOneOf(\"2\"^^xsd:integer))))"
    ));
}

/// The same over strings, another finite-set bucket.
#[test]
fn exactly_one_of_two_strings_with_both_is_unsat() {
    assert!(!c_satisfiable(
        "SubClassOf(:C ObjectIntersectionOf(\
           DataExactCardinality(1 :p DataOneOf(\"a\" \"b\")) \
           DataSomeValuesFrom(:p DataOneOf(\"a\")) \
           DataSomeValuesFrom(:p DataOneOf(\"b\"))))"
    ));
}

/// FP guard: with only one of the two values required, one value satisfies
/// both demands.
#[test]
fn exactly_one_with_one_value_is_sat() {
    assert!(c_satisfiable(
        "SubClassOf(:C ObjectIntersectionOf(\
           DataExactCardinality(1 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)) \
           DataSomeValuesFrom(:p DataOneOf(\"1\"^^xsd:integer))))"
    ));
}
