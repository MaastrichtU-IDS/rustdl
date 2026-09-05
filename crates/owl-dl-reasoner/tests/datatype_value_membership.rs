//! Canaries for the integer-facet data-value membership lowering
//! (`DataHasValue(p, v) ⊑ DataSomeValuesFrom(p, range)` iff `v ∈ range`).
//!
//! These exercise the `∃p.DKey(range)` synthetic-subsumer reduction added
//! to `convert.rs`: `xsd:integer`-typed `DataHasValue` / `DataSomeValuesFrom`
//! restrictions lower to `∃p.DKey(range)` with told-subsumptions
//! `DKey(r1) ⊑ DKey(r2)` iff `r1 ⊆ r2`, seeded in `convert_ontology`.
//!
//! NEGATIVES-FIRST: this is the FP hotspot. Every NOT-subsumed assertion
//! below must hold — a regression there is an unsound positive.
//!
//! Run: `cargo test -p owl-dl-reasoner --test datatype_value_membership`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::time::Duration;

const PFX: &str = r"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
";

fn classify(body: &str) -> owl_dl_reasoner::Classification {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    // Force the slow, complete path so a MISS here is calculus, not a
    // trust_sat/snapshot mask. (DKey is EL-friendly so the Horn
    // shortcircuit handles these, but be explicit.)
    classify_top_down_with_timeout(&onto, Duration::from_secs(2)).expect("classify")
}

const C: &str = "http://t/C";
const D: &str = "http://t/D";

/// POSITIVE: `C ⊑ ∃R.(A ⊓ DataHasValue(h,60))`,
/// `D ≡ ∃R.(A ⊓ DataSomeValuesFrom(h, int(36<x<101)))` ⟹ `C ⊑ D`.
/// 60 ∈ [37,100], so the height-key subsumes.
#[test]
fn value_in_range_subsumes() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "60"^^xsd:integer))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
"#,
    );
    assert!(c.is_subclass(C, D), "60 ∈ (36,101): C ⊑ D must hold");
}

/// NEGATIVE — exclusive lower boundary: value 36 is OUTSIDE (36,101).
#[test]
fn value_on_lower_exclusive_boundary_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "36"^^xsd:integer))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
"#,
    );
    assert!(!c.is_subclass(C, D), "36 ∉ (36,101): C ⊑ D must NOT hold");
}

/// NEGATIVE — exclusive upper boundary: value 101 is OUTSIDE (36,101).
#[test]
fn value_on_upper_exclusive_boundary_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "101"^^xsd:integer))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
"#,
    );
    assert!(!c.is_subclass(C, D), "101 ∉ (36,101): C ⊑ D must NOT hold");
}

/// NEGATIVE — value far outside the range.
#[test]
fn value_outside_range_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "200"^^xsd:integer))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
"#,
    );
    assert!(!c.is_subclass(C, D), "200 ∉ (36,101): C ⊑ D must NOT hold");
}

/// NEGATIVE — WRONG PROPERTY: value 60 on `width`, range on `height`.
/// Even though 60 ∈ range, the property differs so it must NOT subsume.
/// (CR5 role-match: ∃width.DKey ⊄ ∃height.DKey.)
#[test]
fn wrong_property_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:height))
    Declaration(DataProperty(:width))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:width "60"^^xsd:integer))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:height DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "width=60 vs height-range: C ⊑ D must NOT hold (wrong property)"
    );
}

/// POSITIVE — range ⊆ range: `[40,50] ⊆ [37,100]`.
#[test]
fn range_subset_subsumes() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minInclusive "40"^^xsd:integer xsd:maxInclusive "50"^^xsd:integer)))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
"#,
    );
    assert!(c.is_subclass(C, D), "[40,50] ⊆ [37,100]: C ⊑ D must hold");
}

/// NEGATIVE — range ⊄ range: `[37,100] ⊄ [40,50]`.
#[test]
fn range_superset_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minInclusive "40"^^xsd:integer xsd:maxInclusive "50"^^xsd:integer)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "[37,100] ⊄ [40,50]: C ⊑ D must NOT hold"
    );
}

/// NEGATIVE — unbounded-below ⊄ bounded: `(-∞,100] ⊄ [37,100]`.
#[test]
fn unbounded_below_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:maxInclusive "100"^^xsd:integer)))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "(-∞,100] ⊄ [37,100]: C ⊑ D must NOT hold"
    );
}

/// NEGATIVE — non-integer datatype must DROP (whole axiom), no FP.
/// `DataHasValue(h, "60.0"^^xsd:float)` is NOT an integer → the
/// `SubClassOf` axiom drops entirely, so C has no recorded height
/// existential and cannot be classified under D.
#[test]
fn non_integer_datatype_dropped_no_fp() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "60.0"^^xsd:float))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "float value: axiom dropped → C ⊑ D must NOT hold"
    );
}

// ─────────────────────────────────────────────────────────────────────
// Phase D6 Part A — bare xsd:integer (no facet).
// ─────────────────────────────────────────────────────────────────────

/// POSITIVE — bare `xsd:integer`: `DataHasValue(p,5)` (point [5,5]) ⊆
/// `DataSomeValuesFrom(p, xsd:integer)` (unbounded). C ⊑ D must hold.
#[test]
fn bare_integer_unbounded_subsumes_point() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:p))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:p "5"^^xsd:integer))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:p xsd:integer))))
"#,
    );
    assert!(
        c.is_subclass(C, D),
        "5 ∈ xsd:integer (unbounded): C ⊑ D must hold"
    );
}

/// NEGATIVE — bare `xsd:integer` WRONG PROPERTY: value on `q`, range on
/// `p`. Must NOT subsume (CR5 role-match).
#[test]
fn bare_integer_wrong_property_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:p))
    Declaration(DataProperty(:q))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:q "5"^^xsd:integer))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:p xsd:integer))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "q-value vs p-range: C ⊑ D must NOT hold (wrong property)"
    );
}

// ─────────────────────────────────────────────────────────────────────
// Phase D6 Part B — float ranges (boundary minefield, NEGATIVES FIRST).
// ─────────────────────────────────────────────────────────────────────

/// NEGATIVE — float exclusive lower boundary: `DataHasValue(h, 36.0)` is
/// OUTSIDE `(36.0, 101.0)`.
#[test]
fn float_value_on_lower_exclusive_boundary_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "36.0"^^xsd:float))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minExclusive "36.0"^^xsd:float xsd:maxExclusive "101.0"^^xsd:float)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "36.0 ∉ (36.0,101.0): C ⊑ D must NOT hold"
    );
}

/// POSITIVE — float inclusive boundary: `DataHasValue(h, 36.0)` IS inside
/// `[36.0, 101.0]`.
#[test]
fn float_value_on_lower_inclusive_boundary_subsumes() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "36.0"^^xsd:float))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minInclusive "36.0"^^xsd:float xsd:maxInclusive "101.0"^^xsd:float)))))
"#,
    );
    assert!(c.is_subclass(C, D), "36.0 ∈ [36.0,101.0]: C ⊑ D must hold");
}

/// NEGATIVE — float exclusive upper boundary: 101.0 ∉ (36.0,101.0).
#[test]
fn float_value_on_upper_exclusive_boundary_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "101.0"^^xsd:float))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minExclusive "36.0"^^xsd:float xsd:maxExclusive "101.0"^^xsd:float)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "101.0 ∉ (36.0,101.0): C ⊑ D must NOT hold"
    );
}

/// NEGATIVE — float value far outside.
#[test]
fn float_value_outside_range_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "200.0"^^xsd:float))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minExclusive "36.0"^^xsd:float xsd:maxExclusive "101.0"^^xsd:float)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "200.0 ∉ (36.0,101.0): C ⊑ D must NOT hold"
    );
}

