# Incremental functional/≤1 merge in the Horn fixpoint — design (v2, post-advisor)

**Date:** 2026-07-11
**Status:** revised after advisor review (`.superpowers/sdd/advisor-review.md`); pending user approval → implementation
**Depends on / supersedes:** `2026-07-11-funcmerge-inverse-completeness-design.md` (the gated
`distinct_role_succ` inverse-successor counting) and its deferred HF3 note.

## Problem (from the critical analysis)

rustdl decides galen (a **Horn** ontology) via `solve` → `horn_fixpoint` (deterministic)
→ `solve_at_most`/`partition_rec` (nondeterministic `≤n` search). Functional/`≤1` merges
are *entailed* (deterministic) but are routed through the **nondeterministic search
layer**: `find_open_at_most` (`hyper.rs:2600`, which does **not** filter `n==1`) →
`solve_at_most:2507` → `partition_rec:2538`, which for the single forced `≤1` partition
still does `save():2550` → `merge:2557` → **`solve(depth-1):2564`** (restarting
`horn_fixpoint` with a full-graph reseed, `1495–1530`) → `restore:2570`. Consequences on
galen once inverse-induced successors are counted so the merges fire:

1. **O(depth × graph) blowup** — a cascade of *D* forced merges = *D* nested `solve`
   frames, each re-deriving the whole closure (~O(K³) match attempts measured).
2. **Wasted full-graph clones** — `save()` is a full `self.nodes.clone()`; one per entailed
   (never-undone) merge.
3. **Premature `Stalled` = incompleteness** — the deterministic cascade exhausts the 256
   nondeterministic-search `depth` cap and bails `Stalled` (`solve:2221`), so galen's
   subsumptions are missed even though they are deterministically derivable.

Root framing: **a Horn problem should need no backtracking search, but functional merges
force it into the search layer and are then throttled by that layer's budget.** The
saturation kernel applies functional-merge incrementally (why the *acyclic* `funcmerge`
repros pass via saturation), and — decisively — the wedge itself already merges
incrementally in `horn_fixpoint` for nominals via `apply_nn_rule` (`hyper.rs:3298`). This
design generalizes that existing in-fixpoint-merge pattern to `≤1`/functional.

## Goal

galen classifies **fast and complete** by default (MISSED 10 → 0, seconds), preserving
soundness (corpus FP = 0), the `≤n` (n ≥ 2) nondeterministic search, ⊔-branching +
dependency-directed backjumping, and blocking-based termination.

## Design (advisor-recommended "safer alternative")

Three coupled changes in `crates/owl-dl-tableau/src/hyper.rs`. The key architectural
decision (per advisor §4/alternative): **do NOT physically rewrite a folded node's
in-edges (the invasive HF3). Keep merges physically "root-successor-only" and make all
readers resolve-on-read.** This sidesteps the whole stale-reference BLOCKER.

### A. Fire `≤1`/functional merges incrementally inside `horn_fixpoint`

`≤1` (incl. `Functional(R)` lowered to `≤1 R`) is deterministic: two successors under it
are entailed equal. Handle it as a completion rule reached from `process_event`, NOT from
`solve`:

- **Triggers (both required).**
  1. *Successor-added:* on `Event::Edge` giving node `x` a role-successor, if `x` has an
     active `≤1` on that role (from `x.at_most`, honoring the role hierarchy) and
     `distinct_role_succ(x, r, qual)` now has ≥ 2 resolved elements → merge.
  2. *Constraint-added:* **`Atom::AtMost` currently emits no event** (`hyper.rs:3000–3024`),
     so a node that acquires a `≤1` *after* already having ≥ 2 successors would be missed
     once `find_open_at_most` is narrowed to n ≥ 2. Add a re-check at constraint-add time
     (emit an event, or check inline when the `at_most` set gains an `n==1` entry).
- **Act.** Merge the ≥2 successors in place via `merge_with_cause` — **no `save`/`restore`,
  no `solve` recursion** — exactly as `apply_nn_rule` already does. The merge re-queues the
  survivor's edge/label events, so the cascade runs to a proper fixpoint within the single
  `horn_fixpoint` call.
- **Clash.** If the forced successors are pairwise `must_be_distinct` (≠-forced or
  disjoint-labelled), the `≤1` is unsatisfiable → merge/`add_label` fires the `⊥`-clause →
  `horn_fixpoint` returns `Unsat` (same verdict `forced_distinct_exceeds` gives, reached
  incrementally).
- **Scope.** Only `n == 1`. Narrow `find_open_at_most` / `solve_at_most` to **n ≥ 2**
  (genuine nondeterminism) unchanged; a residual `n==1` there becomes a defensive
  assert/no-op.

### B. Resolve-on-read in the hot paths (instead of predecessor-edge rewriting)

`≤1` merges fold nodes reached via `preds` — including a predecessor/root. Today
`merge_with_cause` redirects only outgoing edges and relies on `solve`'s full-graph
**reseed** to make stale in-edges harmless; dropping that reseed (change A) removes the
precondition. Rather than physically redirect every reference to a folded node (advisor
§4 enumerated the miss set: stale `preds`, stale worklist events, folded nodes left in
`block_index`, `add_label` onto a folded node — a fragile "find them all" task), make the
**readers resolve**:

- Add `self.resolve(...)` (with **path compression** in `resolve` for amortized cost) at
  the hot read sites that currently assume merges are root-successor-only:
  `enumerate_matches` (`~2907–2923`, whose docstring states that assumption),
  `fire_exists` (`~3143`), the back-prop predecessor iteration (`~1568/1603`), and
  `process_event`'s node dispatch / `add_label` target.
