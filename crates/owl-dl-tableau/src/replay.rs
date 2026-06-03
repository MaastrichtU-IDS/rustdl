//! Snapshot-replay driver for the Konclude snapshot cache project
//! (Phase 1b).
//!
//! See `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`
//! §4.1 (replay path) and §4.2 (soundness contract).
//!
//! Phase 1b first-cut: **full-re-run**. The replay reconstructs a
//! `HyperEngine` from a snapshot, adds the caller's negated-sup
//! clauses, then runs `decide` from scratch over the seeded state.
//! Correctness equivalent to running the wedge with `q ⊑ sub ⊓ ¬sup`,
//! but the seeded state preserves the snapshot's labels so the
//! `BackPropAborted` sentinel (Task 3) can detect runtime back-prop.
//!
//! Lazy expansion (fingerprint-gated rule firing skip) is Phase 1b.5.
//! With full-re-run, replay wall ≈ wedge wall + seed overhead, so
//! no perf win until lazy expansion lands. Phase 1b's acceptance is
//! correctness + telemetry, not perf.

use crate::hyper::{HyperEngine, HyperResult};
use crate::snapshot::{BackPropRisk, GraphSnapshot};
use owl_dl_core::clause::DlClause;

/// Recursion depth cap for the replay `decide` call. Mirrors the wedge
/// default (`hyper::HyperEngine::decide` takes the cap as an arg; 256 is
/// the value used in `TableauContext::is_satisfiable` and matches the
/// shape of the wedge's typical depth). Module-local until the workspace
/// agrees on a single named constant.
const REPLAY_DEPTH: usize = 256;

/// Verdict from `replay_with_neg_sup`. The orchestrator (Task 4) maps
/// `Subsumed`/`NotSubsumed` to the classify decision and falls through
/// on `BackPropAborted`/`Stalled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayVerdict {
    /// `¬sup` clashed with the snapshot — `sub ⊑ sup` is sound.
    Subsumed,
    /// `¬sup` is satisfiable over the snapshot — `sub ⊑ sup` is
    /// refuted in this model. Sound only when the snapshot's
    /// `BackPropRisk` is `Safe` AND no runtime sentinel fired.
    NotSubsumed,
    /// Either (a) the snapshot was `BackPropRisk::Unsafe` and replay
    /// was skipped entirely, or (b) the runtime sentinel fired during
    /// `decide` (back-prop into a snapshot-origin node). Both paths
    /// are conflated into one variant by design — orchestrator
    /// behaviour is identical (fall through to wedge/tableau), and
    /// telemetry can distinguish the two via the structural risk
    /// recorded on the snapshot itself. See spec §4.3.
    BackPropAborted,
    /// Engine stalled (deadline / iteration cap). Caller falls through.
    Stalled,
}

/// Run snapshot-replay: reconstruct an engine from `snapshot`, add
/// the caller's `neg_sup_clauses`, run `decide`, return a verdict.
///
/// Soundness preconditions:
/// - The snapshot must have been built from a `Sat` verdict (caller's
///   responsibility — capture site verifies this).
/// - The snapshot's `BackPropRisk` should be `Safe` for the
///   `NotSubsumed` verdict to be sound. This function checks
///   `snapshot.risk()` and returns `BackPropAborted` immediately
///   if Unsafe, but the orchestrator (Task 4) gates by risk anyway.
///
/// `neg_sup_clauses` are appended to a clone of `clauses` before the
/// engine reads them. Typically these are 1-2 small clauses representing
/// `¬sup` (e.g., `Atom::Class(sup, X) → ⊥` for atomic sup).
#[must_use]
pub fn replay_with_neg_sup(
    clauses: &[DlClause],
    snapshot: &GraphSnapshot,
    neg_sup_clauses: Vec<DlClause>,
) -> ReplayVerdict {
    if !matches!(snapshot.risk(), BackPropRisk::Safe) {
        return ReplayVerdict::BackPropAborted;
    }
    let mut full_clauses = clauses.to_vec();
    full_clauses.extend(neg_sup_clauses);
    let mut engine = HyperEngine::from_snapshot(&full_clauses, snapshot);
    let result = engine.decide(REPLAY_DEPTH);
    // Sentinel check: if any back-propagation event during decide
    // targeted a snapshot-origin node, the verdict's soundness is
    // suspect — fall through to the wedge/tableau path. Phase 1b
    // on Safe seeds: this never fires (BackPropRisk excludes the
    // hazards). Phase 3: load-bearing once the classifier loosens.
    if engine.snapshot_backprop_aborted() {
        return ReplayVerdict::BackPropAborted;
    }
    match result {
        HyperResult::Sat => ReplayVerdict::NotSubsumed,
        HyperResult::Unsat => ReplayVerdict::Subsumed,
        HyperResult::Stalled => ReplayVerdict::Stalled,
    }
}
