//! Task 3: wire the pseudo-model witness shortcut into `realize` behind
//! `RUSTDL_PSEUDO_MODEL` (default OFF).
//!
//! Fixture is deliberately OFF the saturation fast path
//! (`realize_saturation_eligible`): the `ABox` (`ClassAssertion`) alone
//! already excludes the no-`ABox` `is_pure_el`/`saturator_complete_fragment`
//! arms, and the `EquivalentClasses(:Nom ObjectOneOf(:a))` axiom makes
//! `ontology_uses_nominals` true, which excludes the `ABox`-admitting
//! `tbox_only_saturator_eligible` (Lever 1) arm too — so `realize` on this
//! fixture always runs `realize_tableau_internal`, the per-(individual,class)
//! `{a} ⊓ ¬C` probe loop this task's shortcut prunes.
//!
//! `a` is asserted `:A`; `:A ⊑ :C` so `:C` is entailed; `:A` and `:B` are
//! `DisjointClasses`, so `a` is provably NOT a `:B`.
//!
//! NEGATIVES-FIRST + verdict-identity: the flag must never change what
//! `realize` reports (it is a subtractive, sound-under-the-invariant prune),
//! so every test below compares ON vs OFF (or exercises ON directly against
//! a known-correct verdict) rather than asserting the prune fires by side
//! channel.
//!
//! Run: `cargo test -p owl-dl-reasoner --test pseudo_model_realize`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::realize;
use std::io::Cursor;

// ─── env-mutation plumbing (serialized; restored on Drop) ──────────────
//
// Mirrors the pattern in `tests/inverse_symmetric_domain.rs`: env vars are
// process-global, so tests that set/unset `RUSTDL_PSEUDO_MODEL` /
// `RUSTDL_WEDGE_CONSISTENCY` must serialize against each other (and against
// any other test binary in this crate that touches the same vars — there
// are none today, but the mutex + restore-on-Drop pattern is cheap
// insurance) to avoid a flaky env race.

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. The mutation is held
        // only for the duration of one test (serialized via `ENV_MUTEX`) and
        // restored on `Drop`.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `SetEnvGuard::set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src);
    let (ontology, _prefixes) =
        read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
    ontology
}

/// Off-fragment nominal `ABox`: see module doc for why this stays off the
/// saturation fast path. `a : A`; `A ⊑ C`; `A`/`B` `DisjointClasses`.
fn fixture() -> SetOntology<RcStr> {
    parse(&format!(
        "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:B)) Declaration(Class(:Nom))\n\
    Declaration(NamedIndividual(:a))\n\
    SubClassOf(:A :C)\n\
    DisjointClasses(:A :B)\n\
    EquivalentClasses(:Nom ObjectOneOf(:a))\n\
    ClassAssertion(:A :a)\n\
)\n"
    ))
}

const A: &str = "http://rustdl.test/a";
const C: &str = "http://rustdl.test/C";
const B: &str = "http://rustdl.test/B";

/// RED (Step 1): with `RUSTDL_PSEUDO_MODEL=1`, `realize` on the off-fragment
/// fixture still gives the correct, complete verdict for `a` — includes the
/// derived `C` (via `A ⊑ C`), excludes the disjoint `B`. Before Task 3's
/// implementation this test fails to compile (no `RUSTDL_PSEUDO_MODEL` /
/// `base_types` plumbing exists yet); after GREEN it exercises the shortcut
/// end-to-end via the public `realize` entry point.
#[test]
fn pseudo_model_on_gives_correct_types() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_PSEUDO_MODEL", "1");

    let onto = fixture();
    let r = realize(&onto).expect("realization");
    let entailed = r.entailed_types(A);
    assert!(
        entailed.iter().any(|c| c == C),
        "a must be entailed C via A⊑C; got {entailed:?}",
    );
    assert!(
        !entailed.iter().any(|c| c == B),
        "a must NOT be entailed B (DisjointClasses(A,B)); got {entailed:?}",
    );
}

/// Verdict-identity (Step 3): flipping `RUSTDL_PSEUDO_MODEL` must never
/// change what `realize` reports — the prune is subtractive and placed
/// after the told-closure fast path, so it can only skip probes whose
/// answer is already determined, never alter one. Compares full
/// `entailed_types`/`most_specific_types` for every individual, in order.
#[test]
fn pseudo_model_on_matches_off_exactly() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let onto = fixture();

    let off = {
        let _flag = SetEnvGuard::set("RUSTDL_PSEUDO_MODEL", "0");
        realize(&onto).expect("realization (flag off)")
    };
    let on = {
        let _flag = SetEnvGuard::set("RUSTDL_PSEUDO_MODEL", "1");
        realize(&onto).expect("realization (flag on)")
    };

    assert_eq!(
        off.individuals(),
        on.individuals(),
        "individual set must match"
    );
    for iri in off.individuals() {
        assert_eq!(
            off.entailed_types(iri),
            on.entailed_types(iri),
            "entailed_types diverged for {iri}",
        );
        assert_eq!(
            off.most_specific_types(iri),
            on.most_specific_types(iri),
            "most_specific_types diverged for {iri}",
        );
    }
}

/// None-witness fallback (Step 3): `realize_base_model_types` returns `None`
/// when the wedge consistency cache is unavailable
/// (`RUSTDL_WEDGE_CONSISTENCY=0`, per `pseudo_model_enabled`'s doc coupling
/// note) — every pair then takes the normal path, so results must be
/// unchanged whether the flag is on or off. Uses the same benign,
/// off-fragment `ABox` (an inconsistent one would `Err(Inconsistent)` before
/// reaching the per-pair loop either way, so it wouldn't isolate this case).
#[test]
fn pseudo_model_on_with_no_witness_matches_off() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _consistency = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");

    let onto = fixture();

    let off = {
        let _flag = SetEnvGuard::set("RUSTDL_PSEUDO_MODEL", "0");
        realize(&onto).expect("realization (flag off)")
    };
    let on = {
        let _flag = SetEnvGuard::set("RUSTDL_PSEUDO_MODEL", "1");
        realize(&onto).expect("realization (flag on, but no witness available)")
    };

    assert_eq!(off.individuals(), on.individuals());
    for iri in off.individuals() {
        assert_eq!(off.entailed_types(iri), on.entailed_types(iri));
        assert_eq!(off.most_specific_types(iri), on.most_specific_types(iri));
    }
}
