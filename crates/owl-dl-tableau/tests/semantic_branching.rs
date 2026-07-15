//! Integration tests for `RUSTDL_SEMANTIC_BRANCHING` Layer A (Fix#2):
//! in-search disjointness pruning + unit-forcing at the `⊔` decision.
//!
//! Layer A is **verdict-preserving** — it only drops disjuncts that the
//! reactive `horn_fixpoint` would clash on the next pass anyway, and forces a
//! lone survivor the search was compelled to take. The tests assert the
//! flag-ON verdict is identical to flag-OFF while the pruning/forcing counters
//! prove the mechanism actually fired (non-vacuous).
//!
//! Test 2 is the FP tripwire canary (advisor 2026-07-15): the dropped
//! disjunct's clashing label is placed by an ANCESTOR `⊔` decision whose level
//! is NOT in the inner disjunction's `body_deps`. If Layer A emitted a bare
//! `body_deps` (a SUBSET of the flag-OFF clash dep-set) the inner `Unsat` would
//! be attributed without the ancestor decision, the outer loop would backjump
//! past it, skip the satisfiable sibling branch, and return a FALSE `Unsat` —
//! an unsound false-positive subsumption. The correct SUPERSET dep-set keeps
//! the verdict `Sat`.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::{Atom, DlClause, X, clausify_with_stats};
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::ir::ClassId;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult, SearchStats};
use std::io::Cursor;

fn build(src: &str, root_iri: &str) -> (owl_dl_core::InternalOntology, owl_dl_core::ir::ClassId) {
    let mut reader = Cursor::new(src.to_owned());
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let root = internal
        .vocabulary
        .class_id(root_iri)
        .expect("root class declared");
    (internal, root)
}

/// Build a fresh engine for `root`, optionally with semantic branching on,
/// run `decide`, and return the verdict + stats.
fn run(
    internal: &owl_dl_core::InternalOntology,
    root: owl_dl_core::ir::ClassId,
    semantic: bool,
) -> (HyperResult, SearchStats) {
    let (clauses, _) = clausify_with_stats(internal);
    let mut eng = HyperEngine::new(&clauses, root);
    if semantic {
        eng = eng.with_semantic_branching();
    }
    let verdict = eng.decide(64);
    let stats = eng.stats();
    (verdict, stats)
}

/// Prune + unit-force. An outer covering `C ⊑ E ⊔ F` places `E` at RUNTIME
/// (the disjoint partner cannot be statically eliminated — rustdl's
/// preprocessing already unit-propagates statically-forced disjoint disjuncts,
/// so the pruning we want to exercise only arises dynamically). An inner
/// covering `Y ⊑ B1 ⊔ B2` (`Y` Horn-forced onto `C`) has `B1` told-disjoint
/// with `E`. In the `E`-branch Layer A drops `B1` and unit-forces the lone
/// survivor `B2`; the verdict (`Sat`) is unchanged.
const PRUNE_SRC: &str = "Prefix(:=<http://rustdl.test/>)
Ontology(<http://rustdl.test/prune>
    Declaration(Class(:C))
    Declaration(Class(:E))
    Declaration(Class(:F))
    Declaration(Class(:Y))
    Declaration(Class(:B1))
    Declaration(Class(:B2))
    SubClassOf(:C ObjectUnionOf(:E :F))
    SubClassOf(:C :Y)
    SubClassOf(:Y ObjectUnionOf(:B1 :B2))
    DisjointClasses(:E :B1)
)
";

