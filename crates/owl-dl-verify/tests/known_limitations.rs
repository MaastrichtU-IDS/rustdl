//! Reproducers for three false-`Violated` defects found during the whole-branch review of
//! `feat/negative-certificates-phase1` (documented as F1/F2/F3 in
//! `docs/known-limitations/verify-two-expansion-paths-split-a-witness.md`). Each is a genuine
//! defect in THIS crate's own model builder, not evidence of a rustdl engine gap — the paired
//! control in F1/F2 shows the flat/non-conjunctive shape verifies cleanly, isolating the
//! trigger.
//!
//! These tests pin the CURRENT (defective) behaviour deliberately, unlike an `#[ignore]`d
//! sentinel that would go unnoticed if it silently started passing (see
//! `docs/2026-08-18-ignored-sentinels-went-stale-unobserved.md`): they run on every
//! `cargo test`, and if one starts returning `Verified` instead of `Violated`, the assertion
//! message says so explicitly rather than passing silently. That is good news for the crate,
//! but it means this file (and the known-limitations doc) need updating, not deleting outright
//! — re-check whether the underlying label-closure gap is actually fixed or only not triggered
//! by this particular shape before removing the entry.

use owl_dl_verify::{Bounds, Verdict};

mod common;
use common::load;

/// Runs `owl-dl-verify`'s own instrument end to end (`build_model` then `verify`) and returns
/// the check-time verdict. Panics if the fixture itself trips a BUILD-time bound/refusal, since
/// none of F1/F2/F3 are about build-time behaviour — that would be a different defect.
fn verify_ofn(ofn: &str) -> Verdict {
    let internal = load(ofn);
    let (model, build_reasons) = owl_dl_verify::build_model(&internal, &Bounds::default())
        .expect("build_model should succeed on these small fixtures");
    assert!(
        build_reasons.is_empty(),
        "fixture should not itself trip a build-time bound: {build_reasons:?}"
    );
    let (verdict, _verified_model) = owl_dl_verify::verify(model, &internal, None);
    verdict
}

/// F1: a conjunctive `∃`-body plus a GCI over that conjunction. The witness label is
/// `subsumers_of(A) ∪ subsumers_of(B)` (`model.rs`'s `ConceptExpr::Some` arm, the
/// `label.extend(l)` loop over `required_atoms`), never closed under `A ⊓ B ⊑ C` — so the
/// witness satisfies `A ⊓ B` by its own label, and the model then reports that it does NOT
/// satisfy `C`, i.e. it reports the very axiom that should have closed the label as violated.
#[test]
fn f1_conjunctive_exists_body_gci_is_a_false_violated() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/f1>
Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
Declaration(ObjectProperty(:r))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:A :B)))
SubClassOf(ObjectIntersectionOf(:A :B) :C)
)
";
    let verdict = verify_ofn(ofn);
    assert!(
        matches!(verdict, Verdict::Violated { .. }),
        "F1 known limitation did not reproduce (fixed?): {verdict:?}"
    );
}

/// F1's control: the same shape with a flat (non-conjunctive) `∃`-body verifies cleanly,
/// isolating the conjunction as the trigger.
#[test]
fn f1_control_flat_exists_body_verifies_cleanly() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/f1ctl>
Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:C))
Declaration(ObjectProperty(:r))
SubClassOf(:X ObjectSomeValuesFrom(:r :A))
SubClassOf(:A :C)
)
";
    let verdict = verify_ofn(ofn);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "control should verify cleanly: {verdict:?}"
    );
}

/// F2 — **FIXED** (#87 F4, 2026-09-04). Kept as a regression test, not deleted, exactly as
/// this file's header instructs.
///
/// It was: a nested `∃` plus an ordinary `SubClassOf(owl:Thing, C)`. An element is seeded per
/// Tseitin marker for the nested witness, and its label came back as `target_label`'s minimal
/// `{Q}` row — never closed under `⊤ ⊑ C`. `SubClassOf(owl:Thing, …)` is ordinary in real EL
/// ontologies, not an exotic construct: this fired on 4 ORE ontologies, with the violation
/// count equal to the `⊤ ⊑ C` axiom count EXACTLY.
///
/// The header asks whether the underlying gap is genuinely fixed or merely untriggered by this
/// shape. Answered by measurement, not assertion — `FiniteModel::intern` now closes any
/// label containing NO named class under the ⊤-supers, so the sibling tests below cover a
/// deeper nesting, the `EquivalentClasses(owl:Thing, …)` spelling, and a ⊤-super reached only
/// through the subsumer closure. All three verify clean; none of them is this shape.
#[test]
fn f2_nested_exists_plus_thing_subclass_now_verifies() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex.org/f2>
Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :Y)))
SubClassOf(owl:Thing :C)
)
";
    let verdict = verify_ofn(ofn);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "F2 regressed - a Tseitin witness is no longer closed under the ⊤-supers: {verdict:?}"
    );
}

/// F2, a DEEPER nesting. Three levels of `∃` rather than two, so a different marker sits at a
/// different depth. Covers the mechanism rather than the one shape F2 happened to use.
#[test]
fn a_thrice_nested_witness_is_also_closed_under_the_top_supers() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex.org/f2deep>
Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:t))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:t :Y))))
SubClassOf(owl:Thing :C)
)
";
    let verdict = verify_ofn(ofn);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "a 3-deep Tseitin witness must be closed under the ⊤-supers too: {verdict:?}"
    );
}

