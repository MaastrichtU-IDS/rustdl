# Conflict-driven no-good learning (CDBL) — implementation plan

Drafted 2026-05-27 after the blocking investigation
([`pizza-convergence-diagnosis.md`](pizza-convergence-diagnosis.md))
identified search-branching as pizza's bottleneck and CDBL as the
lever. Multi-week; this file tracks the design across sessions.
Mirrors the [`lazy-unfolding-plan.md`](lazy-unfolding-plan.md)
phase format.

## Goal

Make the tableau *learn* from clashes so it stops re-exploring the
same local conflict in different sub-trees. Pizza's
disjoint-topping structure is a constraint-satisfaction problem;
SAT solvers crush those with clause learning, and the DL-tableau
analog is conflict-driven no-good learning.

## What exists today, and why it's disabled

`TableauContext` carries `learned_nogoods: Vec<(NodeId,
ConceptId, ConceptId, DepSet)>` plus `record_nogood` /
`nogood_blocks` (lib.rs:164-361). The lookup is **not wired into
`search::branch`** because the key is unsound (search.rs ~140):

> The naive "precond ⊆ active ⇒ skip" rule is unsound on pizza —
> verdict went 2 unsat → 0 unsat. Two no-goods recorded in
> different sub-trees can fire jointly at a node that's actually
> sat.

Two distinct problems with the existing key `(node, or_label,
disjunct, branch-id-precond)`:

1. **Node-identity keying doesn't transfer.** A no-good recorded
   at `NodeId(7)` can't apply to a structurally-identical
   `NodeId(19)` reached in another sub-tree — so it rarely fires
   (low value), and when the same `NodeId` *is* reused across
   rollback for a different logical node, it fires *wrongly*
   (unsound).
2. **Branch-id preconditions are run-local indices**, not
   structural facts. `{branch 5}` means "the 5th decision",
   which after backtracking refers to a different decision.

## The sound formulation: label-set no-goods

A no-good should be a **structural** claim:

> The set of concept labels `{L1, ..., Lk}` co-occurring at a
> single node, given this TBox, is unsatisfiable.

This is sound to reuse **anywhere** that label-set recurs,
exactly like a learned SAT clause — *provided the clash that
justified it was derived from those labels alone*, not from the
node's edges / successors / nominal identity.

That proviso is the crux. A clash can be:
- **Node-local**: `{CheeseTopping, MeatTopping}` co-occur and a
  `DisjointClasses` rule derives `⊥` from the node's own labels.
  Sound to generalise to a label-set no-good.
- **Edge-dependent**: the node has `∃hasTopping.X`, the successor
  clashes, and back-jumping attributes the failure here. The
  label-set alone (without the successor structure) does **not**
  reproduce the clash, so generalising is **unsound**.

## Labels-as-evidence (the 1-UIP cut)

To tell the two apart we need provenance: for each label, *why*
was it added.

- Today: `TrailEntry::LabelAdded { node, concept }`.
- Extend: record the *cause* — the set of (label, or edge) that
  triggered the rule which added this label. This is a
  derivation edge; following causes back from a clash to the
  decision labels is the "1-UIP cut" of CDCL.

With provenance, clash explanation becomes:

1. Clash at node `n`: labels `C` and `¬C` present.
2. Walk causes of `C` and of `¬C` back to their roots.
3. If every root is a *decision label* on `n` itself (a chosen
   disjunct) and no cause traversed an edge → the no-good is the
   set of those decision labels, node-local, sound to reuse.
4. If any cause traversed an edge / successor → record nothing
   (or record an edge-qualified no-good, a later phase).

## Phases

**Phase 1 (this session): safe foundation, no lookup.**
- This plan doc.
- A `decision_labels` map on `TableauContext`: `branch_id →
  (NodeId, ConceptId)` recording which disjunct concept each
  branch decision asserted. `push_branch` already exists;
  `branch()` will register the (node, disjunct) when it asserts a
  choice.
- A `clash_decision_labels(clash_deps) -> Vec<ConceptId>`
  translator: map the branch-ids in a clash's `DepSet` to the
  disjunct *concepts* they chose. This is the structural,
  transferable form of the existing branch-id explanation.
- Unit tests: a constructed branch/clash yields the right
  decision-label set.
- **No** change to `search::branch` behaviour — the translator is
  built and tested in isolation. Verdicts unchanged.