#[test]
fn prunes_dead_disjunct_and_forces_survivor() {
    let (internal, c) = build(PRUNE_SRC, "http://rustdl.test/C");

    let (v_off, s_off) = run(&internal, c, false);
    let (v_on, s_on) = run(&internal, c, true);

    assert_eq!(v_on, v_off, "Layer A must preserve the verdict");
    assert_eq!(v_on, HyperResult::Sat, "C is satisfiable");
    assert!(
        s_on.semantic_prunes >= 1,
        "the disjoint disjunct B1 must be pruned in the E-branch (non-vacuous), got {}",
        s_on.semantic_prunes
    );
    assert!(
        s_on.semantic_unit_forces >= 1,
        "the lone survivor B2 must be unit-forced, got {}",
        s_on.semantic_unit_forces
    );
    assert_eq!(s_off.semantic_prunes, 0, "OFF path never prunes");
    assert_eq!(s_off.semantic_unit_forces, 0, "OFF path never unit-forces");
}

/// FP tripwire canary. Outer covering `C ⊑ E ⊔ F` (decision `d_e`); an inner
/// covering `Y ⊑ G ⊔ H` whose body `Y` is Horn-forced onto `C` (so
/// `deps(Y) = ∅`, NOT `d_e`). In the `E`-branch both `G` and `H` are
/// told-disjoint with the ancestor-placed `E`, so the inner disjunction dies —
/// but its clash genuinely depends on `d_e` (via `deps(E)`), which is NOT in
/// `deps(Y)`. The `F`-branch is satisfiable. Correct verdict: `Sat`.
///
/// A bare-`body_deps` (subset) implementation would attribute the inner `Unsat`
/// to `deps(Y) = ∅`, the outer loop would backjump past `d_e`, skip the `F`
/// branch, and return a false `Unsat`. This fixture is discriminating: it flips
/// to `Unsat` under the subset bug and is `Sat` under the correct superset.
const ANCESTOR_DEP_SRC: &str = "Prefix(:=<http://rustdl.test/>)
Ontology(<http://rustdl.test/ancestor>
    Declaration(Class(:C))
    Declaration(Class(:Y))
    Declaration(Class(:E))
    Declaration(Class(:F))
    Declaration(Class(:G))
    Declaration(Class(:H))
    SubClassOf(:C :Y)
    SubClassOf(:C ObjectUnionOf(:E :F))
    SubClassOf(:Y ObjectUnionOf(:G :H))
    DisjointClasses(:E :G)
    DisjointClasses(:E :H)
)
";

#[test]
fn ancestor_placed_clash_does_not_trigger_unsound_backjump() {
    let (internal, c) = build(ANCESTOR_DEP_SRC, "http://rustdl.test/C");

    let (v_off, _s_off) = run(&internal, c, false);
    let (v_on, s_on) = run(&internal, c, true);

    // The correct verdict is Sat (via the F branch). Both flags must agree.
    assert_eq!(
        v_off,
        HyperResult::Sat,
        "control (flag OFF): C is satisfiable via the F branch"
    );
    assert_eq!(
        v_on, v_off,
        "flag ON must NOT flip Sat→Unsat: the ancestor decision d_e must stay \
         in the pruned disjunct's dep-set (superset discipline)"
    );
    // Non-vacuity: the ancestor-dependent prune actually fired in the E branch.
    assert!(
        s_on.semantic_prunes >= 1,
        "the inner disjuncts must be pruned by the ancestor-placed E (non-vacuous), got {}",
        s_on.semantic_prunes
    );
}

