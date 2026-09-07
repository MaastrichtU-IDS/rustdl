//! #121 — the XSD integer-DERIVED datatypes (`xsd:int`, `xsd:long`, …) must get
//! a `DKey` bucket instead of dropping.
//!
//! `parse_integer_range` keyed on the exact `xsd:integer` IRI, so every derived
//! type had no bucket and the whole axiom dropped — `xsd:int` MISSED where
//! `xsd:integer` derived, with `dropped: {'SubClassOf: unsupported data range'}`.
//! That was a SOUND, SURFACED under-approximation (a caller could tell), which is
//! why this is a completeness fix and not a D10.
//!
//! **DIRECTION OF RISK IS A FALSE POSITIVE**, because the fix ADDS subsumptions:
//! all these types share ONE value space and therefore one bucket, so
//! `IntegerRange::subset` gives the type hierarchy directly. The negative
//! controls below carry the weight — `∃p.xsd:integer ⊑ ∃p.xsd:int` must NOT be
//! derivable, since `xsd:int` is the NARROWER type.
//!
//! **EXACTNESS IS LOAD-BEARING, in BOTH directions.** A range wider than the
//! truth makes `nonNegativeInteger ⊆ unsignedLong` derivable; a range narrower
//! than the truth makes `unsignedLong ⊆ [0, i64::MAX]` derivable. Both are false.
//! So `xsd:unsignedLong`, whose max `2^64 - 1` does not fit `IntegerRange`'s
//! `i64`, keeps DROPPING — pinned below.
//!
//! **PROBE SHAPE MATTERS AND THE FIRST VERSION OF IT WAS VACUOUS.** Two
//! `SubClassOf` NECESSARY conditions (`A ⊑ ∃p.int`, `B ⊑ ∃p.integer`) cannot
//! entail `A ⊑ B` — `B` is not defined by its condition — so that probe answered
//! "no" for every pair and could not have produced a positive either. The target
//! needs the SUFFICIENT direction (`∃p.integer ⊑ W`). See
//! [[tests-that-pin-the-bug]]; the sibling trap is #42's vacuous subset canary.
//!
//! ORACLE: **`HermiT` 1.4.3 is the oracle here and agrees 4/4** on the adjudicated
//! probes — it derives `A ⊑ W` for `int`-inside-`integer` and `short`-inside-`int`
//! and refuses both converses. **Konclude v0.7.0-1138 reports NOTHING on any of
//! the four, including the positives**, which is a further instance of the
//! under-reporting this repo already records. That reading is licensed by a
//! DISCRIMINATING CONTROL, not assumed: adding a plain `SubClassOf(:C :E)` to the
//! same file makes Konclude report `C ⊑ E` while still omitting `A ⊑ W`, so it is
//! reasoning over the file and simply not over these datatypes.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify_top_down_with_timeout, dropped_axioms};
use std::io::Cursor;
use std::time::Duration;

const PRE: &str = "Prefix(:=<http://ex.org/>)\n\
     Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";

