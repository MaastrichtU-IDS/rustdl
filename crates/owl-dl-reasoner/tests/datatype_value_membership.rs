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

#[test]
fn forall_cross_datatype_no_clash() {
    // ∃h.{5-int} ⊓ ∀h.[0.0,3.0]-double — different buckets never seed
    // disjointness, so NO clash (sound under-approx, not a wrong ⊥).
    assert!(
        !forall_clash_unsat(
            r#""5"^^xsd:integer"#,
            r#"DatatypeRestriction(xsd:double xsd:minInclusive "0.0"^^xsd:double xsd:maxInclusive "3.0"^^xsd:double)"#
        ),
        "int value vs double range: cross-datatype, no clash"
    );
}
