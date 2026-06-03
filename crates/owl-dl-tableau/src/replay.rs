//! Snapshot-replay driver for the Konclude snapshot cache project
//! (Phase 1b + 1b.5).
//!
//! See `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`
//! §4.1 (replay path) and §4.2 (soundness contract).
//!
//! Two replay paths:
//! - [`replay_with_neg_sup`] (Phase 1b.5 default): lazy expansion.
//!   Reconstructs a `HyperEngine` via [`HyperEngine::from_snapshot_lazy`]
//!   so `horn_fixpoint` re-seed skips pre-captured labels at
//!   snapshot-origin nodes not in the new clauses' trigger atoms.
//!   Correctness equivalent to full-re-run; substantially less work.
//! - [`replay_with_neg_sup_full_rerun`] (Phase 1b first-cut, kept
//!   for A/B): reconstructs via [`HyperEngine::from_snapshot`] and
//!   runs `decide` from scratch over the seeded state. Engaged
//!   when the orchestrator's `RUSTDL_SNAPSHOT_LAZY=0` toggle is set.

use crate::hyper::{HyperEngine, HyperResult};
use crate::snapshot::{BackPropRisk, GraphSnapshot};
use owl_dl_core::clause::{Atom, DlClause};

/// Recursion depth cap for the replay `decide` call. Mirrors the wedge
/// default (`hyper::HyperEngine::decide` takes the cap as an arg; 256 is
/// the value used in `TableauContext::is_satisfiable` and matches the
/// shape of the wedge's typical depth). Module-local until the workspace
/// agrees on a single named constant.
const REPLAY_DEPTH: usize = 256;

/// Verdict from the replay paths. The orchestrator maps
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

/// Phase 1b.5 default: lazy-expansion replay. Reconstructs an engine
/// via [`HyperEngine::from_snapshot_lazy`] with trigger atoms
/// extracted from `neg_sup_clauses`' body class atoms, runs `decide`.
/// `horn_fixpoint`'s re-seed loop skips `Event::Label` events for
/// pre-captured labels at snapshot-origin nodes not in the trigger
/// set — sound by spec §4.1 soundness contract + the Phase 1b.5 plan.
///
/// Soundness preconditions:
/// - The snapshot must have been built from a `Sat` verdict.
/// - The snapshot's `BackPropRisk` must be `Safe` for the
///   `NotSubsumed` verdict to be sound. This function checks
///   `snapshot.risk()` and returns `BackPropAborted` immediately
///   if Unsafe.
///
/// `neg_sup_clauses` are appended to a clone of `clauses` before the
/// engine reads them. Typically one Horn clause `q ⊓ sup → ⊥`.
#[must_use]
pub fn replay_with_neg_sup(
    clauses: &[DlClause],
    snapshot: &GraphSnapshot,
    neg_sup_clauses: Vec<DlClause>,
) -> ReplayVerdict {
    if !matches!(snapshot.risk(), BackPropRisk::Safe) {
        return ReplayVerdict::BackPropAborted;
    }
    // Trigger atoms — collected BEFORE the move that consumes
    // `neg_sup_clauses` into the extended clause set.
    let new_trigger_atoms: std::collections::HashSet<u32> = neg_sup_clauses
        .iter()
        .flat_map(|c| c.body.iter())
        .filter_map(|atom| match atom {
            Atom::Class(cid, _) => Some(cid.index()),
            _ => None,
        })
        .collect();
    let mut full_clauses = clauses.to_vec();
    full_clauses.extend(neg_sup_clauses);
    let mut engine =
        HyperEngine::from_snapshot_lazy(&full_clauses, snapshot, new_trigger_atoms);
    let result = engine.decide(REPLAY_DEPTH);
    if engine.snapshot_backprop_aborted() {
        return ReplayVerdict::BackPropAborted;
    }
    match result {
        HyperResult::Sat => ReplayVerdict::NotSubsumed,
        HyperResult::Unsat => ReplayVerdict::Subsumed,
        HyperResult::Stalled => ReplayVerdict::Stalled,
    }
}

/// Phase 1b first-cut: full-re-run replay. Reconstructs an engine
/// via [`HyperEngine::from_snapshot`] (no lazy state) and re-runs
/// `decide` from scratch. Correctness equivalent to
/// [`replay_with_neg_sup`] but pays the cost of re-firing every
/// snapshot-node label event. Kept as the A/B reference for the
/// `RUSTDL_SNAPSHOT_LAZY=0` toggle in the reasoner's
/// `SnapshotCache::try_replay`.
#[must_use]
pub fn replay_with_neg_sup_full_rerun(
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
    if engine.snapshot_backprop_aborted() {
        return ReplayVerdict::BackPropAborted;
    }
    match result {
        HyperResult::Sat => ReplayVerdict::NotSubsumed,
        HyperResult::Unsat => ReplayVerdict::Subsumed,
        HyperResult::Stalled => ReplayVerdict::Stalled,
    }
}
