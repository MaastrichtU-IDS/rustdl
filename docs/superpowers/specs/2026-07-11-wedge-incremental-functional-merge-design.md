# Incremental functional/≤1 merge in the Horn fixpoint — design

**Date:** 2026-07-11
**Status:** design under advisor review; pending implementation
**Depends on / supersedes:** `2026-07-11-funcmerge-inverse-completeness-design.md` (the gated
`distinct_role_succ` inverse-successor counting) and its deferred HF3 note.

## Problem (from the critical analysis)

rustdl's hypertableau wedge decides galen (a **Horn** ontology) via `solve` →
`horn_fixpoint` (deterministic) → `solve_at_most`/`partition_rec` (nondeterministic
`≤n` search). Functional/`≤1` merges are *entailed* (deterministic) but are routed
through the **nondeterministic search layer**: `partition_rec` treats the single
forced `≤1` partition as a speculative branch — `save()` a full-graph snapshot, merge,
**recurse into `solve(depth-1)`** (which restarts `horn_fixpoint` from scratch),
`restore()`. Consequences on galen (once inverse-induced successors are counted so the
merges actually fire):

1. **O(depth × graph) blowup** — a cascade of *D* forced merges = *D* nested `solve`
   frames, each re-deriving the whole closure (~O(K³) match attempts measured).
2. **Wasted full-graph clones** — a save/restore per entailed (never-undone) merge.
3. **Premature `Stalled` = incompleteness** — the deterministic cascade exhausts the
   256 nondeterministic-search `depth` cap and bails `Stalled`, so galen's
   subsumptions are missed even though they are deterministically derivable.

Root framing: **a Horn problem should need no backtracking search, but functional
merges force it into the search layer and are then throttled by that layer's budget.**
The saturation kernel already applies functional-merge as an incremental completion
rule (why the *acyclic* `funcmerge` repros pass via saturation); the wedge must do the
same.

## Goal

Make galen classify **fast and complete** (MISSED 10 → 0, terminates in seconds) by
default, while preserving soundness (corpus FP = 0), the `≤n` (n ≥ 2) nondeterministic
search, ⊔-branching + dependency-directed backjumping, and blocking-based termination.

## Design

Three coupled changes in `crates/owl-dl-tableau/src/hyper.rs`.

### A. Fire `≤1`/functional merges incrementally inside `horn_fixpoint`

`≤1` (including `Functional(R)` lowered to `≤1 R`) is deterministic: two successors
under it are entailed equal. Handle it as a **completion rule in `process_event`**, not
in `solve`:

- **Trigger.** When `process_event` adds a role edge to a node `x` (the `Event::Edge`
  path) — or a merge/redirect gives `x` a new successor — check each active `≤1`
  constraint on `x` (role `r`, from `x.at_most` with `n == 1`, honoring the role
  hierarchy). If `distinct_role_succ(x, r, qual)` now has ≥ 2 elements, the merge is
  forced.
- **Act.** Merge the (≥2) successors into one **in place** via `merge_with_cause`
  (change B), with **no `save`/`restore` and no `solve` recursion**. `merge_with_cause`
  already re-queues `Event::Edge`/label events for the survivor, so the fixpoint
  continues incrementally and the cascade runs to a proper fixpoint within the single
  `horn_fixpoint` call.
- **Clash.** If the forced successors are pairwise `must_be_distinct` (≠-forced or
  disjoint-labelled), the `≤1` is unsatisfiable → the merge returns clash →
  `horn_fixpoint` returns `Unsat` (same verdict the current `forced_distinct_exceeds`
  pre-check produces, just reached incrementally).
- **Scope.** Only `n == 1`. `find_open_at_most` / `solve_at_most` continue to handle
  `≤n` for **n ≥ 2** (genuine nondeterminism) unchanged. After this change, a violated
  `≤1` should never reach `solve` (the fixpoint resolves it first); `find_open_at_most`
  is narrowed to `n ≥ 2` (and may keep an `n == 1` case as a defensive assert/no-op).

### B. Predecessor-aware merge (the deferred HF3 — now a prerequisite)

Incremental `≤1` merges fold nodes reached via `preds` — including a predecessor/root
(in `funcmerge`, `x`'s two g-successors are the witness `m` and `x`'s own predecessor
`A`). The current `merge_with_cause` redirects only the folded node's **outgoing**
edges and relies on `save`/`restore` + full reseed to mask stale in-edges; incrementally
that is unsafe. Extend `merge_with_cause(survivor s_i, folded s_j)` to also:

- **Redirect in-edges.** For each `(r, p) ∈ s_j.preds`: rewrite `p`'s outgoing edge
  `(r, s_j)` to `(r, s_i)`, append `(r, p)` to `s_i.preds`, and re-queue `Event::Edge(p, r, s_i)`.
- **Update blocking bookkeeping.** Fix any `BlockingSummary` (parent / parent_role /
  label_sig) that references `s_j` to reference `s_i`; if `s_j` was some node's blocker
  or parent, repoint it. Recompute/invalidate affected blocking so no node stays blocked
  by a now-merged-away node.
- **Fold constraint sets** (`at_most`, `at_least_done`) and labels as today, plus ensure
  `s_i` re-checks its own `≤1` constraints after absorbing `s_j`'s successors (which may
  cascade another merge — handled by the re-queued events).
