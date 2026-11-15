//! #527 / #528: `abox_check` (the told-`ABox` consistency pre-check classify and the
//! consistency query rely on) read an inverse role as its named role in three passes, and
//! reported consistent ontologies inconsistent, so `classify` marked every class
//! unsatisfiable:
//! - P7: `Domain(r⁻)` / `Range(r⁻)` typed the wrong end of every `r`-edge;
//! - P5: `Functional(r⁻)` (= `InverseFunctional(r)`) merged two `r`-successors, and P6
//!   then saw the false merge;
//! - P8: `∃r⁻.Q` was read as `∃r.Q` under `Functional(r)`.
//!
//! Negatives first: every "consistent" fixture is consistent by `HermiT` and Konclude
//! (`ClassAssertion(owl:Nothing a)` refuted by both), and every "inconsistent" control is
//! inconsistent by `HermiT` and by the semantics (Konclude under-reports on the
//! asymmetric, irreflexive and disjoint-role controls).
//!
//! These are end-to-end pins: an inconsistent control here is decided by the tableau
//! even when `abox_check` stays silent. The verdicts of the passes the fix added (P5 on
//! `Functional(r⁻)`, P8 on a functional `r⁻`) are pinned by the in-module tests in
//! `src/abox_check.rs`.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/o>\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F))\n\
         Declaration(Class(:Q1)) Declaration(Class(:Q2))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))\n\
         SubClassOf(:F :D)\n{body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// Consistent, and `classify` reports no unsatisfiable class.
#[track_caller]
fn assert_consistent(case: &str, body: &str) {
    let o = onto(body);
    assert!(
        owl_dl_reasoner::is_consistent(&o).unwrap(),
        "{case}: reported inconsistent"
    );
    let c = owl_dl_reasoner::classify(&o).unwrap();
    assert!(
        c.unsatisfiable_classes().is_empty(),
        "{case}: classify marked {:?} unsatisfiable",
        c.unsatisfiable_classes()
    );
}

#[track_caller]
fn assert_inconsistent(case: &str, body: &str) {
    assert!(
        !owl_dl_reasoner::is_consistent(&onto(body)).unwrap(),
        "{case}: the control must be inconsistent"
    );
}

/// #527, P7: `Domain(r⁻) = D` is `Range(r) = D`, so `r(a, b)` types `b`, not `a`.
#[test]
fn an_inverse_role_domain_types_the_edge_s_target() {
    assert_consistent(
        "Domain(r⁻), a:E",
        "ObjectPropertyDomain(ObjectInverseOf(:r) :D) DisjointClasses(:D :E) \
         ClassAssertion(:E :a) ObjectPropertyAssertion(:r :a :b)",
    );
    assert_inconsistent(
        "Domain(r⁻), b:E",
        "ObjectPropertyDomain(ObjectInverseOf(:r) :D) DisjointClasses(:D :E) \
         ClassAssertion(:E :b) ObjectPropertyAssertion(:r :a :b)",
    );
}

/// #527, P7's other arm: `Range(r⁻) = D` is `Domain(r) = D`, so `r(a, b)` types `a`.
#[test]
fn an_inverse_role_range_types_the_edge_s_source() {
    assert_consistent(
        "Range(r⁻), b:E",
        "ObjectPropertyRange(ObjectInverseOf(:r) :D) DisjointClasses(:D :E) \
         ClassAssertion(:E :b) ObjectPropertyAssertion(:r :a :b)",
    );
    assert_inconsistent(
        "Range(r⁻), a:E",
        "ObjectPropertyRange(ObjectInverseOf(:r) :D) DisjointClasses(:D :E) \
         ClassAssertion(:E :a) ObjectPropertyAssertion(:r :a :b)",
    );
}

/// #528, P5: `Functional(r⁻)` bounds `r`-predecessors, not `r`-successors.
#[test]
fn functional_inverse_bounds_predecessors_not_successors() {
    assert_consistent(
        "Functional(r⁻), two successors",
        "FunctionalObjectProperty(ObjectInverseOf(:r)) ObjectPropertyAssertion(:r :a :b) \
         ObjectPropertyAssertion(:r :a :c) DifferentIndividuals(:b :c)",
    );
    // P6 reads P5's merges: no false merge, so no irreflexive violation.
    assert_consistent(
        "Functional(r⁻) via P6",
        "FunctionalObjectProperty(ObjectInverseOf(:r)) ObjectPropertyAssertion(:r :a :b) \
         ObjectPropertyAssertion(:r :a :c) IrreflexiveObjectProperty(:s) \
         ObjectPropertyAssertion(:s :b :c)",
    );
    assert_inconsistent(
        "Functional(r⁻), two predecessors",
        "FunctionalObjectProperty(ObjectInverseOf(:r)) ObjectPropertyAssertion(:r :a :b) \
         ObjectPropertyAssertion(:r :c :b) DifferentIndividuals(:a :c)",
    );
    // The mirror is normalised by the converter (#149: `InverseFunctional(r⁻)` becomes
    // `FunctionalRole(r)`), so it was right before this fix; pinned with that reason.
    assert_consistent(
        "InverseFunctional(r⁻), two predecessors of b",
        "InverseFunctionalObjectProperty(ObjectInverseOf(:r)) ObjectPropertyAssertion(:r :a :b) \
         ObjectPropertyAssertion(:r :c :b) DifferentIndividuals(:a :c)",
    );
}

/// #528, P8: the functional-role existential collapse keys on the signed role.
#[test]
fn the_functional_existential_collapse_keeps_the_role_s_polarity() {
    let types = "DisjointClasses(:Q1 :Q2) ClassAssertion(:C :a)";
    assert_consistent(
        "∃r⁻ under Functional(r)",
        &format!(
            "{types} FunctionalObjectProperty(:r) \
             SubClassOf(:C ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q1)) \
             SubClassOf(:C ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q2))"
        ),
    );
    assert_consistent(
        "∃r under Functional(r⁻)",
        &format!(
            "{types} FunctionalObjectProperty(ObjectInverseOf(:r)) \
             SubClassOf(:C ObjectSomeValuesFrom(:r :Q1)) SubClassOf(:C ObjectSomeValuesFrom(:r :Q2))"
        ),
    );
    assert_consistent(
        "∃r⁻ under Functional(r), EquivalentClasses",
        &format!(
            "{types} FunctionalObjectProperty(:r) EquivalentClasses(:C ObjectIntersectionOf(\
             ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q1) ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q2)))"
        ),
    );
    // `r` and `r⁻` both functional, one existential on each: two different elements
    // (`x:Q2 -r-> a -r-> y:Q1`), so nothing collapses.
    assert_consistent(
        "∃r.Q1 and ∃r⁻.Q2 under Functional(r) and InverseFunctional(r)",
        &format!(
            "{types} FunctionalObjectProperty(:r) InverseFunctionalObjectProperty(:r) \
             SubClassOf(:C ObjectSomeValuesFrom(:r :Q1)) \
             SubClassOf(:C ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q2))"
        ),
    );
    // Controls: the collapse still fires where the signed role IS functional.
    assert_inconsistent(
        "∃r⁻ under InverseFunctional(r) = Functional(r⁻)",
        &format!(
            "{types} InverseFunctionalObjectProperty(:r) \
             SubClassOf(:C ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q1)) \
             SubClassOf(:C ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q2))"
        ),
    );
    assert_inconsistent(
        "∃r under Functional(r)",
        &format!(
            "{types} FunctionalObjectProperty(:r) \
             SubClassOf(:C ObjectSomeValuesFrom(:r :Q1)) SubClassOf(:C ObjectSomeValuesFrom(:r :Q2))"
        ),
    );
}

/// The axiom kinds of the passes the polarity audit found correct (P3, P6, P9), on `r⁻`
/// forms, end to end. The inconsistent controls may be decided by the tableau rather
/// than by those passes (P9 skips inverse pairs, a sound miss); the consistent cases
/// pin that no pass reads `r⁻` as `r`.
#[test]
fn the_other_role_passes_respect_polarity() {
    assert_consistent(
        "P3: ¬r⁻(a,b) with r(a,b)",
        "NegativeObjectPropertyAssertion(ObjectInverseOf(:r) :a :b) ObjectPropertyAssertion(:r :a :b)",
    );
    assert_inconsistent(
        "P3: ¬r⁻(a,b) with r(b,a)",
        "NegativeObjectPropertyAssertion(ObjectInverseOf(:r) :a :b) ObjectPropertyAssertion(:r :b :a)",
    );
    assert_consistent(
        "P6: Asymmetric(r⁻), one edge",
        "AsymmetricObjectProperty(ObjectInverseOf(:r)) ObjectPropertyAssertion(:r :a :b)",
    );
    assert_inconsistent(
        "P6: Asymmetric(r⁻), both edges",
        "AsymmetricObjectProperty(ObjectInverseOf(:r)) ObjectPropertyAssertion(:r :a :b) \
         ObjectPropertyAssertion(:r :b :a)",
    );
    assert_inconsistent(
        "P6: Irreflexive(r⁻), r(a,a)",
        "IrreflexiveObjectProperty(ObjectInverseOf(:r)) ObjectPropertyAssertion(:r :a :a)",
    );
    assert_consistent(
        "P9: Disjoint(r⁻, s), r(a,b), s(a,b)",
        "DisjointObjectProperties(ObjectInverseOf(:r) :s) ObjectPropertyAssertion(:r :a :b) \
         ObjectPropertyAssertion(:s :a :b)",
    );
    assert_inconsistent(
        "P9: Disjoint(r⁻, s), r(b,a), s(a,b)",
        "DisjointObjectProperties(ObjectInverseOf(:r) :s) ObjectPropertyAssertion(:r :b :a) \
         ObjectPropertyAssertion(:s :a :b)",
    );
}
