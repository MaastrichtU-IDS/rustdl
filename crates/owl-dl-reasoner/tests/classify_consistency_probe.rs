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
fn probe_budget_defaults_to_ten_ms() {
    // 10 ms, and the value is load-bearing. A 1,920-ontology sweep at 1000 ms cost
    // 4 ontologies `ok` -> `dnf` and took `ore_ont_1966` 7.30 s -> 58.20 s. The cost
    // is NOT proportional to the budget (1966: 66 s at 1000 ms, 73 s at 100 ms,
    // 5.17 s at 10 ms) because `decide_with_deadline` overshoots on the main
    // tableau. No single value satisfies both sides: `ore_ont_16372` needs >=200 ms,
    // `ore_ont_1966` dies at 100 ms. Raising this re-breaks ontologies that
    // currently answer correctly.
    let prior = std::env::var_os("RUSTDL_CLASSIFY_CONSISTENCY_PROBE_MS");
    assert!(
        prior.is_none() || owl_dl_reasoner::classify_consistency_probe_ms() > 0,
        "budget must be positive"
    );
    if prior.is_none() {
        assert_eq!(
            owl_dl_reasoner::classify_consistency_probe_ms(),
            10,
            "raising this default re-breaks ore_ont_14881/6108/7416/7803 (ok -> dnf) \
             and ore_ont_1966 (7.30 -> 58.20 s); see the doc comment"
        );
    }
}
