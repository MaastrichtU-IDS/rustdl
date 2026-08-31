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
use owl_dl_reasoner::{classify, is_consistent};
use std::io::Cursor;
use std::sync::Mutex;

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

// ─── DP-1 value-level: same-bucket range violations ───────────────────
//
// These canaries cover the new `emit_data_range_value_violations` path.
// The spec calls them "DP-1 value-level": `DataPropertyRange(p, R)` +
// `DataPropertyAssertion(p, a, lit)` where `lit` and `R` share a
// datatype bucket but the value falls outside the bounds.
//
// Negatives (soundness guard): in-range, no range, exclusive boundary
// equals, and sub/super-property direction tests MUST stay consistent.

// ── integer value-level ──────────────────────────────────────────────

/// NEGATIVE: value 5 ∈ [0,10] ⇒ consistent.
#[test]
fn integer_value_in_range_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer))
    DataPropertyAssertion(:p :a "5"^^xsd:integer)"#
    ));
}

/// NEGATIVE: boundary inclusive — value 0 ∈ [>=0] ⇒ consistent.
#[test]
fn integer_value_at_inclusive_lower_bound_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer))
    DataPropertyAssertion(:p :a "0"^^xsd:integer)"#
    ));
}

/// POSITIVE: value -5 ∉ [>=0] ⇒ inconsistent.
#[test]
fn integer_value_below_min_inclusive_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer))
    DataPropertyAssertion(:p :a "-5"^^xsd:integer)"#
    ));
}

/// POSITIVE: boundary exclusive — value 0 ∉ [>0] ⇒ inconsistent.
#[test]
fn integer_value_at_exclusive_lower_bound_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:integer xsd:minExclusive "0"^^xsd:integer))
    DataPropertyAssertion(:p :a "0"^^xsd:integer)"#
    ));
}

/// POSITIVE: value 15 ∉ [0,10] ⇒ inconsistent.
#[test]
fn integer_value_above_max_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer))
    DataPropertyAssertion(:p :a "15"^^xsd:integer)"#
    ));
}

/// NEGATIVE: no range declared for p — free assertion, consistent.
#[test]
fn integer_value_with_no_range_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyAssertion(:p :a "-99"^^xsd:integer)"#
    ));
}

// ── double value-level (xsd:double is f64-exact; xsd:float is DROPPED) ──

/// NEGATIVE: value 0.5 ∈ [0.0, 1.0] ⇒ consistent.
#[test]
fn double_value_in_range_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:double xsd:minInclusive "0.0"^^xsd:double xsd:maxInclusive "1.0"^^xsd:double))
    DataPropertyAssertion(:p :a "0.5"^^xsd:double)"#
    ));
}

/// POSITIVE: value 2.0 ∉ [0.0, 1.0] ⇒ inconsistent. (xsd:double, f64-exact.)
#[test]
fn double_value_outside_range_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:double xsd:minInclusive "0.0"^^xsd:double xsd:maxInclusive "1.0"^^xsd:double))
    DataPropertyAssertion(:p :a "2.0"^^xsd:double)"#
    ));
}

/// CATASTROPHIC-FALSE-FIRE REGRESSION GUARD (xsd:float f32/f64 mismatch).
///
/// Bound `0.1000000014` and value `0.1000000015` denote the SAME f32 value
/// (`0x3DCCCCCD`), so the value IS in range `[.., 0.1000000014]` in the
/// xsd:float value space ⇒ the ontology is CONSISTENT. With f32-exact parsing
/// (parse-as-f32-then-widen), both lexicals widen to the same f64 bit pattern,
/// so they map to the same `DKey` point and the range check correctly reports
/// the value as in-range. A naive f64 parse of both would produce distinct f64
/// values and falsely fire `Top ⊑ Bot`.
#[test]
fn float_boundary_f32_f64_mismatch_stays_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:float xsd:maxInclusive "0.1000000014"^^xsd:float))
    DataPropertyAssertion(:p :a "0.1000000015"^^xsd:float)"#
    ));
}