/// POSITIVE — float interior value subsumes.
#[test]
fn float_value_in_range_subsumes() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "60.0"^^xsd:float))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minExclusive "36.0"^^xsd:float xsd:maxExclusive "101.0"^^xsd:float)))))
"#,
    );
    assert!(c.is_subclass(C, D), "60.0 ∈ (36.0,101.0): C ⊑ D must hold");
}

/// POSITIVE — float range ⊆ range, mixed incl/excl: `[40,50] ⊆ (36,101)`.
#[test]
fn float_range_subset_subsumes() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minInclusive "40.0"^^xsd:float xsd:maxInclusive "50.0"^^xsd:float)))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minExclusive "36.0"^^xsd:float xsd:maxExclusive "101.0"^^xsd:float)))))
"#,
    );
    assert!(c.is_subclass(C, D), "[40,50] ⊆ (36,101): C ⊑ D must hold");
}

/// NEGATIVE — float equal-endpoint inclusive/exclusive: `[36,..) ⊄ (36,..)`.
/// self INCLUDES 36.0, other EXCLUDES it, so 36.0 ∈ self but ∉ other.
#[test]
fn float_inclusive_self_excluded_by_exclusive_other_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minInclusive "36.0"^^xsd:float)))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minExclusive "36.0"^^xsd:float)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "[36,..) ⊄ (36,..): C ⊑ D must NOT hold (inclusive self, exclusive other)"
    );
}

/// POSITIVE — `VeryFastExposure` pattern: `(-∞,0.002) ⊆ (-∞,0.01)`.
#[test]
fn float_open_below_subset_subsumes() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:maxExclusive "0.002"^^xsd:float)))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:maxExclusive "0.01"^^xsd:float)))))
"#,
    );
    assert!(
        c.is_subclass(C, D),
        "(-∞,0.002) ⊆ (-∞,0.01): C ⊑ D must hold"
    );
}

// ─────────────────────────────────────────────────────────────────────
// Phase D6 Part B — DATATYPE KEYING (no cross-datatype subsumption).
// ─────────────────────────────────────────────────────────────────────

/// NEGATIVE — int value 60 vs FLOAT range (60.0 ∈ value-space-wise, but
/// different datatype bucket → no `DKey` edge → NOT subsumed).
#[test]
fn int_value_vs_float_range_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "60"^^xsd:integer))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minExclusive "36.0"^^xsd:float xsd:maxExclusive "101.0"^^xsd:float)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "int 60 vs float range: C ⊑ D must NOT hold (cross-datatype)"
    );
}

/// NEGATIVE — float value 60.0 vs INTEGER range (different datatype
/// bucket → no `DKey` edge → NOT subsumed).
#[test]
fn float_value_vs_int_range_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "60.0"^^xsd:float))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "float 60.0 vs int range: C ⊑ D must NOT hold (cross-datatype)"
    );
}

/// NEGATIVE — float WRONG PROPERTY.
#[test]
fn float_wrong_property_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:height))
    Declaration(DataProperty(:width))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:width "60.0"^^xsd:float))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:height DatatypeRestriction(xsd:float xsd:minExclusive "36.0"^^xsd:float xsd:maxExclusive "101.0"^^xsd:float)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "width=60.0 vs height float-range: C ⊑ D must NOT hold (wrong property)"
    );
}

/// NEGATIVE — float NaN facet must DROP the whole range (no FP). The D
/// definition's existential vanishes, so C cannot classify under D.
#[test]
fn float_nan_facet_dropped_no_fp() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "60.0"^^xsd:float))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:float xsd:minExclusive "NaN"^^xsd:float xsd:maxExclusive "101.0"^^xsd:float)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "NaN facet: range dropped → C ⊑ D must NOT hold"
    );
}

/// REGRESSION GUARD: no synthetic `DKey` IRI may appear in the reported
/// class list. Guards against a future class-enumeration site that
/// bypasses `reportable_class_iris`.
#[test]
fn dkey_classes_not_reported() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h "60"^^xsd:integer))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h DatatypeRestriction(xsd:integer xsd:minExclusive "36"^^xsd:integer xsd:maxExclusive "101"^^xsd:integer)))))
"#,
    );
    assert!(
        c.classes()
            .iter()
            .all(|iri| !iri.starts_with("urn:rustdl-dkey:")),
        "DKey synthetic classes leaked into reported class list: {:?}",
        c.classes()
    );
    assert!(
        c.unsatisfiable_classes()
            .iter()
            .all(|iri| !iri.starts_with("urn:rustdl-dkey:")),
        "DKey synthetic classes leaked into unsatisfiable set"
    );
}

// ── Phase D8: decimal / date / dateTime value membership ─────────────────
//
// Same `∃R.(A ⊓ value/range)` shape as the integer/float canaries above,
// extended to the three new totally-ordered datatype buckets. NEGATIVES
// (boundary, cross-datatype, timezone-drop) carry the soundness weight.

/// Build the `C ⊑ ∃R.(A ⊓ DataHasValue(h,val))`,
/// `D ≡ ∃R.(A ⊓ DataSomeValuesFrom(h,range))` shape and classify.
fn classify_value_range(val: &str, range: &str) -> owl_dl_reasoner::Classification {
    classify(&format!(
        r"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:h {val}))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h {range}))))
"
    ))
}

#[test]
fn decimal_value_in_open_range_subsumes() {
    // 0.5 ∈ (0.0, 1.0): C ⊑ D.
    let c = classify_value_range(
        r#""0.5"^^xsd:decimal"#,
        r#"DatatypeRestriction(xsd:decimal xsd:minExclusive "0.0"^^xsd:decimal xsd:maxExclusive "1.0"^^xsd:decimal)"#,
    );
    assert!(c.is_subclass(C, D), "0.5 ∈ (0.0,1.0): C ⊑ D must hold");
}

#[test]
fn decimal_value_at_exclusive_boundary_not_subsumed() {
    // 1.0 ∉ (0.0, 1.0): excluded endpoint — the decimal FP hotspot.
    let c = classify_value_range(
        r#""1.0"^^xsd:decimal"#,
        r#"DatatypeRestriction(xsd:decimal xsd:minExclusive "0.0"^^xsd:decimal xsd:maxExclusive "1.0"^^xsd:decimal)"#,
    );
    assert!(!c.is_subclass(C, D), "1.0 ∉ (0.0,1.0): C ⊑ D must NOT hold");
}

#[test]
fn decimal_distinct_values_do_not_collide() {
    // 0.45 ∉ [0.5, 1.0]: distinct decimals must not round-collide (would
    // be the classic f64 unsoundness). 0.45 < 0.5, so outside.
    let c = classify_value_range(
        r#""0.45"^^xsd:decimal"#,
        r#"DatatypeRestriction(xsd:decimal xsd:minInclusive "0.5"^^xsd:decimal xsd:maxInclusive "1.0"^^xsd:decimal)"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "0.45 ∉ [0.5,1.0]: C ⊑ D must NOT hold"
    );
}

#[test]
fn date_value_in_range_subsumes() {
    // 2020-06-09 ∈ [2020-01-01, 2021-01-01).
    let c = classify_value_range(
        r#""2020-06-09"^^xsd:date"#,
        r#"DatatypeRestriction(xsd:date xsd:minInclusive "2020-01-01"^^xsd:date xsd:maxExclusive "2021-01-01"^^xsd:date)"#,
    );
    assert!(c.is_subclass(C, D), "date in range: C ⊑ D must hold");
}

#[test]
fn date_value_at_exclusive_boundary_not_subsumed() {
    // 2021-01-01 ∉ [2020-01-01, 2021-01-01).
    let c = classify_value_range(
        r#""2021-01-01"^^xsd:date"#,
        r#"DatatypeRestriction(xsd:date xsd:minInclusive "2020-01-01"^^xsd:date xsd:maxExclusive "2021-01-01"^^xsd:date)"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "2021-01-01 ∉ [.,2021-01-01): C ⊑ D must NOT hold"
    );
}

