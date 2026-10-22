//! #314 — `rdfs:Literal`, `xsd:boolean` and `xsd:anyURI` were dropped as
//! unsupported data ranges, so classes `HermiT` finds unsatisfiable came back
//! satisfiable (with the drop reported).
//!
//! - `rdfs:Literal` is the top data range: `∃p.rdfs:Literal` is `∃p.⊤`, and
//!   `∀p.rdfs:Literal` and `DataPropertyRange(p, rdfs:Literal)` are trivially
//!   true. It lowers to `⊤`, with no `DKey`.
//! - `xsd:boolean` and `xsd:anyURI` get their own `StrSet` `DKey` buckets
//!   (`bool:`, `uri:`). Bare `xsd:boolean` is the exact set `{false, true}`.
//!
//! Every verdict below matches `HermiT` 1.4.5 on the same ontology. The curated
//! corpus has no such axioms, so these tests are the safety net.

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

const C: &str = "http://example.org/C";
const D: &str = "http://example.org/D";

fn onto(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://example.org/>)\n\
         Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://example.org/onto>\n\
         Declaration(DataProperty(:p))\n\
         Declaration(DataProperty(:q))\n\
         Declaration(Class(:C))\n\
         Declaration(Class(:D))\n\
         {axioms}\n\
         )\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse ofn")
        .0
}

/// Classify, insisting nothing was dropped: every axiom here must convert.
fn classify(o: &SetOntology<RcStr>) -> owl_dl_reasoner::Classification {
    let dropped = dropped_axioms(o).unwrap();
    assert!(dropped.is_empty(), "axioms dropped: {dropped:?}");
    classify_top_down_with_timeout(o, Duration::from_secs(10)).expect("classify")
}

/// `C` is unsatisfiable through both `classify` and `sat`, the two surfaces the
/// issue names.
fn assert_c_unsat(axioms: &str) {
    let o = onto(axioms);
    let unsat = classify(&o).unsatisfiable_classes().contains(&C);
    assert!(
        unsat,
        "classify: C should be unsatisfiable (HermiT): {axioms}"
    );
    assert!(
        !is_class_satisfiable(&o, C).unwrap(),
        "sat: C should be unsatisfiable (HermiT): {axioms}"
    );
}

fn assert_c_sat(axioms: &str) {
    let o = onto(axioms);
    let unsat = classify(&o).unsatisfiable_classes().contains(&C);
    assert!(
        !unsat,
        "classify: C should be satisfiable (HermiT): {axioms}"
    );
    assert!(
        is_class_satisfiable(&o, C).unwrap(),
        "sat: C should be satisfiable (HermiT): {axioms}"
    );
}

fn c_sub_d(axioms: &str) -> bool {
    classify(&onto(axioms)).is_subclass(C, D)
}

// ── rdfs:Literal ─────────────────────────────────────────────────────────

/// The issue's first row: `q ⊑ p`, `p` and `q` disjoint, so no `q`-value can
/// exist, and `C` needs one.
#[test]
fn literal_some_on_a_disjoint_sub_property_is_unsat() {
    assert_c_unsat(
        "SubDataPropertyOf(:q :p) DisjointDataProperties(:p :q) \
         SubClassOf(:C DataSomeValuesFrom(:q rdfs:Literal))",
    );
}

/// `DataComplementOf(rdfs:Literal)` is empty, so needing a value in it is
/// unsatisfiable.
#[test]
fn literal_complement_is_empty() {
    assert_c_unsat("SubClassOf(:C DataSomeValuesFrom(:p DataComplementOf(rdfs:Literal)))");
}

/// `∀p.¬rdfs:Literal` forbids every `p`-value, and `C` needs one.
#[test]
fn forall_outside_literal_forbids_every_value() {
    assert_c_unsat(
        "SubClassOf(:C DataAllValuesFrom(:p DataComplementOf(rdfs:Literal))) \
         SubClassOf(:C DataSomeValuesFrom(:p xsd:integer))",
    );
}

