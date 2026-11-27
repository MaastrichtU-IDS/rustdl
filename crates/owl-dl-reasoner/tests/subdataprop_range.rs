//! #626: `SubDataPropertyOf` "Pattern 4" (data-axiom preprocessing) derived `C ⊑ E` from
//! `C ⊑ ∃d.X`, `d ⊑ e` and `∃e.Y ⊑ E` with X and Y ignored, so
//! `C ⊑ hasValue(d, 7)`, `hasValue(e, 1) ⊑ E` gave `C ⊑ E` with
//! `completeness_guaranteed: true`. HermiT and Konclude reject it. The pattern now
//! fires only when Y is all data values (`rdfs:Literal`, or a bare min-cardinality);
//! restricted ranges are compared by the lowering (role hierarchy over `DKey` fillers).
//!
//! Oracles (`~/rustdl/sweeps/wB/626/oracle.out`): HermiT on every case, Konclude on
//! every negative and on `p_contained` / `p_min` / the control. Konclude does not
//! report the `rdfs:Literal` and `xsd:int` positives, an under-report (HermiT derives
//! them). Negative controls first.

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
    ] {
        assert!(!c_sub_e(body), "{what}: not entailed (HermiT, Konclude)");
        let c = owl_dl_reasoner::classify(&onto(body)).unwrap();
        assert!(
            !c.is_subclass(&format!("{EX}C"), &format!("{EX}E")),
            "{what}: classify"
        );
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
    ] {
        assert!(c_sub_e(body), "{what}: entailed");
    }
}
