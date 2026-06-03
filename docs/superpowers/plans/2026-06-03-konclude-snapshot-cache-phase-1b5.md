# Konclude snapshot cache — Phase 1b.5 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add lazy expansion to the snapshot replay path so flag-ON GALEN wall drops below the flag-OFF baseline. Phase 1b shipped full-re-run (no rule-firing skip), +8.3% overhead. Phase 1b.5 recon (`docs/phase1b5-recon.md`) projects ~89% CPU savings (optimistic) or ~69% (pessimistic) by gating re-seeding of pre-captured labels on snapshot-origin nodes. Target: GALEN wall ≤ 100 s with flag ON (vs current 161 s flag-ON / 150 s flag-OFF).

**Architecture:** At snapshot capture, record per-node `pre_capture_labels: Vec<ClassId>`. At replay time, extract `new_trigger_atoms: HashSet<u32>` from the caller's `neg_sup_clauses` (the body-atom class ids of every new clause). Modify `horn_fixpoint`'s worklist re-seed loop to consult both: for snapshot-origin nodes, only push `Event::Label(n, c)` when `c` was added since capture (i.e., not in `pre_capture_labels[n]`) OR `c` is in `new_trigger_atoms`. All other event types (Edge, NodeNew) re-seed normally — only Label re-seeding is gated.

Plus two perf/quality follow-ups from Phase 1b reviewer notes: cache `neg_sup_clauses` per `sup` column (avoid per-call allocation), and capture `parent`/`parent_role` on snapshot nodes for HF2 double-blocking restoration.

**Tech Stack:** Rust 1.88+, edition 2024. No new external deps. Uses workspace `hashbrown` for the trigger-atom set (already a transitive dep).

**Spec:** [docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md](../specs/2026-06-03-konclude-style-global-classification-design.md) §4.1 (lazy expansion) + §6 Phase 1c outcome bands.

**Recon:** `docs/phase1b5-recon.md` (commit `13b930c`) — GO recommendation with projected ~3,200 CPU-sec savings on GALEN.

**Predecessor:** Phase 1b landed at `b4eab21..cb47751`. T1 instrumentation at `3ae716f`. GALEN flag-OFF baseline (this host): 150 s. GALEN flag-ON Phase 1b: 161 s.

---

## Soundness contract (carries forward from Phase 1b spec §4.2)

The guard is sound IFF every effect of pre-captured labels was already
realized in the snapshot's saturated state. Argument:

- Snapshot was captured AFTER `HyperResult::Sat` was returned (caller-
  enforced precondition on `satisfiability_snapshot`).
- `Sat` means `horn_fixpoint` reached fixpoint over the capture-time
  clause set — every Horn rule that could fire on every (node, label)
  did fire.
- Adding a new clause C' at replay extends the clause set. C''s
  effects are only triggered by atoms in C''s body — which is exactly
  the `new_trigger_atoms` set.
- For pre-existing labels NOT in `new_trigger_atoms`: their effects
  under the original clause set are already in the snapshot; adding
  C' doesn't change those effects (C' doesn't trigger on them).
  Skipping re-seed is sound.
- For labels in `new_trigger_atoms`: C' might fire on them; must seed.
- For labels added since capture (e.g., from ¬sup cascade): not in
  `pre_capture_labels`, so the guard's `!contains` branch seeds them.

**Inv-1b.5 (lazy):** Across the corpus diff suite, lazy-expansion
replay verdicts match full-re-run replay verdicts on every (sub, sup)
pair. Tested by env gate `RUSTDL_SNAPSHOT_LAZY` (default ON in Phase
1b.5; OFF reverts to full-re-run) + the existing GALEN closure-diff.

**Per the Phase 1b T6 lesson** (`a1f81ff`): every soundness-touching
task runs the GALEN closure-diff gate before commit. Synthetic
canaries are necessary but not sufficient.

---

## File structure

**Modified files:**
- `crates/owl-dl-tableau/src/snapshot.rs` — add `pre_capture_labels: Vec<Vec<ClassId>>` field on `GraphSnapshot`; update `from_parts` to accept it; populate at capture site.
- `crates/owl-dl-tableau/src/hyper.rs` — add `lazy_replay_state: Option<LazyReplayState>` field on `HyperEngine` (holds `pre_capture_labels + new_trigger_atoms`); update `satisfiability_snapshot` to populate snapshot's `pre_capture_labels`; new `from_snapshot_lazy(clauses, snapshot, new_trigger_atoms)` constructor; modify `horn_fixpoint` re-seed loop to consult the guard.
- `crates/owl-dl-tableau/src/replay.rs` — extract `new_trigger_atoms` from `neg_sup_clauses` body atoms; route through `from_snapshot_lazy`.
- `crates/owl-dl-reasoner/src/lib.rs` — `SnapshotCache` adds per-sup `neg_sup_clauses` cache (DashMap keyed by `sup`); `snapshot_lazy_enabled()` env helper (default ON).
- `crates/owl-dl-reasoner/src/classify.rs` — orchestrator unchanged behaviorally (still calls `snapshot_replay(sub, sup)`); SnapshotCache internals do the per-sup lookup.

