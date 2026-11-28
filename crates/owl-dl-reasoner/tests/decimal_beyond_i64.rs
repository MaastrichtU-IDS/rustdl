//! #641: a whole `xsd:decimal` beyond `i64` in a `DataOneOf`, against an integer range,
//! read as disjoint from it: `DataPropertyRange(d, xsd:integer)` with
//! `C ⊑ ∃d.{1E20, 2E20}` made `C` unsat with `completeness_guaranteed: true`. The
//! integer-decimal cross seeding read "not an `i64`" as "not a member". Membership is
//! now exact: whole values compare as `i128`, and a value beyond `i128` is in a range
//! exactly when that range is unbounded on its side.
//!
//! `HermiT` gives every verdict asserted here (`~/rustdl/sweeps/wB/641/p/`, generated
//! by `cases.py`, `hermit.out`). False unsats first, then the controls that must stay
//! unsat, then subsumptions.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const EX: &str = "http://ex.org/t#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{EX}>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/t>\n\
         Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))\n\
         {body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// `C` satisfiable through `sat` and through `classify`, which must agree.
fn c_sat(what: &str, body: &str) -> bool {
    let o = onto(body);
    let sat = owl_dl_reasoner::is_class_satisfiable(&o, &format!("{EX}C")).unwrap();
    let c = owl_dl_reasoner::classify(&o).unwrap();
    assert_eq!(
        sat,
        !c.unsatisfiable_classes()
            .contains(&format!("{EX}C").as_str()),
        "{what}: sat and classify disagree"
    );
    sat
}

#[test]
fn a_whole_decimal_beyond_i64_meets_an_unbounded_integer_range() {
    for (what, body) in [
        (
            "s_int_e20",
            "DataPropertyRange(:d xsd:integer) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"100000000000000000000.0\"^^xsd:decimal \"200000000000000000000.0\"^^xsd:decimal)))",
        ),
        (
            "s_ge5_e20",
            "DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:minInclusive \"5\"^^xsd:integer)) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"100000000000000000000.0\"^^xsd:decimal \"200000000000000000000.0\"^^xsd:decimal)))",
        ),
        (
            "s_rng_oneof_int",
            "DataPropertyRange(:d DataOneOf(\"100000000000000000000.0\"^^xsd:decimal \"200000000000000000000.0\"^^xsd:decimal)) SubClassOf(:C DataSomeValuesFrom(:d xsd:integer))",
        ),
        (
            "s_int_e40",
            "DataPropertyRange(:d xsd:integer) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"10000000000000000000000000000000000000000.0\"^^xsd:decimal)))",
        ),
        (
            "s_le5_ne20",
            "DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:maxInclusive \"5\"^^xsd:integer)) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"-100000000000000000000.0\"^^xsd:decimal)))",
        ),
        (
            "s_le5_ne40",
            "DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:maxInclusive \"5\"^^xsd:integer)) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"-10000000000000000000000000000000000000000.0\"^^xsd:decimal)))",
        ),
        (
            "s_long_min",
            "DataPropertyRange(:d xsd:long) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"-9223372036854775808.0\"^^xsd:decimal)))",
        ),
        (
            "s_int_ctl",
            "DataPropertyRange(:d xsd:integer) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"1.0\"^^xsd:decimal \"2.0\"^^xsd:decimal)))",
        ),
    ] {
        assert!(c_sat(what, body), "{what}: C satisfiable (HermiT)");
    }
}

#[test]
fn a_whole_decimal_beyond_a_bound_or_fractional_stays_disjoint() {
    for (what, body) in [
        (
            "u_le5_e20",
            "DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:maxInclusive \"5\"^^xsd:integer)) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"100000000000000000000.0\"^^xsd:decimal)))",
        ),
        (
            "u_ge5_ne20",
            "DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:minInclusive \"5\"^^xsd:integer)) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"-100000000000000000000.0\"^^xsd:decimal)))",
        ),
        (
            "u_le5_e40",
            "DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:maxInclusive \"5\"^^xsd:integer)) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"10000000000000000000000000000000000000000.0\"^^xsd:decimal)))",
        ),
        (
            "u_long_e20",
            "DataPropertyRange(:d xsd:long) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"100000000000000000000.0\"^^xsd:decimal)))",
        ),
        (
            "u_int_frac",
            "DataPropertyRange(:d xsd:integer) SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"100000000000000000000.5\"^^xsd:decimal)))",
        ),
    ] {
        assert!(!c_sat(what, body), "{what}: C unsat (HermiT)");
    }
}

#[test]
fn a_whole_decimal_beyond_i64_is_an_integer() {
    for (what, body, expected) in [
        (
            "p_e20_int",
            "SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"100000000000000000000.0\"^^xsd:decimal))) SubClassOf(DataSomeValuesFrom(:d xsd:integer) :E)",
            true,
        ),
        (
            "p_e40_int",
            "SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"10000000000000000000000000000000000000000.0\"^^xsd:decimal))) SubClassOf(DataSomeValuesFrom(:d xsd:integer) :E)",
            true,
        ),
        (
            "n_e20_long",
            "SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"100000000000000000000.0\"^^xsd:decimal))) SubClassOf(DataSomeValuesFrom(:d xsd:long) :E)",
            false,
        ),
    ] {
        let o = onto(body);
        let got =
            owl_dl_reasoner::is_subclass_of(&o, &format!("{EX}C"), &format!("{EX}E")).unwrap();
        assert_eq!(got, expected, "{what}: HermiT says {expected}");
    }
}
