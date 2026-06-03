# Konclude snapshot cache — Phase 1b Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the snapshot replay driver, the `BackPropAborted` runtime sentinel, the per-ontology snapshot cache, and the flag-gated orchestrator wiring. **Correctness + telemetry**, not performance — Phase 1b's acceptance is FP=0/MISSED=0 under flag-ON and counters reporting prune/replay/abort rates; performance measurement is Phase 1c (after lazy expansion lands).

**Architecture:** New file `crates/owl-dl-tableau/src/replay.rs` with `replay_with_neg_sup(snapshot, neg_sup_atoms, clauses) -> ReplayVerdict` reconstructing a `HyperEngine` from snapshot state and running `decide` with the negated sup added. Runtime sentinel hooks into `HyperEngine`'s label/merge mutations to flag back-propagation into snapshot nodes. New `SnapshotCache: Arc<DashMap<ClassId, Arc<GraphSnapshot>>>` owned by `PreparedOntology`, populated lazily per class at first query. Orchestrator wiring slots ahead of the existing wedge in `subsumes_via_tableau`, gated on `RUSTDL_SNAPSHOT_CAPTURE=1`.

**Tech Stack:** Rust 1.88+, edition 2024. Uses workspace `dashmap` (already a dep of `owl-dl-reasoner` via `HyperCache`). No new external crates.

**Spec:** [docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md](../specs/2026-06-03-konclude-style-global-classification-design.md) §4 (replay + soundness) and §6 (Phase 1b row).

**Predecessor:** Phase 1a landed `GraphSnapshot` types + `satisfiability_snapshot` capture + ontology-wide `BackPropRisk::classify_ontology` + Phase 0 canary at `f78d229..89650c1`. Results: `docs/phase1a-results.md`.

---

## Scope decision: full-re-run vs lazy expansion

The spec §4.1 pseudocode sketches a `LazyReplayDriver` that skips rule re-firing on snapshot nodes whose `RuleFingerprint` hasn't shifted. Lazy expansion is the *performance optimization* on top of the replay's correctness story — it lets snapshot reuse beat the wedge's wall-time. But:

- Spec §6 **Phase 1b acceptance** is `Inv-1 + Inv-2 hold` + telemetry + no behavior change with flag OFF. Performance is NOT in Phase 1b's acceptance.
- Spec §6 **Phase 1c acceptance** is the wall measurement (`GALEN ≤ 150 s`, etc.).