- With resolve-on-read, a folded node's identity flows to its representative at every use;
  no `preds`/`block_index`/worklist scrubbing is required, so B is a **bounded audit of
  reader sites**, not an open-ended structure rewrite. (The full predecessor-aware merge
  stays deferred as HF3, to be revisited only if a fixture defeats resolve-on-read.)
- **Survivor policy.** Merge into the **older / lower-index** node (`survivor = min(a,b)`),
  keeping the root (node 0) and ancestors as survivors — stabilizes `root_labels()` and
  minimizes churn.

### C. Backjumping deps + save/restore

- **Merge dep-set (soundness).** Do NOT attribute a `≤1` merge the narrow "union of both
  nodes' births ∪ active decision level" (advisor §6: omits the `≤1`'s own `at_most_dep`
  and the successor-establishing deps → risk of an **unsound backjump** on non-Horn
  inputs). Use the existing `card_clash_deps` (`~1120–1156`) or, conservatively,
  `DepSet::ALL`. (Moot for galen — Horn, no decisions — but required for the non-Horn
  corpus's soundness.)
- **save/restore (already correct — reframed).** `save()` already deep-clones `nodes`
  (incl. `preds`/`edges`/`at_most`/`parent`), `representative`, `neq`, and `block_index`;
  `restore()` reinstates them — so in-fixpoint `≤1` merges inside a ⊔-branch already roll
  back. **No change needed here.** The perf win comes entirely from eliminating the nested
  `solve` frames and their per-merge reseed — *not* from cheaper snapshots.

## Termination

Provide a concrete measure (advisor §3): order states by the lexicographic pair
(number of canonical/representative nodes ↓, then the standard blocking-bounded
completion measure). Every `≤1` merge strictly decreases the canonical-node count (a
union-find union). Node *creation* remains bounded by blocking; a merge can union labels
and thereby re-enable an `∃` or (under double-blocking) unblock a dependent, but each such
step either creates a node (bounded by blocking's finite completion-graph bound) or does a
merge (strictly decreasing canonical nodes), and the two cannot alternate unboundedly
because merges are monotone and node creation is capped. Keep `FIXPOINT_ITERS` as a
backstop and assert it is not hit on the corpus.

## Soundness & completeness

- **Sound:** `≤1` merge is entailed; resolve-on-read preserves R-relationships (a folded
  node's edges/labels are read via its representative); ⊔-branch `restore()` rolls merges
  back; merge dep-set is conservative (`card_clash_deps`/`ALL`). Precedent: `apply_nn_rule`
  already merges in-fixpoint soundly.
- **Complete (for this class):** the deterministic cascade runs to fixpoint instead of
  bailing `Stalled`; both triggers (A) ensure no `≤1` violation is missed; `≤n` (n ≥ 2)
  semantics unchanged.
- **Hard gates:** corpus-wide FP = 0 (konclude_closure_diff, every fixture); galen MISSED
  10 → 0; no new MISSED on any corpus ontology; galen terminates in seconds; no material
  wall regression; clippy `-D warnings` + fmt clean.

## Rollout / gating

Implement behind the existing `RUSTDL_INVERSE_FUNC_MERGE` flag so `main`'s default path is
untouched until proven. Once all gates pass, flip the default ON (retire the flag or
default-on with an escape hatch); `completeness_guaranteed()` ⟹ Horn ⟹ MISSED = 0 becomes
true for galen again.

## Test plan

- **RED→GREEN:** `funcmerge_inverse` (A ⊑ Y / A ⊑ Z) passes at default once defaulted on; a
  scaled **K-ring** cyclic fixture classifies in ~linear time (guards against the O(K³)
  regression).
- **Unit:** `distinct_role_succ` inverse-count = 2; an incremental-`≤1`-merge test where a
  node with a forward + an inverse successor merges them inside the fixpoint and the label
  reaches the predecessor; a **constraint-added** trigger test (add `≤1` to a node that
  already has 2 successors → merge fires); resolve-on-read tests for the advisor's cases:
  folded-node-as-blocker, -as-parent, -with-pending-worklist-event, -receiving-a-late-label.
- **Regression:** `konclude_closure_diff` FP = 0 / MISSED unchanged-or-better on every
  present fixture; galen via the matrix harness MISSED 10 → 0 with finite wall; wall sanity
  on wine/sio/pizza.

## Files

- `crates/owl-dl-tableau/src/hyper.rs` — `process_event`/`horn_fixpoint` (incremental `≤1`
  trigger, mirroring `apply_nn_rule`), constraint-add trigger for `Atom::AtMost`,
  `find_open_at_most` (narrow to n ≥ 2), the resolve-on-read sites (`enumerate_matches`,
  `fire_exists`, back-prop, event dispatch, `add_label`), `resolve` (path compression),
  `merge_with_cause` cause dep-set (`card_clash_deps`/`ALL`). `distinct_role_succ` is
  already inverse-aware behind the flag.
- `crates/owl-dl-reasoner/tests/funcmerge_inverse.rs` + a new K-ring scaling test.
- `crates/owl-dl-tableau/src/lib.rs` — flag handling / eventual default flip.

## Advisor review outcome (summary)

Verdict: **proceed with specified changes.** Adopted the advisor's safer alternative
(resolve-on-read, no in-edge rewrite → the §4 BLOCKER becomes a bounded reader audit).
Folded in the required fixes: constraint-add trigger (§2), conservative merge dep-set
(§6), a concrete termination measure (§3), corrected `block_index`/save-restore framing
(§4/§5), and the `apply_nn_rule` precedent. Full review: `.superpowers/sdd/advisor-review.md`.
Residual watch-items for implementation: confluence under inverse+nominal interactions,
and the termination measure holding under double-blocking unblocking.