/// F2 in its other spelling. `EquivalentClasses(owl:Thing, C)` asserts the same thing as
/// `SubClassOf(owl:Thing, C)` and conversion does NOT rewrite it into that form, so
/// `top_supers_of` has to match it separately. Without that arm this reports `Violated`.
#[test]
fn the_equivalent_classes_spelling_of_a_top_axiom_is_also_collected() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex.org/f2equiv>
Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :Y)))
EquivalentClasses(owl:Thing :C)
)
";
    let verdict = verify_ofn(ofn);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "EquivalentClasses(owl:Thing, C) asserts C of every element too: {verdict:?}"
    );
}

/// A ⊤-super reached only THROUGH THE CLOSURE: `⊤ ⊑ C` and `C ⊑ D` make `D` true of every
/// element, but `D` appears in no ⊤-axiom. Pins the `subs.subsumers_of` step in
/// `top_supers_of` — collecting the axiom heads alone leaves this `Violated`.
#[test]
fn a_top_super_reached_through_the_closure_is_collected() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex.org/f2chain>
Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C)) Declaration(Class(:D))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :Y)))
SubClassOf(owl:Thing :C)
SubClassOf(:C :D)
)
";
    let verdict = verify_ofn(ofn);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "a ⊤-super's own superclass holds of every element too: {verdict:?}"
    );
}

/// F2's control: the same `⊤ ⊑ C` axiom over a FLAT (non-nested) `∃` verifies cleanly — the
/// saturator's own closure already folds `⊤ ⊑ C` into every class's subsumer set, so the
/// fact-driven path (rather than the Tseitin-marker path) never loses it.
#[test]
fn f2_control_flat_exists_plus_thing_subclass_verifies_cleanly() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex.org/f2ctl>
Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C))
Declaration(ObjectProperty(:r))
SubClassOf(:X ObjectSomeValuesFrom(:r :Y))
SubClassOf(owl:Thing :C)
)
";
    let verdict = verify_ofn(ofn);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "control should verify cleanly: {verdict:?}"
    );
}

/// F3: a nested `∃` plus `ObjectPropertyDomain` on the INNER role. The intermediate witness
/// (the outer `∃r`'s target) is minted with a label from `effective_ranges` — empty here,
/// since `r` has no declared domain/range — and `materialise_exists` then recurses into the
/// inner `∃s.Y` body AT that element, giving it an outgoing `s`-edge. `ObjectPropertyDomain(s,
/// D)` is then checked against an edge source the model itself built label-less.
#[test]
fn f3_nested_exists_plus_inner_domain_is_a_false_violated() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/f3>
Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:D))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :Y)))
ObjectPropertyDomain(:s :D)
)
";
    let verdict = verify_ofn(ofn);
    assert!(
        matches!(verdict, Verdict::Violated { .. }),
        "F3 known limitation did not reproduce (fixed?): {verdict:?}"
    );
}

/// End-to-end: a named class missing a ⊤-super is still REPORTED, i.e. the evaluator checks
/// `⊤ ⊑ C` against named-class elements and the F2 fix did not make it stop looking.
///
/// **This test does NOT pin the scoping rule, despite an earlier version of it claiming to.**
/// It was written to catch the tempting simplification of closing EVERY label under the
/// ⊤-supers, and it SURVIVED that sabotage: `test_only_remove_from_label` runs AFTER interning,
/// so the entry is gone under either scoping and the check fails either way. Its green never
/// depended on the thing it advertised.
///
/// The scoping rule is pinned where it can actually be observed —
/// `model::intern_top_supers_tests::a_label_holding_a_named_class_is_left_alone`, which
/// asserts on `intern` directly and DOES fail under that sabotage. Keep both: this one covers
/// the evaluator, that one covers the rule.
#[test]
fn a_named_class_missing_a_top_super_is_still_reported() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex.org/f2named>
Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :Y)))
SubClassOf(owl:Thing :C)
)
";
    let internal = load(ofn);
    let (mut model, build_reasons) =
        owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(build_reasons.is_empty(), "{build_reasons:?}");

    // Clean to begin with - that is the post-fix F2 behaviour asserted above. Rebuilt rather
    // than cloned because `FiniteModel` is deliberately not `Clone`.
    let (clean, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let (verdict, _) = owl_dl_verify::verify(clean, &internal, None);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "precondition: this fixture verifies before the mutation, got {verdict:?}"
    );

    let x = internal
        .vocabulary
        .class_id("http://ex.org/X")
        .expect("X is a named class");
    let c = internal
        .vocabulary
        .class_id("http://ex.org/C")
        .expect("C is a named class");
    let elem_x = model.element_of_class(x).expect("X is satisfiable");
    model.test_only_remove_from_label(elem_x, c);

    let (verdict, _) = owl_dl_verify::verify(model, &internal, None);
    assert!(
        matches!(verdict, Verdict::Violated { .. }),
        "a NAMED class missing a ⊤-super is a genuine engine defect and must still be \
         reported. If this now says Verified, `intern` is closing named-class labels under \
         the ⊤-supers and the instrument has gone blind to a whole defect class: {verdict:?}"
    );
}
