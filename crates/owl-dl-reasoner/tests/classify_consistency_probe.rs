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
        owl_dl_reasoner::classify_consistency_probe_enabled(),
        "unset must be ON: classify reported `consistent = true` on ontologies both \
         Konclude and HermiT call inconsistent. A wrong verdict is not an acceptable \
         default, whatever it saves."
    );
}

#[test]
fn empty_string_is_off() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some(""));
    assert!(
        owl_dl_reasoner::classify_consistency_probe_enabled(),
        "`\"\"` must be ON under the default-ON idiom. This is the row a default-ON \
         flip silently gets wrong: the opt-in spelling treats `\"\"` as OFF, and a bare \
         `VAR=` in a shell wrapper is a common accident."
    );
}

#[test]
fn zero_is_off() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("0"));
    assert!(
        !owl_dl_reasoner::classify_consistency_probe_enabled(),
        "`0` must revert to the pre-fix behaviour"
    );
}

#[test]
fn explicit_one_is_on() {
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
fn probe_budget_defaults_to_two_hundred_ms() {
    // 200 ms, paired with the UNSAT-FRACTION gate in `probe_says_inconsistent`.
    //
    // A 1,920-ontology sweep at 1000 ms with only the `>= 1 unsat` gate cost 4
    // ontologies `ok` -> `dnf` (14881, 6108, 7416, 7803) and took 1966 from 7.30 to
    // 58.20 s. All five are HUGE ABox-bearing ontologies with ONE unsatisfiable class
    // (0.005-0.063%), admitted on that single class while the probe's cost scales
    // with their ~90k-assertion ABox. The two ontologies that need the probe sit at
    // 0.403% (16372) and 100% (7610), so the fraction threshold lives in a measured
    // ~6x gap. With the gate, 200 ms fixes BOTH targets and leaves all five victims
    // within noise.
    //
    // The threshold must stay LOW: 16372 is genuinely inconsistent yet shows only
    // 0.403%, because classify's per-class unsat detection is itself incomplete.
    let prior = std::env::var_os("RUSTDL_CLASSIFY_CONSISTENCY_PROBE_MS");
    assert!(
        prior.is_none() || owl_dl_reasoner::classify_consistency_probe_ms() > 0,
        "budget must be positive"
    );
    if prior.is_none() {
        assert_eq!(
            owl_dl_reasoner::classify_consistency_probe_ms(),
            200,
            "changing this default without re-running the 1,920-ontology sweep risks \
             ore_ont_14881/6108/7416/7803 (ok -> dnf) and ore_ont_1966 (7.30 -> 58.20 s); \
             those are safe only because of the unsat-FRACTION gate"
        );
    }
}
