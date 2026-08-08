//! Canaries for `RUSTDL_LABEL_CACHE_TOTAL_MS` — the AGGREGATE bound on the
//! whole label-cache build phase (2026-08-08).
//!
//! **Why an aggregate bound exists at all.** `RUSTDL_LABEL_CACHE_TIMEOUT_MS`
//! (and its adaptive default) bounds ONE class. The phase costs `n × per-class`,
//! and `n` reaches 8,025 on the affected ontologies, so no per-class value
//! bounds the phase. Profiling the 11 `label_cache_build`-dominated members of
//! the DNF tail found the median per-class overshoot is **0 ms** with a tail of
//! 400–560 ms classes: even a *perfect* 10 ms per-class bound still leaves
//! 17–80 s. See `docs/known-limitations/label-cache-build-unbounded.md`.
//!
//! **THE DIRECTION OF RISK IS UNUSUAL HERE, AND IT IS THE POINT OF THIS FILE.**
//! The label cache is a PRUNE: the orchestrator skips `subsumes_via_tableau`
//! when `D ∉ labels(C)`, justified by a counterexample model. An unbuilt label
//! is `NoVerdict`, which DISABLES the prune. So cutting the phase short cannot
//! lose a subsumption by removing an inference — it removes an *optimisation*,
//! and the tier walk then does MORE tableau probes, not fewer.
//!
//! The real cost is therefore INDIRECT: those extra probes consume the per-pair
//! budget, so pairs can time out that previously never had to be probed. That
//! makes the flag a genuine trade rather than a free win, and it is why the
//! recovery question is settled by measurement rather than by this reasoning.
//!
//! What these canaries pin is the part that must hold unconditionally: the flag
//! parses as documented, the unset path is unchanged, and a bounded run never
//! reports a subsumption an unbounded run does not.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;
use std::fmt::Write as _;
use std::io::Cursor;
use std::sync::Mutex;

/// Serialises the env mutation. `cargo test` runs tests in one process.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard(Option<String>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(v: Option<&str>) -> Self {
        let prev = std::env::var("RUSTDL_LABEL_CACHE_TOTAL_MS").ok();
        match v {
            Some(x) => unsafe { std::env::set_var("RUSTDL_LABEL_CACHE_TOTAL_MS", x) },
            None => unsafe { std::env::remove_var("RUSTDL_LABEL_CACHE_TOTAL_MS") },
        }
        Self(prev)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        match &self.0 {
            Some(x) => unsafe { std::env::set_var("RUSTDL_LABEL_CACHE_TOTAL_MS", x) },
            None => unsafe { std::env::remove_var("RUSTDL_LABEL_CACHE_TOTAL_MS") },
        }
    }
}

/// A small ontology with a genuine subsumption chain, plus enough classes that
/// the label-cache loop actually runs.
fn fixture() -> String {
    let mut s = String::from("Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/o>\n");
    for i in 0..40 {
        let _ = writeln!(s, "Declaration(Class(:C{i}))");
    }
    s.push_str("Declaration(ObjectProperty(:r))\n");
    // A chain C0 ⊑ C1 ⊑ … so there are real subsumptions to report.
    for i in 0..39 {
        let _ = writeln!(s, "SubClassOf(:C{} :C{})", i, i + 1);
    }
    // Some existential structure so the wedge has work to do per class.
    for i in 0..10 {
        let _ = writeln!(s, "SubClassOf(:C{i} ObjectSomeValuesFrom(:r :C{}))", i + 20);
    }
    // OUT-OF-EL, and load-bearing: `∀` is outside `saturator_complete_fragment`,
    // so classify takes the HYBRID path and the label-cache loop actually runs.
    // Without this the fixture is pure EL, classify short-circuits to the
    // saturation fast path, and every behavioural assertion below is VACUOUS —
    // they would pass against a build where the flag does nothing at all.
    // (NB the old idiom of a bare `InverseObjectProperties` declaration no longer
    // leaves the fragment: `RUSTDL_FRAGMENT_BARE_DECL` admits unread declarations.)
    for i in 0..10 {
        let _ = writeln!(s, "SubClassOf(:C{i} ObjectAllValuesFrom(:r :C{}))", i + 25);
    }
    s.push_str(")\n");
    s
}

/// Returns the reported direct subsumptions AND a witness that the label-cache
/// loop was actually reached — without which every behavioural assertion here
/// would be vacuous.
fn classify_subsumptions(total_ms: Option<&str>) -> (Vec<(String, String)>, bool) {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(total_ms);
    let src = fixture();
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let c = classify(&onto).expect("classify");
    let mut v: Vec<(String, String)> = Vec::new();
    for class in c.classes() {
        for sup in c.direct_subsumers(class) {
            v.push((class.clone(), sup.to_owned()));
        }
    }
    v.sort();
    let hybrid = !c.stats().pure_el_mode;
    (v, hybrid)
}

#[test]
fn unset_is_unbounded() {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(None);
    assert_eq!(
        owl_dl_reasoner::label_cache_total_ms(),
        None,
        "unset RUSTDL_LABEL_CACHE_TOTAL_MS must mean UNBOUNDED (opt-in flag)"
    );
}

#[test]
fn zero_is_unbounded() {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("0"));
    assert_eq!(
        owl_dl_reasoner::label_cache_total_ms(),
        None,
        "`0` is the documented spelling for unbounded, matching the sibling \
         RUSTDL_LABEL_CACHE_TIMEOUT_MS convention"
    );
}

#[test]
fn positive_value_parses() {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("2500"));
    assert_eq!(
        owl_dl_reasoner::label_cache_total_ms(),
        Some(2500),
        "a positive value must parse to that many milliseconds"
    );
}

#[test]
fn garbage_is_unbounded_not_zero() {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set(Some("banana"));
    assert_eq!(
        owl_dl_reasoner::label_cache_total_ms(),
        None,
        "an unparseable value must fall back to UNBOUNDED. Falling back to a \
         bounded value would silently degrade every run with a typo'd flag."
    );
}

#[test]
fn bounded_run_reports_no_subsumption_the_unbounded_run_misses() {
    // The FP-shaped direction. Cutting the label cache disables a PRUNE, so the
    // tier walk does MORE probing — every answer still comes from the tableau,
    // and none may be spurious.
    let (unbounded, hybrid) = classify_subsumptions(None);
    let (bounded, _) = classify_subsumptions(Some("1"));
    assert!(
        hybrid,
        "PRECONDITION: the fixture must take the HYBRID path. On the pure-EL fast \
         path the label cache is never built, so this assertion would pass against \
         a build where the flag does nothing."
    );
    assert!(!unbounded.is_empty(), "fixture must produce subsumptions");
    for pair in &bounded {
        assert!(
            unbounded.contains(pair),
            "bounded run reported {pair:?}, absent from the unbounded run — the \
             aggregate bound must never MANUFACTURE a subsumption"
        );
    }
}

#[test]
fn tiny_budget_still_classifies_this_fixture_completely() {
    // On an ontology small enough that the probe path decides every pair, losing
    // the label cache entirely must cost nothing but time. This is what makes
    // the bound safe to offer at all: it degrades throughput, not answers.
    let (unbounded, hybrid) = classify_subsumptions(None);
    let (bounded, _) = classify_subsumptions(Some("1"));
    assert!(
        hybrid,
        "PRECONDITION: fixture must take the hybrid path (see above)"
    );
    assert_eq!(
        bounded, unbounded,
        "with a 1 ms aggregate budget the label cache is effectively absent, yet \
         a fixture this small must still classify identically — the cache is an \
         optimisation, not a source of inferences"
    );
}