Lazy expansion sits in between. This plan ships **full-re-run** (replay rebuilds engine state, adds `¬sup`, then runs `decide` from scratch over the seeded state — same correctness as the wedge, no rule-firing skip). Lazy expansion is a Phase 1b.5 follow-up (or merged into Phase 1c's measurement work).

**Consequence:** Phase 1c measurement will show no wall improvement until lazy expansion lands. The §A revert criterion (`GALEN > 300 s after recon-driven tuning`) does NOT fire from Phase 1b alone — wall stays flat with flag ON, which is the expected post-Phase-1b state. The decision-point happens at Phase 1c after lazy expansion is in.

This is explicit in this plan and the Phase 1b results doc (Task 6).

---

## Phase 1a carry-overs addressed here

Per `docs/phase1a-results.md` and the final-review carry-over notes:

1. **Env-var parsing inconsistency** — `snapshot_capture_enabled` uses `.parse::<u32>().ok().is_some_and(|v| v != 0)` while sibling helpers (`hyper_wedge_enabled`, `label_heuristic_enabled`) use `var_os(...).map_or(default, |v| v != "0" && !v.is_empty())`. Fixed in Task 4 when we wire the consumer.
2. **`SnapshotNode.birth_deps` field** — deferred in Phase 1a to avoid exposing `hyper::DepSet`. Replay needs deps to seed the engine state. Resolved in Task 1 by either (a) exposing `hyper::DepSet` as `pub(crate)`, or (b) computing a fresh `DepSet::EMPTY` for every seeded node and accepting that backjumping is degraded for the snapshot subgraph (acceptable for correctness; Phase 1b.5 lazy expansion will refine).
3. **`fired` fingerprint slot** — placeholder `0` in Phase 1a. Computed only when Phase 1b.5 lazy expansion lands. Phase 1b leaves it `0`.
4. **Per-class `BackPropRisk` classifier** — NOT addressed here. Stays ontology-wide first-cut. Per-class refinement is spec §6 Phase 3. SROIQ workloads (ore-15672, pizza) will see snapshot cache size = 0; that's expected.

---

## File structure (this plan)

**New files:**
- `crates/owl-dl-tableau/src/replay.rs` — `ReplayVerdict`, `replay_with_neg_sup`, `BackPropAborted` runtime sentinel scaffolding.
- `crates/owl-dl-tableau/tests/replay_roundtrip.rs` — unit tests for replay correctness on Horn fixture.
- `crates/owl-dl-tableau/tests/replay_sentinel.rs` — unit test for sentinel firing on synthetic inverse-role fixture (even though such fixture would normally be flagged Unsafe and never reach replay, the sentinel must work as defense-in-depth — see spec §4.3).
- `docs/phase1b-results.md` — results doc.

**Modified files:**
- `crates/owl-dl-tableau/src/lib.rs` — `pub mod replay;` + re-exports of `ReplayVerdict`.
- `crates/owl-dl-tableau/src/hyper.rs` — add `HyperEngine::from_snapshot` constructor (rebuilds engine state from a snapshot); add `add_label_with_sentinel` mutation helper that triggers `BackPropAborted` when targeting a snapshot-originated node.
- `crates/owl-dl-tableau/src/snapshot.rs` — add `SnapshotNode.birth_deps` field + a `pub(crate)` accessor for `hyper::DepSet` (or equivalent, see Task 1 decision); update `from_parts` signature; update Phase 1a's `satisfiability_snapshot` capture to populate `birth_deps`.
- `crates/owl-dl-reasoner/src/lib.rs` — `SnapshotCache` field on `PreparedOntology`; cache built in `from_internal` when `snapshot_capture_enabled()`; new `PreparedOntology::snapshot_replay(sub, neg_sup_for_d) -> ReplayVerdict`; normalize the env helper to match sibling style.
- `crates/owl-dl-reasoner/src/classify.rs` — extend `ClassificationStats` with `snapshot_replay_used`, `snapshot_replay_subsumed`, `snapshot_replay_not_subsumed`, `snapshot_replay_aborts`, `snapshot_cache_falls_through` counters; wire snapshot-replay ahead of wedge in `subsumes_via_tableau` when flag is ON.
- `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs` — add flag-ON test functions extending the canary harness (Phase 0 tests stay).

---

### Task 1: `HyperEngine::from_snapshot` + `SnapshotNode.birth_deps` resolution

**Goal:** Phase 1a left `birth_deps` deferred; replay needs it. Decision: expose `hyper::DepSet` as `pub(crate)`, propagate it through `SnapshotNode`, update `satisfiability_snapshot` to populate. Then add the round-trip constructor.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (DepSet visibility + new `from_snapshot` method)
- Modify: `crates/owl-dl-tableau/src/snapshot.rs` (`birth_deps` field + `from_parts` signature)
- Create: `crates/owl-dl-tableau/tests/replay_roundtrip.rs` (round-trip test only — replay logic comes in Task 2)

- [ ] **Step 1: Decide DepSet visibility — bump `struct DepSet` to `pub(crate)`**

In `crates/owl-dl-tableau/src/hyper.rs` around line 69:

```rust
// before:
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct DepSet { bits: u128, overflow: bool }

// after:
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DepSet { pub(crate) bits: u128, pub(crate) overflow: bool }
```

Fields go `pub(crate)` so the snapshot module (same crate) can construct/inspect. `DepSet::EMPTY` const stays `pub(crate)` accessible.

If the impl block on `DepSet` (around line 74) has methods used externally now, leave them as-is — visibility change is only to allow `snapshot.rs` to name the type.

- [ ] **Step 2: Add `birth_deps` to `SnapshotNode`**

In `crates/owl-dl-tableau/src/snapshot.rs`, replace the existing `SnapshotNode` (which has only `labels` + `is_root`) with:

```rust
#[derive(Debug, Clone)]
pub(crate) struct SnapshotNode {
    /// Sorted-deduped concept labels at this node.
    pub labels: Vec<ClassId>,
    /// `true` iff this node is the seed-graph root.
    pub is_root: bool,
    /// `birth_deps` from the live graph — the decision-set under which
    /// this node was created. Used by replay's lazy expansion (Phase 1b.5)
    /// and future axiom-justification work. Phase 1b reads this when
    /// reconstructing the engine state; Phase 1b first-cut treats it
    /// opaquely (round-tripped, not interpreted).
    pub birth_deps: crate::hyper::DepSet,
}
```

Update `GraphSnapshot::from_parts` to accept the new `SnapshotNode` shape — no signature change at the `from_parts` level (it takes `Vec<SnapshotNode>` so the caller controls the field set).

Remove `#[allow(dead_code)]` on `SnapshotNode` if the compiler now picks up the field via Task 1's other usages; leave the field-level allow on `SnapshotEdge` until Task 2 reads it.

- [ ] **Step 3: Update `satisfiability_snapshot` to populate `birth_deps`**

In `crates/owl-dl-tableau/src/hyper.rs`, the existing `satisfiability_snapshot` constructs nodes with `labels` + `is_root` only. Update the `nodes.push(SnapshotNode { ... })` block (around line 825 — verify with grep) to also pass `birth_deps: hn.birth_deps` (the field already exists on `HyperNode` per the inline comment).

- [ ] **Step 4: Add `HyperEngine::from_snapshot` round-trip constructor**

Add after the existing `HyperEngine::satisfiability_snapshot` method:

```rust
/// Reconstruct a `HyperEngine` from a captured `GraphSnapshot`,
/// suitable as the seed state for a snapshot-replay query.
///
/// The returned engine has the snapshot's node/edge/label/birth_deps
/// state populated, and the clause set ready to receive additional
/// query clauses (e.g., a `¬sup` injection) before `decide` is called.
///
/// Note: this is the first half of the snapshot-replay path. Replay
/// proper lives in `crate::replay::replay_with_neg_sup` (Task 2).
/// Phase 1b first-cut uses full-re-run (no lazy expansion skip);
/// Phase 1b.5 will add fingerprint-gated lazy firing.
#[must_use]
pub fn from_snapshot(
    clauses: &'c [DlClause],
    snapshot: &crate::snapshot::GraphSnapshot,
) -> Self {
    use crate::snapshot::SnapshotNode;
    let mut engine = Self::new(clauses, snapshot.seed());
    // `Self::new` already created node 0 with the seed asserted at
    // EMPTY deps. Replace that bare state with the snapshot's full
    // node/edge graph. (Snapshot's root maps to engine's HNode(0).)
    engine.nodes.clear();
    engine.representative.clear();
    engine.neq.clear();
    for snap_node in snapshot.nodes() {
        let mut hn = HyperNode::default();
        hn.labels = snap_node.labels.clone();
        hn.label_deps = vec![snap_node.birth_deps; snap_node.labels.len()];
        hn.birth_deps = snap_node.birth_deps;
        // parent / parent_role: unknown from snapshot (Phase 1b.5 will
        // capture them in the snapshot data); leave None — sound because
        // double-blocking is a soundness-completeness lever, not a
        // soundness requirement, and the engine will conservatively
        // skip blocking decisions that lack parent info.
        engine.nodes.push(hn);
        engine.representative.push(HNode(u32::try_from(engine.nodes.len() - 1).expect("fits")));
    }
    for (i, edges) in snapshot.edges_per_node().iter().enumerate() {
        for edge in edges {
            engine.nodes[i].edges.push((edge.role, HNode(edge.target)));
            // Mirror as a pred on the target for back-propagation
            // bookkeeping (matches `Self::new_edge`).
            engine.nodes[edge.target as usize].preds.push((edge.role, HNode(u32::try_from(i).expect("fits"))));
        }
    }
    engine
}
```

This method needs new `pub(crate)` accessors on `GraphSnapshot`: `nodes() -> &[SnapshotNode]` and `edges_per_node() -> &[Vec<SnapshotEdge>]`. Add these in `snapshot.rs` alongside the existing accessors.

**Implementation notes:**
- `HyperEngine::new(clauses, seed)` builds with one node (HNode(0)) carrying the seed. The above overwrites that with the snapshot. Confirm by reading the existing `HyperEngine::new` to ensure no other state is left from `new` that needs reset (e.g., `worklist`, `clash_deps` — these are run-state, not graph state, and should start empty for a fresh decide call).
- If `HyperEngine` has lifecycle invariants checked in `new` that the replace-state pattern violates, restructure as `HyperEngine::with_snapshot(clauses, snapshot)` that constructs from scratch without going through `new`.

- [ ] **Step 5: Write the failing round-trip test**

Create `crates/owl-dl-tableau/tests/replay_roundtrip.rs`:

```rust
//! Round-trip test for snapshot capture + engine reconstruction.
//!
//! Phase 1b T1: a snapshot captured from a Sat verdict must, when
//! used to reconstruct a fresh engine, also return Sat when `decide`
//! runs again. (Replay-with-neg-sup is Task 2; this test exercises
//! only the seeded state.)

use horned_owl::io::ofn::reader::read;
use horned_owl::io::ParserConfiguration;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::clausify_with_stats;
use owl_dl_core::convert::convert_ontology;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult};
use std::io::Cursor;

#[test]
fn snapshot_seeded_engine_round_trips_to_sat() {
    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
    SubClassOf(:B :C)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let clauses = clausify_with_stats(&internal).0;
    let a_id = internal.vocabulary.class_id("http://t/A").expect("A exists");

    // Capture phase: run decide, snapshot.
    let mut eng = HyperEngine::new(&clauses, a_id);
    assert_eq!(eng.decide(64), HyperResult::Sat);
    let snap = eng.satisfiability_snapshot(a_id).expect("snapshot built");
    let original_node_count = snap.node_count();
    let original_root_labels: Vec<_> = snap.root_labels().to_vec();

    // Reconstruct + verify: seeded engine returns Sat too.
    let mut eng2 = HyperEngine::from_snapshot(&clauses, &snap);
    assert_eq!(eng2.decide(64), HyperResult::Sat);

    // The reconstructed engine's snapshot of itself should preserve
    // labels at the root (idempotence — snapshot-of-seeded should
    // equal the original up to internal representation).
    let snap2 = eng2.satisfiability_snapshot(a_id).expect("re-snapshot built");
    assert_eq!(snap2.node_count(), original_node_count);
    let mut sorted_a = original_root_labels.clone();
    sorted_a.sort();
    let mut sorted_b: Vec<_> = snap2.root_labels().to_vec();
    sorted_b.sort();
    assert_eq!(sorted_a, sorted_b);
}
```

- [ ] **Step 6: Run the test to verify it fails**

```bash
cargo test -p owl-dl-tableau --test replay_roundtrip
```

Expected: FAIL with "no method named `from_snapshot`".

- [ ] **Step 7: Run the test to verify it passes after implementation**

```bash
cargo test -p owl-dl-tableau --test replay_roundtrip
```

Expected: 1 test passes. If the round-trip fails on label-set equality, the snapshot's edge encoding likely lost something during reconstruction — debug with `dbg!(eng.nodes.len())` before/after `from_snapshot`.

- [ ] **Step 8: Run the full tableau test suite to confirm no regression**

```bash
cargo test -p owl-dl-tableau
```

Expected: all 96+ pre-existing tests still pass, plus the new round-trip test.

- [ ] **Step 9: Crate-scope clippy + fmt**

```bash
cargo clippy -p owl-dl-tableau --all-targets -- -D warnings
cargo fmt -p owl-dl-tableau -- --check
```

Expected: clean (workspace-wide is out of scope for this task).

- [ ] **Step 10: Commit**

```bash
git add crates/owl-dl-tableau/src/hyper.rs \
        crates/owl-dl-tableau/src/snapshot.rs \
        crates/owl-dl-tableau/tests/replay_roundtrip.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): HyperEngine::from_snapshot round-trip + birth_deps (Phase 1b T1)

Lands the snapshot→engine reconstruction half of the replay path.
- DepSet bumped to pub(crate) so snapshot.rs can name it.
- SnapshotNode.birth_deps populated by satisfiability_snapshot
  (was deferred in Phase 1a to avoid exposing hyper::DepSet).
- HyperEngine::from_snapshot rebuilds the engine state from a
  snapshot (nodes, labels, label_deps, birth_deps, edges, preds).
- Round-trip test: snapshot a Sat completion, reconstruct engine,
  rerun decide → Sat. Same root labels (modulo ordering).

Replay-with-neg-sup is Task 2. Phase 1b ships full-re-run
(no lazy expansion skip); Phase 1b.5 adds fingerprint-gated
lazy firing.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §4.1

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `replay_with_neg_sup` driver (full-re-run, no lazy expansion)

**Goal:** the replay function. Takes a snapshot + a negated-sup specification, reconstructs the engine, adds query clauses for `¬sup`, runs `decide`, returns a verdict.

**Files:**
- Create: `crates/owl-dl-tableau/src/replay.rs`
- Modify: `crates/owl-dl-tableau/src/lib.rs` (add `pub mod replay;` + re-export `ReplayVerdict`)
- Create: `crates/owl-dl-tableau/tests/replay_driver.rs`

- [ ] **Step 1: Write the failing driver test**

Create `crates/owl-dl-tableau/tests/replay_driver.rs`:

```rust
//! Phase 1b T2: replay driver test.
//!
//! Synthetic Horn ontology where `A ⊑ B` holds. Replay should
//! return `Subsumed` when probing `A ⊑ B` (because A's snapshot
//! plus `¬B` clashes), and `NotSubsumed` when probing `A ⊑ C`
//! (because A's snapshot plus `¬C` is satisfiable).

use horned_owl::io::ofn::reader::read;
use horned_owl::io::ParserConfiguration;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::{clausify_with_stats, Atom, DlClause, X};
use owl_dl_core::convert::convert_ontology;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult};
use owl_dl_tableau::replay::{replay_with_neg_sup, ReplayVerdict};
use std::io::Cursor;

