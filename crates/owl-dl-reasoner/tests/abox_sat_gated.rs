//! Tests for the Variant A-gated ABox-saturation consistency pre-check.
//!
//! This is the consequence-based named-individual fixpoint
//! (`saturate_abox_for_consistency` in `owl-dl-saturation`) wired into
//! `is_consistent_internal_full` behind `RUSTDL_ABOX_SAT_GATED=1`.
//!
//! Tests:
//! 1. `family_detected_inconsistent` — full family.ofn detected via A-gated
//!    (requires `ontologies/real/family.ofn`; `#[ignore]`d).
//! 2. `fp_smoke_consistent_abox_stays_consistent` — a consistent ontology with
//!    functional + inverse + disjoint roles is NOT flagged as inconsistent.
//! 3. `functional_marker_clash_detected` — a synthetic clash where a class implies
//!    `∃hasSex.Male ⊓ ∃hasSex.Female` with `Functional(hasSex)` and
//!    `Disjoint(Male, Female)` is detected.  Exercises the A-gated wiring
//!    (bypasses A1's P8 pre-check which would otherwise mask it).
//! 4. `el_preservation_gate_off` — with `RUSTDL_ABOX_SAT_GATED=0`, a
//!    consistent ontology remains consistent (gate-off path unchanged).
//!
//! **EL preservation** is guaranteed by construction: `saturate_abox_for_consistency`
//! is unreachable from the `saturate()` classification path. Run:
//!   `cargo test -p owl-dl-saturation`   # all 51+ existing tests must pass
//!
//! **Env isolation:** all tests that mutate `RUSTDL_ABOX_SAT_GATED` hold
//! `ENV_MUTEX` for the duration and restore prior env state on Drop, preventing
//! races when tests run in the same process.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::fs;
use std::io::Cursor;
use std::path::Path;

const FIXTURE_DIR: &str = "tests/fixtures/abox_sat_gated";

// ── Env-mutation plumbing ────────────────────────────────────────────────────
// Serialize all tests that mutate env vars in this file. Using a process-global
// mutex means cargo's default parallel-within-binary execution can't interleave
// the mutations.

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: guarded by ENV_MUTEX (all callers hold the lock before calling
        // this); restored on Drop.
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse and run `is_consistent` on an OFN fixture file.
/// The caller must hold `ENV_MUTEX` and have set `RUSTDL_ABOX_SAT_GATED`.
fn check_fixture(name: &str) -> bool {
    let path = Path::new(FIXTURE_DIR).join(format!("{name}.ofn"));
    let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent succeeds")
}

/// Parse `path` (relative to workspace root) and run `is_consistent`.
/// The caller must hold `ENV_MUTEX`.
fn check_path(path: &Path) -> bool {
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent succeeds")
}

// ── Test 1: family detected via A-gated (requires real corpus) ──────────────

/// Full family.ofn should be detected inconsistent when A-gated is enabled.
/// Requires `ontologies/real/family.ofn` (gitignored; pull with
/// `scripts/fetch-real-ontologies.sh`).
///
/// Run: `RUSTDL_ABOX_SAT_GATED=1 cargo test -p owl-dl-reasoner --test abox_sat_gated family_detected_inconsistent -- --ignored`
#[test]
#[ignore = "requires ontologies/real/family.ofn (run scripts/fetch-real-ontologies.sh)"]
fn family_detected_inconsistent() {
    let path = Path::new("ontologies/real/family.ofn");
    if !path.exists() {
        eprintln!("SKIP: ontologies/real/family.ofn not found (run scripts/fetch-real-ontologies.sh)");
        return;
    }
    let _serial = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _gate = SetEnvGuard::set("RUSTDL_ABOX_SAT_GATED", "1");
    let t0 = std::time::Instant::now();
    let consistent = check_path(path);
    eprintln!(
        "family: consistent={consistent}, elapsed={:.3}s",
        t0.elapsed().as_secs_f64()
    );
    assert!(!consistent, "family.ofn should be detected inconsistent via A-gated");
}

// ── Test 2: FP smoke — consistent ABox with functional + inverse + disjoint ─

/// A consistent ontology WITH Functional(hasSex) + DisjointClasses(Male,Female) +
/// inverse roles, but only ONE hasSex filler — no clash possible.
///
/// This is stronger than a no-disjoint smoke test: it verifies that rules
/// 7b/8 fire ONLY on genuine witnesses, not spuriously.
#[test]
fn fp_smoke_consistent_abox_stays_consistent() {
    let _serial = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    // Gate off: gate-default path, consistent.
    {
        let _gate = SetEnvGuard::set("RUSTDL_ABOX_SAT_GATED", "0");
        let c = check_fixture("fp_smoke_consistent");
        assert!(c, "gate off: consistent ABox must not be flagged");
    }

    // Gate on: still consistent (single functional filler, no disjoint clash).
    {
        let _gate = SetEnvGuard::set("RUSTDL_ABOX_SAT_GATED", "1");
        let c = check_fixture("fp_smoke_consistent");
        assert!(c, "gate on: single functional filler must NOT clash");
    }
}

// ── Test 3: Synthetic functional-marker clash (wiring gate) ──────────────────

/// A class Parent ⊑ ∃hasSex.Male ⊓ ∃hasSex.Female with Functional(hasSex) and
/// Disjoint(Male, Female), asserted on individual :pat → inconsistent.
///
/// **Why not masked by A1 P8:** this test explicitly disables `RUSTDL_ABOX_CHECK`
/// so the A1 P8 functional-collapse pre-check cannot intercept the verdict.
/// The gated saturator is the ONLY path that can detect it here.
/// This verifies the wiring from `is_consistent_internal_full` into
/// `saturate_abox_for_consistency`.
#[test]
fn functional_marker_clash_detected_via_gate() {
    let _serial = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // Disable A1's P8 abox_check so ONLY our gated saturator can detect the clash.
    let _no_abox_check = SetEnvGuard::set("RUSTDL_ABOX_CHECK", "0");
    let _gate = SetEnvGuard::set("RUSTDL_ABOX_SAT_GATED", "1");
    let consistent = check_fixture("functional_chain_clash");
    assert!(
        !consistent,
        "With ABOX_CHECK=0, only A-gated saturator can detect this clash — \
         if this fails, the gated wiring is broken"
    );
}

// ── Test 4: gate-off leaves consistent ontologies untouched ─────────────────

/// With the gate explicitly off (`RUSTDL_ABOX_SAT_GATED=0`), a consistent
/// ontology returns consistent, verifying the gated code path is inactive.
#[test]
fn el_preservation_gate_off() {
    let _serial = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _gate = SetEnvGuard::set("RUSTDL_ABOX_SAT_GATED", "0");
    let consistent = check_fixture("fp_smoke_consistent");
    assert!(consistent, "gate off: consistent ontology must remain consistent");
}
