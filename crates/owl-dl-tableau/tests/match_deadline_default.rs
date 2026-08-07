//! Default-pinning canary for `RUSTDL_HYPER_MATCH_DEADLINE`, flipped **ON** in
//! v0.4.16 after matched MISSED-net arms showed ΔMISSED = +0 and FP = 0.
//!
//! **All four rows are pinned**, because the empty-string row is the one a
//! default-ON flip silently gets wrong: the opt-in idiom this flag used to carry
//! (`is_some_and(|v| v == "1")`) treats `""` as OFF, and a bare `VAR=` in a shell
//! wrapper is a common accident. A flip that changed only the *unset* row would
//! leave `""` quietly reverting.
//!
//! | value | behaviour |
//! |---|---|
//! | unset | **ON** |
//! | `""` | **ON** |
//! | `"0"` | OFF |
//! | `"1"` | ON |
//!
//! Why this flag is ON: without it, `--pair-timeout-ms` / `--global-timeout-ms` are
//! silently unenforceable inside the match cross-product — `solve` checked its
//! deadline only at entry. Measured on `ore_ont_16056`, two `classify_labels` calls
//! ran ~17 s each against a 1 ms budget.

use owl_dl_tableau::hyper_match_deadline_enabled;

// Env mutation is process-wide; serialise it and restore on Drop.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard(Option<std::ffi::OsString>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(v: Option<&str>) -> Self {
        let prior = std::env::var_os("RUSTDL_HYPER_MATCH_DEADLINE");
        // SAFETY: every mutation here happens while ENV_MUTEX is held by the caller.
        unsafe {
            match v {
                Some(x) => std::env::set_var("RUSTDL_HYPER_MATCH_DEADLINE", x),
                None => std::env::remove_var("RUSTDL_HYPER_MATCH_DEADLINE"),
            }
        }
        Self(prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above — the lock is still held by the test body.
        unsafe {
            match self.0.take() {
                Some(v) => std::env::set_var("RUSTDL_HYPER_MATCH_DEADLINE", v),
                None => std::env::remove_var("RUSTDL_HYPER_MATCH_DEADLINE"),
            }
        }
    }
}

fn probe(v: Option<&str>) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(v);
    hyper_match_deadline_enabled()
}

#[test]
fn unset_means_on() {
    assert!(
        probe(None),
        "must default ON when unset (flipped in v0.4.16 on matched MISSED-net arms)"
    );
}

#[test]
fn empty_means_on() {
    assert!(
        probe(Some("")),
        "an EMPTY value must still mean ON — the row the opt-in idiom gets wrong"
    );
}

#[test]
fn zero_reverts() {
    assert!(
        !probe(Some("0")),
        "=0 must revert; it is the documented escape hatch"
    );
}

#[test]
fn one_means_on() {
    assert!(
        probe(Some("1")),
        "=1 must stay ON so pre-flip invocations keep working"
    );
}