fn setup(src: &str) -> (Vec<DlClause>, owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId) {
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let clauses = clausify_with_stats(&internal).0;
    let a = internal.vocabulary.class_id("http://t/A").expect("A");
    let b = internal.vocabulary.class_id("http://t/B").expect("B");
    let c = internal.vocabulary.class_id("http://t/C").expect("C");
    (clauses, a, b, c)
}

#[test]
fn replay_subsumed_when_neg_sup_clashes() {
    let (clauses, a, b, _c) = setup("\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
)
");
    let mut eng = HyperEngine::new(&clauses, a);
    assert_eq!(eng.decide(64), HyperResult::Sat);
    let snap = eng.satisfiability_snapshot(a).expect("snap");

    // Probe A ⊑ B: A's snapshot already contains B (told subsumer);
    // adding ¬B should clash → Subsumed.
    let neg_b_atom = Atom::Class(b, X); // we want to refute Atom::Class(b)
    // Construct a single Horn clause `B(x) → ⊥` to refute the presence
    // of B at any node — equivalent to asserting `¬B` at root.
    let neg_sup_clause = DlClause {
        body: vec![neg_b_atom.clone()],
        head: vec![], // empty head = ⊥ (clash)
    };

    let verdict = replay_with_neg_sup(&clauses, &snap, vec![neg_sup_clause]);
    assert_eq!(verdict, ReplayVerdict::Subsumed);
}