/// Survivors-remain backjump canary (hand-built clauses to bypass the
/// clausifier's static disjunct-elimination, which folds small OFN synthetics).
///
/// This guards the SECOND superset hazard (companion to the ancestor-empty
/// canary above): when Layer A prunes SOME but not all disjuncts and the
/// SURVIVORS are then branched and all fail, the propagated Unsat dep-set must
/// still include the pruned disjuncts' clash deps — otherwise it is a subset of
/// flag-OFF's (which branches-and-clashes those disjuncts, folding their deps
/// into `combined`), causing an unsound backjump past an ancestor decision and
/// a false `Unsat`. Discovered as a pizza FP (12 spurious-unsat classes).
///
/// Scenario (Q root): outer `Q → E ∨ F` (decision `d_e`); `Q → Y` (Horn);
/// inner `Y → A ∨ B ∨ D`. In the `E`-branch, `A` is told-disjoint with `E`
/// (dropped, carrying `d_e`); survivors `B`, `D` are branched and each is
/// intrinsically `⊥` (deps carry only the inner decision, NOT `d_e`). Correct
/// verdict: `Sat` (via `F`). If the pruned `A`'s `d_e` is not folded into the
/// branch loop's `combined`, the inner `Unsat` lacks `d_e`, the outer backjumps
/// past `d_e`, skips the `F` branch, and returns a false `Unsat`.
#[test]
fn survivors_remain_prune_dep_prevents_unsound_backjump() {
    // Q=0, E=1, F=2, Y=3, A=4, B=5, D=6
    let c = ClassId::new;
    let clauses = vec![
        // Q → E ∨ F   (outer covering disjunction; declared first ⇒ picked first)
        DlClause {
            body: vec![Atom::Class(c(0), X)],
            head: vec![Atom::Class(c(1), X), Atom::Class(c(2), X)],
        },
        // Q → Y   (Horn: Y onto the root, deps ∅)
        DlClause {
            body: vec![Atom::Class(c(0), X)],
            head: vec![Atom::Class(c(3), X)],
        },
        // Y → A ∨ B ∨ D   (inner covering disjunction)
        DlClause {
            body: vec![Atom::Class(c(3), X)],
            head: vec![
                Atom::Class(c(4), X),
                Atom::Class(c(5), X),
                Atom::Class(c(6), X),
            ],
        },
        // Disjoint(E, A)   ⇒ A pruned in the E-branch
        DlClause {
            body: vec![Atom::Class(c(1), X), Atom::Class(c(4), X)],
            head: vec![],
        },
        // B → ⊥, D → ⊥   (survivors fail intrinsically; deps carry no d_e)
        DlClause {
            body: vec![Atom::Class(c(5), X)],
            head: vec![],
        },
        DlClause {
            body: vec![Atom::Class(c(6), X)],
            head: vec![],
        },
    ];

    let mut off = HyperEngine::new(&clauses, c(0));
    let v_off = off.decide(64);
    let mut on = HyperEngine::new(&clauses, c(0)).with_semantic_branching();
    let v_on = on.decide(64);

    assert_eq!(
        v_off,
        HyperResult::Sat,
        "control (flag OFF): Q is satisfiable via the F branch"
    );
    assert_eq!(
        v_on, v_off,
        "flag ON must NOT flip Sat→Unsat: the pruned disjunct A's d_e dep must \
         seed the branch loop's `combined` so the outer decision is not \
         unsoundly backjumped past"
    );
    assert!(
        on.stats().semantic_prunes >= 1,
        "A must be pruned in the E-branch (non-vacuous), got {}",
        on.stats().semantic_prunes
    );
}

// ───────────────────────── Layer B (exclusion set) ─────────────────────────

