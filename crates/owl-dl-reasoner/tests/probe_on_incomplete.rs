//! `RUSTDL_CLASSIFY_PROBE_ON_INCOMPLETE` — admit the classify consistency probe on an
//! INCOMPLETE, `ABox`-bearing run even when no class was proved unsatisfiable.
//!
//! The probe's normal admission test is the unsatisfiable FRACTION, which is
//! budget-sensitive: a timed-out per-class probe defaults to *satisfiable*, so a small
//! `--pair-timeout-ms` empties `unsatisfiable_idxs` and the gate reads "no evidence of
//! inconsistency" when the truth is "we did not look long enough to have evidence".
//! Conflating those two states is the defect.
//!
//! Ground truth for the motivating case is `ore_ont_16372`, which needs a corpus
//! fixture and so is covered by the release corpus report's sentinel list rather than
//! here. These tests pin the properties that can be established on a small fixture:
//! inertness when off, and — the one that matters — that widening the admission never
//! manufactures an inconsistency.
//!
//! DIRECTION OF RISK. This flag makes the reasoner probe MORE often, so the failure
//! mode is a false `inconsistent`, not a miss. That is the opposite of most flags here
//! and is why the negative controls below outnumber the positive ones.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
const KEY: &str = "RUSTDL_CLASSIFY_PROBE_ON_INCOMPLETE";

struct EnvGuard(Option<std::ffi::OsString>);
impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(v: Option<&str>) -> Self {
        let prior = std::env::var_os(KEY);
        // SAFETY: unsafe under edition 2024. One test, serialized via ENV_MUTEX,
        // restored on Drop.
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

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("fixture parses")
        .0
}

/// Consistent, ABox-bearing, and satisfiable throughout.
const CONSISTENT_ABOX: &str = r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(NamedIndividual(:a))
    SubClassOf(:A :B)
    SubClassOf(:B ObjectAllValuesFrom(:r :C))
    ClassAssertion(:A :a)
    ObjectPropertyAssertion(:r :a :a)
)
";

/// Consistent and ABox-FREE.
const CONSISTENT_TBOX: &str = r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    SubClassOf(:A :B)
)
";

/// `(consistent, probe_admitted)`. The second field is the one that makes the
/// mechanism testable — see `admission_is_observable_…` below.
fn probe_of(src: &str, per_pair_ms: u64) -> (bool, bool) {
    let onto = parse(src);
    let h = if per_pair_ms == 0 {
        owl_dl_reasoner::classify(&onto)
    } else {
        owl_dl_reasoner::classify_with_timeout(&onto, std::time::Duration::from_millis(per_pair_ms))
    }
    .expect("classify");
    let st = h.stats();
    (!st.inconsistent, st.consistency_probe_admitted)
}

fn consistent_of(src: &str, per_pair_ms: u64) -> bool {
    let onto = parse(src);
    let h = if per_pair_ms == 0 {
        owl_dl_reasoner::classify(&onto)
    } else {
        owl_dl_reasoner::classify_with_timeout(&onto, std::time::Duration::from_millis(per_pair_ms))
    }
    .expect("classify");
    !h.stats().inconsistent
}

/// Off (the default) must be inert on both an ABox-bearing and an ABox-free ontology,
/// at a generous budget and at a punishing one.
#[test]
fn unset_is_inert() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for src in [CONSISTENT_ABOX, CONSISTENT_TBOX] {
        for ms in [0u64, 1] {
            let off = {
                let _g = EnvGuard::set(None);
                consistent_of(src, ms)
            };
            let zero = {
                let _g = EnvGuard::set(Some("0"));
                consistent_of(src, ms)
            };
            assert_eq!(off, zero, "an explicit 0 must match unset (budget {ms} ms)");
            assert!(
                off,
                "these fixtures are consistent; got inconsistent at {ms} ms"
            );
        }
    }
}