#[test]
fn datetime_value_in_range_subsumes() {
    let c = classify_value_range(
        r#""2020-06-09T12:00:00"^^xsd:dateTime"#,
        r#"DatatypeRestriction(xsd:dateTime xsd:minInclusive "2020-06-09T00:00:00"^^xsd:dateTime xsd:maxInclusive "2020-06-09T23:59:59"^^xsd:dateTime)"#,
    );
    assert!(c.is_subclass(C, D), "dateTime in range: C ⊑ D must hold");
}

#[test]
fn decimal_value_vs_integer_range_no_cross_subsumption() {
    // 5.0-decimal numerically sits in the integer range [1,10], but the
    // decimal and integer buckets are DISJOINT — no edge may be seeded.
    let c = classify_value_range(
        r#""5.0"^^xsd:decimal"#,
        r#"DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "decimal value vs integer range: cross-datatype, C ⊑ D must NOT hold"
    );
}

#[test]
fn date_value_with_timezone_dropped_no_subsumption() {
    // The value carries a `Z` timezone → parse drops it → the whole
    // DataHasValue restriction drops → C ⊑ D must NOT hold even though
    // the date would otherwise sit inside the range.
    let c = classify_value_range(
        r#""2020-06-09Z"^^xsd:date"#,
        r#"DatatypeRestriction(xsd:date xsd:minInclusive "2020-01-01"^^xsd:date xsd:maxExclusive "2021-01-01"^^xsd:date)"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "tz-bearing date dropped: C ⊑ D must NOT hold"
    );
}

// ── Phase D9: xsd:string value membership (DataOneOf / bare string) ──────

#[test]
fn string_value_in_oneof_subsumes() {
    // "FULL-TIME" ∈ {"FULL-TIME","PART-TIME"}: C ⊑ D.
    let c = classify_value_range(
        r#""FULL-TIME"^^xsd:string"#,
        r#"DataOneOf("PART-TIME"^^xsd:string "FULL-TIME"^^xsd:string)"#,
    );
    assert!(c.is_subclass(C, D), "value ∈ enumeration: C ⊑ D must hold");
}

#[test]
fn string_value_not_in_oneof_not_subsumed() {
    // "CONTRACT" ∉ {"FULL-TIME","PART-TIME"}.
    let c = classify_value_range(
        r#""CONTRACT"^^xsd:string"#,
        r#"DataOneOf("PART-TIME"^^xsd:string "FULL-TIME"^^xsd:string)"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "value ∉ enumeration: C ⊑ D must NOT hold"
    );
}

#[test]
fn string_value_subsumed_by_bare_string_top() {
    // Any string ∈ xsd:string (Top).
    let c = classify_value_range(r#""anything"^^xsd:string"#, "xsd:string");
    assert!(
        c.is_subclass(C, D),
        "value ⊆ xsd:string Top: C ⊑ D must hold"
    );
}

/// Range-vs-range variant: `DataSomeValuesFrom` on BOTH sides (the C side
/// can't use `DataHasValue`, which takes a literal not a range).
fn classify_range_range(sub: &str, sup: &str) -> owl_dl_reasoner::Classification {
    classify(&format!(
        r"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:h))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h {sub}))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:h {sup}))))
"
    ))
}

#[test]
fn string_oneof_subset_subsumes() {
    // {"a"} ⊆ {"a","b"}: enumeration subset.
    let c = classify_range_range(
        r#"DataOneOf("a"^^xsd:string)"#,
        r#"DataOneOf("a"^^xsd:string "b"^^xsd:string)"#,
    );
    assert!(c.is_subclass(C, D), "{{a}} ⊆ {{a,b}}: C ⊑ D must hold");
}

#[test]
fn string_oneof_superset_not_subsumed() {
    // {"a","b"} ⊄ {"a"}.
    let c = classify_range_range(
        r#"DataOneOf("a"^^xsd:string "b"^^xsd:string)"#,
        r#"DataOneOf("a"^^xsd:string)"#,
    );
    assert!(!c.is_subclass(C, D), "{{a,b}} ⊄ {{a}}: C ⊑ D must NOT hold");
}

#[test]
fn string_wrong_property_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:p))
    Declaration(DataProperty(:q))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:p "x"^^xsd:string))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:q DataOneOf("x"^^xsd:string)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "string on wrong property: C ⊑ D must NOT hold"
    );
}

#[test]
fn string_value_vs_integer_range_no_cross_subsumption() {
    // A string value must never subsume into a numeric bucket.
    let c = classify_value_range(
        r#""5"^^xsd:string"#,
        r#"DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "string \"5\" vs integer [1,10]: cross-datatype, C ⊑ D must NOT hold"
    );
}

#[test]
fn language_tagged_oneof_member_drops_enumeration() {
    // A DataOneOf with a language-tagged member is NOT all-exact-string →
    // the whole enumeration drops → no subsumption even for the plain
    // member that would otherwise match.
    //
    // Still exactly true after issue #72 added the `lang:` bucket, and the
    // reason is worth stating so this does not read as "langString is
    // unsupported": a MIXED enumeration now fails BOTH parsers —
    // `parse_string_range` rejects the tagged member, `parse_lang_range`
    // rejects the untagged one — so it drops for the same all-or-nothing
    // reason, not for want of a langString bucket. The pure-langString case
    // is `lang_value_in_oneof_subsumes`; the mixed case's sibling canary is
    // `lang_mixed_oneof_drops_whole_enumeration`.
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(DataProperty(:p))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataHasValue(:p "hi"^^xsd:string))))
    EquivalentClasses(:D ObjectSomeValuesFrom(:R ObjectIntersectionOf(:A DataSomeValuesFrom(:p DataOneOf("hi"^^xsd:string "bonjour"@fr)))))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "lang-tagged member drops enumeration: C ⊑ D must NOT hold"
    );
}

// ── Phase D11: DataAllValuesFrom (∀p.DKey) ───────────────────────────────
//
// D11a — ∀-monotonicity: ∀p.range1 ⊑ ∀p.range2 iff range1 ⊆ range2 (via the
// told DKey⊑DKey edge + the hybrid tableau's ∀-rule; the lowering yields
// ConceptExpr::All ⟹ out of the saturator fragment ⟹ routes to hybrid).
// NEGATIVES carry the weight (a wrong ∀-direction = unsound).

/// `C ≡ ∀h.sub`, `D ≡ ∀h.sup` — classify and return the result.
fn classify_forall(sub: &str, sup: &str) -> owl_dl_reasoner::Classification {
    classify(&format!(
        r"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C DataAllValuesFrom(:h {sub}))
    EquivalentClasses(:D DataAllValuesFrom(:h {sup}))
"
    ))
}

#[test]
fn forall_range_monotone_subsumes() {
    // ∀h.[0,3] ⊑ ∀h.[0,10]  (since [0,3] ⊆ [0,10]).
    let c = classify_forall(
        r#"DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "3"^^xsd:integer)"#,
        r#"DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)"#,
    );
    assert!(c.is_subclass(C, D), "∀h.[0,3] ⊑ ∀h.[0,10]: C ⊑ D must hold");
}

#[test]
fn forall_range_antitone_not_subsumed() {
    // ∀h.[0,10] ⊄ ∀h.[0,3]  (the wider filler is NOT subsumed by the narrower).
    let c = classify_forall(
        r#"DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "3"^^xsd:integer)"#,
        r#"DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)"#,
    );
    assert!(
        !c.is_subclass(D, C),
        "∀h.[0,10] ⊄ ∀h.[0,3]: D ⊑ C must NOT hold"
    );
}