/// Control: a `rdfs:Literal` range and `∀` are trivially true and must not
/// make anything unsatisfiable.
#[test]
fn literal_range_and_forall_are_trivial() {
    assert_c_sat(
        "DataPropertyRange(:p rdfs:Literal) \
         SubClassOf(:C DataAllValuesFrom(:p rdfs:Literal)) \
         SubClassOf(:C DataSomeValuesFrom(:p xsd:integer))",
    );
}

/// Any data value is a literal: `∃p.xsd:integer ⊑ ∃p.rdfs:Literal`.
#[test]
fn some_value_of_any_datatype_is_some_literal() {
    assert!(c_sub_d(
        "SubClassOf(:C DataSomeValuesFrom(:p xsd:integer)) \
         EquivalentClasses(:D DataSomeValuesFrom(:p rdfs:Literal))"
    ));
}

// ── xsd:boolean ──────────────────────────────────────────────────────────

/// `"1"` is the lexical form of `true`, which is outside `{false}`.
#[test]
fn boolean_value_outside_enumeration_is_unsat() {
    assert_c_unsat(
        "SubClassOf(:C DataHasValue(:p \"1\"^^xsd:boolean)) \
         SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"false\"^^xsd:boolean)))",
    );
}

/// Control: `"1"` and `"true"` are the same value.
#[test]
fn boolean_lexical_forms_are_one_value() {
    assert_c_sat(
        "SubClassOf(:C DataHasValue(:p \"1\"^^xsd:boolean)) \
         SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"true\"^^xsd:boolean)))",
    );
}

/// A functional property cannot hold both `true` and `false`.
#[test]
fn functional_boolean_with_both_values_is_unsat() {
    assert_c_unsat(
        "FunctionalDataProperty(:p) \
         SubClassOf(:C DataHasValue(:p \"true\"^^xsd:boolean)) \
         SubClassOf(:C DataHasValue(:p \"false\"^^xsd:boolean))",
    );
}

/// `{true} ⊆ xsd:boolean`, so `∃p.{true} ⊑ ∃p.xsd:boolean`. Bare
/// `xsd:boolean` must key to the exact set `{false, true}` for this to hold.
#[test]
fn a_boolean_value_is_in_xsd_boolean() {
    assert!(c_sub_d(
        "SubClassOf(:C DataHasValue(:p \"true\"^^xsd:boolean)) \
         EquivalentClasses(:D DataSomeValuesFrom(:p xsd:boolean))"
    ));
}

/// `xsd:boolean` has exactly two values, so it is `DataOneOf(false, true)`.
/// This needs bare `xsd:boolean` to key to that exact set rather than `Top`.
#[test]
fn xsd_boolean_is_the_two_value_enumeration() {
    assert!(c_sub_d(
        "SubClassOf(:C DataSomeValuesFrom(:p xsd:boolean)) \
         EquivalentClasses(:D DataSomeValuesFrom(:p \
           DataOneOf(\"false\"^^xsd:boolean \"true\"^^xsd:boolean)))"
    ));
}

/// FP guard: the STRING `"true"` is not a boolean. A shared bucket with `str:`
/// would derive this.
#[test]
fn a_string_true_is_not_a_boolean() {
    assert!(!c_sub_d(
        "SubClassOf(:C DataHasValue(:p \"true\"^^xsd:string)) \
         EquivalentClasses(:D DataSomeValuesFrom(:p xsd:boolean))"
    ));
}

/// A boolean assertion outside a boolean range makes the KB inconsistent.
#[test]
fn boolean_assertion_outside_range_is_inconsistent() {
    let o = onto(
        "DataPropertyRange(:p DataOneOf(\"true\"^^xsd:boolean)) \
         DataPropertyAssertion(:p :a \"0\"^^xsd:boolean)",
    );
    assert!(dropped_axioms(&o).unwrap().is_empty());
    assert!(!is_consistent(&o).unwrap());
}

