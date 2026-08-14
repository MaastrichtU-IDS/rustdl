//! `RUSTDL_UNSAT_PROBE_MS` — a per-class cap on the unsat probe.
//!
//! **This flag is a measured NEGATIVE RESULT and defaults OFF.** It rescues no
//! ontology. See `unsat_probe_cap`'s doc comment for the numbers. These tests exist
//! for two reasons, and the second is the important one:
//!
//! 1. **Default inertness** — unset (and an explicit `0`) must match no cap at all.
//! 2. **Soundness direction** — a shorter probe can only LOSE an unsat class, never
//!    gain one, and a satisfiable class is never reported unsat at any cap.
//!
//! **WHAT THESE TESTS DO NOT DO: they do not prove the cap FIRES.** Verified by
//! sabotage — replacing `unsat_probe_cap()` with `None`, i.e. disabling the flag
//! entirely, leaves all four GREEN. A millisecond-scale fixture cannot distinguish a
//! 1 ms cap from no cap, and a fixture that could would be timing-flaky.
//!
//! The firing evidence is therefore the CORPUS MEASUREMENT, recorded in
//! `unsat_probe_cap`'s doc comment: on `ore_ont_934` at the default pair budget the
//! cap takes `unsat_probe` from 73,807 ms to 556 ms and `tier_walk` from 0 ms to
//! 73,309 ms. That is what makes "and it still rescued nothing" a finding rather
//! than a possible no-op — and it is not something this test file establishes.
//!
//! Stated explicitly because this arc has one experiment on record
//! (`concrete_domain_clash`, 2026-08-13) whose null was uninterpretable precisely
//! for want of that distinction. A green suite here is not a working mechanism.

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

const KEY: &str = "RUSTDL_UNSAT_PROBE_MS";

/// Env guard following the house pattern in `adaptive_budget.rs`: serialized via
/// `ENV_MUTEX`, restored on `Drop`.
struct EnvGuard(Option<std::ffi::OsString>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(v: Option<&str>) -> Self {
        let prior = std::env::var_os(KEY);
        // SAFETY: set_var/remove_var are unsafe under edition 2024. Held for one
        // test only, serialized via ENV_MUTEX, restored on Drop.
        unsafe {
            match v {
                Some(x) => std::env::set_var(KEY, x),
                None => std::env::remove_var(KEY),
            }
        }
        Self(prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see EnvGuard::set.
        unsafe {
            match &self.0 {
                Some(v) => std::env::set_var(KEY, v),
                None => std::env::remove_var(KEY),
            }
        }
    }
}

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("fixture parses")
        .0
}

/// An ontology with a genuinely unsatisfiable class, reachable by the probe.
const FIXTURE: &str = r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:Bad))
    Declaration(Class(:Ok))
    DisjointClasses(:A :B)
    SubClassOf(:Bad :A)
    SubClassOf(:Bad :B)
    SubClassOf(:Ok :A)
)
";

fn unsat_of(src: &str) -> Vec<String> {
    let h = owl_dl_reasoner::classify(&parse(src)).expect("classify");
    let mut v: Vec<String> = h
        .unsatisfiable_classes()
        .into_iter()
        .map(str::to_owned)
        .collect();
    v.sort();
    v
}

/// Unset ⇒ inert. The classification must match, exactly, a run with the variable
/// absent — this is the property that lets the flag ship without a corpus sweep.
#[test]
fn unset_is_inert() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let base = {
        let _g = EnvGuard::set(None);
        unsat_of(FIXTURE)
    };
    // An explicit 0 is also "no cap" (the `filter(|ms| *ms > 0)` arm).
    let zero = {
        let _g = EnvGuard::set(Some("0"));
        unsat_of(FIXTURE)
    };
    assert_eq!(base, zero, "0 must mean no cap, matching unset");
    assert!(
        base.iter().any(|c| c.ends_with("Bad")),
        ":Bad is unsatisfiable and must be found with no cap; got {base:?}"
    );
}

/// A GENEROUS cap must not change the answer: it is larger than this tiny probe
/// needs, so the clash is still found. Pins that the cap is a `min()` with the pair
/// budget rather than a replacement that could accidentally shorten every probe.
#[test]
fn generous_cap_preserves_the_unsat_verdict() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("60000"));
    let v = unsat_of(FIXTURE);
    assert!(
        v.iter().any(|c| c.ends_with("Bad")),
        "a 60 s cap cannot lose a clash a millisecond-scale probe finds; got {v:?}"
    );
}

/// SOUNDNESS DIRECTION, not a fires-check: a shorter probe cannot prove more, so a
/// capped run's unsatisfiable set must be a SUBSET of the uncapped run's. Gaining an
/// unsat class under a cap would be a genuine soundness violation.
///
/// Note this holds trivially when the cap does nothing, so it passes under sabotage
/// — see the module header. It is worth keeping anyway: it is the assertion that
/// would fail if someone later made the cap alter which classes are probed rather
/// than only how long each probe runs.
#[test]
fn tiny_cap_can_only_lose_unsat_never_gain() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let full = {
        let _g = EnvGuard::set(None);
        unsat_of(FIXTURE)
    };
    let capped = {
        let _g = EnvGuard::set(Some("1"));
        unsat_of(FIXTURE)
    };
    for c in &capped {
        assert!(
            full.contains(c),
            "a 1 ms cap reported {c} as unsatisfiable when the uncapped run did not — \
             a shorter probe cannot prove more, so this is a soundness violation. \
             capped={capped:?} full={full:?}"
        );
    }
    assert!(
        capped.len() <= full.len(),
        "capping can only lose unsat classes: capped={capped:?} full={full:?}"
    );
}

/// A satisfiable class must never be reported unsatisfiable at any cap. The failure
/// direction of this flag is a MISS; an FP here would be the serious defect.
#[test]
fn satisfiable_class_is_never_reported_unsat() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for cap in [None, Some("1"), Some("5"), Some("60000")] {
        let _g = EnvGuard::set(cap);
        let v = unsat_of(FIXTURE);
        assert!(
            !v.iter().any(|c| c.ends_with("Ok")),
            ":Ok is satisfiable and must never be reported unsat (cap={cap:?}); got {v:?}"
        );
    }
}
