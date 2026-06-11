//! Canaries for DP-1: ABox **data-property-range violation** ⇒ inconsistency.
//!
//! `DataPropertyAssertion(p, a, lit)` + `DataPropertyRange(q, R)` (q a
//! reflexive super-data-property of p) forces `lit ∈ R`. When the literal's
//! value-space family is disjoint from R's, the value cannot be in range ⇒
//! the ontology has no model. Detected at convert time (`data_axioms.rs`
//! emits `Top ⊑ Bot`).
//!
//! NEGATIVES-FIRST: a false `Inconsistent` marks EVERY class unsatisfiable —
//! the catastrophic FP. Every "stays consistent" assertion below guards that.
//! In particular `int_value_on_decimal_range_is_consistent` pins the
//! merged-numeric rule (the `int ⊆ decimal` trap) and the union/unknown/
//! wrong-property/wrong-direction cases pin the hard gates.
//!
//! Run: `cargo test -p owl-dl-reasoner --test datatype_inconsistency`.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::io::Cursor;

const PFX: &str = r"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
";

fn consistent(body: &str) -> bool {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent")
}

// ─── NEGATIVES (must stay consistent — the FP gate) ──────────────────

/// Value in the declared range's family ⇒ no violation. (wine pattern:
/// `"1998"^^xsd:positiveInteger` on a `xsd:positiveInteger` range.)
#[test]
fn value_in_range_family_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p xsd:positiveInteger)
    DataPropertyAssertion(:p :a "1998"^^xsd:positiveInteger)"#
    ));
}

/// THE `int ⊆ decimal` TRAP: an `xsd:int` value on a `xsd:decimal` range is
/// VALID (int ⊆ decimal). All numerics share one merged family, so DP-1
/// must NOT flag — guards the catastrophic false-inconsistent.
#[test]
fn int_value_on_decimal_range_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p xsd:decimal)
    DataPropertyAssertion(:p :a "5"^^xsd:int)"#
    ));
}

/// unsignedLong value on an integer range — both numeric, valid.
#[test]
fn unsigned_value_on_integer_range_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p xsd:integer)
    DataPropertyAssertion(:p :a "1394"^^xsd:unsignedLong)"#
    ));
}

/// Union range ⇒ NOT a single value space; never flagged (a value outside
/// one disjunct may be in another). Hard gate against the union FP.
#[test]
fn value_against_union_range_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DataUnionOf(xsd:string xsd:integer))
    DataPropertyAssertion(:p :a "5"^^xsd:integer)"#
    ));
}

/// Unknown/custom datatype range ⇒ unclassifiable family ⇒ never flagged.
#[test]
fn unknown_datatype_range_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p <http://t/MyType>)
    DataPropertyAssertion(:p :a "x"^^xsd:string)"#
    ));
}

/// Range is on an UNRELATED property — does not constrain p's values.
#[test]
fn range_on_unrelated_property_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:q xsd:integer)
    DataPropertyAssertion(:p :a "text"^^xsd:string)"#
    ));
}

/// Wrong subproperty direction: range on the SUB-property `q` does NOT
/// constrain values of the SUPER-property `p` (only super→sub propagates).
#[test]
fn range_on_subproperty_does_not_constrain_superproperty() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(NamedIndividual(:a))
    SubDataPropertyOf(:q :p)
    DataPropertyRange(:q xsd:integer)
    DataPropertyAssertion(:p :a "text"^^xsd:string)"#
    ));
}

// ─── POSITIVES (genuine violations ⇒ inconsistent) ───────────────────

/// 2749 pattern: plain `xsd:string` literal on a numeric (`xsd:unsignedLong`)
/// range — string and numeric value spaces are disjoint.
#[test]
fn string_value_on_numeric_range_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p xsd:unsignedLong)
    DataPropertyAssertion(:p :a "1394")"#
    ));
}

/// 8941 pattern: a language-tagged literal (`rdf:langString`) on an
/// `xsd:string` range — disjoint datatypes.
#[test]
fn langstring_value_on_string_range_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p xsd:string)
    DataPropertyAssertion(:p :a "Managergehälter"@de)"#
    ));
}

/// Boolean value on a temporal range — disjoint families.
#[test]
fn boolean_value_on_datetime_range_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p xsd:dateTime)
    DataPropertyAssertion(:p :a "true"^^xsd:boolean)"#
    ));
}