#[test]
fn forall_disjoint_filler_not_subsumed() {
    // ∀h.[0,3] and ∀h.[5,8] are incomparable — neither subsumes the other.
    let c = classify_forall(
        r#"DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "3"^^xsd:integer)"#,
        r#"DatatypeRestriction(xsd:integer xsd:minInclusive "5"^^xsd:integer xsd:maxInclusive "8"^^xsd:integer)"#,
    );
    assert!(!c.is_subclass(C, D), "∀h.[0,3] ⊄ ∀h.[5,8]");
    assert!(!c.is_subclass(D, C), "∀h.[5,8] ⊄ ∀h.[0,3]");
}

// D11b — ∃p.DKey(v) ⊓ ∀p.DKey(r) membership clash (v ∉ r ⟹ unsat), via the
// seeded DisjointClasses(DKey(v), DKey(r)). The corpus has NO such clash, so
// these canaries are the ENTIRE safety net for `definitely_disjoint`.
// NEGATIVES (overlap / shared-inclusive-boundary must NOT clash) carry it.

/// `C ≡ DataHasValue(h,val) ⊓ DataAllValuesFrom(h,range)`. Returns whether C
/// is unsatisfiable.
fn forall_clash_unsat(val: &str, range: &str) -> bool {
    let c = classify(&format!(
        r"    Declaration(Class(:C))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C ObjectIntersectionOf(DataHasValue(:h {val}) DataAllValuesFrom(:h {range})))
"
    ));
    c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C"))
}

#[test]
fn forall_value_outside_range_clashes() {
    // 5 ∉ [0,3]: ∃h.{5} ⊓ ∀h.[0,3] ⟹ C ⊑ ⊥.
    assert!(
        forall_clash_unsat(
            r#""5"^^xsd:integer"#,
            r#"DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "3"^^xsd:integer)"#
        ),
        "5 ∉ [0,3] under ∀: C must be unsatisfiable"
    );
}

#[test]
fn forall_value_inside_range_satisfiable() {
    // 2 ∈ [0,3]: NO clash — C satisfiable. (FP guard: overlap must not seed ⊥.)
    assert!(
        !forall_clash_unsat(
            r#""2"^^xsd:integer"#,
            r#"DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "3"^^xsd:integer)"#
        ),
        "2 ∈ [0,3]: C must be satisfiable (no spurious clash)"
    );
}

#[test]
fn forall_value_on_inclusive_boundary_satisfiable() {
    // 3 ∈ [0,3] (inclusive endpoint): NO clash. The shared-boundary FP trap.
    assert!(
        !forall_clash_unsat(
            r#""3"^^xsd:integer"#,
            r#"DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "3"^^xsd:integer)"#
        ),
        "3 ∈ [0,3] inclusive: C must be satisfiable"
    );
}

#[test]
fn forall_float_value_outside_clashes() {
    // 5.0 ∉ [0.0, 3.0]: float-bucket membership clash.
    assert!(
        forall_clash_unsat(
            r#""5.0"^^xsd:double"#,
            r#"DatatypeRestriction(xsd:double xsd:minInclusive "0.0"^^xsd:double xsd:maxInclusive "3.0"^^xsd:double)"#
        ),
        "5.0 ∉ [0.0,3.0] under ∀: C must be unsatisfiable"
    );
}

#[test]
fn forall_string_value_outside_enum_clashes() {
    // "z" ∉ {"a","b"}: string-bucket membership clash (disjoint singletons).
    assert!(
        forall_clash_unsat(
            r#""z"^^xsd:string"#,
            r#"DataOneOf("a"^^xsd:string "b"^^xsd:string)"#
        ),
        r#""z" not-in {{a,b}} under forall: C must be unsatisfiable"#
    );
}

#[test]
fn forall_string_value_inside_enum_satisfiable() {
    // "a" ∈ {"a","b"}: NO clash.
    assert!(
        !forall_clash_unsat(
            r#""a"^^xsd:string"#,
            r#"DataOneOf("a"^^xsd:string "b"^^xsd:string)"#
        ),
        r#""a" in {{a,b}}: C must be satisfiable"#
    );
}

/// INVERTED 2026-08-30 (#86). This asserted "no clash" and described itself as a
/// "sound under-approx" — which it was, and #86 is the fix for exactly that MISS.
/// Cross-bucket DISJOINTNESS is now seeded for provably-disjoint datatype pairs
/// (cross-bucket SUBSUMPTION still is not, and must not be).
///
/// Adjudicated on this exact shape:
///   Konclude → `C is UNSATISFIABLE`
///   `HermiT` → lists `<http://ex#C>` alongside `owl:Nothing`
#[test]
fn forall_cross_datatype_clashes() {
    // ∃h.{5^^integer} ⊓ ∀h.[0.0,3.0]^^double: the successor would have to be the
    // integer 5 AND a double in [0,3]. Those value spaces are disjoint, so no
    // value satisfies both and C is unsatisfiable.
    assert!(
        forall_clash_unsat(
            r#""5"^^xsd:integer"#,
            r#"DatatypeRestriction(xsd:double xsd:minInclusive "0.0"^^xsd:double xsd:maxInclusive "3.0"^^xsd:double)"#
        ),
        "int value vs double range: disjoint value spaces ⇒ C unsatisfiable (#86)"
    );
}

// ── DataIntersectionOf lowering ────────────────────────────────────────────
//
// Canaries for the `DataIntersectionOf([r1,r2,...])` → exact range-intersection
// lowering.  NEGATIVES FIRST: the no-FP canary is load-bearing.

/// `C ⊑ ∃p.DataIntersectionOf([≥0],[≤10])` must entail `C ⊑ ∃p.[−∞,∞ with subset [0,10]]`.
/// We verify by defining `D ≡ ∃p.DataSomeValuesFrom(p, xsd:integer [≥0,≤20])` and checking `C ⊑ D`
/// (the intersection [0,10] ⊆ [0,20] → the `DKey` subsumes → `C ⊑ D` holds).
#[test]
fn data_intersection_same_bucket_subsumption() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataSomeValuesFrom(:p DataIntersectionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer)
        DatatypeRestriction(xsd:integer xsd:maxInclusive "10"^^xsd:integer)
    )))
    EquivalentClasses(:D DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "20"^^xsd:integer)))
"#,
    );
    // [0,10] ⊆ [0,20]: DKey([0,10]) ⊑ DKey([0,20]) ⟹ ∃p.DKey([0,10]) ⊑ ∃p.DKey([0,20]) ≡ D
    assert!(
        c.is_subclass(C, D),
        "DataIntersectionOf([≥0],[≤10]) = [0,10] ⊆ [0,20]: C ⊑ D must hold"
    );
}

/// NEGATIVE — no-FP: `C ⊑ ∃p.DataIntersectionOf([≥0],[≤10])` must NOT entail
/// `C ⊑ ∃p.[20,30]`.  The intersection [0,10] is NOT a subset of [20,30].
#[test]
fn data_intersection_no_fp_disjoint_range() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataSomeValuesFrom(:p DataIntersectionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer)
        DatatypeRestriction(xsd:integer xsd:maxInclusive "10"^^xsd:integer)
    )))
    EquivalentClasses(:D DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive "20"^^xsd:integer xsd:maxInclusive "30"^^xsd:integer)))
"#,
    );
    // [0,10] is NOT a subset of [20,30] → C ⊑ D must NOT hold (the critical FP guard).
    assert!(
        !c.is_subclass(C, D),
        "DataIntersectionOf([≥0],[≤10])=[0,10] is NOT ⊆ [20,30]: C ⊑ D must NOT hold"
    );
}

/// Empty same-bucket intersection ([≥10]∩[≤5] = empty) → C ⊑ ⊥.
#[test]
fn data_intersection_empty_same_bucket_unsat() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataSomeValuesFrom(:p DataIntersectionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "10"^^xsd:integer)
        DatatypeRestriction(xsd:integer xsd:maxInclusive "5"^^xsd:integer)
    )))
