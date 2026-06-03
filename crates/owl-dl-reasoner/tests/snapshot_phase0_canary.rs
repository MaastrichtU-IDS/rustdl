//! Phase 0 / 1c canary for the Konclude snapshot cache project.
//!
//! Phase 0 invariant (original): the snapshot-capture machinery
//! exists in the tableau crate AND is gated behind
//! `RUSTDL_SNAPSHOT_CAPTURE`, so default classify behavior is
//! controlled by the env helper.
//!
//! Phase 1c flipped the default OFF → ON. The canary now asserts
//! `snapshot_capture_enabled()` returns true with no env override.
//! Phase 1b's flag-ON tests below are unchanged (still set the env
//! explicitly for isolation against a hypothetical future flip).

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify_top_down_with_timeout, snapshot_capture_enabled};
use std::io::Cursor;
use std::time::Duration;

#[test]
fn snapshot_capture_flag_defaults_on() {
    // Phase 1c invariant (post-headline flip): env flag defaults ON.
    // Test guard: SAFE_TO_UNSET — assumes RUSTDL_SNAPSHOT_CAPTURE is
    // not set in the test process env. A developer who exports it
    // for debugging will fail this test (which is the right warning).
    //
    // ENV_MUTEX serialization: reads process env, must not race with
    // the Phase 1b flag-ON tests below that briefly set/restore
    // RUSTDL_SNAPSHOT_CAPTURE.
    let _serial = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        std::env::var("RUSTDL_SNAPSHOT_CAPTURE").is_err(),
        "Phase 1c canary: RUSTDL_SNAPSHOT_CAPTURE must not be set in the test env"
    );
    assert!(
        snapshot_capture_enabled(),
        "Phase 1c default must be ON (unset → ON)"
    );
}

#[test]
fn classify_unchanged_at_default() {
    // Sanity: a tiny ontology classifies to the same result as
    // it did pre-project, with the Phase 1c default-on path in play.
    let _serial = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    SubClassOf(:A :B)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let result =
        classify_top_down_with_timeout(&onto, Duration::from_millis(200)).expect("classify");
    assert!(result.is_subclass("http://t/A", "http://t/B"));
    assert!(!result.is_subclass("http://t/B", "http://t/A"));
}

// ---------------------------------------------------------------------------
// Phase 1b canary extensions (T5): flag-ON Inv-1 + counter telemetry firing.
// ---------------------------------------------------------------------------
//
// These tests exercise the snapshot-replay path with
// RUSTDL_SNAPSHOT_CAPTURE=1 to assert:
//   - Inv-1 (synthetic): verdicts under flag-ON match flag-OFF on
//     small ontologies.
//   - Counter telemetry fires as designed (snapshot_replay_used > 0
//     on a Safe Horn chain; snapshot_cache_falls_through > 0 and
//     snapshot_replay_used == 0 on an Unsafe inverse-role ontology).
//
// Inv-2 (corpus invariant under flag-ON) was verified manually in
// Phase 1b T4 via konclude_closure_diff on alehif + ore-10908 +
// ore-15672 (FP=0/MISSED=0) — too heavy to belong here.

