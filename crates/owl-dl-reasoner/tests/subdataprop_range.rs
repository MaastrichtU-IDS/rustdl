//! #626: `SubDataPropertyOf` "Pattern 4" (data-axiom preprocessing) derived `C ⊑ E` from
//! `C ⊑ ∃d.X`, `d ⊑ e` and `Y ⊑ E` (Y an `e`-restriction) with X and Y ignored, so
//! `C ⊑ hasValue(d, 7)`, `hasValue(e, 1) ⊑ E` gave `C ⊑ E` with
//! `completeness_guaranteed: true`. HermiT and Konclude reject it. A cardinality Y did the
//! same: `≥2 e`, `=1 e`, `=2 e` and `≥3 e` were each read as "has some e-value". The
//! pattern now fires only when Y is `∃e.rdfs:Literal` or `≥1 e` (over `rdfs:Literal`);
//! every other Y is left to the lowering (role hierarchy over `DKey` fillers, cardinality
//! rules), which compares it.
//!
//! Oracles: `~/rustdl/sweeps/wB/626/oracle.out` and the review probes (`s1`–`s6`, `t1`,
//! `c1`, `c7`), HermiT on every case. Konclude agrees on every negative; it does not report
//! the `rdfs:Literal` and `xsd:int` positives, an under-report (HermiT derives them).
//! Negative controls first.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const EX: &str = "http://ex.org/t#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{EX}>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\n\
         Ontology(<http://ex.org/t>\n\
         Declaration(DataProperty(:d)) Declaration(DataProperty(:e))\n\
         Declaration(Class(:C)) Declaration(Class(:E))\n\
         SubDataPropertyOf(:d :e)\n{body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

fn c_sub_e(body: &str) -> bool {
    owl_dl_reasoner::is_subclass_of(&onto(body), &format!("{EX}C"), &format!("{EX}E")).unwrap()
}

#[test]
fn a_value_outside_the_super_property_range_is_not_subsumed() {
    for (what, body) in [
        (
            "7 against 1",
            r#"SubClassOf(:C DataHasValue(:d "7"^^xsd:integer))
               SubClassOf(DataHasValue(:e "1"^^xsd:integer) :E)"#,
        ),
        (
            "a string against an integer",
            r#"SubClassOf(:C DataSomeValuesFrom(:d xsd:string))
               SubClassOf(DataHasValue(:e "1"^^xsd:integer) :E)"#,
        ),
        (
            "a range only partly inside",
            r#"SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer
                   xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "3"^^xsd:integer)))
               SubClassOf(DataSomeValuesFrom(:e DatatypeRestriction(xsd:integer
                   xsd:minInclusive "1"^^xsd:integer)) :E)"#,
        ),
        (
            "a range with one value against that value",
            r#"SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer
                   xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "3"^^xsd:integer)))
               SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)"#,
        ),
        (
            "7 against 1 through renamed properties",
            r#"Declaration(DataProperty(:pp)) Declaration(DataProperty(:qq))
               SubDataPropertyOf(:pp :qq)
               SubClassOf(:C DataHasValue(:pp "7"^^xsd:integer))
               SubClassOf(DataHasValue(:qq "1"^^xsd:integer) :E)"#,
        ),
    ] {
        assert_not_subsumed(what, body);
    }
}

fn assert_not_subsumed(what: &str, body: &str) {
    assert!(!c_sub_e(body), "{what}: not entailed (HermiT)");
    let c = owl_dl_reasoner::classify(&onto(body)).unwrap();
    assert!(
        !c.is_subclass(&format!("{EX}C"), &format!("{EX}E")),
        "{what}: classify"
    );
}