"#,
    );
    // [≥10] ∩ [≤5] = empty ⟹ C is unsatisfiable.
    assert!(
        c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "DataIntersectionOf([≥10],[≤5]) is empty ⟹ C must be unsatisfiable"
    );
}

/// Empty cross-bucket intersection (integer ∩ string) → C ⊑ ⊥.
/// Integer and string value spaces are disjoint — no shared value.
#[test]
fn data_intersection_cross_bucket_unsat() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataSomeValuesFrom(:p DataIntersectionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "100"^^xsd:integer)
        xsd:string
    )))
"#,
    );
    // xsd:integer ∩ xsd:string = empty (distinct value spaces) ⟹ C unsatisfiable.
    assert!(
        c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "DataIntersectionOf(integer-range, xsd:string) is empty ⟹ C must be unsatisfiable"
    );
}

/// FP guard — `xsd:integer ∩ xsd:decimal`: integer is a sub-datatype of decimal
/// in XSD, so their value spaces OVERLAP (e.g. `5` is in both).  A
/// `DataIntersectionOf` mixing these two buckets must NOT be treated as empty,
/// and therefore must NOT cause C to be declared unsatisfiable.
///
/// This is the canonical soundness canary for the cross-bucket branch: if the
/// implementation incorrectly maps integer×decimal to an empty intersection it
/// emits `C ⊑ ⊥`, which is a false positive.
#[test]
fn data_intersection_integer_decimal_cross_bucket_no_fp() {
    // C ⊑ ∃p.DataIntersectionOf(integer [0,10], decimal [5,5]).
    // The value 5 (integer) is also 5 (decimal) → intersection non-empty.
    // Correct: C is satisfiable.  Wrong: C declared ⊥ (FP).
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataSomeValuesFrom(:p DataIntersectionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)
        DatatypeRestriction(xsd:decimal xsd:minInclusive "5"^^xsd:decimal xsd:maxInclusive "5"^^xsd:decimal)
    )))
"#,
    );
    // MUST be satisfiable — NOT a false-positive ⊥.
    assert!(
        !c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "FP guard: integer ∩ decimal has shared values — C must NOT be declared unsatisfiable"
    );
}

/// Drop on unrecognized member: a `DataIntersectionOf` whose second member is an
/// unrecognized range (`DataComplementOf` — not handled) → the whole intersection
/// drops → C is consistent; no spurious unsat or subsumption.
#[test]
fn data_intersection_unrecognized_member_drops_gracefully() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataSomeValuesFrom(:p DataIntersectionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)
        DataComplementOf(DatatypeRestriction(xsd:integer xsd:minInclusive "5"^^xsd:integer xsd:maxInclusive "15"^^xsd:integer))
    )))
    EquivalentClasses(:D DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive "20"^^xsd:integer xsd:maxInclusive "30"^^xsd:integer)))
"#,
    );
    // Unrecognized member (DataComplementOf) → drop → C consistent, NOT ⊑ D.
    assert!(
        !c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "Drop on DataComplementOf member: C must be satisfiable (not spuriously ⊥)"
    );
    assert!(
        !c.is_subclass(C, D),
        "Drop on DataComplementOf member: C ⊑ D must NOT hold (no spurious subsumption)"
    );
}

// ── DataUnionOf lowering ───────────────────────────────────────────────────
//
// Canaries for the `DataUnionOf([r1,r2,...])` → class-level disjunction
// `∃p.DKey(r1) ⊔ ∃p.DKey(r2) ⊔ ...` lowering (SOME direction only).
// NEGATIVES FIRST: the no-FP canary is the critical one.

/// CRITICAL no-FP canary: `C ⊑ ∃p.DataUnionOf([0,5],[10,15])` must NOT entail
/// `C ⊑ ∃p.[0,5]` — the value could be in [10,15].
/// If the implementation collapses the union to its first disjunct this emits
/// a false-positive subsumption: a class bearing ∃p.[10,12] would be wrongly
/// deemed a subclass of ∃p.[0,5].
#[test]
fn data_union_no_fp_disjunct_not_entailed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataSomeValuesFrom(:p DataUnionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)
        DatatypeRestriction(xsd:integer xsd:minInclusive "10"^^xsd:integer xsd:maxInclusive "15"^^xsd:integer)
    )))
    EquivalentClasses(:D DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)))
"#,
    );
    // MUST NOT hold: a value in [10,15] satisfies the union but NOT [0,5].
    assert!(
        !c.is_subclass(C, D),
        "CRITICAL FP guard: C ⊑ ∃p.DataUnionOf([0,5],[10,15]) must NOT entail C ⊑ ∃p.[0,5]"
    );
}

/// Disjunct-subsumption positive: `C ≡ ∃p.[0,5]` ⟹ `C ⊑ ∃p.DataUnionOf([0,5],[10,15])`.
/// [0,5] is one of the union's disjuncts, so `DKey([0,5])` ⊑ `DKey([0,5])` → the ∃ subsumes.
#[test]
fn data_union_disjunct_subsumption_holds() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:U))
    Declaration(DataProperty(:p))
    EquivalentClasses(:C DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)))
    EquivalentClasses(:U DataSomeValuesFrom(:p DataUnionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)
        DatatypeRestriction(xsd:integer xsd:minInclusive "10"^^xsd:integer xsd:maxInclusive "15"^^xsd:integer)
    )))
"#,
    );
    // C ≡ ∃p.[0,5]; U has ∃p.[0,5] as one disjunct — C ⊑ U must hold.
    assert!(
        c.is_subclass(C, "http://t/U"),
        "C ≡ ∃p.[0,5] ⊑ ∃p.DataUnionOf([0,5],[10,15]) must hold"
    );
}

/// Drop on unrecognized member: `DataUnionOf([0,5], DataComplementOf(...))` —
/// the second member is not lowerable, so the whole union drops → C is
/// consistent and no spurious subsumption is introduced.
#[test]
fn data_union_drop_on_unrecognized_member() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataSomeValuesFrom(:p DataUnionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)
        DataComplementOf(DatatypeRestriction(xsd:integer xsd:minInclusive "3"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer))
    )))
    EquivalentClasses(:D DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)))
"#,
    );
    // Unrecognized DataComplementOf → whole union dropped → no ∃p.DKey emitted.
    assert!(
        !c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "Drop on unrecognized member: C must be satisfiable (not spuriously ⊥)"
    );
    assert!(
        !c.is_subclass(C, D),
        "Drop on unrecognized member: C ⊑ D must NOT hold (no spurious subsumption)"
    );
}

/// **FLIPPED 2026-09-05 — this test was PINNING THE DEFECT, and its own name said so.**
///
/// It asserted that `C ⊑ ∀p.DataUnionOf([0,5], [10,15])` alongside `C ⊑ ∃p.[20,25]`
/// leaves `C` satisfiable, ON THE GROUNDS THAT THE UNION DROPS — a sound
/// under-approximation, correctly framed at the time. But `20..25` is disjoint from both
/// components, so **Konclude has always derived `C` unsatisfiable here** (re-verified
/// 2026-09-05, 1121 bytes of real output). The satisfiable verdict was the MISS, not the
/// answer, and this test was the thing that would have to change for the miss to close.
///
/// #42 item 1's interval-set representation closed it: the union now converts (`dropped`
/// is empty) and the clash fires. The assertion is inverted rather than deleted, so the
/// file keeps a record of which verdict moved and why.
///
/// Sibling `data_union_drop_on_unrecognized_member` above still asserts a DROP and
/// still passes — its union carries a `DataComplementOf`, which no interval set can
/// express, so the all-or-nothing rule correctly declines the whole range. That is the
/// difference between a representable union and an unrepresentable one, and it is why
/// only this one flipped.
///
/// See [[tests-that-pin-the-bug]]: a fixture chosen for a DEFECT becomes a bug-pin, and
/// there is no signal until someone fixes the bug. This one surfaced during sabotage
/// testing of the fix, not from the fix's own suite.
#[test]
fn data_union_forall_union_now_clashes() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataAllValuesFrom(:p DataUnionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)
        DatatypeRestriction(xsd:integer xsd:minInclusive "10"^^xsd:integer xsd:maxInclusive "15"^^xsd:integer)
    )))
    SubClassOf(:C DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive "20"^^xsd:integer xsd:maxInclusive "25"^^xsd:integer)))
