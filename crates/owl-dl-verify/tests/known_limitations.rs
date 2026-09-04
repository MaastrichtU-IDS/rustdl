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

/// F2 — **RESOLVED 2026-09-04 by the #87 F4 fix.** This test asserted the false
/// `Violated` and now asserts its absence; the name is kept so the history is
/// searchable.
///
/// F2 and #87's F4 are ONE mechanism, which was not obvious from either write-up:
/// F2 was filed as a builder imprecision ("the Tseitin marker's label … never
/// closed under `⊤ ⊑ C`") and F4 as a corpus-scale instrument false positive, and
/// they are the same sentence. `⊤ ⊑ C` is now applied at `FiniteModel::intern`, so
/// every element — Tseitin synthetics included — carries it.
///
/// The test tripping is what surfaced the connection: it was written to fail on a
/// fix ("F2 known limitation did not reproduce (fixed?)") rather than being
/// `#[ignore]`d, which is the only reason this did not go unnoticed. F1 and F3 are
/// genuinely different mechanisms and still reproduce.
#[test]
fn f2_nested_exists_plus_thing_subclass_no_longer_false_violates() {
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
        "F2 regressed — `⊤ ⊑ C` must reach the nested Tseitin witness: {verdict:?}"
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

/// #87 F4's own reproducer shape, kept separate from the retargeted F2 test above
/// because it pins the OTHER half of the fix: a universal antecedent must fire as a
/// RULE, not just contribute a label.
///
/// `⊤ ⊑ ∃r.C` is the discriminating case. `top_floor` cannot handle it —
/// `materialise_exists` is what builds an existential, and it is reached only if the
/// universal rule fires at all, which `expand_from_axioms` used to refuse because
/// `required_atoms` on `⊤` returns an empty antecedent.
#[test]
fn a_universal_antecedent_fires_as_a_rule_not_only_as_a_label() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex.org/topexists>
Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :Y)))
SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :C))
)
";
    let verdict = verify_ofn(ofn);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "`⊤ ⊑ ∃r.C` must be materialised on every element: {verdict:?}"
    );
}

/// THE GUARD THAT KEEPS THE #87 F4 FIX FROM BEING TOO BROAD, and the reason it tests
/// on `⊤` structurally rather than on "`required_atoms` returned nothing".
///
/// `required_atoms` yields an empty vector for every shape it cannot label from —
/// `Some`, `Or`, `Not`, `Min`, `All`. Reading THAT as universal would apply the
/// consequent to every element unconditionally, and the danger is not a spurious
/// violation but a **false all-clear**: extra labels can satisfy an axiom that should
/// have been reported violated.
///
/// Construction: `∃r.Y ⊑ C` has an antecedent `required_atoms` cannot decompose, and
/// `Z` is deliberately not a `∃r.Y`, so `Z`'s element must not acquire `C`.
/// `DisjointClasses(:Z :C)` makes that observable — if the predicate is loosened to an
/// emptiness test, every element gets `C`, `Z`'s element then carries both `Z` and `C`,
/// and the disjointness is violated.
///
/// **Sabotage-verified**: flipping `is_universal_antecedent`'s `_ => false` arm to
/// `_ => true` fails this test. A first version of it asserted only "the baseline
/// verifies" and did NOT fail under that sabotage — it was caught by an unrelated
/// cascade test instead, which is the difference between a guard and a decoration.
#[test]
fn an_unevaluable_antecedent_is_not_treated_as_universal() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/nonuniv>
Declaration(Class(:Z)) Declaration(Class(:Y)) Declaration(Class(:C))
Declaration(ObjectProperty(:r))
SubClassOf(ObjectSomeValuesFrom(:r :Y) :C)
DisjointClasses(:Z :C)
)
";
    let verdict = verify_ofn(ofn);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "an antecedent `required_atoms` cannot decompose must NOT be read as universal — \
         `C` on every element would violate `DisjointClasses(:Z :C)`: {verdict:?}"
    );
}