#[test]
fn replay_not_subsumed_when_neg_sup_satisfiable() {
    let (clauses, a, _b, c) = setup("\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
)
");
    let mut eng = HyperEngine::new(&clauses, a);
    eng.decide(64);
    let snap = eng.satisfiability_snapshot(a).expect("snap");

    // Probe A ⊑ C: A's snapshot doesn't contain C; adding ¬C is sat → NotSubsumed.
    let neg_c_clause = DlClause {
        body: vec![Atom::Class(c, X)],
        head: vec![],
    };
    let verdict = replay_with_neg_sup(&clauses, &snap, vec![neg_c_clause]);
    assert_eq!(verdict, ReplayVerdict::NotSubsumed);
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p owl-dl-tableau --test replay_driver
```

Expected: FAIL with "unresolved import `owl_dl_tableau::replay`".

- [ ] **Step 3: Create `replay.rs`**

Create `crates/owl-dl-tableau/src/replay.rs`:

```rust
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
//! BackPropAborted sentinel (Task 3) can detect runtime back-prop.
//!
//! Lazy expansion (fingerprint-gated rule firing skip) is Phase 1b.5.
//! With full-re-run, replay wall ≈ wedge wall + seed overhead, so
//! no perf win until lazy expansion lands. Phase 1b's acceptance is
//! correctness + telemetry, not perf.

use crate::hyper::{HyperEngine, HyperResult};
use crate::snapshot::{BackPropRisk, GraphSnapshot};
use owl_dl_core::clause::DlClause;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayVerdict {
    /// `¬sup` clashed with the snapshot — `sub ⊑ sup` is sound.
    Subsumed,
    /// `¬sup` is satisfiable over the snapshot — `sub ⊑ sup` is
    /// refuted in this model. Sound only when the snapshot's
    /// `BackPropRisk` is `Safe` AND no runtime sentinel fired.
    NotSubsumed,
    /// Runtime back-propagation sentinel fired (Phase 1b T3) OR the
    /// snapshot was Unsafe to begin with. Caller falls through to
    /// the existing wedge/tableau path.
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
///   `NotSubsumed` verdict to be sound. This function does NOT check
///   the risk; the caller (orchestrator) gates the call by risk.
///
/// `neg_sup_clauses` are appended to a clone of `clauses` before the
/// engine reads them. Typically these are 1-2 small clauses representing
/// `¬sup` (e.g., `Atom::Class(sup, X) → ⊥` for atomic sup).
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
    // Lifetime: HyperEngine borrows clauses for its lifetime, so we
    // need full_clauses to outlive engine. Box::leak avoided — collect
    // and drop together.
    let mut engine = HyperEngine::from_snapshot(&full_clauses, snapshot);
    match engine.decide(crate::HYPER_REPLAY_DEPTH) {
        HyperResult::Sat => ReplayVerdict::NotSubsumed,
        HyperResult::Unsat => ReplayVerdict::Subsumed,
        HyperResult::Stalled => ReplayVerdict::Stalled,
    }
}
```

If `crate::HYPER_REPLAY_DEPTH` doesn't exist, define it as `pub const HYPER_REPLAY_DEPTH: usize = 64;` near the top of `lib.rs`. Or just inline `64` here and add a `// TODO(phase1c): tune depth for replay workloads` comment.

- [ ] **Step 4: Wire `replay` module + re-export `ReplayVerdict`**

In `crates/owl-dl-tableau/src/lib.rs`, after `pub mod snapshot;`:

```rust
pub mod replay;
```

And in the re-exports:

```rust
pub use replay::{replay_with_neg_sup, ReplayVerdict};
```

- [ ] **Step 5: Run the test to verify it passes**

```bash
cargo test -p owl-dl-tableau --test replay_driver
```

Expected: 2 tests pass.

If the `Subsumed` test fails (engine returns Sat when it should clash), the snapshot may not have B as a label at the root (Horn closure should derive it). Verify: `dbg!(snap.root_labels())` should include both `a` and `b`.

If the `NotSubsumed` test fails (Sat expected but got Unsat), the test's clause encoding may be wrong — re-check `DlClause { body: [Atom::Class(c, X)], head: [] }` reads as "if C(x) then ⊥" which forces ¬C at every node.

- [ ] **Step 6: Run the full tableau test suite**

```bash
cargo test -p owl-dl-tableau
```

Expected: all pre-existing tests + 4 new tests (round-trip + 2 replay + snapshot capture tests from Phase 1a) all pass.

- [ ] **Step 7: Crate-scope clippy + fmt**

```bash
cargo clippy -p owl-dl-tableau --all-targets -- -D warnings
cargo fmt -p owl-dl-tableau -- --check
```

- [ ] **Step 8: Commit**

```bash
git add crates/owl-dl-tableau/src/replay.rs \
        crates/owl-dl-tableau/src/lib.rs \
        crates/owl-dl-tableau/tests/replay_driver.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): replay_with_neg_sup driver (full-re-run) (Phase 1b T2)

Snapshot replay path: reconstruct HyperEngine from snapshot, add
caller's ¬sup clauses, run decide, return ReplayVerdict {Subsumed,
NotSubsumed, BackPropAborted, Stalled}. Phase 1b ships full-re-run
(no rule-firing skip) — correctness equivalent to the wedge; perf
wins wait for Phase 1b.5 lazy expansion.

BackPropAborted gating on snapshot.risk() != Safe is the first-cut
soundness check (orchestrator-side gating is the safety story).
Runtime sentinel is Task 3.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §4.1, §4.2

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: `BackPropAborted` runtime sentinel

**Goal:** spec §4.3 defense-in-depth. Tag snapshot-originated nodes during `from_snapshot`; any mutation that would propagate a label into one of them via an inverse edge or merge sets an abort flag; replay reads the flag after `decide` and returns `BackPropAborted` instead of the verdict.

For full-re-run Phase 1b, the sentinel is mostly belt-and-suspenders: if `BackPropRisk::Safe` correctly excluded back-prop hazards, the sentinel never fires. But it must work and be tested, because (a) Phase 3 will loosen the structural classifier and the sentinel becomes the safety net, and (b) a buggy `Safe` classification needs runtime detection.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (add `snapshot_origin: Vec<bool>` + abort flag; hook into label mutations; inline unit test in `#[cfg(test)] mod tests`)
- Modify: `crates/owl-dl-tableau/src/replay.rs` (read abort flag, return `BackPropAborted`)

The sentinel test goes **inline** in `hyper.rs`'s existing `#[cfg(test)] mod tests` block, not as an integration test under `tests/`. Reason: the sentinel exercises engine internals (`snapshot_origin` vec, `add_label_via_backprop` helper) that would otherwise need `pub(crate)` accessors to be testable from `tests/`. Inline is simpler.

- [ ] **Step 1: Write the failing inline sentinel unit test**

In `crates/owl-dl-tableau/src/hyper.rs`'s `#[cfg(test)] mod tests` block (around line 1270 — locate with grep), add:

```rust
#[test]
fn sentinel_fires_on_simulated_backprop_into_snapshot_node() {
    // Build a tiny Horn snapshot, reconstruct via from_snapshot,
    // then manually trigger add_label_via_backprop on the seeded
    // root and assert the abort flag is set.
    let clauses: Vec<DlClause> = vec![];
    let a = ClassId::new(0);
    let mut eng = HyperEngine::new(&clauses, a);
    eng.decide(8);
    let snap = eng.satisfiability_snapshot(a).expect("snap");

    let mut eng2 = HyperEngine::from_snapshot(&clauses, &snap);
    assert!(!eng2.snapshot_backprop_aborted());
    let b = ClassId::new(1);
    eng2.add_label_via_backprop(HNode(0), b, DepSet::EMPTY);
    assert!(eng2.snapshot_backprop_aborted(), "sentinel must fire on back-prop into snapshot node");
}
```

Run to verify it fails (no method `add_label_via_backprop` or `snapshot_backprop_aborted`):

```bash
cargo test -p owl-dl-tableau --lib sentinel_fires_on_simulated_backprop
```

Expected: FAIL.

- [ ] **Step 2: Add sentinel fields to `HyperEngine`**

In `crates/owl-dl-tableau/src/hyper.rs`, add to the `HyperEngine<'c>` struct:

```rust
/// Phase 1b snapshot-origin tracking: `snapshot_origin[i]` is true
/// iff node i was reconstructed from a `GraphSnapshot` (via
/// `from_snapshot`), not created during the current decide run.
/// Used by the `BackPropAborted` runtime sentinel — see spec §4.3.
snapshot_origin: Vec<bool>,
/// Phase 1b BackPropAborted runtime sentinel: set to `true` if
/// any mutation during decide would propagate a label into a
/// `snapshot_origin` node via an inverse edge or merge. Reset to
/// false on every `decide` call. Replay reads this after decide
/// to decide whether to return BackPropAborted instead of the
/// raw verdict.
snapshot_backprop_aborted: bool,
```

Initialize both in `Self::new` (`vec![false]` for `snapshot_origin` — just node 0 — and `false` for the flag) and in `Self::from_snapshot` (`vec![true; n_nodes]` for `snapshot_origin`, `false` for the flag).

- [ ] **Step 3: Add the sentinel hook in mutation sites**

Identify the mutation sites:
- `Self::add_label(n, c, deps)` — when `n` is a snapshot-origin node AND the call was triggered by a back-propagation event (i.e., from a predecessor edge or merge), set the flag.
- `Self::merge_nodes(into, from)` or the corresponding union-find operation — when either side is snapshot-origin AND the merge is triggered by query-side state (not snapshot reconstruction), set the flag.

The cleanest hook is to wrap or modify `add_label` to take an additional `via_backprop: bool` parameter (default `false` for normal forward propagation). Wherever the existing code computes labels-via-inverse or merges-via-cardinality, pass `true`.

Simpler interim: add an unconditional `if snapshot_origin[n.index()] { snapshot_backprop_aborted = true }` in `add_label`. This OVER-fires (any label added to a snapshot node sets the flag, including the seeded labels themselves which are added during `from_snapshot`). Counter: in `from_snapshot`, set a `building` flag that suppresses the sentinel; clear it after construction. This is ugly. Prefer the explicit `via_backprop` parameter.

**Recommended pattern:**

```rust
fn add_label(&mut self, n: HNode, c: ClassId, deps: DepSet) -> bool {
    self.add_label_inner(n, c, deps, /* via_backprop */ false)
}

fn add_label_via_backprop(&mut self, n: HNode, c: ClassId, deps: DepSet) -> bool {
    self.add_label_inner(n, c, deps, /* via_backprop */ true)
}

fn add_label_inner(&mut self, n: HNode, c: ClassId, deps: DepSet, via_backprop: bool) -> bool {
    if via_backprop && self.snapshot_origin.get(n.index()).copied().unwrap_or(false) {
        self.snapshot_backprop_aborted = true;
    }
    // existing add_label body
}
```

Then update the existing call sites: forward propagation (label added via a clause body firing) stays `add_label`; back-propagation (label added via an inverse-role atom matching a predecessor) calls `add_label_via_backprop`.

If you can't easily disentangle which sites are back-prop without restructuring more than this task allows, **report DONE_WITH_CONCERNS** and ship a sentinel that always fires on snapshot-origin label additions but is gated by `via_inverse_or_merge` only at the most clearly back-prop sites (the inverse-role apply_all rule and the cardinality merge rule). Document the imprecision.

- [ ] **Step 4: Expose `snapshot_backprop_aborted` and integrate with `replay_with_neg_sup`**

Add a `pub(crate)` accessor:

```rust
#[must_use]
pub(crate) fn snapshot_backprop_aborted(&self) -> bool {
    self.snapshot_backprop_aborted
}
```

Modify `replay::replay_with_neg_sup` to check the flag after `decide`:

```rust
let result = engine.decide(64);
if engine.snapshot_backprop_aborted() {
    return ReplayVerdict::BackPropAborted;
}
match result { ... }
```

- [ ] **Step 5: Run the inline sentinel unit test to verify it now passes**

```bash
cargo test -p owl-dl-tableau --lib sentinel_fires_on_simulated_backprop
```

Expected: PASS.

- [ ] **Step 6: Run the tableau test suite**

```bash
cargo test -p owl-dl-tableau
```

Expected: all pre-existing + new sentinel test pass.

- [ ] **Step 7: Clippy + fmt + commit**

```bash
cargo clippy -p owl-dl-tableau --all-targets -- -D warnings
cargo fmt -p owl-dl-tableau -- --check
git add crates/owl-dl-tableau/src/hyper.rs \
        crates/owl-dl-tableau/src/replay.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): BackPropAborted runtime sentinel (Phase 1b T3)

Spec §4.3 defense-in-depth. HyperEngine tracks snapshot_origin per
node + an aborted flag set by add_label_via_backprop on snapshot
nodes. replay_with_neg_sup reads the flag after decide and returns
BackPropAborted (orchestrator falls through). Sentinel test in
hyper.rs verifies firing on simulated back-prop.

Phase 1b: sentinel rarely fires (BackPropRisk::Safe already excludes
hazards). Becomes load-bearing in Phase 3 when classifier loosens.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §4.3

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Snapshot cache + orchestrator wiring + env normalization

**Goal:** the part that actually changes behavior when flag is ON. Add `SnapshotCache` to `PreparedOntology`, populate per class on first query, call replay ahead of wedge in `subsumes_via_tableau`. Normalize the env helper. **NOT default-on** — gate stays `RUSTDL_SNAPSHOT_CAPTURE=1`.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (env normalization; `SnapshotCache` struct; `PreparedOntology::snapshot_replay` method)
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (`ClassificationStats` counter fields; wiring in `subsumes_via_tableau`)

- [ ] **Step 1: Normalize `snapshot_capture_enabled`**

Per the Phase 1a carry-over, change `snapshot_capture_enabled` to match the sibling style. Find the current implementation in `reasoner/src/lib.rs` (likely around line 673), and read the implementation of `hyper_wedge_enabled` for the canonical pattern. Replace `snapshot_capture_enabled` with the same shape — typically:

```rust
#[must_use]
pub fn snapshot_capture_enabled() -> bool {
    std::env::var_os("RUSTDL_SNAPSHOT_CAPTURE")
        .map_or(false, |v| v != "0" && !v.is_empty())
}
```

This accepts `=1`, `=true`, `=yes`, `=on`, any nonzero/nonempty string. Rejects `=0` and unset.

- [ ] **Step 2: Define `SnapshotCache`**

In `reasoner/src/lib.rs`, near `HyperCache` definition (around line 794), add:

```rust
/// Per-class snapshot cache for the Konclude snapshot cache project
/// (Phase 1b). Populated lazily: on first `snapshot_replay(C, ...)`
/// call for a given C, build a snapshot via the hyper engine and
/// stash it. Subsequent calls for the same C reuse the cached snapshot.
///
/// Cache is per-`PreparedOntology` instance. TBox is frozen for the
/// instance's lifetime, so cached snapshots stay valid across the
/// pair loop.
///
/// Sound only for classes whose ontology is `BackPropRisk::Safe`
/// (see spec §4.2). Phase 1b first-cut: ontology-wide risk; if the
/// ontology is Unsafe, the cache is `None` and every snapshot_replay
/// returns `ReplayVerdict::BackPropAborted` immediately.
pub(crate) struct SnapshotCache {
    /// Base clauses shared with the wedge (clausified once at build).
    clauses: Vec<owl_dl_core::clause::DlClause>,
    /// Per-class snapshot, lazily populated. Arc for cheap clone-on-read.
    snapshots: dashmap::DashMap<owl_dl_core::ir::ClassId, std::sync::Arc<owl_dl_tableau::GraphSnapshot>>,
    /// Ontology-wide BackPropRisk classification, computed once at build.
    risk: owl_dl_tableau::BackPropRisk,
}

impl SnapshotCache {
    pub(crate) fn build(internal: &InternalOntology) -> Self {
        let (clauses, _stats) = owl_dl_core::clause::clausify_with_stats(internal);
        let risk = owl_dl_tableau::BackPropRisk::classify_ontology(internal);
        Self { clauses, snapshots: dashmap::DashMap::new(), risk }
    }

    /// Try a snapshot-replay for `sub ⊑ sup`. Returns `None` when:
    /// - the ontology is Unsafe (no snapshot cache available), OR
    /// - building the snapshot for `sub` failed (decide returned Unsat
    ///   or Stalled — caller should fall through to wedge).
    pub(crate) fn try_replay(
        &self,
        sub: owl_dl_core::ir::ClassId,
        neg_sup_clauses: Vec<owl_dl_core::clause::DlClause>,
    ) -> Option<owl_dl_tableau::ReplayVerdict> {
        use owl_dl_tableau::{BackPropRisk, ReplayVerdict};
        if !matches!(self.risk, BackPropRisk::Safe) {
            return None;
        }
        let snap = self.snapshots.entry(sub).or_try_insert_with(|| {
            use owl_dl_tableau::hyper::{HyperEngine, HyperResult};
            let mut eng = HyperEngine::new(&self.clauses, sub);
            match eng.decide(HYPER_WEDGE_DEPTH) {
                HyperResult::Sat => eng
                    .satisfiability_snapshot(sub)
                    .map(std::sync::Arc::new)
                    .ok_or(()),
                _ => Err(()),
            }
        }).ok()?;
        Some(owl_dl_tableau::replay_with_neg_sup(&self.clauses, &snap, neg_sup_clauses))
    }
}
```

Note: `dashmap` is already a dep of `owl-dl-reasoner` (used by `HyperCache`). If `entry::or_try_insert_with` doesn't exist on your dashmap version, fall back to `get`/`insert` with manual locking — or use `entry().or_insert_with()` if you accept that a failed snapshot build will be empty-cached forever (acceptable for Phase 1b; refine in Phase 1c if it bites).

- [ ] **Step 3: Add `snapshot_cache` field to `PreparedOntology`**

Find the existing `PreparedOntology` struct (around line 1434). Add:

```rust
snapshot_cache: Option<SnapshotCache>,
```

In `PreparedOntology::from_internal` (around line 1444 where `hyper` is built), after the `hyper` line, add:

```rust
let snapshot_cache = snapshot_capture_enabled().then(|| SnapshotCache::build(&internal));
```

And add a public method:

```rust
pub(crate) fn snapshot_replay(
    &self,
    sub: owl_dl_core::ir::ClassId,
    neg_sup_clauses: Vec<owl_dl_core::clause::DlClause>,
) -> Option<owl_dl_tableau::ReplayVerdict> {
    self.snapshot_cache.as_ref().and_then(|c| c.try_replay(sub, neg_sup_clauses))
}
```

- [ ] **Step 4: Add counters to `ClassificationStats`**

In `crates/owl-dl-reasoner/src/classify.rs`, find `ClassificationStats` (use grep) and add fields:

```rust
pub snapshot_replay_used: usize,
pub snapshot_replay_subsumed: usize,
pub snapshot_replay_not_subsumed: usize,
pub snapshot_replay_aborts: usize,
pub snapshot_cache_falls_through: usize,
```

- [ ] **Step 5: Wire snapshot-replay ahead of wedge in `subsumes_via_tableau`**

In `crates/owl-dl-reasoner/src/classify.rs:1249`, before the `let hyper_deadline = ...` line (where the wedge call begins), insert:

```rust
// Phase 1b snapshot-replay shortcut. Try the per-class snapshot
// cache first; on Subsumed/NotSubsumed, return immediately.
// On BackPropAborted/Stalled/None, fall through to the wedge.
if crate::snapshot_capture_enabled() {
    let neg_sup_clauses = build_neg_sup_clauses_for(sup, prepared); // see helper below
    if let Some(verdict) = prepared.snapshot_replay(sub, neg_sup_clauses) {
        stats.snapshot_replay_used += 1;
        match verdict {
            owl_dl_tableau::ReplayVerdict::Subsumed => {
                stats.snapshot_replay_subsumed += 1;
                return Ok(Some(true));
            }
            owl_dl_tableau::ReplayVerdict::NotSubsumed if trust_sat => {
                stats.snapshot_replay_not_subsumed += 1;
                return Ok(Some(false));
            }
            owl_dl_tableau::ReplayVerdict::BackPropAborted => {
                stats.snapshot_replay_aborts += 1;
                // fall through
            }
            _ => {
                stats.snapshot_cache_falls_through += 1;
                // fall through
            }
        }
    } else {
        stats.snapshot_cache_falls_through += 1;
    }
}
// existing wedge code follows...
```

The `build_neg_sup_clauses_for(sup, prepared)` helper needs to encode `¬sup` as Horn clauses for the replay. The cleanest version mirrors what `HyperCache::decide` does at `lib.rs:855-864`:

```rust
fn build_neg_sup_clauses_for(
    sup: owl_dl_core::ir::ClassId,
    prepared: &crate::PreparedOntology,
) -> Vec<owl_dl_core::clause::DlClause> {
    use owl_dl_core::clause::{Atom, DlClause, X};
    // For atomic sup: a single clause "sup(x) → ⊥" — any node carrying
    // sup clashes. (The snapshot's root carries sub; if sub ⊑ sup, the
    // closure puts sup at root, which clashes.)
    vec![DlClause {
        body: vec![Atom::Class(sup, X)],
        head: vec![],
    }]
}
```

This is correct for atomic sup; for defined sup the wedge uses a more elaborate `sup_neg` table — Phase 1b can leave defined-sup pairs to fall through to the wedge by returning an empty vec (caller will fall through). Or look at the `HyperCache::sup_neg` table and reuse it. Simplest: handle atomic only in Phase 1b, document that defined sup falls through.

- [ ] **Step 6: Verify scope guard — flag-OFF behavior unchanged**

```bash
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude
```

Expected: Phase 0 canary still passes; closure-diff alehif still FP=0/MISSED=0. (Flag is still OFF by default; this verifies the wiring didn't accidentally trigger.)

- [ ] **Step 7: Verify flag-ON behavior — Horn closure still correct**

```bash
RUSTDL_SNAPSHOT_CAPTURE=1 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude
```

Expected: alehif closure still 247=247, FP=0/MISSED=0, but now counters show `snapshot_replay_*` activity. This is the **Inv-2 corpus invariant test**.

If this fails, the most likely culprits:
- `neg_sup_clauses` encoding is wrong → check by comparing replay verdict to wedge verdict on a single (sub, sup) pair.
- `from_snapshot` reconstructed engine state is incomplete → `dbg!` the seeded labels and compare to live capture.
- `BackPropRisk::classify_ontology(internal)` is wrongly classifying alehif as Unsafe → check: alehif is Horn so it should be Safe.

Report BLOCKED if Inv-2 fails after debugging.

- [ ] **Step 8: Clippy + fmt + commit**

```bash
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner -- --check
git add crates/owl-dl-reasoner/src/lib.rs \
        crates/owl-dl-reasoner/src/classify.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): SnapshotCache + orchestrator wiring (Phase 1b T4)

