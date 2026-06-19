# Wedge backtracking trail (SP2) — design

**Date:** 2026-06-19
**Status:** approved (brainstorming session 2026-06-19)
**Program:** "Konclude-class engine" (sub-project 2)
**Predecessors:** [SP1 spec](2026-06-18-wedge-declared-inverse-symmetric-design.md)
(shipped, merged `ae13521`). Attribution memory:
`sp2-perf-attribution-2026-06-19`.

---

## 1. Program context + the measured bottleneck

Goal: close the Konclude perf gap. SP2 orientation (2026-06-19) **re-measured the
corpus** (the prior perf memories were stale) and found the gap has **collapsed to a
few SROIQ-hard outliers**: ore-15672-shoin **138s**, wine **1991s** (54s with
`--pair-timeout-ms 25`), family stall. Everything else is ≤2s. Konclude does all in
**ms**.

**Decisive single-pair attribution** (`rustdl hyper-sat ore-15672`): the exploders are
`e-usage-situation` + `e-interaction-situation` (not `epistemic-workflow-enactment`,
whose own sat is 2.4ms). For `e-usage-situation`:

```
Stalled  branches=60858  (disj=60858 merge=0)  restores=60858  depth=256
match_attempts=88,100,256   node_clones=88,635
```

Reading:
- **Pure disjunctive branching** (`merge=0` ⟹ not cardinality; `is_blocked` not hot
  ⟹ not blocking; only **17** disjunctive clauses ⟹ not an absorption problem —
  Approach "stronger absorption" was **refuted** by `clausify_with_stats` and
  `tbox-stats`).
- `restores == branches` ⟹ the search exhaustively explores failing disjunctive
  combinations of a SAT class (the satisfying model exists; the search can't find it
  cheaply).
- **The dominant cost is per-branch save/restore.** The wedge has **no trail**: at
  every `⊔` choice point it clones the *entire* graph (`Snapshot` in `hyper.rs`;
  `node_clones == branches`) and the Horn fixpoint re-propagates (88M match attempts).
  The main tableau solved this years ago with `TableauTrail` (`trail.rs`), whose own
  module doc states: *"Snapshotting the entire graph at every ⊔ choice point is
  O(graph) per branch — fatal. A flat trail records O(1) per mutation and rolls back
  by replaying in reverse; cost proportional to changes since the checkpoint, not
  graph size."* **The wedge is doing the exact thing that doc calls fatal.**

## 2. Goal & non-goals

### Goal

Replace the wedge's whole-graph `Snapshot` save/restore with a **log-and-undo trail**
(modeled on the proven `TableauTrail`), so per-branch cost is **O(changes since the
checkpoint)** instead of **O(graph)**. Target: materially cut the wall on the
disjunctive-branching outliers (ore-15672, wine, family) while preserving every
verdict and FP=0.

### Non-goals

- **Reducing the branch *count*** (the 60,858 branches). That is the separate, harder,
  research-grade lever (search heuristics / sub-search caching / conflict learning —
  prior NO-GO, bjgap≈1). SP2 reduces per-branch *cost*, not count. If the trail proves
  insufficient (branch count is itself unbounded), that lever becomes a future
  sub-project — but the constant-factor win is banked regardless.
- **Soundness/verdict changes.** The trail is pure save/restore mechanics; verdicts
  must be byte-identical. FP=0/MISSED=0 corpus-wide is the gate.
- **Main tableau, saturator.** Wedge-only (`hyper.rs`).

### Ship/revert criterion

Ships iff: the P0 gate passes (ore-15672 wall materially reduced), **verdicts are
identical** on every corpus fixture (FP=0/MISSED=0, closure byte-identical), and the
wedge's own test suite is green. If the trail does not move the wall (branch count
dominates after all), record the finding and revert per the model-caching-plan
criterion.

## 3. P0 GATE (measure before committing to the full build)

The one open question: *is cheaper-per-branch enough, or is the 60,858 branch count
itself the wall?* The first plan task must answer it cheaply, before the full trail is
built out:

1. **Estimate the ceiling.** Profile `e-usage-situation`'s probe (samply, available)
   and attribute the 5000ms: fraction in graph-clone (`Snapshot::clone`) +
   re-propagation vs. essential search. If clone+re-derive is the large majority, the
   trail's ceiling is high.
2. **Bound the branch count.** Run `e-usage-situation` with the depth cap raised and a
   long deadline (or `node_clones`/branch instrumentation) to learn whether the search
   *terminates* at a bounded branch count (trail will then crack it) or grows
   unboundedly (trail gives only a constant factor, and Lever C is also needed).

**Gate decision:** proceed with the full trail build iff (1) shows clone+re-derive is
the dominant cost AND (2) the branch count to a verdict is bounded (or the constant
factor alone takes ore-15672 under, say, a few seconds). Otherwise stop, record, and
re-scope toward branch-count reduction.

## 4. Architecture

Port the `TableauTrail` pattern (`crates/owl-dl-tableau/src/trail.rs`) to the
`HyperEngine`.

### What the `Snapshot` currently captures (must all be trail-logged)

From `hyper.rs` `struct Snapshot`: `nodes: Vec<HyperNode>` (labels + edges + preds +
birth/parent), `representative: Vec<HNode>` (merge union-find), `neq: Vec<(HNode,
HNode)>` (inequalities), `block_index: Option<HashMap<Role, Vec<HNode>>>`, `origin:
Vec<bool>` (snapshot-origin sentinel bits). The engine flag `snapshot_backprop_aborted`
is deliberately *not* restored (sticky for the whole query) — the trail must preserve
that exclusion.

