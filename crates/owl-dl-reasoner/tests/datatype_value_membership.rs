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
