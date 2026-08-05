//! Default-pinning canary for `RUSTDL_DOMAIN_ABSORPTION`, flipped **ON** on
//! 2026-08-05 after the two-arm corpus sweep in
//! `docs/2026-08-04-domain-absorption-default-decision.md`.
//!
//! **This pins all four rows of the default**, which is the part that is easy to
//! get wrong. The house idiom for a default-ON flag is `is_none_or(|v| v != "0")`:
//!
//! | value        | behaviour |
//! |--------------|-----------|
//! | unset        | **ON**    |
//! | `""` (empty) | **ON**    |
//! | `"0"`        | OFF       |
//! | `"1"`        | ON        |
//!
//! The **empty-string row is the one that silently breaks**. This flag previously
//! carried the opt-in idiom `is_some_and(|v| v == "1")`, under which `""` means
//! OFF — and a bare `VAR=` in a shell wrapper is a common accident. A flip that
//! changed only the *unset* row would leave `""` quietly reverting to the old
//! behaviour, which is exactly the kind of half-flip a sweep cannot see.
//!
//! **Known cost of this default, recorded so a future reader does not have to
//! re-derive it** (all with byte-identical output, verified serially at min-of-3):
//! `ore_ont_7011` 5.05 s → 17.53 s (3.5×) and `ore_ont_13545` 5.35 s → 15.47 s
//! (2.9×). `ore_ont_14351` also crosses a 60 s cap (59.96 s → 61.47 s, output
//! unchanged), so at a 60 s budget the sweep's net is +2 completions, not +3.
//! Bought with 3 recoveries (`ore_ont_16372` 60 s → 8.36 s, `6132` → 32.46 s,
//! `9899` → 32.86 s) and **0 answer changes over 1,750 both-arm completers**.

use owl_dl_core::absorb::domain_absorption_enabled;

// Env mutation is process-wide; serialize it and restore on Drop so these tests
// cannot leak a value into each other or into the rest of the suite.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard(Option<std::ffi::OsString>);

impl EnvGuard {
    /// `None` removes the variable; `Some(v)` sets it (including `Some("")`).
    #[allow(unsafe_code)]
    fn set(value: Option<&str>) -> Self {
        let prior = std::env::var_os("RUSTDL_DOMAIN_ABSORPTION");
        // SAFETY: every mutation of this variable in this file happens while
        // ENV_MUTEX is held by the caller, so there is no concurrent access.
        unsafe {
            match value {
                Some(v) => std::env::set_var("RUSTDL_DOMAIN_ABSORPTION", v),
                None => std::env::remove_var("RUSTDL_DOMAIN_ABSORPTION"),
            }
        }
        Self(prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above — ENV_MUTEX is still held by the test body.
        unsafe {
            match self.0.take() {
                Some(v) => std::env::set_var("RUSTDL_DOMAIN_ABSORPTION", v),
                None => std::env::remove_var("RUSTDL_DOMAIN_ABSORPTION"),
            }
        }
    }
}

#[test]
fn unset_means_on() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(None);
    assert!(
        domain_absorption_enabled(),
        "RUSTDL_DOMAIN_ABSORPTION must default ON when unset (flipped 2026-08-05); \
         see docs/2026-08-04-domain-absorption-default-decision.md"
    );
}

#[test]
fn empty_means_on() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some(""));
    assert!(
        domain_absorption_enabled(),
        "an EMPTY value must still mean ON — this is the row the opt-in idiom \
         `is_some_and(|v| v == \"1\")` gets wrong, so a half-completed flip fails here"
    );
}

#[test]
fn zero_reverts() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("0"));
    assert!(
        !domain_absorption_enabled(),
        "=0 must revert to the pre-2026-08-05 behaviour; it is the documented escape hatch"
    );
}

#[test]
fn one_means_on() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("1"));
    assert!(
        domain_absorption_enabled(),
        "=1 must stay ON so the pre-flip opt-in invocation keeps working"
    );
}
