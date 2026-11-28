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

/// `C ⊑ E` for `C ⊑ sub`, `sup ⊑ E`.
fn entails(sub: &str, sup: &str) -> bool {
    c_sub_e(&format!("SubClassOf(:C {sub})\nSubClassOf({sup} :E)"))
}

fn some(range: &str) -> String {
    format!("DataSomeValuesFrom(:d {range})")
}

fn has(lit: &str) -> String {
    format!("DataHasValue(:d {lit})")
}

/// Negative controls, one per direction and bucket shape; both oracles say no
/// (`HermiT` alone for signed zero, where Konclude says yes).
#[test]
fn negative_controls_per_direction() {
    let cases: [(&str, String, String); 6] = [
        (
            "a range with a value outside the enumeration (4 ∉ {3,5,7})",
            some(
                r#"DatatypeRestriction(xsd:integer xsd:minInclusive "3"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)"#,
            ),
            some(r#"DataOneOf("3"^^xsd:integer "5"^^xsd:integer "7"^^xsd:integer)"#),
        ),
        (
            "a dense decimal range is not its two endpoints",
            some(
                r#"DatatypeRestriction(xsd:decimal xsd:minInclusive "1.5"^^xsd:decimal xsd:maxInclusive "2.5"^^xsd:decimal)"#,
            ),
            some(r#"DataOneOf("1.5"^^xsd:decimal "2.5"^^xsd:decimal)"#),
        ),
        (
            "-0 is not the enumerated +0 (HermiT; Konclude says yes)",
            has(r#""-0.0"^^xsd:double"#),
            some(r#"DataOneOf("0.0"^^xsd:double)"#),
        ),
        (
            "an xsd:float enumeration is not an xsd:double point",
            some(r#"DataOneOf("2.5"^^xsd:float)"#),
            has(r#""2.5"^^xsd:double"#),
        ),
        (
            "an exclusive bound leaves out its endpoint",
            some(r#"DataOneOf("1.5"^^xsd:float)"#),
            some(r#"DatatypeRestriction(xsd:float xsd:minExclusive "1.5"^^xsd:float)"#),
        ),
        (
            "an integer outside a derived type's range (300 ∉ xsd:byte)",
            some(r#"DataOneOf("300"^^xsd:integer)"#),
            some("xsd:byte"),
        ),
    ];
    for (what, sub, sup) in cases {
        assert!(!entails(&sub, &sup), "{what}: no subsumption");
    }
    // `xsd:date` is outside the OWL 2 datatype map (HermiT rejects it); Konclude
    // says no here and yes on the twin below.
    assert!(!entails(
        &some(r#"DataOneOf("2020-06-01"^^xsd:date)"#),
        &has(r#""2020-06-02"^^xsd:date"#)
    ));
}

/// Positives per bucket and direction; both oracles say yes unless noted.
#[test]
fn positives_per_direction_and_bucket() {
    let cases: [(&str, String, String); 7] = [
        (
            "decimal point ⊑ enumeration (\"1.50\" is 1.5)",
            has(r#""1.50"^^xsd:decimal"#),
            some(r#"DataOneOf("1.5"^^xsd:decimal "2.5"^^xsd:decimal)"#),
        ),
        (
            "dateTime point ⊑ enumeration",
            has(r#""2020-06-01T00:00:00"^^xsd:dateTime"#),
            some(
                r#"DataOneOf("2020-06-01T00:00:00"^^xsd:dateTime "2021-06-01T00:00:00"^^xsd:dateTime)"#,
            ),
        ),
        (
            "finite integer range ⊑ enumeration ((2,6) is {3,4,5}; HermiT, Konclude under-reports)",
            some(
                r#"DatatypeRestriction(xsd:integer xsd:minExclusive "2"^^xsd:integer xsd:maxExclusive "6"^^xsd:integer)"#,
            ),
            some(
                r#"DataOneOf("3"^^xsd:integer "4"^^xsd:integer "5"^^xsd:integer "7"^^xsd:integer)"#,
            ),
        ),
        (
            "xsd:float rounding (16777217 and 16777216 are one f32)",
            some(r#"DataOneOf("16777217"^^xsd:float)"#),
            has(r#""16777216"^^xsd:float"#),
        ),
        (
            "an integer inside a derived type (100 ∈ xsd:byte)",
            some(r#"DataOneOf("100"^^xsd:integer)"#),
            some("xsd:byte"),
        ),
        (
            "a zero strictly inside the range",
            some(r#"DataOneOf("0.0"^^xsd:double "1.0"^^xsd:double)"#),
            some(r#"DatatypeRestriction(xsd:double xsd:minInclusive "-1.0"^^xsd:double)"#),
        ),
        (
            "xsd:date enumeration ⊑ point (Konclude; HermiT rejects xsd:date)",
            some(r#"DataOneOf("2020-06-01"^^xsd:date)"#),
            has(r#""2020-06-01"^^xsd:date"#),
        ),
    ];
    for (what, sub, sup) in cases {
        assert!(entails(&sub, &sup), "{what}: subsumed");
    }
}

/// `{+0} ⊑ hasValue(+0)`. `HermiT` and Konclude derive it. It was a #556 residual
/// while the buckets collapsed `-0` and `+0`; #590 keeps the zeros apart (sign-exact
/// enumeration members, a literal zero point holds one zero), so the edge is exact.
#[test]
fn an_enumerated_zero_against_a_zero_point() {
    assert!(entails(
        &some(r#"DataOneOf("0.0"^^xsd:double)"#),
        &has(r#""0.0"^^xsd:double"#)
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

/// The `TBox` form of the residual above; it reads `completeness_guaranteed: true`
/// (`docs/known-limitations/dkey-enumeration-case-split-and-signed-zero.md`).
#[test]
#[ignore = "#556 residual: needs the enumeration split into its points"]
fn a_multi_member_enumeration_entails_what_each_member_does() {
    assert!(c_sub_e(
        r#"SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
           SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
           SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)"#
    ));
}

/// #556 review (draft B): a facet literal of another datatype is not read as the
/// restricted type. `xsd:float maxInclusive "0.1"^^xsd:double` used to be read as
/// the float 0.1, a different number (f32 0.1 > f64 0.1), and `xsd:decimal
/// minInclusive "0.1"^^xsd:double` as the decimal 0.1 (decimal 0.1 < f64 0.1). Both
/// gave false subsumptions, for `DataHasValue` on main and for `DataOneOf` with the
/// new edges. `HermiT` rejects the inputs (`UnsupportedFacetException`); rustdl now
/// drops the restriction and says so in `dropped`. None may derive anything.
#[test]
fn a_facet_literal_of_another_datatype_derives_nothing_and_is_dropped() {
    let float_facet = some(r#"DatatypeRestriction(xsd:float xsd:maxInclusive "0.1"^^xsd:double)"#);
    let dec_facet = some(r#"DatatypeRestriction(xsd:decimal xsd:minInclusive "0.1"^^xsd:double)"#);
    let double_facet = some(r#"DatatypeRestriction(xsd:double xsd:maxInclusive "0.1"^^xsd:float)"#);
    for (what, sub, sup) in [
        (
            "float enumeration",
            some(r#"DataOneOf("0.1"^^xsd:float)"#),
            &float_facet,
        ),
        ("float value", has(r#""0.1"^^xsd:float"#), &float_facet),
        (
            "smaller float enumeration",
            some(r#"DataOneOf("0.05"^^xsd:float)"#),
            &float_facet,
        ),
        (
            "decimal enumeration",
            some(r#"DataOneOf("0.1"^^xsd:decimal)"#),
            &dec_facet,
        ),
        ("decimal value", has(r#""0.1"^^xsd:decimal"#), &dec_facet),
        (
            "double enumeration",
            some(r#"DataOneOf("0.1"^^xsd:double)"#),
            &double_facet,
        ),
    ] {
        let body = format!("SubClassOf(:C {sub})\nSubClassOf({sup} :E)");
        assert!(
            !c_sub_e(&body),
            "{what}: nothing is derived from a mistyped facet"
        );
        let h = owl_dl_reasoner::classify(&onto(&body)).expect("classify");
        assert!(
            !h.stats().dropped.is_empty(),
            "{what}: the mistyped restriction is reported in `dropped`"
        );
    }
}

/// The control for the test above: an integer literal on an `xsd:decimal` facet is a
/// decimal and is kept (`HermiT` and Konclude derive this).
#[test]
fn an_integer_literal_on_a_decimal_facet_is_kept() {
    let body = format!(
        "SubClassOf(:C {})\nSubClassOf({} :E)",
        some(r#"DataOneOf("3"^^xsd:decimal)"#),
        some(r#"DatatypeRestriction(xsd:decimal xsd:minInclusive "2"^^xsd:integer)"#)
    );
    assert!(c_sub_e(&body));
    let h = owl_dl_reasoner::classify(&onto(&body)).expect("classify");
    assert!(h.stats().dropped.is_empty(), "nothing dropped");
}
