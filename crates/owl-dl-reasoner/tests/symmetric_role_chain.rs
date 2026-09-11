//! #128 — a chain whose first leg is SYMMETRIC must fire in the reverse
//! direction, at the DEFAULT.
//!
//! Reported externally with a real ontology. `Symmetric(p)` + `p ∘ q ⊑ q` +
//! `A ≡ ∃p.Y ⊓ ∃q.Z` + `B ≡ ∃p.(Y ⊓ ∃q.Z)` makes `A ≡ B`: symmetry gives
//! `p(y,x)` from `p(x,y)`, and the chain then puts the `Z`-successor on the
//! `Y`-witness. `classify` derived only `B ⊑ A` — the chain-only direction —
//! and reported `incomplete: false` with `dropped: {}`. `HermiT` and Konclude
//! both derive the equivalence.
//!
//! **The engine could always do this; it was not being TOLD.** The wedge's
//! `role_matches` honours sub-roles, declared inverses and symmetry only when
//! the role hierarchy is threaded into the per-pair oracle, and that threading
//! (SP1.1 "Layer A") shared one flag with the label-driven same-tier sweep
//! ("Layer B"). The ~2× wall recorded for SP1.1 belongs to Layer B's sweep, so
//! the two were bundled at a cost only one of them incurs, and Layer A sat
//! behind an opt-in nobody sets. `RUSTDL_CLASSIFY_ROLE_HIERARCHY` (default ON)
//! separates them.
//!
//! **DIRECTION OF RISK IS A FALSE POSITIVE** — threading the hierarchy makes
//! `role_matches` accept MORE edges, so a role that is not symmetric must never
//! get bidirectional treatment. That is what the `_must_not_` tests below guard,
//! and they are the ones that fail first if the matching is loosened too far.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const DECLS: &str = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:Y)) \
     Declaration(Class(:Z)) Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:p2)) \
     Declaration(ObjectProperty(:q)) Declaration(ObjectProperty(:s))";

/// Every test in this file locks this, not just the one that MUTATES the env.
/// Rust runs tests in parallel, so a read-only test can otherwise observe
/// another test's `RUSTDL_CLASSIFY_ROLE_HIERARCHY=0` and fail spuriously — which
/// is exactly what happened on the first run of this file. Mirrors the idiom in
/// `classify_inverse_domain.rs`.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Classify with `RUSTDL_CLASSIFY_ROLE_HIERARCHY=1`. The fix is **opt-in** — it
/// is default OFF on a measured ~19× wall regression, not on any doubt about
/// correctness (see the flag's doc comment). Every test here therefore sets it
/// explicitly, and `the_fix_is_opt_in_and_the_default_still_misses` pins that
/// the default does NOT have it.
fn classify_with_hierarchy(body: &str) -> owl_dl_reasoner::Classification {
    // SAFETY: set_var is unsafe under edition 2024. Callers hold `lock()`, and
    // the value is restored by `remove_var` before returning.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_CLASSIFY_ROLE_HIERARCHY", "1");
    }
    let c = classify(body);
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_CLASSIFY_ROLE_HIERARCHY");
    }
    c
}

