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
fn derived_inverse_functional_needs_the_flag() {
    // InverseObjectProperties + Functional(R) => InverseFunctional(S).
    assert!(is_consistent("a-derived-inverse-functional.ofn", false));
    assert!(
        !is_consistent("a-derived-inverse-functional.ofn", true),
        "the flag must derive InverseFunctional(S) from Functional(R) across the pair"
    );
}

#[test]
fn derived_functional_needs_the_flag() {
    // The reverse: InverseObjectProperties + InverseFunctional(R) => Functional(S).
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
    assert!(is_consistent("g-chained-needs-fixpoint.ofn", false));
    assert!(
        !is_consistent("g-chained-needs-fixpoint.ofn", true),
        "a two-link inverse chain must still reach the functional clash"
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
fn functional_data_property_route_needs_edge_materialisation() {
    assert!(is_consistent("ore_ont_4141-7axiom-core.ofn", false));
    assert!(
        !is_consistent("ore_ont_4141-7axiom-core.ofn", true),
        "the 7-axiom core must be decided — deriving the characteristic is not enough, \
         the entailed inverse EDGE has to be materialised so the forward ≤1 rule fires"
    );
}
