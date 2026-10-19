//! Canaries for `RUSTDL_INVERSE_PAIR_FUNC` (default OFF): deriving functionality
//! across a declared `InverseObjectProperties` pair.
//!
//! Background and full record:
//! `docs/known-limitations/inverse-pair-functionality-not-derived.md`.
//!
//! rustdl honours a **declared** `InverseFunctionalObjectProperty` but derived none
//! from `InverseObjectProperties(R,S) + FunctionalObjectProperty(R)`, so a 5-axiom
//! `ABox` that `Konclude` and `HermiT` both call inconsistent was reported `consistent`.
//!
//! **The two CONTROLS are the point of this file.** `d` (same shape, role asserted
//! directly) and `e` (same shape, characteristic declared) are decided correctly with
//! the flag OFF, which is what proves the gap is *only* the inverse derivation and not
//! the merge or clash machinery. If a future change breaks them, the flag is not the
//! suspect.
//!
//! The fix has **two** parts, and the second is what makes it work:
//!
//! 1. derive the characteristic across the pair (`Functional(R)` ⟹ `InverseFunctional(S)`);
//! 2. **materialise the entailed inverse `ABox` edge** where the partner is functional.
//!
//! Part 1 alone closes only the `DifferentIndividuals` route. It cannot close the
//! functional-data-property route, because the engine does not merge PREDECESSORS —
//! `derive_functional_max_cardinality` is forward-only by design, `∃R⁻.⊤ ⊑ ≤1 R⁻` being
//! a measured no-op. Part 2 sidesteps that by making the forward path applicable.
//!
//! **Scope, stated plainly: the 7-axiom CORE is decided; the full `ore_ont_4141` still
//! times out.** The clash is only reachable on the tableau path (the direct analogue is
//! decided even with both `ABox` pre-checks disabled), and that path does not scale to a
//! 67k-axiom `ABox`. Deciding the full ontology needs the clash in a PRE-CHECK — see the
//! known-limitations doc.
//!
//! **SUPERSEDED IN PART BY #177 (signed role hierarchy, 2026-09-28).** Three of the four
//! routes are now decided NATIVELY, flag OFF: `a`, `g` and the 7-axiom core all report
//! inconsistent at the default, because `Inverse(R,S)` means `S ≡ R⁻` and the signed
//! hierarchy lets `Functional(R)`'s forward `≤1` accept an S-asserted edge as an
//! R⁻-edge — no derived characteristic and no materialised edge needed. Verified against
//! the pinned pre-#177 baseline (`consistent` → `inconsistent` on exactly those three;
//! controls unmoved), and both peers call all three inconsistent, so the new verdicts are
//! correct. Those tests now assert detection at BOTH flag settings. **The flag's sole
//! remaining non-vacuous route is `f`** (`InverseFunctional(R)` ⟹ `Functional(S)`), which
//! is still missed flag-OFF — that test is what keeps `RUSTDL_INVERSE_PAIR_FUNC`
//! load-bearing rather than retired.
//!
//! **RETIRED BY #290 (2026-10-19).** `f` is now decided natively too:
//! `inv_func_merge_consumable` admits `InverseFunctional(R)`'s `≤1 R⁻` bound when an
//! assertion is on any role in `R`'s signed sub-role closure, and `S ≡ R⁻` is in it. As
//! `derived_functional_needs_the_flag` instructed, the flag was retired rather than the
//! test flipped; every test below now checks the DEFAULT.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/inverse_functional_derivation")
        .join(name)
}

/// `true` == consistent, at the default settings.
fn is_consistent(name: &str) -> bool {
    let text = std::fs::read_to_string(fixture(name)).expect("read fixture");
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(text), ParserConfiguration::default()).expect("parse ofn");
    owl_dl_reasoner::is_consistent(&onto).expect("consistency check")
}