**New files:**
- `docs/phase1b5-results.md` — final results doc with GALEN wall measurement + Phase 1c green-light/revert decision.

---

### Task 1: `pre_capture_labels` on snapshot + round-trip test

**Goal:** thread the captured-labels-per-node through the snapshot data structure. Phase 1b's `SnapshotNode.labels` field captures labels at capture-time, but those labels mutate during replay (new ones added via cascade). The lazy guard needs an IMMUTABLE record of "what was in the snapshot when captured" — a separate field protected from replay-time mutation.

**Files:**
- Modify: `crates/owl-dl-tableau/src/snapshot.rs` (add field; update `from_parts` signature; accessor)
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (populate at capture in `satisfiability_snapshot`)
- Modify: `crates/owl-dl-tableau/src/replay.rs` (no change yet — Task 2 wires it)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (no change — `from_parts` callers in tests will need adjusting; check for any inline construction)
- Modify: `crates/owl-dl-tableau/tests/replay_roundtrip.rs` (extend round-trip test to assert `pre_capture_labels == labels` at snapshot time)

- [ ] **Step 1: Add the field to `SnapshotNode`**

In `crates/owl-dl-tableau/src/snapshot.rs`, extend `SnapshotNode`:

```rust
#[derive(Debug, Clone)]
pub(crate) struct SnapshotNode {
    /// Sorted-deduped concept labels at this node. MUTABLE during
    /// replay — the engine appends new labels via the cascade.
    pub labels: Vec<ClassId>,
    /// `true` iff this node is the seed-graph root.
    pub is_root: bool,
    /// `birth_deps` — see Phase 1b T1.
    pub birth_deps: crate::hyper::DepSet,
    /// Phase 1b.5: IMMUTABLE snapshot of `labels` at the time
    /// `satisfiability_snapshot` was called. Distinct from `labels`
    /// (which mutates during replay) — this field is the lazy
    /// expansion guard's reference: a snapshot-origin node's
    /// pre_capture_labels[n] tells the engine which labels were
    /// already-saturated at capture time. Empty `Vec` is a valid
    /// state (a node with no labels at capture; uncommon).
    pub pre_capture_labels: Vec<ClassId>,
}
```

Note: `pre_capture_labels` is stored on the snapshot, not the live engine. The engine reads it via `snapshot.nodes()[i].pre_capture_labels` at horn_fixpoint re-seed time (after Task 2 wires the guard).

- [ ] **Step 2: Populate at capture site**

In `crates/owl-dl-tableau/src/hyper.rs`'s `satisfiability_snapshot`, the existing `nodes.push(SnapshotNode { labels, is_root, birth_deps })` block needs to also set `pre_capture_labels`. Simplest: clone `labels` (they're identical at capture time):

```rust
nodes.push(SnapshotNode {
    labels: hn.labels.clone(),
    is_root: snap_id == root_snap_idx,
    birth_deps: hn.birth_deps,
    pre_capture_labels: hn.labels.clone(),
});
```

Both `labels` and `pre_capture_labels` start equal at capture; they diverge during replay (only `labels` mutates).

- [ ] **Step 3: Update `GraphSnapshot::from_parts` if it's called inline anywhere**

`from_parts` constructs `nodes: Vec<SnapshotNode>` from the caller — the caller already populates each SnapshotNode's fields. No signature change needed IF callers update their construction. Grep for `SnapshotNode { ` and add `pre_capture_labels: labels.clone()` (or equivalent) at each site.

Likely sites:
- `hyper.rs` `satisfiability_snapshot` (Step 2 above).
- Any test that constructs `SnapshotNode` directly.

- [ ] **Step 4: Add accessor on GraphSnapshot for the per-node pre_capture_labels**

```rust
impl GraphSnapshot {
    /// Per-node immutable snapshot of labels at capture time. Used
    /// by the lazy-expansion guard in `horn_fixpoint` re-seed.
    #[must_use]
    pub(crate) fn pre_capture_labels_per_node(&self) -> Vec<&[ClassId]> {
        self.nodes.iter().map(|n| n.pre_capture_labels.as_slice()).collect()
    }
}
```

(Or expose via the existing `nodes()` accessor — caller indexes into `nodes[i].pre_capture_labels`.)

- [ ] **Step 5: Extend `replay_roundtrip.rs` to assert pre_capture_labels matches labels at snapshot time**

In `crates/owl-dl-tableau/tests/replay_roundtrip.rs`, add an assertion after capturing the snapshot:

```rust
// Phase 1b.5: pre_capture_labels must match labels at snapshot time.
for snap_node in snap.nodes() {
    assert_eq!(
        snap_node.labels.as_slice(),
        snap_node.pre_capture_labels.as_slice(),
        "Phase 1b.5 invariant: pre_capture_labels equals labels at capture"
    );
}
```

