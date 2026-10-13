//! #180 — an `xsd:integer` value lies in an `xsd:decimal` range
//! (`RUSTDL_DKEY_INT_WITHIN_DECIMAL`, default ON).
//!
//! In OWL 2 the value space of `xsd:integer` is a subset of that of `xsd:decimal`:
//! the integer `12` IS the decimal `12`. rustdl kept the two in separate `DKey`
//! buckets and never related them, so `∃p.{"12"^^xsd:integer}` was not seen to be
//! in `∃p.(xsd:decimal > 0)`. `ore_ont_11647` missed 20 pairs of that shape.
//!
//! **DIRECTION OF RISK IS A FALSE POSITIVE**, because the fix adds subsumptions.
//! The negatives below carry the weight: boundary exclusivity, a range the value
//! is outside of, the unbounded-end rules, and the guarantee that no
//! integer/decimal DISJOINTNESS appears (that would make an in-range integer
//! unsatisfiable).
//!
//! Probes use the SUFFICIENT direction on the target (`∃p.R ⊑ W`), so a negative
//! is a real non-derivation rather than a vacuous one.

#![allow(clippy::unwrap_used)]
#![allow(unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::sync::Mutex;
use std::time::Duration;

static ENV: Mutex<()> = Mutex::new(());

const PRE: &str = "Prefix(:=<http://ex.org/>)\n\
     Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";

/// `A` carries `have`; anything with a `dp`-value in `target` is `W`. Returns
/// `(A ⊑ W, A unsatisfiable)`.
fn probe(have: &str, target: &str) -> (bool, bool) {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    probe_unlocked(have, target)
}

fn probe_unlocked(have: &str, target: &str) -> (bool, bool) {
    let ofn = format!(
        "{PRE}Ontology(<http://ex.org/o>\n\
         Declaration(Class(:A)) Declaration(Class(:W)) Declaration(DataProperty(:dp))\n\
         SubClassOf(:A {have})\n\
         SubClassOf(DataSomeValuesFrom(:dp {target}) :W)\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let r = classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify");
    let unsat = r.unsatisfiable_classes().contains(&"http://ex.org/A");
    (r.is_subclass("http://ex.org/A", "http://ex.org/W"), unsat)
}

fn value(v: &str) -> String {
    format!("DataHasValue(:dp \"{v}\"^^xsd:integer)")
}

fn dec(facets: &str) -> String {
    format!("DatatypeRestriction(xsd:decimal {facets})")
}

// ---------------------------------------------------------------- negatives

/// FP guard: an exclusive bound excludes its endpoint.
#[test]
fn an_integer_on_an_exclusive_decimal_bound_is_outside() {
    let (sub, _) = probe(&value("0"), &dec("xsd:minExclusive \"0\"^^xsd:decimal"));
    assert!(!sub);
}

/// FP guard: a value outside the range is not in it, either side.
#[test]
fn an_integer_outside_the_decimal_range_is_not_in_it() {
    let range = dec("xsd:minInclusive \"1.5\"^^xsd:decimal xsd:maxInclusive \"9.5\"^^xsd:decimal");
    assert!(!probe(&value("1"), &range).0);
    assert!(!probe(&value("10"), &range).0);
}

/// FP guard: an integer RANGE fits only if both of its endpoints do. `[1, 20]`
/// is not inside `(0, 10]`, although `1` is.
#[test]
fn an_integer_range_must_fit_at_both_ends() {
    let have = "DataSomeValuesFrom(:dp DatatypeRestriction(xsd:integer \
                xsd:minInclusive \"1\"^^xsd:integer xsd:maxInclusive \"20\"^^xsd:integer))";
    let target = dec("xsd:minExclusive \"0\"^^xsd:decimal xsd:maxInclusive \"10\"^^xsd:decimal");
    assert!(!probe(have, &target).0);
}

/// FP guard: all integers do not fit in a decimal range bounded below.
#[test]
fn unbounded_integers_do_not_fit_a_bounded_decimal_range() {
    let have = "DataSomeValuesFrom(:dp xsd:integer)";
    assert!(!probe(have, &dec("xsd:minInclusive \"0\"^^xsd:decimal")).0);
}

/// FP guard: no integer/decimal disjointness is introduced, so an integer value
/// together with a `∀` over a decimal range that CONTAINS it stays satisfiable.
#[test]
fn an_in_range_integer_under_a_decimal_forall_stays_satisfiable() {
    let have = format!(
        "ObjectIntersectionOf({} DataAllValuesFrom(:dp {}))",
        value("12"),
        dec("xsd:minExclusive \"0\"^^xsd:decimal")
    );
    let (_, unsat) = probe(&have, "xsd:decimal");
    assert!(!unsat);
}

// ---------------------------------------------------------------- positives

/// The `ore_ont_11647` shape: `"12"^^xsd:integer` in `xsd:decimal > 0`.
#[test]
fn an_integer_value_in_a_decimal_range_is_subsumed() {
    assert!(probe(&value("12"), &dec("xsd:minExclusive \"0\"^^xsd:decimal")).0);
}

/// An inclusive bound includes its endpoint; negative values compare correctly.
#[test]
fn inclusive_bounds_and_negative_values() {
    assert!(probe(&value("0"), &dec("xsd:minInclusive \"0\"^^xsd:decimal")).0);
    let range = dec("xsd:minExclusive \"-10\"^^xsd:decimal xsd:maxExclusive \"-0.5\"^^xsd:decimal");
    assert!(probe(&value("-5"), &range).0);
}

/// An integer range fits when both ends do, including against a fractional
/// decimal bound.
#[test]
fn an_integer_range_inside_a_decimal_range_is_subsumed() {
    let have = "DataSomeValuesFrom(:dp DatatypeRestriction(xsd:integer \
                xsd:minInclusive \"1\"^^xsd:integer xsd:maxInclusive \"9\"^^xsd:integer))";
    let target = dec("xsd:minExclusive \"0.5\"^^xsd:decimal xsd:maxInclusive \"9.5\"^^xsd:decimal");
    assert!(probe(have, &target).0);
}

/// Every integer is a decimal: `∃p.xsd:integer ⊑ ∃p.xsd:decimal`.
#[test]
fn the_integer_datatype_is_inside_the_decimal_datatype() {
    assert!(probe("DataSomeValuesFrom(:dp xsd:integer)", "xsd:decimal").0);
}

/// `RUSTDL_DKEY_INT_WITHIN_DECIMAL=0` restores the old, separate buckets.
#[test]
fn the_flag_switches_the_seeding_off() {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: every env access in this binary holds `ENV`.
    unsafe { std::env::set_var("RUSTDL_DKEY_INT_WITHIN_DECIMAL", "0") };
    let (sub, _) = probe_unlocked(&value("12"), &dec("xsd:minExclusive \"0\"^^xsd:decimal"));
    // SAFETY: as above.
    unsafe { std::env::remove_var("RUSTDL_DKEY_INT_WITHIN_DECIMAL") };
    assert!(!sub);
}
