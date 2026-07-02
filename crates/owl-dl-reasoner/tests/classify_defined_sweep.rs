//! Canary for the opt-in defined-sup VERIFY sweep (`RUSTDL_CLASSIFY_DEFINED_SWEEP`).
//!
//! For a class `D` defined via a non-EL body (`D ≡ A ⊓ ¬B`), the wedge's label
//! countermodel can be an unreliable counterexample: on a large ontology it may
//! satisfy `cand ⊓ ¬D` only because the wedge is incomplete on complement /
//! disjunction, so the label-heuristic prune drops a TRUE `cand ⊑ D`. The
//! flag makes the defined-sup sweep bypass the label prune for defined sups and
//! verify each candidate with the full tableau (`trust_sat=false`).
//!
//! ## What this file guards (CI, synthetic)
//! - **Soundness (FP=0 under the flag):** the verify path must not add a
//!   spurious `cand ⊑ D` — `Y ⊑ A` but not disjoint from `B` ⟹ `Y ⋢ D`.
//! - **No default-behaviour change:** flag OFF is byte-identical (the verify
//!   branch is gated unreachable); asserted directly.
//! - **Plumbing:** the flag drives the verify path without panic and reports the
//!   genuine `X ⊑ D`.
//!
//! ## What this file does NOT (and cannot) guard synthetically
//! The *recovery* itself — the label prune firing on a TRUE subsumption — is a
//! large-scale wedge-model-incompleteness phenomenon; the wedge is complete on
//! tiny inputs, so a small synthetic finds `X ⊑ D` even flag-OFF. The recovery
//! is measured on the real ORE `ore_ont_15167` (MISSED 42→34, FP=0; see
//! `docs/ore-sweep-2026-07-01.md`) and by the `#[ignore]`d fixture test below.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::Classification;
use std::io::Cursor;
use std::path::Path;

// Serialize env mutation; restore on Drop. Mirrors classify_inverse_domain.rs.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}
impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Serialized via ENV_MUTEX,
        // restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
    #[allow(unsafe_code)]
    fn remove(key: &'static str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: see set.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn has(c: &Classification, sub: &str, sup: &str) -> bool {
    c.is_subclass(sub, sup)
}

// D ≡ A ⊓ ¬B (defined via complement). X ⊑ A and disjoint from B ⟹ X ⊑ D.
// Y ⊑ A only (may be B) ⟹ Y ⋢ D — the FP guard.
const SRC: &str = r"Prefix(:=<http://e#>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D)) Declaration(Class(:X)) Declaration(Class(:Y))
EquivalentClasses(:D ObjectIntersectionOf(:A ObjectComplementOf(:B)))
SubClassOf(:X :A)
DisjointClasses(:X :B)
SubClassOf(:Y :A)
)
";

fn load(src: &str) -> SetOntology<RcStr> {
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(src.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse");
    o
}

#[test]
fn defined_sweep_flag_on_sound_and_plumbed() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_DEFINED_SWEEP", "1");
    let c = owl_dl_reasoner::classify(&load(SRC)).expect("classify");
    // Positive: the verify path reports the genuine X ⊑ D.
    assert!(
        has(&c, "http://e#X", "http://e#D"),
        "flag ON: X ⊑ D (X ⊑ A, X disjoint B ⟹ X ⊑ A ⊓ ¬B)"
    );
    // FP guard (soundness): Y ⊑ A but not disjoint from B ⟹ Y ⋢ D. The verify
    // path must not add this (tableau confirms Y ⊓ ¬D satisfiable).
    assert!(
        !has(&c, "http://e#Y", "http://e#D"),
        "flag ON: Y ⋢ D — verify sweep must not add a spurious defined-sup edge"
    );
}

#[test]
fn defined_sweep_flag_off_default_unchanged() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Default OFF: the verify branch is gated unreachable. Y ⋢ D still holds.
    let _flag = SetEnvGuard::remove("RUSTDL_CLASSIFY_DEFINED_SWEEP");
    let c = owl_dl_reasoner::classify(&load(SRC)).expect("classify");
    assert!(
        !has(&c, "http://e#Y", "http://e#D"),
        "flag OFF: Y ⋢ D (no spurious edge in default path)"
    );
}

/// Recovery isolation on the real ORE `ore_ont_15167` (MONDIAL): flag OFF misses
/// the complement-defined `AdministrativeArea ⊑ EncompassedArea`
/// (`EncompassedArea ≡ LargeArea ⊓ ¬Continent ⊓ ¬Sea`) via the label prune; flag
/// ON recovers it. `#[ignore]`d + skip-if-absent (the ORE corpus is gitignored),
/// mirroring the `konclude_closure_diff.rs` fixture tests. Run locally with the
/// fixture placed at `ontologies/real/ore_ont_15167.owx`:
///   `cargo test -p owl-dl-reasoner --test classify_defined_sweep -- --ignored`
#[test]
#[ignore = "needs ontologies/real/ore_ont_15167.owx (gitignored ORE corpus)"]
fn defined_sweep_recovers_15167_on_real_fixture() {
    let path = Path::new("../../ontologies/real/ore_ont_15167.owx");
    if !path.exists() {
        eprintln!("SKIP: missing {}", path.display());
        return;
    }
    use horned_owl::io::owx::reader::read as read_owx;
    let load_owx = || {
        let f = std::fs::File::open(path).unwrap();
        let (o, _): (SetOntology<RcStr>, _) = read_owx(
            &mut std::io::BufReader::new(f),
            ParserConfiguration::default(),
        )
        .unwrap();
        o
    };
    const SUB: &str = "f://m#AdministrativeArea";
    const SUP: &str = "f://m#EncompassedArea";
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    {
        let _flag = SetEnvGuard::remove("RUSTDL_CLASSIFY_DEFINED_SWEEP");
        let c = owl_dl_reasoner::classify(&load_owx()).expect("classify off");
        assert!(
            !has(&c, SUB, SUP),
            "flag OFF: 15167 misses the label-pruned pair"
        );
    }
    {
        let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_DEFINED_SWEEP", "1");
        let c =
            owl_dl_reasoner::classify_with_timeout(&load_owx(), std::time::Duration::from_secs(60))
                .expect("classify on");
        assert!(
            has(&c, SUB, SUP),
            "flag ON: 15167 recovers AdministrativeArea ⊑ EncompassedArea"
        );
    }
}