`snap.nodes()` is already public per Phase 1b T1. If the inner `SnapshotNode` fields are `pub(crate)` and the test is in an integration test (not the lib), this assertion needs an accessor. Use whichever style the existing tests use; the simplest is a per-node accessor on GraphSnapshot:

```rust
impl GraphSnapshot {
    #[must_use]
    pub fn pre_capture_labels_at(&self, i: usize) -> &[ClassId] {
        &self.nodes[i].pre_capture_labels
    }
    #[must_use]
    pub fn labels_at(&self, i: usize) -> &[ClassId] {
        &self.nodes[i].labels
    }
}
```

Then the test asserts `snap.pre_capture_labels_at(i) == snap.labels_at(i)` for all i.

- [ ] **Step 6: Run tests**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo test -p owl-dl-tableau
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary
```

Expected: all tableau tests pass (~101) including the extended round-trip; Phase 0 canary still passes (no behavior change for default classify).

- [ ] **Step 7: GALEN soundness gate (per Phase 1b T6 lesson)**

```bash
RUSTDL_SNAPSHOT_CAPTURE=1 timeout 1500 cargo test -p owl-dl-reasoner \
    --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture
```

Expected: FP=0/MISSED=0, closure 27,997=Konclude. Wall: same as Phase 1b (~161s) — no behavior change yet.

If FP > 0 or MISSED > 0, **STOP** — Task 1 introduced a soundness regression somewhere (shouldn't, since the new field is read-only-from-the-perspective-of-this-task). Investigate before commit.

- [ ] **Step 8: Clippy + fmt (crate-scope) + commit**

```bash
cargo clippy -p owl-dl-tableau --all-targets -- -D warnings
cargo fmt -p owl-dl-tableau -- --check
git add crates/owl-dl-tableau/src/snapshot.rs \
        crates/owl-dl-tableau/src/hyper.rs \
        crates/owl-dl-tableau/tests/replay_roundtrip.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): pre_capture_labels on SnapshotNode (Phase 1b.5 T1)