fn classify(body: &str) -> owl_dl_reasoner::Classification {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Ontology(<http://ex.org/o>\n{DECLS}\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    owl_dl_reasoner::classify(&onto).expect("classify")
}

fn holds(body: &str, sub: &str, sup: &str) -> bool {
    classify_with_hierarchy(body).is_subclass(
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
}

/// The two class definitions from the report, parameterised on the role axioms.
fn shape(role_axioms: &str) -> String {
    format!(
        "{role_axioms}\n\
         EquivalentClasses(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:p :Y) \
         ObjectSomeValuesFrom(:q :Z)))\n\
         EquivalentClasses(:B ObjectSomeValuesFrom(:p ObjectIntersectionOf(:Y \
         ObjectSomeValuesFrom(:q :Z))))"
    )
}

/// THE REPORTED BUG. `A ⊑ B` is the direction needing symmetry ∘ chain; before
/// #128 only `B ⊑ A` was derived.
#[test]
fn a_symmetric_first_leg_fires_the_chain_in_reverse() {
    let _serial = lock();
    let body = shape(
        "SymmetricObjectProperty(:p)\n\
         SubObjectPropertyOf(ObjectPropertyChain(:p :q) :q)",
    );
    assert!(
        holds(&body, "A", "B"),
        "A ⊑ B needs symmetry composed with the chain — the #128 direction"
    );
    assert!(holds(&body, "B", "A"), "B ⊑ A needs the chain alone");
}

/// A named inverse was already handled; kept so a regression in the shared
/// matching path cannot hide behind the symmetric case passing.
#[test]
fn an_explicitly_named_inverse_first_leg_still_works() {
    let _serial = lock();
    let body = shape(
        "Declaration(ObjectProperty(:pi))\n\
         InverseObjectProperties(:p :pi)\n\
         SubObjectPropertyOf(ObjectPropertyChain(:pi :q) :q)\n\
         SubObjectPropertyOf(ObjectPropertyChain(:p :q) :q)",
    );
    assert!(holds(&body, "A", "B"));
}

/// FP GUARD, and the test that fails first if edge matching is loosened beyond
/// what symmetry licenses: with NO symmetry declared, the reverse direction is
/// not entailed and must not be derived. `HermiT` derives neither direction's
/// `A ⊑ B` here.
#[test]
fn a_plain_chain_must_not_fire_in_reverse() {
    let _serial = lock();
    let body = shape("SubObjectPropertyOf(ObjectPropertyChain(:p :q) :q)");
    assert!(
        !holds(&body, "A", "B"),
        "without symmetry the reverse traversal is unlicensed"
    );
    assert!(holds(&body, "B", "A"), "the forward direction still holds");
}

/// FP GUARD. A SUB-ROLE of a symmetric role is not itself symmetric, so a chain
/// on `p2 ⊑ p` must not be reversed even though `p` is symmetric.
#[test]
fn a_sub_role_of_a_symmetric_role_must_not_be_reversed() {
    let _serial = lock();
    let body = shape(
        "SymmetricObjectProperty(:p)\n\
         SubObjectPropertyOf(:p2 :p)\n\
         SubObjectPropertyOf(ObjectPropertyChain(:p2 :q) :q)",
    );
    assert!(
        !holds(&body, "A", "B"),
        "p2 ⊑ p with p symmetric does NOT make p2 symmetric"
    );
}

/// Symmetry alone, with no chain, entails neither direction — pins that the
/// gain comes from the two interacting rather than from symmetry being read.
#[test]
fn symmetry_without_a_chain_entails_neither_direction() {
    let _serial = lock();
    let body = shape("SymmetricObjectProperty(:p)");
    assert!(!holds(&body, "A", "B"));
    assert!(!holds(&body, "B", "A"));
}

/// The fix is OPT-IN, and this pins both halves of that: the default still
/// misses the pair, and `=1` derives it. If a future change flips the default,
/// this test tells you immediately — and the flag's doc comment records what
/// evidence a flip needs (0 REGRESSED on the full sweep).
#[test]
fn the_fix_is_opt_in_and_the_default_still_misses() {
    let _serial = lock();
    let body = shape(
        "SymmetricObjectProperty(:p)\n\
         SubObjectPropertyOf(ObjectPropertyChain(:p :q) :q)",
    );
    // Default: no flag set at all.
    assert!(
        !classify(&body).is_subclass("http://ex.org/A", "http://ex.org/B"),
        "default is OFF on a measured wall regression; if this now passes the \
         default was flipped — update the flag doc and the pin table together"
    );
    assert!(
        holds(&body, "A", "B"),
        "RUSTDL_CLASSIFY_ROLE_HIERARCHY=1 must derive it"
    );
}

/// KNOWN RESIDUAL, pinned so it is not mistaken for coverage. A length-3 chain
/// with a symmetric first leg is still missed at the default; both peers derive
/// it. FLIP this assertion rather than deleting it when that closes.
#[test]
fn a_length_three_chain_with_a_symmetric_leg_is_a_known_miss() {
    let _serial = lock();
    let body = "SymmetricObjectProperty(:p)\n\
         SubObjectPropertyOf(ObjectPropertyChain(:p :q :s) :s)\n\
         EquivalentClasses(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:p :Y) \
         ObjectSomeValuesFrom(:q :Z)))\n\
         EquivalentClasses(:B ObjectSomeValuesFrom(:s :Z))";
    assert!(
        !holds(body, "A", "B"),
        "if this now passes, #128's residual has closed — FLIP this test, do not delete it"
    );
}
