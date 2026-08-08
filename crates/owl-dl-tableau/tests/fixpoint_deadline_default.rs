//! Canaries for `RUSTDL_FIXPOINT_DEADLINE` — the wall-clock bound on
//! `horn_fixpoint`'s drain loop (2026-08-08).
//!
//! **The defect.** The drain loop was bounded by a `max_iters` STEP count and
//! never consulted the clock, so a fixpoint whose events are individually
//! expensive overran its time budget without limit. Measured on `ore_ont_6134`
//! at `--pair-timeout-ms 50`: **19,906 pairs cost 100–999 ms** and 26 cost
//! ≥1000 ms — a 2–20× overshoot. A stride sweep of the sibling
//! `enumerate_matches` probe (4096 / 256 / 64) left that bucket flat while
//! demonstrably firing (the ≥1000 ms tail fell 82 → 17), which is what excluded
//! the match cross-product and localised the cause here.
//!
//! **Effect of the fix**, same ontology, arms differing only in the flag:
//!
//! | | rows | timed-out pairs | 100–999 ms | ≥1000 ms |
//! |---|---|---|---|---|
//! | OFF | 2,358 | 21,573 | 16,269 | 354 |
//! | ON | **2,387** | **66,752** | **0** | **0** |
//!
//! The budget becomes real (everything lands ≤99 ms), 3.1× more pairs are
//! decided in the same wall budget, and **completeness increases** by 29 rows —
//! pairs that previously timed out into `not-subsumed` are now actually proven.
//!
//! **Why this is sound, and why the argument is unusually strong.** The fix
//! returns the SAME `HyperResult::Stalled` that the `max_iters` branch two lines
//! above already returns, so every caller's handling is pre-existing and
//! exercised. A clock-truncated fixpoint is indistinguishable, to callers, from a
//! step-truncated one. `Stalled` is never `Sat`, so truncating can only MISS a
//! subsumption, never manufacture one — no new verdict, no new soundness surface.
//!
//! **All four env rows are pinned.** The empty-string row is the one a future
//! default-ON flip gets wrong: the opt-in idiom used here
//! (`is_some_and(|v| v == "1")`) treats `""` as OFF, and a bare `VAR=` in a shell
//! wrapper is a common accident.
//!
//! | value | behaviour |
//! |---|---|
//! | unset | OFF |
//! | `""` | OFF |
//! | `"0"` | OFF |
//! | `"1"` | **ON** |
//!
//! Default is **OFF** pending the two gates a flip requires here: a corpus-scale
//! ΔMISSED arm (this trades a step bound for a time bound, so a fixpoint that
//! would have completed just over budget now returns `Stalled` — a MISS) and a
//! full-corpus two-arm sweep (whose frame, unlike the MISSED net's, can observe
//! an `ok → dnf`).

use owl_dl_tableau::hyper_fixpoint_deadline_enabled;

// Env mutation is process-wide; serialise it and restore on Drop.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard(Option<std::ffi::OsString>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(v: Option<&str>) -> Self {
        let prior = std::env::var_os("RUSTDL_FIXPOINT_DEADLINE");
        // SAFETY: every mutation happens while ENV_MUTEX is held by the caller.
        unsafe {
            match v {
                Some(x) => std::env::set_var("RUSTDL_FIXPOINT_DEADLINE", x),
                None => std::env::remove_var("RUSTDL_FIXPOINT_DEADLINE"),
            }
        }
        Self(prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above — the caller still holds ENV_MUTEX.
        unsafe {
            match &self.0 {
                Some(v) => std::env::set_var("RUSTDL_FIXPOINT_DEADLINE", v),
                None => std::env::remove_var("RUSTDL_FIXPOINT_DEADLINE"),
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
        !hyper_fixpoint_deadline_enabled(),
        "unset must be OFF — this flag is opt-in until both flip gates are run"
    );
}

#[test]
fn empty_string_is_off() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some(""));
    assert!(
        !hyper_fixpoint_deadline_enabled(),
        "`\"\"` must be OFF under the opt-in idiom. A bare `VAR=` in a shell \
         wrapper is a common accident, and this is the row a future default-ON \
         flip silently gets wrong."
    );
}

#[test]
fn zero_is_off() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("0"));
    assert!(!hyper_fixpoint_deadline_enabled(), "`0` must be OFF");
}

#[test]
fn one_is_on() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("1"));
    assert!(
        hyper_fixpoint_deadline_enabled(),
        "`1` must be ON — without this the whole lever is unreachable and every \
         measurement attributing an effect to it would be measuring nothing"
    );
}
