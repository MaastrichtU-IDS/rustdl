//! Canaries for `RUSTDL_CLASSIFY_CONSISTENCY_PROBE` — a gated wedge-consistency
//! probe on the classify path (2026-08-08).
//!
//! **The gap it closes.** `classify`'s inconsistency detection never consults the
//! wedge-consistency route `is_consistent` uses. Measured over all 1,920 ORE
//! ontologies: `is_consistent` finds **43** inconsistent; classify agrees on 41
//! and reports `consistent = true` on **2** (`ore_ont_16372`, `ore_ont_7610`).
//! Those are WRONG ANSWERS. The probe fixes `ore_ont_7610`; `ore_ont_16372` is
//! **not** fixed, because its wedge returns `Stalled` and its detection happens in
//! the bounded main-tableau fall-through, which this probe does not reach.
//!
//! **Why the gate, and why it is not merely an optimisation.** Running a
//! consistency check unconditionally on the classify path costs a mean of
//! **5.1 s** on consistent ontologies (60 sampled; 16 over 1 s, max 30 s) — the
//! dead-end already recorded in the design notes. The gate:
//!
//! > An inconsistent KB makes `⊤` unsatisfiable, hence EVERY class unsatisfiable.
//! > Contrapositive: **zero unsatisfiable classes ⟹ consistent.**
//!
//! Measured, that admits **1 of 60** sampled ontologies (~1.6%).
//!
//! **Direction of risk.** The probe can only turn `consistent = true` into
//! `false`, so its failure mode is a false INCONSISTENCY. It is bounded by the
//! same justification `is_consistent` already relies on: a wedge `Unsat` is a real
//! clause clash. A 40-ontology consistent sample showed **0 flips**.

use std::sync::Mutex;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

struct EnvGuard(Option<std::ffi::OsString>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(v: Option<&str>) -> Self {
        let prior = std::env::var_os("RUSTDL_CLASSIFY_CONSISTENCY_PROBE");
        // SAFETY: mutation happens while ENV_MUTEX is held by the caller.
        unsafe {
            match v {
                Some(x) => std::env::set_var("RUSTDL_CLASSIFY_CONSISTENCY_PROBE", x),
                None => std::env::remove_var("RUSTDL_CLASSIFY_CONSISTENCY_PROBE"),
            }
        }
        Self(prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above.
        unsafe {
            match &self.0 {
                Some(v) => std::env::set_var("RUSTDL_CLASSIFY_CONSISTENCY_PROBE", v),
                None => std::env::remove_var("RUSTDL_CLASSIFY_CONSISTENCY_PROBE"),
            }
        }
    }
}

#[test]
fn unset_is_off() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(None);
    assert!(
        !owl_dl_reasoner::classify_consistency_probe_enabled(),
        "unset must be OFF — opt-in pending a corpus sweep"
    );
}

#[test]
fn empty_string_is_off() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some(""));
    assert!(
        !owl_dl_reasoner::classify_consistency_probe_enabled(),
        "`\"\"` must be OFF: a bare `VAR=` in a shell wrapper is a common accident, \
         and this is the row a future default-ON flip silently gets wrong"
    );
}

#[test]
fn zero_is_off() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("0"));
    assert!(!owl_dl_reasoner::classify_consistency_probe_enabled());
}

#[test]
fn one_is_on() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("1"));
    assert!(
        owl_dl_reasoner::classify_consistency_probe_enabled(),
        "`1` must enable — otherwise the lever is unreachable and any measurement \
         attributing an effect to it is measuring nothing"
    );
}

#[test]
fn probe_budget_defaults_to_one_second() {
    // The budget bounds the gated probe. Both known targets resolve in <0.4 s, so
    // 1000 ms is generous; the value matters because the probe runs on ~1.6% of
    // ontologies and a large budget there is the whole cost of the feature.
    let prior = std::env::var_os("RUSTDL_CLASSIFY_CONSISTENCY_PROBE_MS");
    assert!(
        prior.is_none() || owl_dl_reasoner::classify_consistency_probe_ms() > 0,
        "budget must be positive"
    );
    if prior.is_none() {
        assert_eq!(owl_dl_reasoner::classify_consistency_probe_ms(), 1000);
    }
}
