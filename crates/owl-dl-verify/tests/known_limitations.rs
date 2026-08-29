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

/// F2: a nested `∃` plus an ordinary `SubClassOf(owl:Thing, C)`. An element is seeded per
/// Tseitin marker for the nested witness, and its label comes back as `target_label`'s minimal
/// `{Q}` row — never closed under `⊤ ⊑ C`. `SubClassOf(owl:Thing, …)` is ordinary in real EL
/// ontologies, not an exotic construct.
#[test]
fn f2_nested_exists_plus_thing_subclass_is_a_false_violated() {
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
        matches!(verdict, Verdict::Violated { .. }),
        "F2 known limitation did not reproduce (fixed?): {verdict:?}"
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