/// POSITIVE: value 2.0^xsd:float ∉ [0.0, 1.0]^xsd:float ⇒ INCONSISTENT.
/// With f32-exact DKey parsing, 2.0 and the bounds round to exact f32 values,
/// all land in separate DKey points/ranges, and the range disjointness fires.
/// (Previously this was dropped entirely; now xsd:float is sound+complete.)
#[test]
fn float_clearly_outside_range_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:float xsd:minInclusive "0.0"^^xsd:float xsd:maxInclusive "1.0"^^xsd:float))
    DataPropertyAssertion(:p :a "2.0"^^xsd:float)"#
    ));
}

// ── string DataOneOf (DP-1b, value-level string) ──────────────────────

/// NEGATIVE: value "a" ∈ {"a","b"} ⇒ consistent.
#[test]
fn string_value_in_oneof_range_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DataOneOf("a" "b"))
    DataPropertyAssertion(:p :a "a")"#
    ));
}

/// POSITIVE: value "c" ∉ {"a","b"} ⇒ inconsistent.
#[test]
fn string_value_outside_oneof_range_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DataOneOf("a" "b"))
    DataPropertyAssertion(:p :a "c")"#
    ));
}

// ── super-property propagation ────────────────────────────────────────

/// POSITIVE: range on the SUPER-property q, violation via sub-property p.
#[test]
fn integer_value_violates_superproperty_range() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(NamedIndividual(:a))
    SubDataPropertyOf(:p :q)
    DataPropertyRange(:q DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer))
    DataPropertyAssertion(:p :a "-1"^^xsd:integer)"#
    ));
}

/// NEGATIVE: range on the SUB-property q does NOT constrain super p ⇒ consistent.
#[test]
fn integer_value_range_on_subproperty_does_not_constrain_super() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(NamedIndividual(:a))
    SubDataPropertyOf(:q :p)
    DataPropertyRange(:q DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer))
    DataPropertyAssertion(:p :a "-1"^^xsd:integer)"#
    ));
}

// ── unparseable literal ───────────────────────────────────────────────

/// NEGATIVE: unparseable literal for the range's type ⇒ don't fire (sound
/// under-approximation), stays consistent.
#[test]
fn unparseable_integer_literal_stays_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer))
    DataPropertyAssertion(:p :a "not-an-int"^^xsd:integer)"#
    ));
}

// ─── DP-2: FunctionalDataProperty ABox cardinality violation ──────────
//
// `FunctionalDataProperty(f)` means individual a has AT MOST ONE f-value.
// Two provably-distinct values (different literals mapping to different
// DistinctVal keys) → inconsistent. Negatives (MUST stay consistent) are
// the catastrophic-FP guard: over-counting = false Top⊑Bot.

// ── POSITIVE (INCONSISTENT) cases ────────────────────────────────────

/// Two distinct xsd:integer values on a functional property ⇒ inconsistent.
#[test]
fn dp2_functional_two_integers_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "2"^^xsd:integer)"#
    ));
}

/// Two distinct xsd:double values on a functional property ⇒ inconsistent.
#[test]
fn dp2_functional_two_doubles_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "1.0"^^xsd:double)
    DataPropertyAssertion(:p :a "2.0"^^xsd:double)"#
    ));
}

/// `1^^xsd:integer` and `1.0^^xsd:double` are in DISJOINT OWL value spaces
/// (xsd:double's value space is disjoint from xsd:decimal/integer), so on a
/// functional property they are two distinct values ⇒ inconsistent. Locks the
/// cross-bucket distinctness decision (Num(Decimal) vs Double are NOT folded).
#[test]
fn dp2_functional_integer_and_double_distinct_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "1.0"^^xsd:double)"#
    ));
}

/// Two distinct xsd:string values (plain literals) ⇒ inconsistent.
#[test]
fn dp2_functional_two_strings_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "hello")
    DataPropertyAssertion(:p :a "world")"#
    ));
}

/// CROSS-BUCKET: one xsd:integer and one xsd:string value — disjoint
/// value spaces ⇒ provably distinct ⇒ inconsistent.
#[test]
fn dp2_functional_cross_bucket_integer_string_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "x")"#
    ));
}