"#,
    );
    // ∀-union now lowers to an interval-set DKey → [20,25] is disjoint from
    // [0,5] ⊔ [10,15] → clash. Konclude agrees.
    assert!(
        c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "[20,25] is disjoint from [0,5] ⊔ [10,15], so C is unsatisfiable — the verdict \
         Konclude always gave and this test used to pin the miss of"
    );
}

// ── DataComplementOf canaries ─────────────────────────────────────────────────
//
// The lowering: DataComplementOf(r) → ¬DKey(r).
// - `∃p.¬DKey(r)`: a value outside r.
// - `∀p.¬DKey(r)`: all p-values must be outside r.
// Clash only fires when told DKey({v}) ⊑ DKey(r) (i.e. v∈r) meets ¬DKey(r).
// The NEGATIVES (no-FP guards) are the load-bearing tests.

/// `C ≡ DataHasValue(h,val) ⊓ DataAllValuesFrom(h, DataComplementOf(range))`.
/// Returns whether C is unsatisfiable.
/// If val ∈ range: ∀ says "must be outside range" but ∃ witnesses val∈range → clash → unsat.
/// If val ∉ range: no clash → sat.
fn forall_complement_unsat(val: &str, range: &str) -> bool {
    let c = classify(&format!(
        r"    Declaration(Class(:C))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C ObjectIntersectionOf(DataHasValue(:h {val}) DataAllValuesFrom(:h DataComplementOf({range}))))
"
    ));
    c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C"))
}

/// POSITIVE clash: value 5 ∈ [0,10], but ∀ requires value ∉ [0,10] → ⊥.
#[test]
fn forall_complement_value_in_range_clashes() {
    assert!(
        forall_complement_unsat(
            r#""5"^^xsd:integer"#,
            r#"DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)"#
        ),
        "5 ∈ [0,10] but ∀ DataComplementOf([0,10]) says outside: C must be unsatisfiable"
    );
}

/// CRITICAL FP GUARD: value 5 ∉ [10,20] → NO clash; C must be satisfiable.
/// If this fires ⊥ that is a false positive — STOP and report.
#[test]
fn forall_complement_value_outside_range_satisfiable() {
    assert!(
        !forall_complement_unsat(
            r#""5"^^xsd:integer"#,
            r#"DatatypeRestriction(xsd:integer xsd:minInclusive "10"^^xsd:integer xsd:maxInclusive "20"^^xsd:integer)"#
        ),
        "CRITICAL FP GUARD: 5 ∉ [10,20] → ∀ DataComplementOf([10,20]) is consistent with DataHasValue 5; C must be satisfiable"
    );
}

/// ∃p.DataComplementOf([0,10]) is satisfiable on its own (a value outside [0,10]
/// can exist). Must NOT be unsatisfiable.
#[test]
fn some_complement_satisfiable() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataSomeValuesFrom(:p DataComplementOf(DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer))))
"#,
    );
    assert!(
        !c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "∃p.DataComplementOf([0,10]) is satisfiable: C must NOT be unsatisfiable"
    );
}

/// Contravariant subsumption: `∃p.¬[0,10] ⊑ ∃p.¬[2,8]` since `[2,8]⊆[0,10]`.
// Any value outside [0,10] is also outside [2,8].
// The told `DKey([2,8]) ⊑ DKey([0,10])` edge (seeded by `seed_dkey_subsumptions`
// because [2,8]⊆[0,10]) propagates `DKey([0,10])` onto the ¬DKey([0,10])
// successor → clash → refutes C⊄D. Completeness-optional; asserted at full strength.
#[test]
fn some_complement_contravariant_subsumption() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:p))
    EquivalentClasses(:C DataSomeValuesFrom(:p DataComplementOf(DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer))))
    EquivalentClasses(:D DataSomeValuesFrom(:p DataComplementOf(DatatypeRestriction(xsd:integer xsd:minInclusive "2"^^xsd:integer xsd:maxInclusive "8"^^xsd:integer))))
"#,
    );
    // [2,8] ⊆ [0,10] ⇒ ¬[0,10] ⊆ ¬[2,8] ⇒ ∃p.¬[0,10] ⊑ ∃p.¬[2,8] = D.
    // This is a completeness claim (may miss), not a soundness claim.
    assert!(
        c.is_subclass(C, D),
        "∃p.¬[0,10] ⊑ ∃p.¬[2,8] (contravariant; [2,8]⊆[0,10]): C ⊑ D must hold"
    );
    // REVERSE DIRECTION FP guard: D = ∃p.¬[2,8] must NOT subsume C = ∃p.¬[0,10].
    // 9 is outside [2,8] but inside [0,10], so ¬[2,8] ⊄ ¬[0,10].
    // The told edge DKey([0,10]) ⊑ DKey([2,8]) does not exist (since [0,10] ⊄ [2,8]),
    // so no clash → not subsumed. If this fires, it is a false positive — STOP.
    assert!(
        !c.is_subclass(D, C),
        "FP GUARD: ∃p.¬[2,8] ⊄ ∃p.¬[0,10] (9 ∉ [2,8] but 9 ∈ [0,10]): D ⊑ C must NOT hold"
    );
}

/// Drop on composite inner: `DataComplementOf(DataUnionOf(...))` is unrecognized
/// (`data_range_dkey` returns `None` for `DataUnionOf`) → the whole axiom drops.
/// C is satisfiable and no spurious subsumption is introduced.
#[test]
fn complement_composite_inner_dropped() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataSomeValuesFrom(:p DataComplementOf(DataUnionOf(
        DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)
        DatatypeRestriction(xsd:integer xsd:minInclusive "10"^^xsd:integer xsd:maxInclusive "15"^^xsd:integer)
    ))))
    EquivalentClasses(:D DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)))
"#,
    );
    // Composite inner → drop → no ∃p.¬DKey emitted → C stays satisfiable.
    assert!(
        !c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "DataComplementOf(DataUnionOf(...)) dropped: C must be satisfiable"
    );
    assert!(
        !c.is_subclass(C, D),
        "DataComplementOf(DataUnionOf(...)) dropped: no spurious C ⊑ D"
    );
}

/// Cardinality over complement drops: `DataMinCardinality(2, p, DataComplementOf([0,10]))`
/// is not recognized by any `lower_*_data_cardinality` chain (complement is not a
/// plain range type), so the axiom drops. C must be satisfiable.
#[test]
fn complement_cardinality_dropped() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(DataProperty(:p))
    SubClassOf(:C DataMinCardinality(2 :p DataComplementOf(DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer))))
"#,
    );
    // The cardinality axiom over a complement drops — no spurious ⊥.
    assert!(
        !c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "DataMinCardinality over DataComplementOf must drop: C must be satisfiable"
    );
}

