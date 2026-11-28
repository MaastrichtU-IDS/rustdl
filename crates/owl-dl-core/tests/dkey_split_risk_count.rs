//! #589: the split-risk detector's count, `InternalOntology::dkey_split_risk_keys`.
//!
//! The reasoner-level canaries (`owl-dl-reasoner/tests/dkey_case_split_signal.rs`) pin the
//! flag on each shape; these pin the parts of the rule a flag cannot show: a key with a
//! seeded `⊑` into every key it meets is not counted, a point is never counted, and an
//! IEEE zero is two values.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;

fn count(body: &str) -> usize {
    let src = format!(
        "Prefix(:=<http://ex.org/t#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/t>\nDeclaration(DataProperty(:d))\n\
         Declaration(Class(:C)) Declaration(Class(:E))\n{body}\n)\n"
    );
    let o: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0;
    owl_dl_core::convert::convert_ontology(&o)
        .unwrap()
        .dkey_split_risk_keys
}

fn int_range(lo: i64, hi: i64) -> String {
    format!(
        "DatatypeRestriction(xsd:integer xsd:minInclusive \"{lo}\"^^xsd:integer \
         xsd:maxInclusive \"{hi}\"^^xsd:integer)"
    )
}

/// `[2,3] ⊂ [1,10]`: the inner key has a seeded edge into the outer one, so only the
/// outer key (which meets the inner one and is not inside it) counts.
#[test]
fn a_key_inside_every_key_it_meets_is_not_counted() {
    let body = format!(
        "SubClassOf(:C DataSomeValuesFrom(:d {}))\nSubClassOf(:E DataSomeValuesFrom(:d {}))",
        int_range(2, 3),
        int_range(1, 10)
    );
    assert_eq!(count(&body), 1);
}

/// Points are never counted, however many meet: `{1}` twice over, as a range point and
/// an enumeration point.
#[test]
fn points_are_not_counted() {
    let body = "SubClassOf(:C DataHasValue(:d \"1\"^^xsd:integer))\n\
         SubClassOf(:E DataSomeValuesFrom(:d DataOneOf(\"1\"^^xsd:integer)))";
    assert_eq!(count(body), 0);
}

/// The issue's repro: `{1,2}` meets both points, and is inside neither.
#[test]
fn the_issue_repro_counts_its_enumeration() {
    let body = "SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
         SubClassOf(DataHasValue(:d \"1\"^^xsd:integer) :E)\n\
         SubClassOf(DataHasValue(:d \"2\"^^xsd:integer) :E)";
    assert_eq!(count(body), 1);
}

/// An IEEE zero is two values (the buckets merge `-0` and `+0`), so the double point
/// `0.0` and the enumeration `{0.0}` are not points, and they meet with no edge between
/// them (#556 leaves zero at a bound unseeded). A non-zero point is one value.
#[test]
fn a_zero_point_is_two_values() {
    let zero = "SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"0.0\"^^xsd:double)))\n\
         SubClassOf(DataHasValue(:d \"0.0\"^^xsd:double) :E)";
    assert_eq!(count(zero), 2);
    let one = "SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"1.0\"^^xsd:double)))\n\
         SubClassOf(DataHasValue(:d \"1.0\"^^xsd:double) :E)";
    assert_eq!(count(one), 0);
}

/// An empty key counts though it meets nothing (Fable review of #589): the engines treat
/// it as an atom, so `C ⊑ ∃d.decimal[3,2]` left `C` satisfiable, certified. (An empty
/// INTEGER range lowers to `⊥` since #635, so it interns no key and counts nothing.)
#[test]
fn an_empty_key_is_counted() {
    let body = "SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal \
         xsd:minInclusive \"3\"^^xsd:decimal xsd:maxInclusive \"2\"^^xsd:decimal)))";
    assert_eq!(count(body), 1);
    let int = format!("SubClassOf(:C DataSomeValuesFrom(:d {}))", int_range(5, 1));
    assert_eq!(count(&int), 0, "#635 lowers an empty integer range to ⊥");
}

/// Float bounds with no `f32` strictly between them hold nothing (`1.0000001` is the
/// next float after `1.0`); the same exclusive bounds as doubles hold values, and a lone
/// non-empty range is not counted.
#[test]
fn adjacent_exclusive_float_bounds_are_empty() {
    let range = |dt: &str| {
        format!(
            "SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:{dt} \
             xsd:minExclusive \"1.0\"^^xsd:{dt} xsd:maxExclusive \"1.0000001\"^^xsd:{dt})))"
        )
    };
    assert_eq!(count(&range("float")), 1);
    assert_eq!(count(&range("double")), 0);
}