/// CORRECT SUB-PROPERTY DIRECTION: functional SUPER + assertion on SUB
/// ⇒ sub-values count toward the super's ≤1 budget ⇒ inconsistent.
#[test]
fn dp2_functional_super_assertions_on_sub_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:f)) Declaration(DataProperty(:q))
    Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:f)
    SubDataPropertyOf(:q :f)
    DataPropertyAssertion(:q :a "1"^^xsd:integer)
    DataPropertyAssertion(:q :a "2"^^xsd:integer)"#
    ));
}

/// Mixed: one value on the functional property itself and another via its
/// sub-property; still two distinct values ⇒ inconsistent.
#[test]
fn dp2_functional_mixed_direct_and_sub_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(DataProperty(:f)) Declaration(DataProperty(:q))
    Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:f)
    SubDataPropertyOf(:q :f)
    DataPropertyAssertion(:f :a "1"^^xsd:integer)
    DataPropertyAssertion(:q :a "2"^^xsd:integer)"#
    ));
}

// ── NEGATIVE (CONSISTENT, the catastrophic-FP guard) ─────────────────

/// VALUE-DEDUP: "1"^^xsd:integer asserted twice is ONE distinct value ⇒
/// consistent. The core dedup guard: same value, two assertions.
#[test]
fn dp2_functional_same_integer_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)"#
    ));
}

/// LITERAL NORMALISATION: "1"^^xsd:integer and "01"^^xsd:integer denote the
/// SAME integer value (01 normalises to 1) ⇒ consistent. Proves the parser
/// deduplicates same-value-different-literal pairs (over-counting would
/// false-fire).
#[test]
fn dp2_functional_integer_normalisation_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "01"^^xsd:integer)"#
    ));
}

/// INTEGER/DECIMAL FOLD: "1"^^xsd:integer and "1"^^xsd:decimal denote the
/// SAME value (xsd:integer ⊆ xsd:decimal value space) ⇒ consistent. Proves
/// the two datatypes fold into one Num(Decimal) bucket — separate buckets
/// would count them as 2 distinct and false-fire.
#[test]
fn dp2_integer_and_decimal_same_value_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "1"^^xsd:decimal)"#
    ));
}

/// SINGLE VALUE: only one value asserted ⇒ consistent (≤1 is satisfied).
#[test]
fn dp2_functional_single_value_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "42"^^xsd:integer)"#
    ));
}

/// NOT FUNCTIONAL: two distinct values, but the property is NOT declared
/// functional ⇒ no ≤1 constraint ⇒ consistent.
#[test]
fn dp2_not_functional_two_values_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "2"^^xsd:integer)"#
    ));
}

/// NEGATIVE: two xsd:float literals that denote the SAME f32 value MUST
/// NOT false-fire as distinct. With f32-exact parsing, `"0.1000000014"` and
/// `"0.1000000015"` both parse to f32 `0x3DCCCCCD` and widen to the same f64
/// bit pattern → same `DKey` point → NOT two distinct values → NOT inconsistent.
#[test]
fn dp2_functional_xsd_float_same_f32_value_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "0.1000000014"^^xsd:float)
    DataPropertyAssertion(:p :a "0.1000000015"^^xsd:float)"#
    ));
}

/// WRONG CLOSURE DIRECTION: functional SUB-property `q` + assertions only
/// on the SUPER-property `f` ⇒ must NOT fire. The super-property's values
/// don't count toward a sub-property's ≤1. This pinpoints the unsound
/// direction (closure[super] ∌ q).
#[test]
fn dp2_functional_sub_assertions_on_super_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:f)) Declaration(DataProperty(:q))
    Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:q)
    SubDataPropertyOf(:q :f)
    DataPropertyAssertion(:f :a "1"^^xsd:integer)
    DataPropertyAssertion(:f :a "2"^^xsd:integer)"#
    ));
}

/// UNRELATED PROPERTY: functional p but assertions on unrelated q ⇒
/// consistent.
#[test]
fn dp2_functional_unrelated_property_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q))
    Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:q :a "1"^^xsd:integer)
    DataPropertyAssertion(:q :a "2"^^xsd:integer)"#
    ));
}

