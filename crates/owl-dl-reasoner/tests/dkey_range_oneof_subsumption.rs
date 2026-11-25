//! #556: the SUBSUMPTION half of #42 item 2. A `DataOneOf` lowers into an
//! enumeration bucket (`io:` / `deo:` / `fo:` / `dbo:` / `dto:` …), while a
//! `NegativeDataPropertyAssertion`, a `DataPropertyAssertion`, `DataHasValue` and a
//! facet restriction lower into the RANGE bucket of the same datatype. Only
//! disjointness was seeded across the two buckets, so `a : ∃d.{1}` with `¬d(a, 1)`
//! read consistent on every path, and `C ⊑ ∃d.{3}` never reached `∃d.[3,3]`.
//!
//! Oracles: every expectation below was run on `HermiT` and Konclude 0.7 and both
//! agree, except where a test says otherwise. Negative controls come first; each is
//! one value or one datatype away from a positive twin.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const EX: &str = "http://ex.org/t#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{EX}>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/t>\n\
         Declaration(DataProperty(:d)) Declaration(NamedIndividual(:a))\n\
         Declaration(Class(:C)) Declaration(Class(:E))\n\
         {body}\n)\n"
    );
    let mut reader = Cursor::new(src);
    let (o, _) = read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    o
}

/// `a : ∃d.{oneof}` and `¬d(a, excluded)`.
fn negated(oneof: &str, excluded: &str) -> bool {
    let o = onto(&format!(
        "ClassAssertion(DataSomeValuesFrom(:d DataOneOf({oneof})) :a)\n\
         NegativeDataPropertyAssertion(:d :a {excluded})"
    ));
    owl_dl_reasoner::is_consistent(&o).expect("consistency")
}

fn c_sub_e(body: &str) -> bool {
    owl_dl_reasoner::is_subclass_of(&onto(body), &format!("{EX}C"), &format!("{EX}E"))
        .expect("subclass")
}

#[test]
fn negative_controls_stay_consistent() {
    // A different value.
    assert!(negated(r#""1"^^xsd:integer"#, r#""2"^^xsd:integer"#));
    // A different datatype family.
    assert!(negated(r#""1"^^xsd:integer"#, r#""1"^^xsd:string"#));
    // A two-member enumeration with one member excluded: `2` is left.
    assert!(negated(
        r#""1"^^xsd:integer "2"^^xsd:integer"#,
        r#""1"^^xsd:integer"#
    ));
    // Signed zero: HermiT says consistent (`-0` and `+0` are distinct values);
    // Konclude says inconsistent. The new edges leave zero out, so this stays
    // consistent and cannot turn into an FP.
    assert!(negated(r#""0.0"^^xsd:double"#, r#""-0.0"^^xsd:double"#));
}

#[test]
fn negative_control_a_two_member_enumeration_is_not_a_point() {
    // `∃d.{3,4}` does not entail `∃d.{3}`.
    assert!(!c_sub_e(
        r#"SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("3"^^xsd:integer "4"^^xsd:integer)))
           SubClassOf(DataHasValue(:d "3"^^xsd:integer) :E)"#
    ));
}

#[test]
fn an_excluded_enumerated_value_is_inconsistent_in_every_datatype() {
    for (oneof, excluded) in [
        (r#""1"^^xsd:integer"#, r#""1"^^xsd:integer"#),
        (r#""1.5"^^xsd:decimal"#, r#""1.5"^^xsd:decimal"#),
        (r#""2.5"^^xsd:double"#, r#""2.5"^^xsd:double"#),
        (r#""2.5"^^xsd:float"#, r#""2.5"^^xsd:float"#),
        (
            r#""2020-01-01T00:00:00"^^xsd:dateTime"#,
            r#""2020-01-01T00:00:00"^^xsd:dateTime"#,
        ),
    ] {
        assert!(
            !negated(oneof, excluded),
            "a : ∃d.{{{oneof}}}, ¬d(a, {excluded}) is inconsistent (HermiT, Konclude)"
        );
    }
}

#[test]
fn an_enumeration_meets_the_range_bucket_both_ways() {
    // `{3} ⊑ [3,3]`.
    assert!(c_sub_e(
        r#"SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("3"^^xsd:integer)))
           SubClassOf(DataHasValue(:d "3"^^xsd:integer) :E)"#
    ));
    // `[3,3] ⊑ {3,4}`.
    assert!(c_sub_e(
        r#"SubClassOf(:C DataHasValue(:d "3"^^xsd:integer))
           SubClassOf(DataSomeValuesFrom(:d DataOneOf("3"^^xsd:integer "4"^^xsd:integer)) :E)"#
    ));
    // `{3,4} ⊑ [3, ∞)`.
    assert!(c_sub_e(
        r#"SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("3"^^xsd:integer "4"^^xsd:integer)))
           SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "3"^^xsd:integer)) :E)"#
    ));
}

/// Residual: excluding every member of a multi-member enumeration needs reasoning
/// by cases (`{1,2} ⊑ {1} ⊔ {2}`), which `DKey ⊑ DKey` edges cannot express.
/// `HermiT` and Konclude say inconsistent; rustdl says consistent.
#[test]
#[ignore = "#556 residual: needs the enumeration split into its points"]
fn excluding_every_member_of_a_multi_member_enumeration() {
    let o = onto(
        r#"ClassAssertion(DataSomeValuesFrom(:d DataOneOf("1"^^xsd:integer "2"^^xsd:integer)) :a)
           NegativeDataPropertyAssertion(:d :a "1"^^xsd:integer)
           NegativeDataPropertyAssertion(:d :a "2"^^xsd:integer)"#,
    );
    assert!(!owl_dl_reasoner::is_consistent(&o).expect("consistency"));
}
