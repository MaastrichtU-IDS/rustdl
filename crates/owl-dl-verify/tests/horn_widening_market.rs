//! Is there ANY ontology the `verify-el` CLI gate refuses as non-`PureEl`,
//! that is nonetheless `Horn` AND that this crate can actually check?
//!
//! The market analysis behind this file found the gate refuses almost every
//! Horn-but-not-EL construct outright (`eval.rs` returns `Unresolved` for it),
//! and `verify` reaches `Verified` only with ZERO unresolved axioms. It found
//! exactly two holes where the gate and the checker disagreed: a non-atomic
//! `DisjointClasses` member, and a conjunctive `ObjectPropertyDomain`/`Range`
//! filler. **The second hole is now CLOSED by #110** — the fix moved the gate
//! and the engine in lockstep, so that fixture reports `PureEl` (see the
//! retargeted test below). These tests are the discriminating probes for the
//! wider claim — they call `build_model`/`verify` DIRECTLY, which bypasses
//! `main.rs`'s `analyze_fragment != PureEl` early exit, so they can observe
//! what the checker would say if the gate were widened.
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

/// THE SECOND HOLE — RETARGETED (#110). This test used to pin a live defect:
/// a conjunctive `ObjectPropertyDomain` filler (`Domain(r, P ⊓ Q)`) was
/// silently dropped by the saturator while `is_el_axiom` certified the
/// ontology `PureEl` (via `is_el_concept`, which has no atomicity check) —
/// D10-shaped, and it cost `classify` two real subsumptions
/// (`X ⊑ P`, `X ⊑ Q`) that Konclude, `HermiT` and rustdl's own `subclass` all
/// confirmed. #110 fixed it at the engine: `decompose_role_filler` now
/// processes every atomic conjunct of a fully-decomposable filler, and the
/// gate (`is_processed_role_filler`, née `is_atomic_or_trivial_concept`) moved
/// in lockstep, so this exact fixture now correctly reports `PureEl` — the gate
/// and the engine agree, and there is no hole here anymore.
///
/// That does NOT shrink the market-analysis finding to zero holes: it moves
/// this shape out of "Horn-non-EL-and-checkable" and into "now decomposes to
/// EL", which is a stronger result than the original two-hole count implied.
/// A `Verified` verdict on this fixture (reachable through the fast path) was
/// itself the finding this widening spike was chasing.
///
/// So the fixture is retargeted to a shape that stays out-of-EL for a reason
/// that is a DESIGN DECISION, not a defect: an **existential**
/// `ObjectPropertyDomain` filler, `Domain(r, ∃s.S)`. Unlike a conjunction,
/// this is not a case of "the decomposition exists but the engine forgot to
/// apply it" — `decompose_role_filler`'s contract only ever pushes ATOMIC
/// conjuncts into `role_domains`/`role_ranges: HashMap<RoleId, Vec<ClassId>>`,
/// so an existential filler has no `ClassId` to push at all; admitting it
/// would need a structurally different rule (materializing `x ⊑ ∃s.S` at
/// every `r`-edge source), not a wider decomposer. (`ObjectUnionOf` was tried
/// first, per the original plan; it measures `OutOfFragment`, not `Horn` — a
/// disjunctive head is non-Horn by definition, so it cannot serve this test's
/// purpose of exhibiting a Horn-but-non-EL shape. Measure before asserting.)
/// See [[tests-that-pin-the-bug]].
#[test]
fn an_existential_domain_filler_is_horn_non_el_and_the_checker_reaches_a_verdict() {
    let (fragment, verdict) = verdict_of(&format!(
        "{HEADER}Ontology(\n\
  Declaration(Class(:X)) Declaration(Class(:B)) Declaration(Class(:S))\n\
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
  SubClassOf(:X ObjectSomeValuesFrom(:r :B))\n\
  ObjectPropertyDomain(:r ObjectSomeValuesFrom(:s :S))\n)\n"
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
