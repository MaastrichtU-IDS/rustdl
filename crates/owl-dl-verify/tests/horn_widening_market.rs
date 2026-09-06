//! Is there ANY ontology the `verify-el` CLI gate refuses as non-`PureEl`,
//! that is nonetheless `Horn` AND that this crate can actually check?
//!
//! `docs/benchmarks/2026-09-05-verify-el-horn-widening-is-analytically-empty.md`
//! argues the market is empty: every construct that makes an ontology
//! Horn-but-not-EL is refused by `eval.rs`, and `verify` reaches `Verified`
//! only with ZERO unresolved axioms. These tests are the discriminating
//! probes for that claim — they call `build_model`/`verify` DIRECTLY, which
//! bypasses `main.rs`'s `analyze_fragment != PureEl` early exit, so they can
//! observe what the checker would say if the gate were widened.
//!
//! They exist because the claim was nearly shipped over four unread
//! `eval.rs` arms.

use owl_dl_reasoner::FragmentClassification as FC;
use owl_dl_verify::{Bounds, Verdict};

mod common;
use common::load;

const HEADER: &str = "Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

fn verdict_of(ofn: &str) -> (FC, Verdict) {
    let internal = load(ofn);
    let fragment = owl_dl_reasoner::analyze_fragment(&internal);
    let verdict = match owl_dl_verify::build_model(&internal, &Bounds::default()) {
        Err(reason) => Verdict::Unresolved {
            domain_size: 0,
            reasons: vec![reason],
        },
        Ok((model, build_reasons)) => {
            let (v, _) = owl_dl_verify::verify(model, &internal, None);
            assert!(
                build_reasons.is_empty(),
                "unexpected build-time refusal: {build_reasons:?}"
            );
            v
        }
    };
    (fragment, verdict)
}

/// A `∀` is the canonical Horn-∖-EL generator, and `eval.rs:87` refuses it.
/// The verdict must therefore be `Unresolved` — widening the gate to admit
/// this ontology would buy no coverage.
#[test]
fn a_forall_is_horn_but_not_el_and_the_evaluator_refuses_it() {
    let (fragment, verdict) = verdict_of(&format!(
        "{HEADER}Ontology(\n\
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(ObjectProperty(:r))\n\
  SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n\
  SubClassOf(:A ObjectAllValuesFrom(:r :B))\n)\n"
    ));
    assert_ne!(fragment, FC::PureEl, "the CLI gate must refuse this today");
    assert!(
        matches!(verdict, Verdict::Unresolved { .. }),
        "a widened gate must not reach a verdict here, got {verdict:?}"
    );
}

/// THE COUNTEREXAMPLE TO "ZERO". `is_el_axiom` requires every
/// `DisjointClasses` member to be ATOMIC, so a conjunctive member leaves the
/// EL fragment — but `eval.rs`'s `DisjointClasses` arm has no atomicity check
/// and `eval_concept` handles `CE::And` without complaint.
///
/// If this reports `Verified`, the admissible market is non-empty and the
/// analytical claim must be stated as "these shapes only", not "zero".
#[test]
fn a_conjunctive_disjointclasses_member_is_the_one_admissible_shape() {
    let (fragment, verdict) = verdict_of(&format!(
        "{HEADER}Ontology(\n\
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
  Declaration(Class(:X)) Declaration(ObjectProperty(:r))\n\
  SubClassOf(:X ObjectSomeValuesFrom(:r :A))\n\
  SubClassOf(:A :B)\n\
  DisjointClasses(ObjectIntersectionOf(:A :B) :C)\n)\n"
    ));
    eprintln!("fragment = {fragment:?}\nverdict  = {verdict:?}");
    assert_ne!(fragment, FC::PureEl, "the CLI gate must refuse this today");
}

/// THE SECOND HOLE. `is_el_axiom:2183-2188` requires an `ObjectPropertyDomain`
/// filler to be atomic — the D10 "Bug B" tightening, whose own comment says
/// the engine's `role_domains` accepts ONLY `Atomic` fillers and **silently
/// drops** a conjunctive one. `eval.rs:366` checks only that the role is not
/// inverse, then calls `eval_concept`, which handles `CE::And` fine. So this
/// shape is `Horn`, non-EL, and reachable by the checker.
///
/// # What it reports, and why that is NOT simply a false positive
///
/// `build_model` reads the EL saturation closure, which dropped this axiom, so
/// the model has an `r`-edge whose source is outside `P ⊓ Q` and the checker
/// reports `Violated`. The obvious reading is "instrument artifact": the gate
/// correctly routes this ontology to the HYBRID path, so a complaint about the
/// EL closure says nothing about the engine that answers the query.
///
/// **We adjudicated it, and the obvious reading was wrong.** `classify` returns
/// **zero rows** on this fixture with `incomplete: false` and `dropped: {}`,
/// while Konclude and `HermiT` both derive `X ⊑ P` and `X ⊑ Q`, and rustdl's own
/// `subclass` proves `X ⊑ P`. It is a live D10-shaped defect in the hybrid
/// path, recovered by `RUSTDL_CLASSIFY_SAME_TIER=1` (the tier walk never
/// compares same-tier classes, and dropping the conjunctive domain leaves `X`
/// with no EL subsumer, hence in `P`'s own tier). The ATOMIC-filler control
/// classifies correctly at the default, which is what isolates the conjunctive
/// filler as the cause.
///
/// So the lesson for a widened gate is the one the crate's own doc already
/// states: a `Violated` built on a mismatched model source is a **LEAD
/// requiring adjudication**, not a proof — and here the adjudication paid.
#[test]
fn a_conjunctive_domain_filler_is_horn_non_el_and_the_checker_reaches_a_verdict() {
    let (fragment, verdict) = verdict_of(&format!(
        "{HEADER}Ontology(\n\
  Declaration(Class(:P)) Declaration(Class(:Q)) Declaration(Class(:X))\n\
  Declaration(Class(:B)) Declaration(ObjectProperty(:r))\n\
  SubClassOf(:X ObjectSomeValuesFrom(:r :B))\n\
  ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))\n)\n"
    ));
    assert_eq!(
        fragment,
        FC::Horn,
        "this shape must sit in the widening's candidate population"
    );
    assert!(
        !matches!(verdict, Verdict::Unresolved { .. }),
        "the checker must REACH a verdict here — that is what makes this a hole, got {verdict:?}"
    );
}