/// DP-1 COEXISTENCE: a plain DP-1-consistent ontology (integer value in a
/// numeric range) stays consistent alongside a DP-2-safe assertion (single
/// value on a functional property). Verifies neither check interferes.
#[test]
fn dp2_dp1_coexistence_is_consistent() {
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q))
    Declaration(NamedIndividual(:a))
    DataPropertyRange(:q xsd:integer)
    DataPropertyAssertion(:q :a "5"^^xsd:integer)
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "hello")"#
    ));
}

// ─── DP-2b: typed/faceted from-type data-cardinality ──────────────────

/// FIRES: C ⊑ ≤2 dp.xsd:integer, individual:C with 3 distinct integers.
#[test]
fn typed_card_three_integers_over_max_two_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(2 :p xsd:integer))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "2"^^xsd:integer)
    DataPropertyAssertion(:p :a "3"^^xsd:integer)"#
    ));
}

/// FIRES: faceted range [0,10], ≤1, two distinct in-range values.
#[test]
fn typed_card_faceted_range_two_in_range_over_max_one_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "3"^^xsd:integer)
    DataPropertyAssertion(:p :a "7"^^xsd:integer)"#
    ));
}

/// FIRES: DataExactCardinality(1) behaves as ≤1.
#[test]
fn typed_card_exact_one_two_values_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataExactCardinality(1 :p xsd:double))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "1.0"^^xsd:double)
    DataPropertyAssertion(:p :a "2.0"^^xsd:double)"#
    ));
}

/// FIRES: values split across dp and a sub-dp dp' ⊑ dp sum past n.
#[test]
fn typed_card_subproperty_routing_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(NamedIndividual(:a))
    SubDataPropertyOf(:q :p)
    SubClassOf(:C DataMaxCardinality(1 :p xsd:integer))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:q :a "2"^^xsd:integer)"#
    ));
}

// ── FP GUARDS (must stay consistent) ──

#[test]
fn typed_card_out_of_range_value_uncounted_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "5"^^xsd:integer)
    DataPropertyAssertion(:p :a "20"^^xsd:integer)"#
    ));
}

#[test]
fn typed_card_cross_datatype_uncounted_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p xsd:integer))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "2.0"^^xsd:double)"#
    ));
}

#[test]
fn typed_card_duplicate_values_count_once_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p xsd:integer))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "01"^^xsd:integer)"#
    ));
}

#[test]
fn typed_card_exclusive_boundary_uncounted_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxExclusive "5"^^xsd:integer)))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "2"^^xsd:integer)
    DataPropertyAssertion(:p :a "5"^^xsd:integer)"#
    ));
}

#[test]
fn typed_card_untyped_individual_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p xsd:integer))
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "2"^^xsd:integer)
    DataPropertyAssertion(:p :a "3"^^xsd:integer)"#
    ));
}

/// FIRES: ≤1 dp.xsd:dateTime with two distinct (tz-free) dateTimes.
#[test]
fn typed_card_two_datetimes_over_max_one_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p xsd:dateTime))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "2020-01-01T00:00:00"^^xsd:dateTime)
    DataPropertyAssertion(:p :a "2021-06-15T12:30:00"^^xsd:dateTime)"#
    ));
}

/// FP GUARD: faceted dateTime range, one in-range + one out-of-range ⇒ only
/// one counts ⇒ consistent.
#[test]
fn typed_card_datetime_out_of_range_uncounted_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p DatatypeRestriction(xsd:dateTime xsd:minInclusive "2020-01-01T00:00:00"^^xsd:dateTime xsd:maxInclusive "2020-12-31T23:59:59"^^xsd:dateTime)))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "2020-06-01T00:00:00"^^xsd:dateTime)
    DataPropertyAssertion(:p :a "2025-01-01T00:00:00"^^xsd:dateTime)"#
    ));
}

// ── xsd:float f32-exact new canaries ─────────────────────────────────
//
// These pin the behaviour of the new complete+sound xsd:float handling.
// The landmine is the f32/f64 value-identity problem: the fix is to
// parse xsd:float literals as f32 then widen to f64 so same-f32 lexicals
// produce the same DKey IRI.