#[test]
fn replay_returns_subsumed_on_horn_chain_with_flag_on() {
    let _serial = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _guard = SetEnvGuard::set("RUSTDL_SNAPSHOT_CAPTURE", "1");
    // The Phase 7 per-class label cache prunes non-subsumptions
    // *before* `subsumes_via_tableau` is called — bypassing the
    // snapshot-replay shortcut we want to exercise. Disable it for
    // this test so the replay path is reached.
    let _label_guard = SetEnvGuard::set("RUSTDL_LABEL_HEURISTIC", "0");
    assert!(
        snapshot_capture_enabled(),
        "flag-ON: snapshot_capture_enabled() must report true"
    );

    // 4-class non-pure-EL Safe ontology: a Horn chain A ⊑ B ⊑ C plus
    // an isolated class D and DisjointObjectProperties(r, s). The
    // disjoint-properties axiom prevents `pure_el_mode`; the isolated
    // D forces the top-down walk to probe `D ⊑ B?` / `D ⊑ C?` via
    // `subsumes_via_tableau`, exercising the snapshot-replay path
    // (Safe ⇒ replay engages). Mirrors the existing
    // `selective_verify_triggers_when_threshold_high` shape.
    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    DisjointObjectProperties(:r :s)
    SubClassOf(:A :B)
    SubClassOf(:B :C)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let result =
        classify_top_down_with_timeout(&onto, Duration::from_millis(5_000)).expect("classify");

    // Inv-1 synthetic: verdicts match flag-OFF expectations.
    assert!(result.is_subclass("http://t/A", "http://t/B"));
    assert!(result.is_subclass("http://t/B", "http://t/C"));
    assert!(result.is_subclass("http://t/A", "http://t/C"));
    assert!(!result.is_subclass("http://t/B", "http://t/A"));
    assert!(!result.is_subclass("http://t/D", "http://t/A"));

    // Telemetry firing: the snapshot-replay path was consulted at
    // least once. The ontology is Safe (no inverse/nominal/cardinality),
    // so the gate returns `Some(verdict)` and increments
    // `snapshot_replay_used`. If this stays 0, T4's wiring did not
    // fire on a Safe ontology — escalate.
    let stats = result.stats();
    assert!(
        stats.snapshot_replay_used > 0,
        "Phase 1b T5: Safe ontology must exercise replay path; stats = {stats:?}"
    );
}

#[test]
fn replay_no_op_on_unsafe_ontology_with_flag_on() {
    let _serial = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _guard = SetEnvGuard::set("RUSTDL_SNAPSHOT_CAPTURE", "1");
    // Same rationale as the Safe test: keep the label cache from
    // pruning probes before they hit `subsumes_via_tableau`.
    let _label_guard = SetEnvGuard::set("RUSTDL_LABEL_HEURISTIC", "0");
    assert!(
        snapshot_capture_enabled(),
        "flag-ON: snapshot_capture_enabled() must report true"
    );

    // Inverse role forces BackPropRisk::Unsafe at the whole-ontology
    // gate (Phase 1b). The isolated class D + non-trivial chain
    // (A ⊑ B ⊑ C) forces the top-down walk to call
    // `subsumes_via_tableau` for at least one (sub, sup) probe, so
    // the falls-through counter must increment while replay never
    // does.
    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:rinv))
    InverseObjectProperties(:r :rinv)
    SubClassOf(:A :B)
    SubClassOf(:B :C)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let result =
        classify_top_down_with_timeout(&onto, Duration::from_millis(5_000)).expect("classify");

    // Inv-1 synthetic: verdicts unchanged on Unsafe inputs.
    assert!(result.is_subclass("http://t/A", "http://t/B"));
    assert!(result.is_subclass("http://t/A", "http://t/C"));
    assert!(!result.is_subclass("http://t/B", "http://t/A"));
    assert!(!result.is_subclass("http://t/D", "http://t/A"));

    // Telemetry firing: Unsafe ⇒ no replay attempted; the gate
    // observes BackPropRisk::Unsafe and falls through to the wedge,
    // incrementing snapshot_cache_falls_through but never
    // snapshot_replay_used.
    let stats = result.stats();
    assert_eq!(
        stats.snapshot_replay_used, 0,
        "Phase 1b T5: Unsafe ontology must not exercise replay; stats = {stats:?}"
    );
    assert!(
        stats.snapshot_cache_falls_through > 0,
        "Phase 1b T5: Unsafe ontology must record falls-through; stats = {stats:?}"
    );
}

// ---------------------------------------------------------------------------
// Env-mutation plumbing: SetEnvGuard restores the prior value on Drop;
// ENV_MUTEX serializes env-mutating tests against the Phase 0 "flag
// defaults OFF" test under parallel `cargo test`.
// ---------------------------------------------------------------------------

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: std::env::set_var is unsafe due to global thread-safety
        // concerns. We hold the var only for the duration of one test,
        // serialized via ENV_MUTEX, and restore on Drop. Same pattern
        // as classify.rs::selective_verify_triggers_when_threshold_high.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set. Restoring the prior value is
        // required so the Phase 0 "flag defaults OFF" test stays valid
        // across parallel test execution in the same process.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