Per-class snapshot cache on PreparedOntology, populated lazily on
first query. subsumes_via_tableau consults replay ahead of wedge
when RUSTDL_SNAPSHOT_CAPTURE=1; on Subsumed/NotSubsumed returns
immediately; on BackPropAborted/Stalled falls through.

Counters: snapshot_replay_used / snapshot_replay_subsumed /
snapshot_replay_not_subsumed / snapshot_replay_aborts /
snapshot_cache_falls_through.

Env helper normalized to match sibling RUSTDL_* helpers
(accepts =true/=yes/=on; rejects =0/empty/unset).

Phase 0 canary still passes (flag OFF). Inv-2 (flag-ON corpus
invariant) verified on alehif closure_diff.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §4.1

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Phase 1b canary extensions — flag-ON tests

**Goal:** extend `snapshot_phase0_canary.rs` with flag-ON test functions. Phase 0 tests stay; new tests assert Inv-1 (synthetic) + counter telemetry firing.

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs`

- [ ] **Step 1: Add flag-ON tests**

Append to the existing canary file (after the two Phase 0 tests):

```rust
//! Phase 1b extensions: flag-ON behavior assertions.
//!
//! These tests set RUSTDL_SNAPSHOT_CAPTURE=1 process-locally via
//! the std::env::set_var pattern. They run sequentially with
//! `#[test]` ordering by `cargo test` — see the
//! safe-env-mutation comment in each test.