/// NEGATIVE: two xsd:float literals that map to the same f32 on a
/// functional data property ⇒ NOT two distinct values ⇒ CONSISTENT.
/// (Complements `dp2_functional_xsd_float_same_f32_value_is_consistent`
/// with a different same-f32 pair: "1.0" repeated is trivially the same.)
#[test]
fn float_two_lexicals_same_f32_not_distinct_consistent() {
    // "1.0" and "1.00" both parse to f32 1.0 and widen to the same f64.
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "1.0"^^xsd:float)
    DataPropertyAssertion(:p :a "1.00"^^xsd:float)"#
    ));
}

/// POSITIVE: two GENUINELY DISTINCT f32 values on a functional data
/// property ⇒ INCONSISTENT (cannot have two different values for a
/// functional property).
#[test]
fn float_distinct_f32_values_functional_inconsistent() {
    // 1.0 and 2.0 are distinct f32 values; they widen to distinct f64 →
    // distinct DKeys → two distinct witnesses → violates FunctionalDataProperty.
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    FunctionalDataProperty(:p)
    DataPropertyAssertion(:p :a "1.0"^^xsd:float)
    DataPropertyAssertion(:p :a "2.0"^^xsd:float)"#
    ));
}

/// INVERTED 2026-08-30 (#86) — this asserted `consistent` and was WRONG.
///
/// The half of its rationale that stands: `xsd:float` and `xsd:double` are in
/// SEPARATE DKey buckets and must never cross-SUBSUME. That is untouched — #86
/// adds cross-bucket DISJOINTNESS, not subsumption.
///
/// The half that was wrong: it concluded that a float value therefore "does NOT
/// violate the double-typed range", reasoning from the implementation ("the float
/// assertion simply drops into the float bucket, which has no range constraint")
/// rather than from OWL 2. Disjoint value spaces mean the asserted value cannot
/// lie in the declared range, so the KB IS inconsistent.
///
/// Adjudicated on THIS EXACT FIXTURE, not on a similar one:
///   Konclude `consistency` → `false`
///   HermiT  `--consistency` → `owl:Thing is not satisfiable`
/// Both agree the ontology is inconsistent; rustdl now does too.
#[test]
fn float_value_vs_double_range_clashes_across_buckets() {
    // Float value "2.0" against a double range: the value spaces of xsd:float and
    // xsd:double are disjoint, so this violates the range regardless of the facet.
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    DataPropertyRange(:p DatatypeRestriction(xsd:double xsd:minInclusive "0.0"^^xsd:double xsd:maxInclusive "1.0"^^xsd:double))
    DataPropertyAssertion(:p :a "2.0"^^xsd:float)"#
    ));
}

// ─── DP-DJ: DisjointDataProperties same-value clash canaries ─────────
//
// NEGATIVES-FIRST: a spurious `Inconsistent` marks EVERY class as
// unsatisfiable — the catastrophic FP. The "stays consistent" tests guard
// that. ENV_MUTEX serialises all DP-DJ tests so no test observes a
// mid-test env-var mutation from a sibling.

static ENV_MUTEX: Mutex<()> = Mutex::new(());

struct DataGateGuard {
    prior: Option<std::ffi::OsString>,
}
impl DataGateGuard {
    #[allow(unsafe_code)]
    fn on() -> Self {
        let prior = std::env::var_os("RUSTDL_DATA_PROPERTIES");
        // SAFETY: serialised via ENV_MUTEX; restored on Drop.
        unsafe { std::env::set_var("RUSTDL_DATA_PROPERTIES", "1") };
        Self { prior }
    }
}
impl Drop for DataGateGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var("RUSTDL_DATA_PROPERTIES", v),
                None => std::env::remove_var("RUSTDL_DATA_PROPERTIES"),
            }
        }
    }
}

/// NEGATIVE: same integer value on two disjoint data properties ⇒ INCONSISTENT.
#[test]
fn disjoint_dp_same_integer_value_inconsistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DataGateGuard::on();
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q))
    Declaration(NamedIndividual(:a))
    DisjointDataProperties(:p :q)
    DataPropertyAssertion(:p :a "42"^^xsd:integer)
    DataPropertyAssertion(:q :a "42"^^xsd:integer)"#
    ));
}