**Phase 2 (next session): provenance + node-local detection.**
- Extend the trail with per-label causes.
- A `clash_is_node_local(n) -> bool` check via cause-walking.
- Record label-set no-goods only for node-local clashes.

**Phase 3: sound lookup + measurement.**
- Wire a label-set no-good check into `branch()`: before
  asserting disjunct `D` at node `n`, if `L(n) ∪ {D}` is a
  superset of a known node-local no-good, skip `D` (sound).
- Measure pizza / SIO / family. Revert per
  [`moms-plan.md`](moms-plan.md) §A if no wall movement.

**Phase 4: edge-qualified no-goods** (optional) — generalise to
clashes that involve successor structure, keyed on a richer
fingerprint. Only if Phase 3 plateaus.

## §A — Phase 2b/3 integration attempt (2026-05-27): sound but 0 hits

Wired the full record + lookup on top of the Phase 1/2a
primitives and gated on the pizza regression:

- **Recording**: at a clash where `my_id` contributed, translate
  `clash_deps` → disjunct concepts via `clash_decision_labels`,
  run `verify_node_local_clash` on the set, and if it clashes
  node-locally, store it as a label-set no-good.
- **Lookup**: before asserting disjunct `D` at node `N`, skip `D`
  if `S ⊆ L(N) ∪ {D}` for some recorded no-good `S` containing
  `D`.

**Soundness held** — pizza still reported exactly 2 unsat
(`CheeseyVegetableTopping`, `IceCream`), matching HermiT. This is
the key result: the label-set + node-local-verify design is sound
where the original branch-id CDBL was not (that one went 2 → 0).

**But it was ineffective — 0 lookup hits.** Debug counters on the
NamedPizza sat probe: 22 no-goods recorded (sizes 3-5), **zero**
lookup hits. Walls flat (pizza 29 s, family 6.3 s).

### Why 0 hits — the keying bug

`clash_decision_labels(clash_deps)` gathers the disjunct concepts
from *every* branch id in the clash's dependency set. Those
decisions were made at **different nodes** across the search tree
(a clash at node `N` can depend on a disjunct chosen at an
ancestor and propagated via `∀`). `verify_node_local_clash` then
puts them all on *one* isolated node and — correctly — finds they
clash there. So the no-good is *sound* ("these labels co-located
⇒ clash") but **never matches**, because in the real search no
single node ever accumulates that cross-node set.

### The fix: node-keyed no-goods

The no-good must be the **clash node's own decision labels**, not
the whole dependency chain's. That requires:

1. Propagate the clash *node* up through `search`/`branch` (today
   `SaturationResult::Clash(node, deps)` carries it, but the
   conversion to `SearchVerdict::Unsat(deps)` drops the node).
2. At record time, intersect the decision concepts with
   `L(clash_node)` — keep only decisions actually present at the
   clash node. That set genuinely co-occurs there, so the no-good
   can match when the same labels recur at another node.
3. (Still gate with `verify_node_local_clash` for the
   edge-dependence check.)

### Status

Reverted the 2b/3 wiring; kept the tested primitives
(`clash_decision_labels`, `verify_node_local_clash`,
`record_decision`). Phase 2b/3 redone with node-keyed no-goods is
the next increment — and note that even a *hitting* CDBL is still
bounded by the timeout-bound nature of pizza/SIO: pruning
branches *within* a pair's search may not make the pair converge
inside the 200 ms budget. The honest expectation is that CDBL,
done right, helps **convergent** workloads (like lazy unfolding
helped family) more than the timeout-bound ones.

## Soundness invariants (enforced by tests every phase)

- All ≥260 in-tree unit tests pass.
- 87-fixture differential corpus: zero verdict diff.
- Real-corpus regression: pizza/sio/family/ro/sulo/go unsat sets
  match HermiT-via-ROBOT. **This is the gate the existing
  branch-id CDBL failed** (2 unsat → 0 unsat on pizza); the
  label-set + node-local design must pass it.

## Why this is the right multi-week bet

- Reuses the existing `learned_nogoods` scaffolding and the
  backjumping `DepSet` machinery — not a from-scratch build.
- Smaller than hypertableau (which replaces the expansion
  algorithm); CDBL adds learning on top of the current tableau.
- Targets the measured bottleneck (branching), not a guess.
- Well-understood: it's CDCL clause learning, adapted. The DL
  adaptation (node-local vs edge-dependent clashes) is the only
  novel part, and Phase 1/2 isolate exactly that.
