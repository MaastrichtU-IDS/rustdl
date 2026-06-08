# Reuse-trap (approach A) — scoping + go/no-go plan

Drafted 2026-06-08. The user chose to pursue **A** (the model-reuse
generalization) from the option set in
`reuse-trap-nominal-termination-scoping-2026-06-07.md`. This doc scopes the
first increment (A1) and states the go/no-go that gates building it. A is the
*independent* lever — it does NOT fix the wine wall (that's architectural, §2
rewrite; see the other doc's GO/NO-GO), but it's the HermiT-style global-classify
win on ontologies where model construction converges.

## What A is

The snapshot cache (`crates/owl-dl-reasoner/src/lib.rs`, `SnapshotCache`;
`crates/owl-dl-tableau/src/{snapshot.rs,replay.rs}`) reuses a per-class
satisfiability model to answer many `(C, *)` subsumption pairs by *replaying*
`¬D` against the cached model instead of a cold wedge/tableau. Today it is gated
to `BackPropRisk::Safe` (inverse/nominal/cardinality-free ≈ Horn) ontologies. A
generalizes reuse to the non-Horn fragment.

## The load-bearing finding: the runtime sentinel is DEAD CODE

`replay_with_neg_sup` (`replay.rs:69`) has TWO soundness guards: (1) the
structural `snapshot.risk() == Safe` check, and (2) the runtime sentinel
`engine.snapshot_backprop_aborted()`. **Guard (2) never fires:** the only method
that sets the flag, `add_label_via_backprop` (`hyper.rs:722`), is
`#[allow(dead_code)]` and *no production path calls it* (its own comment: "Phase
1b ships the infrastructure but no production code path invokes it yet — Phase 3
will hook this at the inverse-role / nominal / cardinality back-prop sites"). And
`GraphSnapshot.risk()` is a Phase-1a **placeholder always stamped `Safe`**. So
**the ontology-wide `BackPropRisk::Safe` gate (in `SnapshotCache::build`) does
ALL the soundness work today** — the sentinel is unbuilt scaffolding.

That is exactly why the 2026-06-07 spike (force `SnapshotCache` risk = `Safe`,
bypassing the only live guard) produced **FP=100 on pizza** (§hypertableau-
dead-ends §19/§20 + the spike): unsound reuse proceeds silently because the
sentinel that should abort it was never wired.

## A1 — the sound-first increment

Wire a **complete** back-prop sentinel: during replay, ANY state mutation into a
snapshot-origin node sets the abort flag → `replay` returns `BackPropAborted` →
the orchestrator falls through to the wedge. This is **FP-safe by construction**
(abort = no claim) — *provided the trigger is complete*. Then loosen the
ontology gate (per-class or sentinel-only) and the live sentinel keeps FP=0.

## The two cruxes that gate building A1 (measure BEFORE building — advisor)

**Crux 1 — soundness is completeness over an OPEN set of mutation sites.**
"Abort on any back-prop into a snapshot node" is FP-safe only if EVERY
mutation path into such a node is hooked: label-add **and** node-merge **and**
edge-add (the dead sentinel only contemplated label-add). A missed site does not
fail loud — it ships an FP. So A1's soundness is not "wire one hook"; it's "prove
the hook set is complete." This is the crux the gate exists to avoid.

**Crux 2 — the payoff may be empty, and is already measured small.** A complete
sentinel only helps if it aborts *rarely* on non-Horn ontologies. But:
- §19 measured the per-class-reuse payoff at **~10%** on ore-15672 (tier_walk is
  dominated by the §18 hard-class search cluster, which reuse can't touch).
- §20 *implemented* per-class gating and found it **unsound (1 FP on ore-10908,
  the `sup`'s definition embeds cardinality) AND a perf regression on ore-15672**
  (snapshot dispatch cost on SROIQ classes).
- A1's ONLY edge over §20's structural per-class gate is **abort-on-actual-
  back-prop** (runtime) vs **abort-on-structural-risk** (static). That edge is
  real only if many non-Horn pairs *don't actually back-prop at runtime* even
  though their classes are structurally Unsafe.

## The decisive go/no-go measurement (RUNNING 2026-06-08, worktree agent)

Count (do NOT abort) back-prop events into snapshot-origin nodes during replay,
by type (label / merge / edge), in the gate-forced-open config, on pizza +
ore-10908 + ore-15672. Report, per ontology:
- the **fraction of replayed pairs with ZERO back-prop events** = the sound-reuse
  ceiling a complete sentinel would preserve;
- the **event-type breakdown** = the exact hook set a complete sentinel needs
  (resolves crux 1's "open set").

**Decision rule:**
- zero-event fraction ≈ 0 (most pairs back-prop) → **A1 is dead** (complete
  sentinel aborts the pairs that matter; hard soundness work for ~nothing). Bank
  it; the only remaining wins are §2 (rewrite) or C (pivot).
- zero-event fraction meaningful AND those pairs' verdicts are FP-free vs oracle
  → **A1 is alive**; then design the complete sentinel (crux 1) and re-measure
  FP=0 + wall corpus-wide.

(Results to be appended here when the agent completes.)

## Pointers
- `crates/owl-dl-tableau/src/replay.rs` (replay + verdict), `hyper.rs`
  (`add_label_via_backprop` ~722, `snapshot_origin`, `from_snapshot_lazy`,
  `merge`, edge-add), `snapshot.rs` (`BackPropRisk`, `GraphSnapshot`).
- `crates/owl-dl-reasoner/src/lib.rs` (`SnapshotCache::build`/`try_replay`,
  `classify_class`).
- `docs/hypertableau-dead-ends.md` §19/§20 (per-class gating: ~10% payoff,
  unsound + regressing); the snapshot spec §4.2 (Inv-1) / §4.3 (sentinel).
- Memory: [[snapshot-gate-loosening-dead-end]],
  [[next-big-bet-reuse-trap-nominal-termination]].