// ── 2026-08-01: numeric `DataOneOf` DKey seeding (`RUSTDL_DKEY_ONEOF_SEED`) ──
//
// The six numeric enumeration buckets (`io:` / `fo:` / `dbo:` / `deo:` / `dao:` /
// `dto:`) were minted by `data_range_dkey` but never collected into
// `seed_dkey_subsumptions`, so they got neither told `DKey ⊑ DKey` edges nor
// `DisjointClasses(DKey, DKey)` entries — while `is_pure_el` still reported
// `incomplete: false`. (The `str:` enumeration bucket WAS always seeded; that
// asymmetry is the bug.) Konclude AND HermiT both derive the positives below.
//
// NEGATIVES-FIRST, and doubly so here: the disjointness half ADDS clashes, so a
// wrong "disjoint" is a false UNSAT, not a miss. The curated corpus contains no
// numeric `DataOneOf` at all, so THESE CANARIES ARE THE ENTIRE SAFETY NET.

/// Serializes the `RUSTDL_DKEY_ONEOF_SEED` flips: `cargo test` runs the tests in
/// this binary on several threads and the env is process-wide.
static ONEOF_FLAG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct OneofSeedOn {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl OneofSeedOn {
    #[allow(unsafe_code)]
    fn on() -> Self {
        let lock = ONEOF_FLAG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: `set_var` is unsafe under edition 2024. The mutex held in
        // `_lock` makes this the only thread mutating the variable for the
        // duration of the guard, and every reader in the conversion path reads
        // it per call. Removed on Drop.
        unsafe { std::env::set_var("RUSTDL_DKEY_ONEOF_SEED", "1") };
        Self { _lock: lock }
    }
}

impl Drop for OneofSeedOn {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `OneofSeedOn::on`.
        unsafe { std::env::remove_var("RUSTDL_DKEY_ONEOF_SEED") };
    }
}

const E: &str = "http://t/E";
const F: &str = "http://t/F";

/// `C ≡ ∃h.{1}`, `F ≡ ∃h.{1}`, `D ≡ ∃h.{1,2}`, `E ≡ ∃h.{1,2,3}`.
/// Oracle (`Konclude` AND `HermiT`, independently): `C ≡ F`, `F ⊑ D`, `D ⊑ E`.
fn oneof_ladder() -> owl_dl_reasoner::Classification {
    classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:E))
    Declaration(Class(:F))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer)))
    EquivalentClasses(:F DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer)))
    EquivalentClasses(:D DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
    EquivalentClasses(:E DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer "2"^^xsd:integer "3"^^xsd:integer)))
"#,
    )
}

/// POSITIVE (recovered #1): `{1} ⊆ {1,2}` ⟹ `F ⊑ D`.
#[test]
fn int_oneof_subset_subsumes() {
    let _g = OneofSeedOn::on();
    let c = oneof_ladder();
    assert!(c.is_subclass(F, D), "{{1}} ⊆ {{1,2}}: F ⊑ D must hold");
}

/// POSITIVE (recovered #2): `{1,2} ⊆ {1,2,3}` ⟹ `D ⊑ E`.
#[test]
fn int_oneof_chain_subsumes() {
    let _g = OneofSeedOn::on();
    let c = oneof_ladder();
    assert!(c.is_subclass(D, E), "{{1,2}} ⊆ {{1,2,3}}: D ⊑ E must hold");
}

/// POSITIVE (recovered #3): transitively, `C ⊑ E` (`{1} ⊆ {1,2} ⊆ {1,2,3}`).
#[test]
fn int_oneof_transitive_subsumes() {
    let _g = OneofSeedOn::on();
    let c = oneof_ladder();
    assert!(c.is_subclass(C, E), "{{1}} ⊆ {{1,2,3}}: C ⊑ E must hold");
}

/// POSITIVE (recovered #4) — the `∀`/`∃` membership clash, the analogue of the
/// interval-bucket `forall_value_outside_range_clashes` that already passes:
/// `∃h.{3} ⊓ ∀h.{1,2}` is unsatisfiable because `{3} ∩ {1,2} = ∅`.
/// Oracle: `Konclude` AND `HermiT` both report `U ≡ owl:Nothing`.
#[test]
fn forall_int_oneof_value_outside_enum_clashes() {
    let _g = OneofSeedOn::on();
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C ObjectIntersectionOf(DataSomeValuesFrom(:h DataOneOf("3"^^xsd:integer)) DataAllValuesFrom(:h DataOneOf("1"^^xsd:integer "2"^^xsd:integer))))
"#,
    );
    assert!(
        c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "3 ∉ {{1,2}} under ∀: C must be unsatisfiable"
    );
}

/// POSITIVE (recovered #5) — a SECOND bucket, so the fix is not integer-only:
/// `xsd:decimal` `{1.5} ⊆ {1.5, 2.5}`. Decimals are compared by the EXACT
/// normalized-lexical `Decimal` type, never `f64` (rounding two distinct
/// decimals to one `f64` would be a spurious equality = FP).
#[test]
fn decimal_oneof_subset_subsumes() {
    let _g = OneofSeedOn::on();
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C DataSomeValuesFrom(:h DataOneOf("1.5"^^xsd:decimal)))
    EquivalentClasses(:D DataSomeValuesFrom(:h DataOneOf("1.5"^^xsd:decimal "2.5"^^xsd:decimal)))
"#,
    );
    assert!(
        c.is_subclass(C, D),
        "{{1.5}} ⊆ {{1.5,2.5}} (decimal): C ⊑ D must hold"
    );
}

/// NEGATIVE — superset is not a subset: `{1,2} ⊄ {1}`, so `D ⊄ F`.
#[test]
fn int_oneof_superset_not_subsumed() {
    let _g = OneofSeedOn::on();
    let c = oneof_ladder();
    assert!(!c.is_subclass(D, F), "{{1,2}} ⊄ {{1}}: D ⊑ F must NOT hold");
    assert!(
        !c.is_subclass(E, D),
        "{{1,2,3}} ⊄ {{1,2}}: E ⊑ D must NOT hold"
    );
}

/// NEGATIVE — NOT-A-MEMBER: `5 ∉ {1,2}`, so `∃h.{5} ⊄ ∃h.{1,2}` in EITHER
/// direction, and neither class is unsatisfiable (two distinct values are
/// perfectly satisfiable on separate `h`-successors — disjointness of the two
/// `DKey`s must not by itself make anything ⊥).
#[test]
fn int_oneof_value_not_a_member_not_subsumed() {
    let _g = OneofSeedOn::on();
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C DataSomeValuesFrom(:h DataOneOf("5"^^xsd:integer)))
    EquivalentClasses(:D DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
"#,
    );
    assert!(!c.is_subclass(C, D), "5 ∉ {{1,2}}: C ⊑ D must NOT hold");
    assert!(!c.is_subclass(D, C), "{{1,2}} ⊄ {{5}}: D ⊑ C must NOT hold");
    assert!(
        c.unsatisfiable_classes().is_empty(),
        "disjoint enumerations alone must not make any class ⊥, got {:?}",
        c.unsatisfiable_classes()
    );
}

/// NEGATIVE — CROSS-DATATYPE: an `xsd:integer` enumeration and an
/// `xsd:float` enumeration live in DIFFERENT buckets (`io:` vs `fo:`) and must
/// never interact, even though `1` and `1.0` look numerically equal. A single
/// cross-bucket edge here would be a false positive.
#[test]
fn cross_datatype_int_vs_float_oneof_no_interaction() {
    let _g = OneofSeedOn::on();
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer)))
    EquivalentClasses(:D DataSomeValuesFrom(:h DataOneOf("1.0"^^xsd:float "2.0"^^xsd:float)))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "integer {{1}} ⊄ float {{1.0,2.0}}: cross-datatype must NOT subsume"
    );
    assert!(
        !c.is_subclass(D, C),
        "float {{1.0,2.0}} ⊄ integer {{1}}: cross-datatype must NOT subsume"
    );
}