#[test]
fn replay_returns_subsumed_on_horn_chain_with_flag_on() {
    // SAFETY: set_var modifies process env. cargo test runs each
    // test in its own thread but all in one process; ordering
    // between this test and others is undefined. Use a guard.
    let _guard = SetEnvGuard::set("RUSTDL_SNAPSHOT_CAPTURE", "1");

    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
    SubClassOf(:B :C)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let result = classify_top_down_with_timeout(&onto, Duration::from_millis(200))
        .expect("classify");

    // Verdict must be unchanged from flag-OFF behavior (Inv-1).
    assert!(result.is_subclass("http://t/A", "http://t/C"), "A ⊑ C via chain");
    assert!(!result.is_subclass("http://t/B", "http://t/A"), "B ⋢ A");

    // Counter telemetry: snapshot path must have been used.
    let stats = result.stats();
    assert!(
        stats.snapshot_replay_used > 0,
        "Phase 1b: replay must fire at least once with flag ON (got 0)"
    );
}

// Guard helper at the bottom of the file:
struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}
impl SetEnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: see std::env::set_var docs.
        unsafe { std::env::set_var(key, value); }
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    fn drop(&mut self) {
        // SAFETY: see std::env::set_var docs.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
```

**IMPORTANT:** `std::env::set_var` is `unsafe` as of recent Rust versions due to thread-safety concerns. If clippy or the compiler complains, follow the guidance in the local rustdl style (search for any existing `set_var` usage in tests for the pattern that's already accepted).

If process-env mutation is too messy, an alternative is to wire `snapshot_capture_enabled()` to also read a process-local override (e.g., a `OnceLock<AtomicBool>` set by a test helper), and have the test helper bypass the env var entirely. This is more invasive but avoids the env-mutation hazard. Use whichever style the existing test suite has settled on (`cargo test -p owl-dl-reasoner --no-run` and grep for env_var or set_var precedent).

- [ ] **Step 2: Run the canary file**

```bash
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary
```

Expected: 3 tests pass (2 Phase 0 + 1 Phase 1b).

- [ ] **Step 3: Verify Inv-2 still holds via the corpus gate**

```bash
RUSTDL_SNAPSHOT_CAPTURE=1 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin
```

Expected: all three closure-diff tests pass with FP=0/MISSED=0, demonstrating the orchestrator wiring is sound under flag-ON across the 3 representative corpus fixtures.

- [ ] **Step 4: Commit**

```bash
git add crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs
git commit -m "$(cat <<'EOF'
test(snapshot): Phase 1b canary extensions — flag-ON Inv-1 + counter firing (Phase 1b T5)

Extends the Phase 0 canary file with a flag-ON test asserting (a)
verdict invariance vs. flag-OFF behavior on a Horn chain (Inv-1
synthetic) and (b) snapshot_replay_used counter > 0 (telemetry
firing).

Phase 0 tests stay. Inv-2 (corpus invariant under flag-ON) verified
manually via konclude_closure_diff with RUSTDL_SNAPSHOT_CAPTURE=1
on alehif + ore-10908 + ore-15672.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §6 Phase 1b

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Phase 1b results doc + GALEN flag-ON soundness gate

**Goal:** measure + document. Run GALEN closure-diff with flag ON to prove Inv-2 on the headline workload; report counters; document the carry-over for Phase 1b.5 (lazy expansion) → Phase 1c (default-on + perf).

**Files:**
- Create: `docs/phase1b-results.md`

- [ ] **Step 1: Run the full Phase 0 soundness gate with flag ON**

```bash
mkdir -p /tmp/p1b
RUSTDL_SNAPSHOT_CAPTURE=1 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude \
    ore_10908_sroiq ore_15672_shoin 2>&1 | tee /tmp/p1b/soundness-flag-on.log
```

Expected: FP=0 + MISSED=0 across all three. Walls likely flat or slightly elevated (full-re-run replay = wedge cost + seed overhead). Capture the log.

- [ ] **Step 2: Run GALEN closure-diff with flag ON**

```bash
RUSTDL_SNAPSHOT_CAPTURE=1 timeout 1500 \
    cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture 2>&1 \
    | tee /tmp/p1b/galen-flag-on.log
```

Expected: FP=0/MISSED=0, wall flat or slightly elevated vs. Phase 1a's 452 s. Phase 1b ships **correctness**; perf wins come from Phase 1b.5 lazy expansion. If wall > 500 s (>10% regression), document the regression — it'll inform Phase 1b.5 priorities — but it's NOT a blocker for Phase 1b. Soundness is the only blocker.

If FP > 0 or MISSED > 0, **STOP** — Phase 1b violated Inv-2 on the headline Horn workload. Report BLOCKED before proceeding.

- [ ] **Step 3: Counter telemetry summary**

Extract counter values from the closure-diff output (the `# label heuristic: ...` style banner emits stats per fixture). Document `snapshot_replay_used`, `snapshot_replay_subsumed`, `snapshot_replay_not_subsumed`, `snapshot_replay_aborts`, `snapshot_cache_falls_through` per fixture.

For GALEN (Horn, Safe), expect:
- `snapshot_replay_used` ≈ total pair count (~165k after Phase 7 label cache prunes)
- `snapshot_replay_aborts` ≈ 0 (Safe classification)
- `snapshot_cache_falls_through` ≈ small (only when build failed)
- `snapshot_replay_subsumed` + `snapshot_replay_not_subsumed` ≈ used

If aborts > 1% on Horn workloads, the sentinel is misfiring (or the per-class snapshot is leaking back-prop state). Investigate before declaring done.

- [ ] **Step 4: Write the results doc**

Create `docs/phase1b-results.md` with the structure used in `docs/phase1a-results.md`:

- Headline: correctness shipped, perf measurement deferred to Phase 1b.5/1c.
- What landed (commits + scope).
- Measurements table: wall numbers with flag ON vs. Phase 1a baseline.
- Counter telemetry per fixture.
- Carry-overs: lazy expansion (Phase 1b.5) is the unblocking work for Phase 1c.
- Cross-references.

Use `git log --oneline 89650c1..HEAD` to enumerate the Phase 1b commits.

- [ ] **Step 5: Commit results doc**

```bash
git add docs/phase1b-results.md
git commit -m "$(cat <<'EOF'
docs(phase1b): results — replay driver + sentinel + cache + wiring

Phase 1b lands snapshot-replay correctness path: HyperEngine::from_snapshot,
replay_with_neg_sup, BackPropAborted sentinel, SnapshotCache on
PreparedOntology, orchestrator wiring in subsumes_via_tableau.
Flag-gated RUSTDL_SNAPSHOT_CAPTURE (default OFF unchanged); env
helper normalized to sibling style.

Inv-1 + Inv-2 hold across Phase 0 net + GALEN with flag ON.
Counter telemetry reports snapshot_replay_used > 0 on every Horn
fixture (alehif, ore-10908, GALEN).

Wall: flat-to-slightly-elevated vs. Phase 1a baseline — expected,
Phase 1b is full-re-run (no rule-firing skip). Perf wins require
Phase 1b.5 lazy expansion → Phase 1c default-on + measure.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## After all tasks

Phase 1b complete. Next plan (`docs/superpowers/plans/2026-06-XX-konclude-snapshot-cache-phase-1b5-lazy-expansion.md`) covers the lazy-expansion optimization:

- Compute `RuleFingerprint` per snapshot node (bloom-hashed `(rule_id, label_set)`).
- Modify `HyperEngine::decide` (or just the inner loop) to consult the fingerprint before re-firing a rule on a snapshot node. Skip if the trigger set hasn't shifted.
- Measure: GALEN wall with lazy expansion ON vs. flag-OFF baseline. Acceptance: substantial wall reduction proving the architecture is right; cleared for Phase 1c default-on + headline measurement.

Phase 1b.5 is scoped as 2-3 sessions (smaller than 1b; lazy expansion is a focused optimization given the infrastructure 1b lands).

Phase 1c (`docs/superpowers/plans/2026-06-XX-konclude-snapshot-cache-phase-1c.md`) then flips the default + runs the full corpus + soundness gate + writes the project-headline results doc against the spec §6 acceptance criteria.