Adds an IMMUTABLE per-node snapshot of `labels` at capture time,
distinct from the mutating `labels` field. Phase 1b.5's lazy
expansion guard consults this field to decide whether a label
event needs re-seeding: pre-captured labels can be skipped (their
effects are already in the snapshot's saturated state) unless a
new clause indexes them as triggers (Task 3 will gate on this).

Round-trip test extended to assert pre_capture_labels == labels
at snapshot time. GALEN soundness gate passes (no behavior change
yet — Task 2 wires the guard).

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §4.1

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `LazyReplayState` on HyperEngine + `from_snapshot_lazy` constructor

**Goal:** add the engine-side machinery: a per-engine `Option<LazyReplayState>` field carrying `(pre_capture_labels, new_trigger_atoms)`, and a new `from_snapshot_lazy(clauses, snapshot, new_trigger_atoms)` constructor that populates it. The horn_fixpoint re-seeding (Task 3) reads this state.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (new struct + field + constructor)
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (Snapshot branch-save needs to NOT save lazy state — it's a read-only contract from from_snapshot, untouched by branching)

- [ ] **Step 1: Define `LazyReplayState`**

In `crates/owl-dl-tableau/src/hyper.rs`, after the existing struct definitions:

```rust
/// Phase 1b.5 lazy expansion state for snapshot replay. When set
/// (via `HyperEngine::from_snapshot_lazy`), `horn_fixpoint` re-seed
/// consults this state to skip pushing `Event::Label(n, c)` for
/// snapshot-origin nodes whose `c` is in `pre_capture_labels[n]`
/// AND `c` is not in `new_trigger_atoms`.
///
/// Soundness: pre-captured labels' effects are already realized
/// in the snapshot's saturated state; new clauses only trigger on
/// `new_trigger_atoms`. See spec §4.1 + this plan's soundness
/// contract section.
///
/// `None` means full-re-run mode (Phase 1b first-cut behavior).
struct LazyReplayState {
    /// Per-node immutable labels at snapshot capture. Parallel to
    /// `HyperEngine.nodes` (indexed by HNode.index()). Snapshot-origin
    /// nodes have populated entries; non-snapshot nodes (created
    /// during decide via new_node) have empty Vec — the guard's
    /// "not pre-captured" branch catches them naturally.
    pre_capture_labels: Vec<Vec<ClassId>>,
    /// Body-atom class ids of every clause appended at replay (i.e.,
    /// the caller's `neg_sup_clauses`). hashbrown::HashSet for
    /// constant-time lookup in the re-seed loop.
    new_trigger_atoms: hashbrown::HashSet<u32>,
}
```

If `hashbrown` isn't already imported in this file, add `use hashbrown::HashSet;` (it's a workspace dep via DashMap). Or use `std::collections::HashSet<u32>` — semantically equivalent, slightly slower lookup.

- [ ] **Step 2: Add the field to `HyperEngine`**

In the `HyperEngine` struct definition, add:

```rust
/// Phase 1b.5: optional lazy-replay state. `None` for fresh
/// engines (via `Self::new`) or full-re-run replays (via the
/// existing `Self::from_snapshot`). `Some` only via
/// `Self::from_snapshot_lazy`.
lazy_replay_state: Option<LazyReplayState>,
```

Initialize to `None` in `Self::new`:

```rust
// in Self::new's Self { ... } block:
lazy_replay_state: None,
```

Initialize to `None` in `Self::from_snapshot` (the existing full-re-run constructor) so calling it doesn't accidentally trigger lazy behavior:

```rust
// in Self::from_snapshot, after engine.snapshot_origin = vec![true; n_nodes]:
engine.lazy_replay_state = None;
```

- [ ] **Step 3: Add `from_snapshot_lazy` constructor**

After the existing `from_snapshot` method:

```rust
/// Phase 1b.5: lazy-expansion constructor for snapshot replay.
/// Same shape as `from_snapshot` but additionally populates
/// `lazy_replay_state` with the snapshot's pre_capture_labels +
/// the caller's `new_trigger_atoms`. `horn_fixpoint` re-seed
/// will skip Event::Label events for pre-captured labels at
/// snapshot-origin nodes when those labels are not in
/// `new_trigger_atoms`.
///
/// Sound iff the snapshot was built from a `Sat` verdict AND the
/// caller's `new_trigger_atoms` is a complete enumeration of body
/// atoms in clauses appended since capture. See spec §4.1 + the
/// Phase 1b.5 plan's soundness contract.
///
/// `new_trigger_atoms` accepts ClassId indices (u32) for the
/// constant-time lookup the re-seed loop wants. Caller derives
/// from the new clauses' body atoms.
#[must_use]
pub fn from_snapshot_lazy(
    clauses: &'c [DlClause],
    snapshot: &crate::snapshot::GraphSnapshot,
    new_trigger_atoms: hashbrown::HashSet<u32>,
) -> Self {
    // Delegate the graph-state population to from_snapshot.
    let mut engine = Self::from_snapshot(clauses, snapshot);
    // Collect pre_capture_labels parallel to snapshot.nodes(); pad
    // with empty Vec for any nodes the engine might create later via
    // new_node (the re-seed guard's "not in pre_capture_labels"
    // branch handles them).
    let pre_capture_labels: Vec<Vec<ClassId>> = snapshot
        .nodes()
        .iter()
        .map(|n| n.pre_capture_labels.clone())
        .collect();
    engine.lazy_replay_state = Some(LazyReplayState {
        pre_capture_labels,
        new_trigger_atoms,
    });
    engine
}
```

- [ ] **Step 4: Branch save/restore — do NOT include lazy_replay_state**

The branch save/restore (`Snapshot` struct + `save`/`restore`) was extended in Phase 1b T3 to include `snapshot_origin`. The `LazyReplayState` is a read-only contract from `from_snapshot_lazy` — it never changes during decide. Don't add it to the branch Snapshot.

Verify by reading `save`/`restore` and confirming they don't touch `lazy_replay_state`. If they do (e.g., field-by-field clone via derive), explicitly NOT clone it via `..self` or skip.

- [ ] **Step 5: Run tests**

```bash
cargo test -p owl-dl-tableau
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary
```

Expected: all pass. Task 2 is pure machinery — no behavior change yet (no one calls `from_snapshot_lazy`).

- [ ] **Step 6: GALEN soundness gate**

```bash
RUSTDL_SNAPSHOT_CAPTURE=1 timeout 1500 cargo test -p owl-dl-reasoner \
    --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture
```

Expected: FP=0/MISSED=0, wall ~161s (no behavior change).

- [ ] **Step 7: Clippy + fmt + commit**

```bash
cargo clippy -p owl-dl-tableau --all-targets -- -D warnings
cargo fmt -p owl-dl-tableau -- --check
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): HyperEngine::from_snapshot_lazy + LazyReplayState (Phase 1b.5 T2)

Adds the engine-side machinery for lazy expansion: optional
LazyReplayState field holding pre_capture_labels + new_trigger_atoms,
populated by the new from_snapshot_lazy constructor. Existing
from_snapshot remains as the full-re-run path (lazy_replay_state =
None). horn_fixpoint re-seed (Task 3) reads this state to gate
event seeding.

Branch save/restore intentionally does NOT include lazy_replay_state
— it's a read-only contract from the constructor, untouched by
branching.

No behavior change yet (no caller invokes from_snapshot_lazy). GALEN
soundness gate clean.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §4.1

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Gate `horn_fixpoint` re-seed + wire `from_snapshot_lazy` into replay

**Goal:** the load-bearing task. Modify `horn_fixpoint`'s worklist re-seed loop to consult `lazy_replay_state` (when present); update `replay_with_neg_sup` to extract trigger atoms from `neg_sup_clauses` and call `from_snapshot_lazy`. After this task, lazy expansion is wired and GALEN wall should drop.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`horn_fixpoint` re-seed loop guard)
- Modify: `crates/owl-dl-tableau/src/replay.rs` (compute new_trigger_atoms, call from_snapshot_lazy)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`snapshot_lazy_enabled` env helper, default ON)

- [ ] **Step 1: Add `snapshot_lazy_enabled` env helper**

In `crates/owl-dl-reasoner/src/lib.rs`, near `snapshot_capture_enabled`:

```rust
/// Phase 1b.5 lazy expansion toggle. Default ON when
/// `RUSTDL_SNAPSHOT_CAPTURE` is also ON — flag-OFF reverts replay
/// to Phase 1b's full-re-run behavior (correctness equivalent;
/// useful for A/B comparison + debugging). Sibling-style env
/// helper: accepts `=1`/`=true`/`=yes`/`=on`; rejects `=0`/empty/unset.
///
/// Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §4.1
#[must_use]
pub fn snapshot_lazy_enabled() -> bool {
    // Default ON when capture is also ON; OFF otherwise (since lazy
    // requires capture). Explicit RUSTDL_SNAPSHOT_LAZY=0 disables.
    std::env::var_os("RUSTDL_SNAPSHOT_LAZY")
        .map_or(true, |v| v != "0" && !v.is_empty())
}
```

- [ ] **Step 2: Modify `replay_with_neg_sup` to extract trigger atoms + dispatch**

In `crates/owl-dl-tableau/src/replay.rs`, extend the function:

```rust
pub fn replay_with_neg_sup(
    clauses: &[DlClause],
    snapshot: &GraphSnapshot,
    neg_sup_clauses: Vec<DlClause>,
) -> ReplayVerdict {
    if !matches!(snapshot.risk(), BackPropRisk::Safe) {
        return ReplayVerdict::BackPropAborted;
    }
    let mut full_clauses = clauses.to_vec();
    full_clauses.extend(neg_sup_clauses.iter().cloned());

    // Phase 1b.5: extract body-atom class ids from the appended
    // clauses. These are the only triggers that could fire NEW
    // rules during replay; pre-captured labels at snapshot nodes
    // not in this set can have their re-seed events safely skipped.
    let new_trigger_atoms: hashbrown::HashSet<u32> = neg_sup_clauses
        .iter()
        .flat_map(|c| c.body.iter())
        .filter_map(|atom| match atom {
            owl_dl_core::clause::Atom::Class(cid, _) => Some(cid.index()),
            _ => None,
        })
        .collect();

    let mut engine = HyperEngine::from_snapshot_lazy(
        &full_clauses,
        snapshot,
        new_trigger_atoms,
    );
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
```

The change is mechanical: collect trigger atoms from body class atoms; call `from_snapshot_lazy` instead of `from_snapshot`. The verdict mapping is unchanged.

Note: `neg_sup_clauses` is consumed twice — once to extend `full_clauses`, once to compute trigger atoms. Either `iter().cloned()` for the extend (as shown) or compute triggers first then extend. Choose whichever reads cleaner.

If `hashbrown` isn't accessible from replay.rs, use `std::collections::HashSet` — replay rarely runs without snapshot caching, and the perf difference at this size (≤4 trigger atoms typically) is negligible.

- [ ] **Step 3: Modify `horn_fixpoint` re-seed loop**

In `crates/owl-dl-tableau/src/hyper.rs`, locate `horn_fixpoint` (around line 675). The current re-seed loop:

```rust
for c in self.nodes[idx].labels.clone() {
    self.worklist.push(Event::Label(n, c));
}
```

Replace with:

```rust
for c in self.nodes[idx].labels.clone() {
    // Phase 1b.5 lazy expansion guard: skip Event::Label seeding
    // for snapshot-origin nodes whose label `c` was pre-captured
    // AND not a new-clause trigger. The label's effects under the
    // capture-time clause set are already realized in the snapshot;
    // skipping the event saves a redundant rule firing.
    if let Some(ref lazy) = self.lazy_replay_state {
        let was_pre_captured = lazy
            .pre_capture_labels
            .get(idx)
            .is_some_and(|pre| pre.binary_search(&c).is_ok());
        let is_new_trigger = lazy.new_trigger_atoms.contains(&c.index());
        if was_pre_captured && !is_new_trigger {
            continue; // SKIP — Phase 1b.5 lazy-expansion savings.
        }
    }
    self.worklist.push(Event::Label(n, c));
}
```

The `binary_search` on `pre_capture_labels[idx]` relies on labels being sorted by ClassId — which is the existing invariant from `HyperNode::add` (binary_search insertion). Since `pre_capture_labels` is a clone of `labels` at capture time, it's also sorted. If for any reason the invariant doesn't hold, replace with `.contains(&c)` (linear scan) — still sound, slower.

The guard fires ONLY when `lazy_replay_state` is `Some`. Engines built via `Self::new` or `Self::from_snapshot` (full-re-run) have `lazy_replay_state = None` and the loop behaves identically to Phase 1b.

- [ ] **Step 4: Wire env helper into the orchestrator**

`replay_with_neg_sup` always uses `from_snapshot_lazy` now. To support the A/B `RUSTDL_SNAPSHOT_LAZY=0` toggle, branch on the env helper:

In `crates/owl-dl-reasoner/src/lib.rs`'s `SnapshotCache::try_replay` (or wherever `replay_with_neg_sup` is invoked from the reasoner side), keep the call as-is — but the reasoner's `SnapshotCache::try_replay` can choose to skip lazy by passing an empty trigger-atom set OR by calling a new lazy-disabled variant. Simplest:

Either keep replay always-lazy and document that `RUSTDL_SNAPSHOT_LAZY=0` doesn't actually do anything in Phase 1b.5 (defer the toggle to a future phase), OR add a separate `replay_with_neg_sup_full_rerun` function that explicitly calls `from_snapshot` (Phase 1b path).

For Phase 1b.5: keep it always-lazy. The `snapshot_lazy_enabled` env helper exists but is reserved for future use (e.g., if Phase 1c finds a regression and needs to A/B isolate). Document this in the helper's doc-comment + a note in the results doc.

(Alternative: actually wire the toggle. Adds ~10 lines in SnapshotCache::try_replay to pick between `replay_with_neg_sup` and `replay_with_neg_sup_full_rerun`. Worth doing for debuggability.)

**Recommended:** wire the toggle. Add `replay_with_neg_sup_full_rerun` as a thin variant in replay.rs that calls `from_snapshot` (the existing Phase 1b path), and have SnapshotCache::try_replay branch on `snapshot_lazy_enabled()`.

- [ ] **Step 5: Run tests**

```bash
cargo test -p owl-dl-tableau
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary
```

Expected: all pass. Lazy mode is now wired into replay; Phase 0 canary's flag-ON test should exercise it.

If a test fails, the most likely culprit is the binary_search vs labels-not-sorted assumption — fall back to `.contains(&c)` in the guard.

- [ ] **Step 6: GALEN soundness gate**

```bash
RUSTDL_SNAPSHOT_CAPTURE=1 timeout 1500 cargo test -p owl-dl-reasoner \
    --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture \
    2>&1 | tee /tmp/p1b5/galen-t3.log
```

Expected: **FP=0/MISSED=0**, closure 27,997=Konclude. Wall: **expected to drop substantially** — anywhere from 30s (optimistic) to 130s (pessimistic) per the recon. Capture the wall and FP/MISSED numbers in the commit message.

If FP > 0 or MISSED > 0, **STOP** — the lazy guard is over-skipping somewhere. The most likely bug class: a new-clause trigger atom that's not in `new_trigger_atoms` because the trigger atom enumeration missed it. Re-check `replay_with_neg_sup`'s atom extraction.

If FP=0/MISSED=0 but wall is essentially unchanged (within 10% of Phase 1b's 161s), the guard isn't firing — debug by adding a temporary counter `lazy_skips: u64` next to `snapshot_origin` and asserting it's nonzero on GALEN.

- [ ] **Step 7: A/B verification with `RUSTDL_SNAPSHOT_LAZY=0`**

```bash
RUSTDL_SNAPSHOT_CAPTURE=1 RUSTDL_SNAPSHOT_LAZY=0 timeout 1500 \
    cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture \
    2>&1 | tee /tmp/p1b5/galen-t3-full-rerun.log
```

Expected: FP=0/MISSED=0, wall ~161s (matches Phase 1b first-cut). This verifies that flag-OFF reverts to Phase 1b behavior, confirming the toggle works.

- [ ] **Step 8: Clippy + fmt + commit**

```bash
cargo clippy -p owl-dl-tableau --all-targets -- -D warnings
cargo clippy -p owl-dl-reasoner --lib --tests -- -D warnings
cargo fmt -p owl-dl-tableau -- --check
cargo fmt -p owl-dl-reasoner -- --check
git add crates/owl-dl-tableau/src/hyper.rs \
        crates/owl-dl-tableau/src/replay.rs \
        crates/owl-dl-reasoner/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): lazy expansion wired into replay (Phase 1b.5 T3)

Gates horn_fixpoint re-seed: for snapshot-origin nodes,
Event::Label(n, c) is skipped when c was pre-captured AND not in
new_trigger_atoms. replay_with_neg_sup extracts trigger atoms from
neg_sup_clauses' body atoms; calls from_snapshot_lazy.

RUSTDL_SNAPSHOT_LAZY (default ON) toggles between lazy and Phase 1b
full-re-run for A/B isolation. SnapshotCache::try_replay branches
on the env helper.

GALEN soundness gate clean: FP=0/MISSED=0, closure 27,997=Konclude.
GALEN wall (flag ON, lazy ON): <wall> s vs Phase 1b's 161 s (<delta>%).
A/B with RUSTDL_SNAPSHOT_LAZY=0 reverts to Phase 1b ~161s wall.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §4.1

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

Fill in `<wall>` and `<delta>%` from /tmp/p1b5/galen-t3.log.

---

### Task 4: Per-sup `neg_sup_clauses` caching in `SnapshotCache`

**Goal:** T2 reviewer's perf note. Currently `SnapshotCache::try_replay` allocates a 1-element `Vec<DlClause>` for the `fresh_q ⊓ sup → ⊥` clause on every call (~1.86M times on GALEN). Replace with a per-sup cache: one entry per sup column, hit it on every (sub, sup) call. Optional but cheap — bounded improvement, easy code.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`SnapshotCache` adds `per_sup_clauses: DashMap<ClassId, Arc<Vec<DlClause>>>`)

- [ ] **Step 1: Add the cache field**

In `crates/owl-dl-reasoner/src/lib.rs`, extend `SnapshotCache`:

```rust
pub(crate) struct SnapshotCache {
    // ... existing fields ...
    /// Phase 1b.5: per-sup `fresh_q ⊓ sup → ⊥` clauses, lazily
    /// populated. Avoids re-allocating the 1-element Vec on every
    /// try_replay call (~1.86M times on GALEN).
    per_sup_neg_clauses: std::sync::Arc<
        dashmap::DashMap<
            owl_dl_core::ir::ClassId,
            std::sync::Arc<Vec<owl_dl_core::clause::DlClause>>,
        >,
    >,
}
```

Initialize in `SnapshotCache::build`:

```rust
per_sup_neg_clauses: std::sync::Arc::new(dashmap::DashMap::new()),
```

- [ ] **Step 2: Hit the cache in `try_replay`**

Replace the existing `let neg_sup_clauses = vec![...];` in `try_replay` with:

```rust
let neg_sup_clauses_arc = self.get_or_build_neg_sup_clauses(sup);
// replay_with_neg_sup wants Vec<DlClause> (owned), so clone the Arc'd
// vec — clone is shallow (Arc reference bump + Vec clone of 1 element).
let neg_sup_clauses = (*neg_sup_clauses_arc).clone();
```

And add the helper:

```rust
fn get_or_build_neg_sup_clauses(
    &self,
    sup: owl_dl_core::ir::ClassId,
) -> std::sync::Arc<Vec<owl_dl_core::clause::DlClause>> {
    use owl_dl_core::clause::{Atom, DlClause, X};
    if let Some(existing) = self.per_sup_neg_clauses.get(&sup) {
        return existing.clone();
    }
    let clauses = std::sync::Arc::new(vec![DlClause {
        body: vec![Atom::Class(self.fresh_q, X), Atom::Class(sup, X)],
        head: vec![],
    }]);
    self.per_sup_neg_clauses.insert(sup, clauses.clone());
    clauses
}
```

- [ ] **Step 3: Run tests + GALEN soundness gate**

```bash
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary
RUSTDL_SNAPSHOT_CAPTURE=1 timeout 1500 cargo test -p owl-dl-reasoner \
    --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture
```

Expected: 4/4 canary pass; GALEN FP=0/MISSED=0. Wall: marginal improvement (~1-3% on top of T3 lazy expansion; cache hit is cheap but bounded).

- [ ] **Step 4: Clippy + fmt + commit**

```bash
cargo clippy -p owl-dl-reasoner --lib --tests -- -D warnings
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "$(cat <<'EOF'
perf(snapshot): per-sup neg_sup_clauses cache in SnapshotCache (Phase 1b.5 T4)

Avoid re-allocating the 1-element fresh_q ⊓ sup → ⊥ clause vec on
every try_replay call (~1.86M times on GALEN). Per-sup DashMap
keyed by sup ClassId; lazily populated on first query.

Marginal wall improvement; addresses Phase 1b T2 reviewer's perf
note. GALEN soundness gate clean.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Phase 1b.5 results doc + Phase 1c readiness decision

**Goal:** measure final GALEN wall (lazy expansion + per-sup cache); write results doc; recommend Phase 1c green-light or surface blockers.

**Files:**
- Create: `docs/phase1b5-results.md`

- [ ] **Step 1: Run the full Phase 0 net with flag ON (lazy ON)**

```bash
mkdir -p /tmp/p1b5
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
RUSTDL_SNAPSHOT_CAPTURE=1 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude \
    ore_10908_sroiq ore_15672_shoin 2>&1 | tee /tmp/p1b5/soundness-lazy.log
```

Expected: FP=0/MISSED=0 across all three.

- [ ] **Step 2: Run GALEN with lazy ON**

```bash
RUSTDL_SNAPSHOT_CAPTURE=1 timeout 1500 cargo test -p owl-dl-reasoner \
    --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture \
    2>&1 | tee /tmp/p1b5/galen-lazy.log
```

Expected: FP=0/MISSED=0. Wall: hopefully <100s. Capture wall.

- [ ] **Step 3: Re-run notgalen with lazy ON (extends the win)**

```bash
RUSTDL_SNAPSHOT_CAPTURE=1 timeout 2400 cargo test -p owl-dl-reasoner \
    --test konclude_closure_diff --release \
    notgalen_closure_matches_konclude -- --exact --ignored --nocapture \
    2>&1 | tee /tmp/p1b5/notgalen-lazy.log
```

Expected: FP=0/MISSED ≤ 18 (per Phase 7 baseline). Wall: down from ~1170s (per `docs/perf-2026-06-03-konclude-vs-rustdl.md`). Notgalen is the same Horn fragment as GALEN; same architectural lever applies.

If notgalen test isn't named `notgalen_closure_matches_konclude`, grep `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` for the actual test name.

- [ ] **Step 4: Write the results doc**

Create `docs/phase1b5-results.md`:

```markdown
# Phase 1b.5 — lazy expansion results

Run 2026-06-XX at HEAD `<sha>`. Phase 1b.5 wires the lazy expansion
guard into horn_fixpoint re-seeding: snapshot-origin nodes skip
re-seeding Event::Label for pre-captured labels not in new-clause
triggers. Plus per-sup neg_sup_clauses caching.

## Headline

- GALEN classify wall (flag ON, lazy ON): **<wall> s**.
- vs Phase 1b first-cut (full-re-run, flag ON): 161 s → <wall> s
  (Δ <delta>%).
- vs flag-OFF baseline: 150 s → <wall> s (Δ <delta>%).
- Soundness: FP=0/MISSED=0 across Phase 0 net + GALEN.
- Notgalen: <notgalen_wall> s (vs 1170s pre-project Δ <pct>%).

## Measurements

| Fixture | Wall (Phase 1b.5 flag ON, lazy ON) | vs Phase 1b first-cut | vs flag-OFF |
|---|---:|---:|---:|
| alehif-test | <s> | <delta> | <delta> |
| ORE-10908 | <s> | <delta> | <delta> |
| ORE-15672 (Unsafe → no-op) | <s> | <delta> | <delta> |
| GALEN (load-bearing) | <s> | <delta> | <delta> |
| notgalen | <s> | <delta> | <delta> |

Soundness: FP=0/MISSED=0 (notgalen: 18 expected MISSED from
dl-approximation, unchanged).

## Phase 1c readiness

Spec §6 Phase 1c outcome bands:

| GALEN wall | Decision | Status |
|---|---|---|
| ≤ 150 s | Ship + proceed to Phase 2a | <hit/miss> |
| 150-300 s | Ship + mandatory Phase 2 build | <hit/miss> |
| > 300 s (post-tuning) | §A revert | <hit/miss> |

Recommended action: <ship default-on / hold / revert>.

## What landed

- T1 `<sha>`: `pre_capture_labels` on SnapshotNode.
- T2 `<sha>`: `LazyReplayState` + `from_snapshot_lazy` constructor.
- T3 `<sha>`: horn_fixpoint re-seed guard + replay rewiring + RUSTDL_SNAPSHOT_LAZY env toggle.
- T4 `<sha>`: per-sup neg_sup_clauses caching.

## Carry-overs

- `parent`/`parent_role` capture (Phase 1b T1 reviewer's note): not
  needed for Phase 1b.5's lazy expansion; defer to Phase 3 if SROIQ
  workloads need double-blocking restoration.
- T1 instrumentation (pairs_per_sub + wedge_cost_histogram) kept as
  profiling telemetry per recon doc decision.

## Phase 1c plan (next)

If GALEN wall ≤ 150 s (expected per recon), Phase 1c plan flips
`RUSTDL_SNAPSHOT_CAPTURE` and `RUSTDL_SNAPSHOT_LAZY` defaults to ON,
runs the full corpus + soundness gate, writes project-headline
results doc against spec §6 acceptance criteria.

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`.
- Phase 1b.5 recon: `docs/phase1b5-recon.md`.
- Phase 1b.5 plan: `docs/superpowers/plans/2026-06-03-konclude-snapshot-cache-phase-1b5.md`.
- Phase 1b results: `docs/phase1b-results.md`.
```

Fill in all `<placeholders>` with real values from the logs.

- [ ] **Step 5: Commit results doc**

```bash
git add docs/phase1b5-results.md
git commit -m "$(cat <<'EOF'
docs(phase1b5): results — lazy expansion delivers GALEN <wall>s flag ON

Phase 1b.5 lazy expansion + per-sup caching brings flag-ON GALEN
wall from Phase 1b's 161s to <wall>s (Δ <delta>%); vs flag-OFF
150s baseline Δ <delta>%. Soundness clean across Phase 0 net +
GALEN + notgalen.

Phase 1c readiness: <ship/hold> per spec §6 outcome bands.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## After all tasks

Phase 1b.5 complete. If GALEN wall ≤ 150 s, write the Phase 1c plan to flip defaults + project-headline measurement. If wall is 150-300 s, write Phase 2 (Layered global saturation filter) plan. If wall > 300 s post-tuning, write the §A revert + dead-end ledger §19 entry.

Each subsequent plan is its own brainstorm → spec → plan cycle informed by Phase 1b.5 results.