/// NEGATIVE — CROSS-DATATYPE, the `xsd:float` / `xsd:double` pair specifically.
/// OWL 2 gives them DISJOINT value spaces, so a float `1.0` and a double `1.0`
/// are different data values and the two classes are NOT equivalent (Konclude
/// and `HermiT` both leave them incomparable). `rustdl` <= v0.4.8 folded both into
/// one f64-keyed `fo:` bucket and reported them EQUIVALENT — a false positive
/// present with no seeding at all; the `dbo:` bucket split fixes it, so this
/// canary must hold with the seeding flag BOTH on and off.
#[test]
fn float_and_double_oneof_not_equivalent() {
    let body = r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C DataSomeValuesFrom(:h DataOneOf("1.0"^^xsd:float)))
    EquivalentClasses(:D DataSomeValuesFrom(:h DataOneOf("1.0"^^xsd:double)))
"#;
    {
        let _g = OneofSeedOn::on();
        let c = classify(body);
        assert!(
            !c.is_subclass(C, D) && !c.is_subclass(D, C),
            "xsd:float 1.0 and xsd:double 1.0 are different values (seed ON)"
        );
    }
    let c = classify(body);
    assert!(
        !c.is_subclass(C, D) && !c.is_subclass(D, C),
        "xsd:float 1.0 and xsd:double 1.0 are different values (seed OFF)"
    );
}

/// NEGATIVE — the `∀` clash's FP guard: the value IS in the enumeration, so
/// `∃h.{2} ⊓ ∀h.{1,2}` must stay SATISFIABLE. This is the shared-member trap
/// that the disjointness predicate has to get right.
#[test]
fn forall_int_oneof_value_inside_enum_satisfiable() {
    let _g = OneofSeedOn::on();
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C ObjectIntersectionOf(DataSomeValuesFrom(:h DataOneOf("2"^^xsd:integer)) DataAllValuesFrom(:h DataOneOf("1"^^xsd:integer "2"^^xsd:integer))))
"#,
    );
    assert!(
        !c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
        "2 ∈ {{1,2}}: C must be satisfiable (no spurious clash)"
    );
}

/// NEGATIVE — WRONG PROPERTY: `{1} ⊆ {1,2}` but on different data properties,
/// so no subsumption (CR5 role match).
#[test]
fn int_oneof_wrong_property_not_subsumed() {
    let _g = OneofSeedOn::on();
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:g))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C DataSomeValuesFrom(:g DataOneOf("1"^^xsd:integer)))
    EquivalentClasses(:D DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "∃g.{{1}} ⊄ ∃h.{{1,2}}: wrong property must NOT subsume"
    );
}

// ── Issue #72 (2026-08-26): rdf:langString value membership ──────────────
//
// A language-tagged literal's identity is the PAIR (lexical form, tag), and
// `rdf:langString` is a DIFFERENT datatype from `xsd:string`. Before this,
// `exact_string_literal` rejected `Literal::Language` and nothing else picked
// it up, so a langString `DataHasValue` failed conversion outright and was
// DROPPED — no membership was derivable by any route.
//
// NEGATIVES-FIRST, as above: the new `lang:` bucket is new FP surface, and
// the curated corpus is inert for it (see this file's header). These are the
// entire safety net.

/// POSITIVE: same lexical form AND same tag ⇒ member of the enumeration.
#[test]
fn lang_value_in_oneof_subsumes() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:g))
    EquivalentClasses(:C DataHasValue(:g "bonjour"@fr))
    EquivalentClasses(:D DataSomeValuesFrom(:g DataOneOf("bonjour"@fr "hallo"@de)))
"#,
    );
    assert!(c.is_subclass(C, D), r#""bonjour"@fr ∈ {{@fr, @de}}"#);
}

/// NEGATIVE — DIFFERENT TAG, SAME LEXICAL FORM. The whole point of the pair
/// key: `"bonjour"@de` is a different literal from `"bonjour"@fr`.
#[test]
fn lang_same_lexical_different_tag_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:g))
    EquivalentClasses(:C DataHasValue(:g "bonjour"@de))
    EquivalentClasses(:D DataSomeValuesFrom(:g DataOneOf("bonjour"@fr)))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        r#""bonjour"@de ∉ {{"bonjour"@fr}}: the TAG is part of the key"#
    );
}

/// NEGATIVE — CROSS-DATATYPE, the FP this bucket exists to prevent. A plain
/// `xsd:string` and a language-tagged literal with the same lexical form are
/// different literals in different datatypes; a shared bucket would make them
/// subsume each other.
#[test]
fn lang_vs_plain_string_not_subsumed_either_direction() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:g))
    EquivalentClasses(:C DataHasValue(:g "bonjour"@fr))
    EquivalentClasses(:D DataHasValue(:g "bonjour"^^xsd:string))
"#,
    );
    assert!(!c.is_subclass(C, D), "langString ⊄ xsd:string");
    assert!(!c.is_subclass(D, C), "xsd:string ⊄ langString");
}

/// POSITIVE: a tagged literal IS an `rdf:langString`, so the bare datatype
/// acts as `Top` for this bucket.
#[test]
fn lang_value_subsumed_by_bare_langstring() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:g))
    EquivalentClasses(:C DataHasValue(:g "bonjour"@fr))
    EquivalentClasses(:D DataSomeValuesFrom(:g <http://www.w3.org/1999/02/22-rdf-syntax-ns#langString>))
"#,
    );
    assert!(c.is_subclass(C, D), r#""bonjour"@fr ∈ rdf:langString"#);
}

/// NEGATIVE: bare `xsd:string` must NOT swallow a language-tagged value.
#[test]
fn lang_value_not_subsumed_by_bare_xsd_string() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:g))
    EquivalentClasses(:C DataHasValue(:g "bonjour"@fr))
    EquivalentClasses(:D DataSomeValuesFrom(:g xsd:string))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "a langString value is NOT an xsd:string"
    );
}

/// POSITIVE: BCP47 tags compare case-insensitively (RDF 1.1 §3.3), so `@FR`
/// and `@fr` are the same literal. Normalising is a COMPLETENESS matter — a
/// missed membership, never an FP — but it is the documented semantics.
#[test]
fn lang_tag_comparison_is_case_insensitive() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:g))
    EquivalentClasses(:C DataHasValue(:g "bonjour"@FR))
    EquivalentClasses(:D DataSomeValuesFrom(:g DataOneOf("bonjour"@fr)))
"#,
    );
    assert!(c.is_subclass(C, D), r#""bonjour"@FR ≡ "bonjour"@fr"#);
}

/// NEGATIVE — WRONG PROPERTY, mirroring the other buckets' control: the pair
/// matches but the data property does not, so CR5 must not relay.
#[test]
fn lang_wrong_property_not_subsumed() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:g))
    Declaration(DataProperty(:h))
    EquivalentClasses(:C DataHasValue(:g "bonjour"@fr))
    EquivalentClasses(:D DataSomeValuesFrom(:h DataOneOf("bonjour"@fr)))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "∃g.{{@fr}} ⊄ ∃h.{{@fr}}: wrong property must NOT subsume"
    );
}

/// NEGATIVE — MIXED ENUMERATION DROPS WHOLE. A `DataOneOf` mixing a tagged
/// and an untagged literal is not a langString set; it must drop entirely
/// rather than silently become the tagged subset (a partial set would be
/// unsound in a sufficient-direction RHS).
#[test]
fn lang_mixed_oneof_drops_whole_enumeration() {
    let c = classify(
        r#"    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(DataProperty(:g))
    EquivalentClasses(:C DataHasValue(:g "bonjour"@fr))
    EquivalentClasses(:D DataSomeValuesFrom(:g DataOneOf("bonjour"@fr "plain"^^xsd:string)))
"#,
    );
    assert!(
        !c.is_subclass(C, D),
        "a mixed DataOneOf must drop whole, not become its tagged subset"
    );
}