/// Subproperty propagation (correct direction): range on the SUPER `q`
/// constrains the SUB `p`'s values ⇒ a string value on a numeric super-range
/// is a violation.
#[test]
fn string_value_violates_superproperty_numeric_range() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(NamedIndividual(:a))
    SubDataPropertyOf(:p :q)
    DataPropertyRange(:q xsd:integer)
    DataPropertyAssertion(:p :a "text"^^xsd:string)"#
    ));
}

// ─── DP-1b: string DataOneOf enumeration membership ──────────────────

/// NEGATIVE: asserted value IS a member of the enumeration ⇒ consistent.
#[test]
fn value_in_string_oneof_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DataOneOf("all" "driver"))
    DataPropertyAssertion(:p :a "driver")"#
    ));
}

/// NEGATIVE: a non-string value against a string enumeration is NOT handled
/// (DP-1b is string-only) ⇒ must NOT flag (under-approximation, sound).
#[test]
fn nonstring_value_on_string_oneof_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DataOneOf("all" "driver"))
    DataPropertyAssertion(:p :a "5"^^xsd:integer)"#
    ));
}

/// NEGATIVE: enumeration on an unrelated property doesn't constrain p.
#[test]
fn string_oneof_on_unrelated_property_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:q DataOneOf("all" "driver"))
    DataPropertyAssertion(:p :a "anything")"#
    ));
}

/// POSITIVE (13219 pattern): asserted string NOT in the enumeration ⇒
/// inconsistent. The empty string is the real ore_ont_13219 culprit.
#[test]
fn value_not_in_string_oneof_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DataOneOf("all" "driver" "driver and front passenger"))
    DataPropertyAssertion(:p :a "")"#
    ));
}

/// POSITIVE: enumeration on the SUPER-property constrains the SUB's values.
#[test]
fn value_violates_superproperty_string_oneof() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(NamedIndividual(:a))
    SubDataPropertyOf(:p :q)
    DataPropertyRange(:q DataOneOf("yes" "no"))
    DataPropertyAssertion(:p :a "maybe")"#
    ));
}

// ─── DP-2: data-cardinality (≤n dp) with >n distinct string values ───

/// POSITIVE (12174 pattern): `C ⊑ ≤1 p` (unqualified) + an individual with two
/// distinct string values ("L" and "L ") ⇒ inconsistent.
#[test]
fn two_distinct_strings_on_max1_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    ClassAssertion(:C :a)
    SubClassOf(:C DataMaxCardinality(1 :p))
    DataPropertyAssertion(:p :a "L")
    DataPropertyAssertion(:p :a "L ")"#
    ));
}

/// POSITIVE: typing via told-subclass + filler via sub-property both route in.
#[test]
fn cardinality_via_subclass_and_subproperty_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(NamedIndividual(:a))
    SubClassOf(:D :C)
    SubClassOf(:C DataMaxCardinality(1 :p))
    SubDataPropertyOf(:q :p)
    ClassAssertion(:D :a)
    DataPropertyAssertion(:p :a "x")
    DataPropertyAssertion(:q :a "y")"#
    ));
}

/// THE QUALIFIED-CARDINALITY GATE: `≤1 p xsd:integer` bounds only INTEGER
/// fillers; two distinct STRING values don't count ⇒ must stay consistent.
/// Guards the false-Inconsistent from counting strings against a numeric bound.
#[test]
fn two_strings_on_integer_qualified_max1_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    ClassAssertion(:C :a)
    SubClassOf(:C DataMaxCardinality(1 :p xsd:integer))
    DataPropertyAssertion(:p :a "x")
    DataPropertyAssertion(:p :a "y")"#
    ));
}

/// NEGATIVE: count not exceeded (2 distinct strings, ≤2) ⇒ consistent.
#[test]
fn distinct_count_within_bound_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    ClassAssertion(:C :a)
    SubClassOf(:C DataMaxCardinality(2 :p))
    DataPropertyAssertion(:p :a "x")
    DataPropertyAssertion(:p :a "y")"#
    ));
}

/// NEGATIVE: the SAME string asserted twice is ONE distinct value ⇒ consistent.
#[test]
fn duplicate_string_on_max1_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    ClassAssertion(:C :a)
    SubClassOf(:C DataMaxCardinality(1 :p))
    DataPropertyAssertion(:p :a "same")
    DataPropertyAssertion(:p :a "same")"#
    ));
}

/// NEGATIVE: individual is NOT (told) typed the constrained class ⇒ consistent.
#[test]
fn cardinality_untyped_individual_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p))
    DataPropertyAssertion(:p :a "x")
    DataPropertyAssertion(:p :a "y")"#
    ));
}