// ── xsd:anyURI ───────────────────────────────────────────────────────────

#[test]
fn anyuri_value_outside_enumeration_is_unsat() {
    assert_c_unsat(
        "SubClassOf(:C DataHasValue(:p \"http://a\"^^xsd:anyURI)) \
         SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"http://b\"^^xsd:anyURI)))",
    );
}

/// Control: any `anyURI` value is in `xsd:anyURI`.
#[test]
fn anyuri_value_inside_anyuri_is_sat() {
    assert_c_sat(
        "SubClassOf(:C DataHasValue(:p \"http://a\"^^xsd:anyURI)) \
         SubClassOf(:C DataAllValuesFrom(:p xsd:anyURI))",
    );
}

/// FP guard: a string that looks like a URI is not an `anyURI` value.
#[test]
fn a_uri_shaped_string_is_not_an_anyuri() {
    assert!(!c_sub_d(
        "SubClassOf(:C DataHasValue(:p \"http://a\"^^xsd:string)) \
         EquivalentClasses(:D DataSomeValuesFrom(:p xsd:anyURI))"
    ));
}

// ── Disjointness from the other datatypes (#315's families) ─────────────

/// The issue's range rows: `C` needs an integer `p`-value, and every `p`-value is
/// a boolean or an anyURI.
#[test]
fn boolean_and_anyuri_ranges_exclude_integers() {
    for dt in ["xsd:boolean", "xsd:anyURI"] {
        assert_c_unsat(&format!(
            "DataPropertyRange(:p {dt}) SubClassOf(:C DataSomeValuesFrom(:p xsd:integer))"
        ));
    }
}

/// Every pair checked against `HermiT`, in both directions where it matters.
/// A bare `xsd:anyURI` range excludes plain strings (unlike bare
/// `rdf:langString`, which #315 leaves out of its family).
#[test]
fn boolean_and_anyuri_are_disjoint_from_strings_and_each_other() {
    for (range, value) in [
        ("xsd:anyURI", "\"http://a\"^^xsd:string"),
        ("xsd:string", "\"http://a\"^^xsd:anyURI"),
        ("xsd:boolean", "\"true\"^^xsd:string"),
        ("xsd:string", "\"true\"^^xsd:boolean"),
        ("xsd:boolean", "\"http://a\"^^xsd:anyURI"),
        ("xsd:anyURI", "\"2020-01-01T00:00:00\"^^xsd:dateTime"),
        ("DataOneOf(\"x\"@en)", "\"http://a\"^^xsd:anyURI"),
    ] {
        assert_c_unsat(&format!(
            "DataPropertyRange(:p {range}) SubClassOf(:C DataHasValue(:p {value}))"
        ));
    }
}

/// Control: `rdfs:Literal` is in no family, so a `rdfs:Literal` range admits
/// values of every datatype.
#[test]
fn a_literal_range_admits_booleans_and_uris() {
    for value in ["\"true\"^^xsd:boolean", "\"http://a\"^^xsd:anyURI"] {
        assert_c_sat(&format!(
            "DataPropertyRange(:p rdfs:Literal) SubClassOf(:C DataHasValue(:p {value}))"
        ));
    }
}

/// `DataPropertyRange(p, xsd:boolean)` and `(p, xsd:anyURI)` convert.
#[test]
fn boolean_and_anyuri_ranges_are_not_dropped() {
    for dt in ["xsd:boolean", "xsd:anyURI"] {
        let o = onto(&format!(
            "DataPropertyRange(:p {dt}) SubClassOf(:C DataSomeValuesFrom(:p xsd:integer))"
        ));
        let dropped = dropped_axioms(&o).unwrap();
        assert!(dropped.is_empty(), "{dt}: dropped {dropped:?}");
    }
}