/// Review of #632: the cardinality forms of the super side. One `e`-value does not give
/// two, an exact count also caps the number, and a qualified `≥1` still constrains the
/// value. HermiT rejects every one (`s1`–`s6` and the qualified case).
#[test]
fn a_cardinality_beyond_one_unrestricted_value_is_not_subsumed() {
    for (what, body) in [
        (
            "one value against at least two (s1)",
            r#"SubClassOf(:C DataHasValue(:d "7"^^xsd:integer))
               SubClassOf(DataMinCardinality(2 :e) :E)"#,
        ),
        (
            "one value against exactly one (s2)",
            r#"SubClassOf(:C DataHasValue(:d "7"^^xsd:integer))
               SubClassOf(DataExactCardinality(1 :e) :E)"#,
        ),
        (
            "some value against exactly two (s3)",
            "SubClassOf(:C DataSomeValuesFrom(:d rdfs:Literal))
             SubClassOf(DataExactCardinality(2 :e) :E)",
        ),
        (
            "at least one against at least three (s4)",
            "SubClassOf(:C DataMinCardinality(1 :d))
             SubClassOf(DataMinCardinality(3 :e rdfs:Literal) :E)",
        ),
        (
            "the same property against exactly one (s5)",
            r#"SubClassOf(:C DataHasValue(:e "7"^^xsd:integer))
               SubClassOf(DataExactCardinality(1 :e) :E)"#,
        ),
        (
            "exactly one against exactly one (s6)",
            "SubClassOf(:C DataExactCardinality(1 :d))
             SubClassOf(DataExactCardinality(1 :e) :E)",
        ),
        (
            "a string against at least one integer",
            r#"SubClassOf(:C DataHasValue(:d "a"^^xsd:string))
               SubClassOf(DataMinCardinality(1 :e xsd:integer) :E)"#,
        ),
    ] {
        assert_not_subsumed(what, body);
    }
}

#[test]
fn a_value_inside_the_super_property_range_is_subsumed() {
    for (what, body) in [
        (
            "the same value (control)",
            r#"SubClassOf(:C DataHasValue(:d "1"^^xsd:integer))
               SubClassOf(DataHasValue(:e "1"^^xsd:integer) :E)"#,
        ),
        (
            "any value against rdfs:Literal (HermiT)",
            r#"SubClassOf(:C DataHasValue(:d "7"^^xsd:integer))
               SubClassOf(DataSomeValuesFrom(:e rdfs:Literal) :E)"#,
        ),
        (
            "any value against a bare min-cardinality",
            r#"SubClassOf(:C DataHasValue(:d "7"^^xsd:integer))
               SubClassOf(DataMinCardinality(1 :e) :E)"#,
        ),
        (
            "a contained range",
            r#"SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer
                   xsd:minInclusive "2"^^xsd:integer xsd:maxInclusive "3"^^xsd:integer)))
               SubClassOf(DataSomeValuesFrom(:e DatatypeRestriction(xsd:integer
                   xsd:minInclusive "1"^^xsd:integer)) :E)"#,
        ),
        (
            "the same datatype (HermiT)",
            "SubClassOf(:C DataSomeValuesFrom(:d xsd:int))
             SubClassOf(DataSomeValuesFrom(:e xsd:int) :E)",
        ),
        (
            "an integer against a decimal (c1)",
            "SubClassOf(:C DataSomeValuesFrom(:d xsd:integer))
             SubClassOf(DataSomeValuesFrom(:e xsd:decimal) :E)",
        ),
        (
            "an enumeration against its datatype (c7)",
            r#"SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
               SubClassOf(DataSomeValuesFrom(:e xsd:integer) :E)"#,
        ),
        (
            "at least two against at least two (t1)",
            "SubClassOf(:C DataMinCardinality(2 :d))
             SubClassOf(DataMinCardinality(2 :e) :E)",
        ),
    ] {
        assert!(c_sub_e(body), "{what}: entailed");
        let c = owl_dl_reasoner::classify(&onto(body)).unwrap();
        assert!(
            c.is_subclass(&format!("{EX}C"), &format!("{EX}E")),
            "{what}: classify"
        );
    }
}
