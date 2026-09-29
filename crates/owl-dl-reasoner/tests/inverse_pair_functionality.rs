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

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::path::{Path, PathBuf};

// Env mutation is process-wide; serialise it and restore on Drop so a value cannot
// leak between tests or into the rest of the suite.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard(Option<std::ffi::OsString>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(on: bool) -> Self {
        let prior = std::env::var_os("RUSTDL_INVERSE_PAIR_FUNC");
        // SAFETY: every mutation here happens while ENV_MUTEX is held by the caller.
        unsafe { std::env::set_var("RUSTDL_INVERSE_PAIR_FUNC", if on { "1" } else { "0" }) };
        Self(prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above — the lock is still held by the test body.
        unsafe {
            match self.0.take() {
                Some(v) => std::env::set_var("RUSTDL_INVERSE_PAIR_FUNC", v),
                None => std::env::remove_var("RUSTDL_INVERSE_PAIR_FUNC"),
            }
        }
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/inverse_functional_derivation")
        .join(name)
}

/// `true` == consistent. The flag is set EXPLICITLY in both arms so the ambient
/// environment cannot decide the result — otherwise a developer with the flag
/// exported would see these tests pass vacuously.
fn is_consistent(name: &str, flag_on: bool) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(flag_on);
    let text = std::fs::read_to_string(fixture(name)).expect("read fixture");
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(text), ParserConfiguration::default()).expect("parse ofn");
    owl_dl_reasoner::is_consistent(&onto).expect("consistency check")
}

#[test]
fn derived_inverse_functional_is_decided_natively_since_177() {
    // InverseObjectProperties + Functional(R): decided at BOTH flag settings since the
    // signed role hierarchy (#177) — an S-edge satisfies an R⁻ body, so the forward ≤1
    // fires without deriving InverseFunctional(S). FLIPPED from "needs the flag";
    // pre-#177 the flag-OFF arm was consistent (the recorded miss).
    for flag in [false, true] {
        assert!(
            !is_consistent("a-derived-inverse-functional.ofn", flag),
            "the inverse-pair functional clash must be found regardless of the flag"
        );
    }
}

#[test]
fn derived_functional_needs_the_flag() {
    // The reverse: InverseObjectProperties + InverseFunctional(R) => Functional(S).
    // STILL missed flag-OFF after #177 (measured) — the ≤1 here sits on R⁻'s side and
    // the engine does not merge predecessors, so signed edge-matching alone cannot
    // reach it. This test is now the flag's ONLY non-vacuous route: if it starts
    // failing on its first assertion, the flag has gone redundant — retire it rather
    // than flipping this test.
    assert!(is_consistent("f-derived-functional.ofn", false));
    assert!(
        !is_consistent("f-derived-functional.ofn", true),
        "the derivation must run in BOTH directions, not just Functional -> InverseFunctional"
    );
}

#[test]
fn control_direct_role_is_decided_without_the_flag() {
    // No inverse involved — isolates the gap to the derivation.
    for flag in [false, true] {
        assert!(
            !is_consistent("d-direct-functional-CONTROL.ofn", flag),
            "a directly asserted functional role must clash regardless of the flag"
        );
    }
}

#[test]
fn control_declared_characteristic_is_decided_without_the_flag() {
    // Semantically equivalent to fixture `a`, but declared rather than derived.
    // This pair (declared decided / derived missed) is the sharpest statement of the bug.
    for flag in [false, true] {
        assert!(
            !is_consistent("e-declared-inverse-functional-CONTROL.ofn", flag),
            "a DECLARED InverseFunctionalObjectProperty must clash regardless of the flag"
        );
    }
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
    for flag in [false, true] {
        assert!(
            !is_consistent("g-chained-needs-fixpoint.ofn", flag),
            "a two-link inverse chain must reach the functional clash regardless of the flag"
        );
    }
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
    for flag in [false, true] {
        assert!(
            !is_consistent("ore_ont_4141-7axiom-core.ofn", flag),
            "the 7-axiom core must be decided regardless of the flag"
        );
    }
}