fn parse(ofn: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(ofn.to_string());
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// `A` carries a `have` value; anything carrying a `target` value is `W`.
/// Returns whether `A ⊑ W` is derived. The SUFFICIENT direction on `target` is
/// what makes this non-vacuous — see the module header.
fn a_is_w(have: &str, target: &str) -> bool {
    let ofn = format!(
        "{PRE}Ontology(<http://ex.org/o>\n\
         Declaration(Class(:A)) Declaration(Class(:W)) Declaration(DataProperty(:dp))\n\
         SubClassOf(:A DataSomeValuesFrom(:dp xsd:{have}))\n\
         SubClassOf(DataSomeValuesFrom(:dp xsd:{target}) :W)\n)\n"
    );
    let onto = parse(&ofn);
    let r = classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify");
    r.is_subclass("http://ex.org/A", "http://ex.org/W")
}

/// Every representable integer-derived type must now reach its own bucket, so a
/// value of that type satisfies a restriction to the SAME type.
#[test]
fn each_integer_derived_type_reaches_a_bucket() {
    for t in [
        "integer",
        "long",
        "int",
        "short",
        "byte",
        "nonNegativeInteger",
        "positiveInteger",
        "nonPositiveInteger",
        "negativeInteger",
        "unsignedInt",
        "unsignedShort",
        "unsignedByte",
    ] {
        assert!(a_is_w(t, t), "xsd:{t} must reach an integer DKey bucket");
    }
}

/// The narrower type's values sit inside the wider type. These are the pairs
/// `HermiT` confirms.
#[test]
fn a_narrower_type_is_subsumed_by_a_wider_one() {
    for (narrow, wide) in [
        ("int", "integer"),
        ("long", "integer"),
        ("int", "long"),
        ("short", "int"),
        ("byte", "short"),
        ("positiveInteger", "nonNegativeInteger"),
        ("unsignedByte", "unsignedShort"),
        ("unsignedInt", "long"),
    ] {
        assert!(
            a_is_w(narrow, wide),
            "xsd:{narrow} ⊆ xsd:{wide} must be derived"
        );
    }
}

/// THE FP GUARD, and the whole reason this fix needs controls. Mapping a derived
/// type to the same UNBOUNDED range as `xsd:integer` would make these mutually
/// subsumable and every one of them would fire.
#[test]
fn a_wider_type_is_not_subsumed_by_a_narrower_one() {
    for (wide, narrow) in [
        ("integer", "int"),
        ("integer", "long"),
        ("long", "int"),
        ("int", "short"),
        ("short", "byte"),
        ("nonNegativeInteger", "positiveInteger"),
        ("unsignedShort", "unsignedByte"),
    ] {
        assert!(
            !a_is_w(wide, narrow),
            "xsd:{wide} ⊄ xsd:{narrow} — the wider type is NOT inside the narrower one"
        );
    }
}

/// Ranges that overlap in neither direction must stay unrelated both ways.
#[test]
fn incomparable_types_are_related_in_neither_direction() {
    for (a, b) in [
        ("positiveInteger", "negativeInteger"),
        ("unsignedByte", "negativeInteger"),
    ] {
        assert!(!a_is_w(a, b), "xsd:{a} ⊄ xsd:{b}");
        assert!(!a_is_w(b, a), "xsd:{b} ⊄ xsd:{a}");
    }
}

/// `xsd:integer` and `xsd:decimal` are DISTINCT value spaces with distinct
/// buckets and are deliberately never cross-seeded, so widening the integer
/// parser must not have leaked across. `dt_family` lumps both under `Numeric`,
/// which is a different classification and must not be mistaken for a bucket.
#[test]
fn the_integer_family_does_not_cross_seed_with_decimal() {
    assert!(!a_is_w("int", "decimal"), "int must not reach dec:");
    assert!(
        !a_is_w("decimal", "int"),
        "decimal must not reach the int bucket"
    );
}

/// `xsd:unsignedLong`'s max is `2^64 - 1`, outside `IntegerRange`'s `i64`, and
/// NEITHER direction of approximation is sound (see the module header). So it
/// keeps dropping, and the drop stays VISIBLE.
///
/// If `IntegerRange` ever widens to `i128`, FLIP this test rather than deleting
/// it — `unsignedLong` becomes representable and should join the family.
#[test]
fn unsigned_long_is_not_representable_and_still_drops_visibly() {
    let ofn = format!(
        "{PRE}Ontology(<http://ex.org/o>\n\
         Declaration(Class(:A)) Declaration(Class(:W)) Declaration(DataProperty(:dp))\n\
         SubClassOf(:A DataSomeValuesFrom(:dp xsd:unsignedLong))\n\
         SubClassOf(DataSomeValuesFrom(:dp xsd:unsignedLong) :W)\n)\n"
    );
    let onto = parse(&ofn);
    assert!(
        !dropped_axioms(&onto).expect("dropped_axioms").is_empty(),
        "an unrepresentable data range must drop VISIBLY, never silently"
    );
    assert!(!a_is_w("unsignedLong", "unsignedLong"));
}

/// A `DatatypeRestriction` TIGHTENS the datatype's own value space, so the two
/// must be INTERSECTED. `xsd:byte` capped at 200 is still `[-128, 127]`, so the
/// value 150 lies outside it and the class is unsatisfiable.
///
/// This is the discriminator for the intersect: taking the facets alone gives
/// `(-∞, 200]`, which CONTAINS 150, and the probe would read `sat`. Before #121
/// only `xsd:integer` reached that arm, whose base range is unbounded, which is
/// why facets-alone was correct then and is not now.
#[test]
fn a_restriction_on_a_derived_type_intersects_with_its_value_space() {
    let probe = |value: &str| {
        let ofn = format!(
            "{PRE}Ontology(<http://ex.org/o>\n\
             Declaration(Class(:A)) Declaration(DataProperty(:dp))\n\
             SubClassOf(:A DataHasValue(:dp \"{value}\"^^xsd:integer))\n\
             SubClassOf(:A DataAllValuesFrom(:dp DatatypeRestriction(xsd:byte \
             xsd:maxInclusive \"200\"^^xsd:integer)))\n)\n"
        );
        let onto = parse(&ofn);
        let r = classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify");
        r.unsatisfiable_classes().contains(&"http://ex.org/A")
    };
    assert!(probe("150"), "150 is outside xsd:byte, so A must be unsat");
    assert!(
        !probe("100"),
        "100 is inside xsd:byte — discriminating control"
    );
}