/// Layer B mover: excluding a cleanly-refuted sibling collapses a DOWNSTREAM
/// disjunction to a unit-force. `Q → A ∨ B`; `A → ⊥` (A refuted → exclude A);
/// a downstream `Q → A ∨ C` then has `A` pruned-by-exclusion, unit-forcing `C`.
/// Verdict `Sat` (via the B branch, where A is excluded and C forced). Proves
/// Layer B fires (`semantic_exclusions ≥ 1`) and drives a Layer-A unit-force
/// off the exclusion (`semantic_unit_forces ≥ 1`).
#[test]
fn layer_b_exclusion_collapses_downstream_disjunction() {
    // Q=0, A=1, B=2, C=3
    let c = ClassId::new;
    let clauses = vec![
        // Q → A ∨ B
        DlClause {
            body: vec![Atom::Class(c(0), X)],
            head: vec![Atom::Class(c(1), X), Atom::Class(c(2), X)],
        },
        // A → ⊥   (the first sibling is cleanly refuted ⇒ A excluded)
        DlClause {
            body: vec![Atom::Class(c(1), X)],
            head: vec![],
        },
        // Q → A ∨ C   (downstream disjunction; after A is excluded, C is forced)
        DlClause {
            body: vec![Atom::Class(c(0), X)],
            head: vec![Atom::Class(c(1), X), Atom::Class(c(3), X)],
        },
    ];

    let mut off = HyperEngine::new(&clauses, c(0));
    let v_off = off.decide(64);
    let mut on = HyperEngine::new(&clauses, c(0)).with_semantic_branching();
    let v_on = on.decide(64);

    assert_eq!(
        v_off,
        HyperResult::Sat,
        "control (OFF): Q is satisfiable via B"
    );
    assert_eq!(v_on, v_off, "Layer B must preserve the verdict");
    assert!(
        on.stats().semantic_exclusions >= 1,
        "A must be excluded after its clean Unsat (non-vacuous), got {}",
        on.stats().semantic_exclusions
    );
    assert!(
        on.stats().semantic_unit_forces >= 1,
        "the downstream A∨C must collapse to a unit-force of C via the exclusion, got {}",
        on.stats().semantic_unit_forces
    );
}

/// Layer B soundness invariant: a sibling that only STALLS is NEVER excluded
/// (excluding an unproven `¬Dⱼ` is the reuse-trap FP hazard). `Q → A ∨ B` at
/// `decide(1)`: the `A` branch opens `A → G ∨ H` at depth 0 → `Stalled` (not
/// refuted). `B → A` re-derives `A`; if `A` were wrongly excluded on its stall,
/// the `B` branch would clash on the exclusion and the verdict would flip away
/// from the correct one. With the invariant honored, `A` is not excluded, so
/// `B → A` does not clash. Correct verdict: NOT a false `Unsat` (A stalled ⇒ the
/// frame is `Stalled`, i.e. no definite subsumption). Discriminating: injecting
/// "exclude on Stalled too" flips the verdict.
#[test]
fn layer_b_never_excludes_a_stalled_sibling() {
    // Q=0, A=1, B=2, G=3, H=4
    let c = ClassId::new;
    let clauses = vec![
        // Q → A ∨ B
        DlClause {
            body: vec![Atom::Class(c(0), X)],
            head: vec![Atom::Class(c(1), X), Atom::Class(c(2), X)],
        },
        // A → G ∨ H   (opens a disjunction ⇒ at depth 0 the A branch STALLS)
        DlClause {
            body: vec![Atom::Class(c(1), X)],
            head: vec![Atom::Class(c(3), X), Atom::Class(c(4), X)],
        },
        // B → A   (Horn: the B branch re-derives A)
        DlClause {
            body: vec![Atom::Class(c(2), X)],
            head: vec![Atom::Class(c(1), X)],
        },
    ];

    // decide(1): outer Q→A∨B at depth 1; each branch recurses at depth 0, where
    // an open disjunction ⇒ Stalled.
    let mut off = HyperEngine::new(&clauses, c(0));
    let v_off = off.decide(1);
    let mut on = HyperEngine::new(&clauses, c(0)).with_semantic_branching();
    let v_on = on.decide(1);

    // The A branch stalls (A→G∨H at depth 0); the frame is therefore Stalled,
    // NOT a definite Unsat. The invariant (never exclude a stalled sibling)
    // keeps ON from manufacturing a false clash in the B branch.
    assert_eq!(
        v_on, v_off,
        "flag ON must not flip the verdict by excluding a merely-stalled sibling"
    );
    assert_ne!(
        v_on,
        HyperResult::Unsat,
        "must NOT be a false Unsat — A only stalled, so ¬A is unproven"
    );
    // A stalled ⇒ never excluded.
    assert_eq!(
        on.stats().semantic_exclusions,
        0,
        "a stalled sibling must never be excluded, got {}",
        on.stats().semantic_exclusions
    );
}