/// NEGATIVE: different integer values on disjoint data properties ⇒ CONSISTENT
/// (the values are distinct — no same-value clash).
#[test]
fn disjoint_dp_different_integer_values_consistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DataGateGuard::on();
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q))
    Declaration(NamedIndividual(:a))
    DisjointDataProperties(:p :q)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:q :a "2"^^xsd:integer)"#
    ));
}

/// NEGATIVE: no DisjointDataProperties axiom ⇒ CONSISTENT even with same value.
#[test]
fn disjoint_dp_no_axiom_consistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DataGateGuard::on();
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q))
    Declaration(NamedIndividual(:a))
    DataPropertyAssertion(:p :a "5"^^xsd:integer)
    DataPropertyAssertion(:q :a "5"^^xsd:integer)"#
    ));
}

/// NEGATIVE: same xsd:float value (f32 precision) on two disjoint data
/// properties ⇒ INCONSISTENT. Uses two DISTINCT f64 literals
/// ("0.1000000014" vs "0.1000000015") that round to the SAME f32 value.
/// This discriminates the correct "parse as f32, then widen" path from a
/// naive "parse as f64" path: parsed as f64 they differ; parsed as f32
/// they are both exactly 0.10000000149011612 (the nearest f32 to 0.1).
/// A passing result here proves the f32-precision path is actually taken.
#[test]
fn disjoint_dp_same_f32_value_inconsistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DataGateGuard::on();
    assert!(!consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q))
    Declaration(NamedIndividual(:a))
    DisjointDataProperties(:p :q)
    DataPropertyAssertion(:p :a "0.1000000014"^^xsd:float)
    DataPropertyAssertion(:q :a "0.1000000015"^^xsd:float)"#
    ));
}

/// NEGATIVE: different xsd:float values on disjoint data properties ⇒ CONSISTENT.
#[test]
fn disjoint_dp_different_f32_values_consistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DataGateGuard::on();
    assert!(consistent(
        r#"    Declaration(DataProperty(:p)) Declaration(DataProperty(:q))
    Declaration(NamedIndividual(:a))
    DisjointDataProperties(:p :q)
    DataPropertyAssertion(:p :a "1.0"^^xsd:float)
    DataPropertyAssertion(:q :a "2.0"^^xsd:float)"#
    ));
}

// ─── Finding 2: data-clash path marks classes unsatisfiable ───────────────
//
// `data_axioms.rs` emits `SubClassOf(owl:Thing, owl:Nothing)` for ABox
// data-range violations. Before the `⊤ ⊑ ⊥` fix the saturator silently
// dropped that axiom while certifying the closure complete; after the fix
// every user class is reported unsatisfiable.
//
// Pattern: `DataPropertyRange(:p xsd:integer)` + an `xsd:string` assertion
// → string and integer families are disjoint → `Top ⊑ Bot` is emitted →
// every declared class must appear in `unsatisfiable_classes()`.

/// POSITIVE (Finding 2 canary): a data-range violation emits `⊤ ⊑ ⊥`
/// which now propagates through the saturator so that ALL named classes
/// are reported unsatisfiable — not just `is_consistent` returning false.
/// Before the `global_unsat` fix: `classify` reported `SubClassOf(:A :B)` as
/// a direct subsumption and the unsat list was empty (silent inconsistency).
/// After the fix: A and B are both in `unsatisfiable_classes()`.
#[test]
fn data_range_violation_marks_classes_unsat() {
    let src = format!(
        r#"{PFX}Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(DataProperty(:p))
    Declaration(NamedIndividual(:i))
    DataPropertyRange(:p xsd:integer)
    DataPropertyAssertion(:p :i "foo")
    SubClassOf(:A :B)
)"#
    );
    let mut reader = std::io::Cursor::new(src);
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(
        &mut reader,
        horned_owl::io::ParserConfiguration::default(),
    )
    .expect("parse");
    let c = classify(&onto).expect("classify");
    let mut unsat: Vec<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();
    unsat.sort();
    assert_eq!(
        unsat,
        vec!["http://t/A".to_string(), "http://t/B".to_string()],
        "data-range violation (⊤ ⊑ ⊥) must mark all named classes unsatisfiable"
    );
}