#[test]
fn derived_inverse_functional_is_decided_natively_since_177() {
    // InverseObjectProperties + Functional(R): decided at the default since the
    // signed role hierarchy (#177) — an S-edge satisfies an R⁻ body, so the forward ≤1
    // fires without deriving InverseFunctional(S). FLIPPED from "needs the flag";
    // pre-#177 the flag-OFF arm was consistent (the recorded miss).
    assert!(
        !is_consistent("a-derived-inverse-functional.ofn"),
        "the inverse-pair functional clash must be found"
    );
}

#[test]
fn derived_functional_is_decided_natively_since_290() {
    // The reverse: InverseObjectProperties(R, S) + InverseFunctional(R), assertions on S.
    // Missed at the default until #290: the `≤1 R⁻` bound was emitted only for
    // assertions on R itself, and `S ≡ R⁻` is a different role id. The gate now admits
    // any role in R's signed sub-role closure. This was the flag's last route, so the
    // flag was retired (as this test instructed) rather than the test flipped.
    assert!(!is_consistent("f-derived-functional.ofn"));
}

#[test]
fn control_direct_role_is_decided() {
    // No inverse involved — isolates the gap to the derivation.
    assert!(
        !is_consistent("d-direct-functional-CONTROL.ofn"),
        "a directly asserted functional role must clash"
    );
}

#[test]
fn control_declared_characteristic_is_decided() {
    // Semantically equivalent to fixture `a`, but declared rather than derived.
    // This pair (declared decided / derived missed) is the sharpest statement of the bug.
    assert!(
        !is_consistent("e-declared-inverse-functional-CONTROL.ofn"),
        "a DECLARED InverseFunctionalObjectProperty must clash"
    );
}

/// A two-link chain: `r ≡ q⁻ ≡ (p⁻)⁻ ≡ p`, so `Functional(p)` should reach
/// `Functional(r)` via `InverseFunctional(q)`. Guards that multi-link chains work at
/// all, which they do.
///
/// **It does NOT guard the fixpoint loop, despite being written for that.** Replacing
/// the loop with a single pass leaves this green — verified by sabotage, twice,
/// including with the inverse-pair axioms written in deliberately adverse source order.
/// So either one pass suffices here (the loop mutates its sets as it walks them, so a
/// chain can resolve within a pass) or the clash is reached by another route entirely;
/// I did not determine which.
///
/// **Consequence, stated rather than hidden: the fixpoint loop is UNCOVERED, and may
/// even be redundant.** A future reader wanting to simplify it should not take these
/// canaries as protection. Constructing a genuine 2-iteration witness needs a shape
/// where no single pass can close the chain, which I did not find.
#[test]
fn chained_derivation_across_two_inverse_pairs() {
    // FLIPPED by #177: the signed closure composes across BOTH inverse links
    // (r ⊑ q⁻ ⊑ p transitively in the signed graph), so the clash is reached at the
    // default too — the signed transitive closure IS the fixpoint this fixture wanted.
    assert!(
        !is_consistent("g-chained-needs-fixpoint.ofn"),
        "a two-link inverse chain must reach the functional clash"
    );
}

/// The motivating ontology's actual shape, reduced from `ore_ont_4141`'s 67,143 axioms
/// to 7 by Konclude-oracle delta-debugging: the same inverse-induced merge, but the
/// clash arrives via a **functional data property** rather than `DifferentIndividuals`.
///
/// Deriving the characteristic alone did **not** close this — the engine cannot merge
/// PREDECESSORS (`derive_functional_max_cardinality` is forward-only by design, because
/// `∃R⁻.⊤ ⊑ ≤1 R⁻` is a measured no-op). What closes it is Part 2: materialising the
/// entailed inverse edge so the proven *forward* `≤1` path fires. `Konclude` and `HermiT`
/// both call this inconsistent.
#[test]
fn functional_data_property_route_is_decided_natively_since_177() {
    // FLIPPED by #177: the signed hierarchy makes the forward ≤1 path applicable to the
    // inverse-asserted edge directly, which is exactly what Part 2's edge
    // materialisation existed to arrange — so the core is decided at the default too.
    assert!(
        !is_consistent("ore_ont_4141-7axiom-core.ofn"),
        "the 7-axiom core must be decided"
    );
}