- **Survivor choice.** Prefer keeping the **older / lower-index** node as survivor
  (`s_i = min(a, b)`), so the root (node 0) and ancestors survive rather than being
  folded — keeps `root_labels()` and predecessor structure stable and minimizes
  in-edge churn. (`root_labels()` is already `resolve()`-safe as a backstop.)

### C. Snapshot/restore + backjumping correctness for in-fixpoint merges

`≤1` merges now happen inside `horn_fixpoint`, which runs within `solve`'s ⊔-branches.
Two invariants to preserve:

- **Restore rolls back merges.** `save()`/`restore()` must capture and restore the full
  merge state — the `representative` union-find, redirected `preds`, and blocking
  bookkeeping — so a restored ⊔-branch fully undoes any `≤1` merges it performed.
  (Merges are part of graph state; verify `save` clones it and `restore` reinstates it.)
- **Dependency tracking.** A `≤1` merge performed under active ⊔-decisions must carry
  those decisions' `DepSet` so a clash it later causes backjumps correctly. Thread the
  current decision dep-set into the incremental merge's `merge_with_cause` cause, mirroring
  how `card_clash_deps` / branch merges attribute deps today. Conservatively, a merge may
  carry the union of the deps of the two merged nodes' births plus the active decision
  level.

## Termination

Each `≤1` merge strictly reduces the number of canonical (representative) nodes; node
*creation* is bounded by blocking (galen node count is capped ~1099); re-queued events
are finite (bounded by edges × labels). So the incremental fixpoint reaches a proper
fixpoint and terminates without consuming the nondeterministic `depth` budget. The
`FIXPOINT_ITERS` cap remains as a backstop only.

## Soundness & completeness

- **Sound:** a `≤1` merge is entailed by `Functional`/`≤1`; predecessor redirection
  preserves the R-relationships; ⊔-branch restore rolls merges back. No new FP.
- **Complete (for this class):** the deterministic cascade now runs to fixpoint instead
  of bailing `Stalled`, so galen's 10 are derived. `≤n` (n ≥ 2) semantics unchanged.
- **Hard gates:** corpus-wide FP = 0 (konclude_closure_diff, every fixture); galen
  MISSED 10 → 0; no new MISSED on any corpus ontology; galen terminates in seconds; no
  material wall regression on the rest of the corpus.

## Rollout / gating

Implement behind the existing `RUSTDL_INVERSE_FUNC_MERGE` flag during development (so
`main` default is untouched until proven). Once all gates pass — galen fast + complete,
FP = 0 corpus-wide, no wall regression — flip the default ON (make the incremental merge
the default path and retire the flag, or default-on with an escape hatch). The
completeness contract (`completeness_guaranteed()` ⟹ Horn ⟹ MISSED = 0) becomes true
again for galen.

## Test plan

- **RED→GREEN:** `funcmerge_inverse` (A ⊑ Y / A ⊑ Z) passes at default (no flag) once
  defaulted on; a scaled K-ring cyclic fixture classifies in ~linear time.
- **Unit:** `distinct_role_succ` inverse-count = 2; an incremental-merge unit test where
  a `≤1` on a node with a forward + an inverse successor merges them inside the fixpoint
  and propagates the label to the predecessor; a predecessor-redirection unit test
  (folded node's in-edges point to survivor).
- **Regression:** `konclude_closure_diff` FP = 0 / MISSED unchanged-or-better on every
  present fixture; galen via the matrix harness MISSED 10 → 0 with a finite wall; wall
  sanity on wine/sio/pizza; clippy `-D warnings` + fmt clean.

## Files

- `crates/owl-dl-tableau/src/hyper.rs` — `process_event` (incremental `≤1` trigger),
  `merge_with_cause` (predecessor-aware redirection + blocking fix), `find_open_at_most`
  (narrow to n ≥ 2), `save`/`restore` (verify merge-state capture), `distinct_role_succ`
  (inverse-aware, already implemented behind the flag).
- `crates/owl-dl-reasoner/tests/funcmerge_inverse.rs` + a new K-ring scaling test.
- `crates/owl-dl-tableau/src/lib.rs` — flag handling / eventual default flip.

## Risks (for the advisor to scrutinize)

1. **Backjumping correctness** — do incremental merges carry the right `DepSet` so ⊔
   backjumping stays sound/complete? Is the conservative dep union safe?
2. **Predecessor redirection completeness** — is `preds` + blocking-summary + at_most/
   at_least + union-find the *complete* set of structures referencing a folded node? A
   missed reference → unsoundness or a dangling node.
3. **Termination under merge-triggered ∃ generation** — can a merge re-enable an ∃ that
   creates a node that triggers another merge in a way blocking doesn't bound?
4. **Restore fidelity** — does `save`/`restore` already deep-capture the union-find +
   redirected preds, or must it be extended (and at what cost — the diagnosis flagged
   save/restore as a full clone)?
5. **Blocking interaction** — merging a blocked node, or a node that is another's blocker;
   does blocking stay sound (no lost successors) and terminating?
6. **Is `≤1`-in-fixpoint actually confluent** — could different merge orders reach
   different fixpoints? (For deterministic `≤1` it should be confluent, but the inverse/
   nominal interactions deserve a check.)