### The trail

- A `HyperTrail` holding a `Vec<HyperTrailEntry>` with a `Checkpoint` marker variant
  (exactly the `TableauTrail` shape). Entry variants, one per mutation kind:
  - `LabelAdded(HNode, ClassId, DepSet?)` — undo: remove the label.
  - `EdgeAdded(HNode src, Role, HNode tgt)` — undo: pop from `src.edges` + `tgt.preds`.
  - `NodeCreated(HNode)` — undo: truncate `nodes`/`representative`/`origin` back (new
    nodes are always appended; rollback truncates the tail).
  - `Merged(HNode, prev_representative)` — undo: restore the union-find slot.
  - `NeqAdded(HNode, HNode)` — undo: pop `neq`.
  - `BlockIndexAdded(Role, HNode)` — undo: pop the block-index vector.
  - `OriginSet(HNode, prev_bool)` — undo: restore the bit.
- `checkpoint()` pushes a `Checkpoint`; `rollback_to(cp)` replays entries in reverse
  until the marker. `Cost = O(entries since cp)`, not `O(graph)`.

### The single mutation chokepoint

`TableauContext` is the main tableau's "only sanctioned mutation interface." The wedge
needs the analogous discipline: **every** label/edge/node/merge/neq/block/origin
mutation during a branch must push a trail entry. Audit the existing mutation sites
(`add_label`, the edge pushes at the `derive_role_edge`/∃-generation sites, `new_node`,
the merge/union-find writes, `neq` pushes, `block_index` inserts, origin writes) and
route each through a logged helper. This is the bulk of the work and the soundness-
critical part: a *missed* trail entry = incorrect rollback = wrong verdict (could be an
FP). The corpus verdict-identity gate is the backstop.

### Branch driver change

At each `⊔` choice point (`find_open_disjunction` / the branch recursion ~`hyper.rs`
1226+): replace "clone graph → try disjunct → on fail restore clone" with "`checkpoint()`
→ apply disjunct (logged) → propagate incrementally → on fail `rollback_to(cp)`". The
incremental propagation (worklist seeded only by the disjunct's new atoms) replaces the
full re-seed; confirm the worklist is already delta-driven (the `Event` queue) so
rollback + re-propagation stays incremental.

### Files

- `crates/owl-dl-tableau/src/hyper.rs` — the trail type (or a new `hyper_trail.rs`
  module), the logged mutation helpers, the branch driver swap, removal of `Snapshot`
  clone-save/restore.
- (No core/reasoner API change — internal to the wedge.)

## 5. Soundness argument

The trail is **pure save/restore mechanics** — it changes *how* a failed branch is
reverted, not *what* the calculus derives. Correctness reduces to one invariant:
**every mutation since a checkpoint is logged and exactly undone on rollback** (the
graph after `rollback_to(cp)` is bit-identical to the graph at `checkpoint()`). This is
the same contract `TableauTrail` already upholds for the main tableau. The risk is
purely implementation (a missed/incorrect entry), caught by:
- the wedge's existing unit suite (124 tests incl. backjump/merge fixtures), and
- the **corpus verdict-identity gate**: every fixture's closure must be byte-identical
  to the pre-SP2 result (FP=0/MISSED=0), not merely FP=0.

## 6. Testing & gates

1. **Verdict-identity unit tests** — a save/restore differential: for a set of
   synthetic disjunctive ontologies, assert the trail-based engine's verdict +
   resulting graph equal the clone-based engine's (run both, compare) before the clone
   path is removed.
2. **P0 wall measurement** — `hyper-sat ore-15672`: `e-usage-situation` /
   `e-interaction-situation` wall + `node_clones` (should drop ~to zero) + total wall.
3. **Corpus closure-identity net** — FP=0/MISSED=0 AND byte-identical closures across
   galen, notgalen, sio, wine, ore-10908, ore-15672, alehif, ro, pizza, bibtex
   (`konclude_closure_diff`). Any verdict change → revert.
4. **Wedge suite** (`cargo test -p owl-dl-tableau`) green; clippy/fmt clean.
5. **Perf non-regression** on the already-fast fixtures (galen/sio): the trail must not
   slow the common case.

## 7. Decomposition (the build, after the P0 gate)

1. **P0 measurement** (§3) — go/no-go.
2. `HyperTrail` type + entry variants + checkpoint/rollback + unit tests (no wiring).
3. Logged mutation helpers + audit every mutation site (the soundness-critical bulk),
   behind a flag, with the clone path still default.
4. Branch driver swap + verdict-identity differential tests.
5. Flip default + corpus closure-identity net + P0 wall + perf non-regression;
   accept/revert.
6. Remove the `Snapshot` clone path once the trail is proven.

## 8. Open questions for implementation

- Is the Horn fixpoint already delta-driven per branch (so rollback + re-propagate is
  incremental), or does it re-seed all nodes? If the latter, the trail must be paired
  with incremental re-propagation to realize the full win (the 88M match attempts).
  The P0 profile (§3.1) settles which cost dominates.
- `block_index` and `origin` rollback: confirm these are append-only since the
  checkpoint (truncation undo) or need per-entry logging.