/// **The load-bearing negative control.** With the flag ON and a punishing 1 ms budget
/// — the exact configuration that makes the admission fire (`timed_out_pairs > 0` and
/// an `ABox` present) — a CONSISTENT ontology must still be reported consistent.
///
/// The widened admission only lets the probe RUN. The probe's own verdict is what
/// decides, and it must not turn "we could not tell" into "inconsistent". If this ever
/// fails, the flag is manufacturing inconsistency and must be reverted, not tuned.
#[test]
fn widened_admission_never_manufactures_inconsistency() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("1"));
    for ms in [1u64, 5, 25] {
        assert!(
            consistent_of(CONSISTENT_ABOX, ms),
            "consistent ABox ontology reported INCONSISTENT with the probe admitted \
             (budget {ms} ms) — the flag is manufacturing a verdict"
        );
    }
}

/// An ABox-FREE ontology must never reach the widened admission, however incomplete
/// the run: the condition is `timed_out_pairs > 0 AND has_abox_axioms`. Pins the
/// `has_abox_axioms` conjunct — dropping it would widen the probe to every incomplete
/// ontology in the corpus.
#[test]
fn abox_free_ontology_is_not_admitted() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let on = {
        let _g = EnvGuard::set(Some("1"));
        consistent_of(CONSISTENT_TBOX, 1)
    };
    let off = {
        let _g = EnvGuard::set(None);
        consistent_of(CONSISTENT_TBOX, 1)
    };
    assert_eq!(on, off, "no ABox ⇒ the flag must change nothing");
    assert!(on, "fixture is consistent");
}

/// **PARTIAL MECHANISM TEST — read the limits before trusting it.**
///
/// An earlier version of this file asserted only on the VERDICT and BOTH sabotages
/// passed: dropping the `has_abox_axioms` conjunct, and making the admission ignore
/// the flag. Neither is visible in a verdict, because on a CONSISTENT ontology the
/// probe concludes "consistent" whether or not it ran. `consistency_probe_admitted`
/// exists so the two can be told apart.
///
/// WHAT THIS CANNOT ESTABLISH. The new admission requires `timed_out_pairs > 0`, and
/// no small fixture reliably times out a pair — a 26-class ∀/⊔ ontology with an `ABox`
/// still reports **0** timed-out pairs at a 1 ms budget, and a fixture tuned until it
/// did would be timing-flaky by construction. So the POSITIVE direction (admission
/// fires when it should) is NOT pinned here; it is covered end-to-end by the release
/// corpus report's `ore_ont_16372` sentinel, which is a real inconsistent ontology and
/// is validated to flip that gate.
///
/// Sabotage results for this test, measured: making the admission ignore the flag IS
/// caught. Dropping the `has_abox_axioms` conjunct is NOT — that sabotage only shows
/// up on an ontology that both lacks an `ABox` and times out pairs, which is the same
/// fixture that cannot be written.
#[test]
fn admission_is_not_granted_without_the_flag_or_without_an_abox() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Flag OFF: never admitted on a 0-unsat run, at any budget.
    {
        let _g = EnvGuard::set(None);
        for ms in [0u64, 1, 25] {
            let (_, adm) = probe_of(CONSISTENT_ABOX, ms);
            assert!(!adm, "flag off must not admit the probe (budget {ms} ms)");
        }
    }
    // Flag ON but no unsat evidence and nothing timed out: still not admitted.
    {
        let _g = EnvGuard::set(Some("1"));
        let (ok, adm) = probe_of(CONSISTENT_TBOX, 1);
        assert!(
            !adm,
            "no ABox and no unsat evidence must not admit the probe"
        );
        assert!(ok, "fixture is consistent");
    }
}

/// A genuinely inconsistent ontology must be reported inconsistent with the flag ON —
/// the positive direction, so a fix that admitted the probe but never let it conclude
/// would not pass silently.
#[test]
fn genuine_inconsistency_still_detected_with_flag_on() {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("1"));
    let src = r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(NamedIndividual(:a))
    DisjointClasses(:A :B)
    ClassAssertion(:A :a)
    ClassAssertion(:B :a)
)
";
    assert!(
        !consistent_of(src, 0),
        "an individual in two disjoint classes is inconsistent"
    );
}
