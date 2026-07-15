# Fix #2 Layer A (wedge semantic branching) — findings (2026-07-15)

**Plan:** `docs/superpowers/plans/2026-07-15-wedge-semantic-branching-layerA.md`
**Spec:** `docs/superpowers/specs/2026-07-15-wedge-semantic-branching-design.md`
**Branch:** `feat/wedge-semantic-branching`. **Flag:** `RUSTDL_SEMANTIC_BRANCHING`, **default OFF**.

## What shipped

Layer A: at the wedge `⊔` decision (`hyper.rs` `solve`), when the flag is on, drop
each atomic `Class(c,v)` disjunct whose resolved landing node already carries a
told-disjoint label `e` (via `disjoint_pairs`) — that disjunct would clash on the
next `horn_fixpoint` pass. If all die → `Unsat`; exactly one survives → unit-force
it without a decision level (same-depth recursion); else → branch the filtered
`live`. **Verdict-preserving by construction; no negative/exclusion state (that is
Layer B).**

**Dep-set discipline (soundness-critical).** Every dep-set Layer A emits is a
SUPERSET of the flag-OFF engine's, or backjumping is unsound → false `Unsat` → FP
subsumption. Per dropped disjunct we fold `decision_deps ∪ deps(e)` (or `DepSet::ALL`
when the node is `nn_tainted`, mirroring `fire_clause`) into `prune_deps`, which:
- seeds the empty-live `Unsat` (`body_deps ∪ prune_deps).remove(d)`),
- attributes the unit-force (`body_deps ∪ prune_deps.remove(d)`, no decision `d`),
- **and seeds the branch loop's `combined`** — the survivors-remain fix (below).

## Bugs found and fixed during implementation (both were latent FP hazards)

1. **Survivors-remain backjump (found via the pizza identity gate; the advisor's
   original flag only covered the empty-live case).** When Layer A prunes SOME but
   not all disjuncts and the survivors are then branched and all fail, the branch
   loop must fold the pruned disjuncts' clash deps into `combined` — flag-OFF
   branches-and-clashes them and folds their `child_deps`. Starting `combined =
   EMPTY` narrowed the propagated `Unsat` dep-set → unsound backjump past an
   ancestor decision → skipped Sat sibling → false `Unsat`. Symptom: **pizza flag-ON
   reported 14 unsatisfiable classes vs 2** (Caprina, Margherita, … spuriously
   unsat). Fix: `combined = prune_deps`. Ground-truth probe (`SB_VERIFY`) confirmed
   every individual drop genuinely clashes — the bug was purely dep-propagation.
2. **Landing-node resolution (advisor latent-gap catch).** The prune read labels on
   `self.resolve(t0)` unconditionally, but `add_label` resolves through the merge
   union-find only when `inverse_func_merge` is on. With `RUSTDL_INVERSE_FUNC_MERGE=0`
   and a `≤n` merge, the disjunct could land on `t0` while `e` was read on
   `resolve(t0)` — different nodes, so the same-variable disjointness clause need not
   fire → unsound drop. Fix: resolve iff `self.inverse_func_merge` (mirror `add_label`).

Both are now regression-guarded (canaries + curated byte-identity under both merge
settings).

## Soundness gate — GREEN (the deciding gate, per the advisor)

- **Non-Horn FP oracle `ore_ont_13723` (Konclude∩HermiT), the designed FP tripwire:**
  OFF and ON both **FP=0 / MISSED=0, closure 10166 = 10166 (byte-identical).**
- **Curated byte-identity (classify OFF vs ON), FP-sniff = unsat-class count:**

  | fixture | verdict lines | unsat | OFF vs ON |
  |---|---|---|---|
  | galen | 3309 | 0 | byte-identical |
  | notgalen | 4111 | 0 | byte-identical |
  | sio | 1617 | 0 | byte-identical |
  | wine (pair-ms 250) | 201 | 0 | byte-identical |
  | ore-15672-shoin | 75 | 0 | byte-identical |
  | ore-10908-sroiq | 759 | 0 | byte-identical |
  | alehif | 51 | 0 | byte-identical |
  | pizza (merge ON) | 314 | 2 | byte-identical |
  | pizza (merge OFF) | — | 2 | byte-identical |

- **Unit canaries** (`crates/owl-dl-tableau/tests/semantic_branching.rs`, all proven
  discriminating via RED/GREEN bug-injection): `prunes_dead_disjunct_and_forces_survivor`,
  `ancestor_placed_clash_does_not_trigger_unsound_backjump` (empty-live subset hazard),
  `survivors_remain_prune_dep_prevents_unsound_backjump` (hand-built, bypasses the
  clausifier's static disjunct-elimination). CLI differential gate:
  `crates/owl-dl-cli/tests/semantic_branching_identity.rs`.

## `ore_ont_10019` measurement (the point of Layer A)

47 classes; `--pair-timeout-ms 250`, `RUSTDL_AGGREGATE_DEADLINE_MS=90000`:

| | OFF | ON |
|---|---|---|
| subsumption (saturation/tableau) | 136 / 12 | 136 / 12 |
| hyper-proven pairs | 9 | 9 |
| timed-out (incomplete) pairs | 1253 | 1253 |
| tier_walk wall | 78.7 s | 78.2 s |

**Layer A moves nothing on `ore_ont_10019` — identical verdicts, ±0.5 % wall.** This
is the plan's explicit prediction, not a failure: the reactive `horn_fixpoint`
already catches disjoint co-occurrence a pass later, so Layer A's local pruning
duplicates work it already does and does not cure the H2 disjunctive-DFS thrash. The
mechanism firing is established by the canaries; the point of Layer A was to land +
validate the mechanism, gate, and dep discipline for Layer B.

## Decision

**Layer A: DONE, sound (verdict-preserving, FP=0 incl. the non-Horn oracle), default
OFF.** Do NOT flip default-ON on its own — it is corpus-invisible (no verdict change,
no measured speedup).

**Proceed to Layer B** (separate plan) — the per-node `excluded: Vec<ClassId>`
exclusion set with the **`Unsat`-only-exclusion invariant** (never exclude a merely
`Stalled` sibling — the reuse-trap FP hazard). Layer B is the intended mover:
asserting `¬Dⱼ` after a sibling's clean `Unsat` propagates through the 55 disjointness
axioms, collapsing downstream disjunctions to unit. The `ore_ont_10019` GO/NO-GO
(decide ≥ ~half of the stalled classes within the Konclude/HermiT budget) is measured
after Layer B. The dep-discipline and landing-node lessons here directly de-risk
Layer B, whose exclusion state is the same hazard family one level worse.

## Non-blocking notes (for the Layer B build / final review)

- Unit-force recurses at the SAME depth with no depth-cap backstop (only blocking +
  deadline bound it). Sound, but a unit-force chain on a blocking-non-convergent input
  can run to the per-pair deadline where the normal path would hit `depth==0 → Stalled`
  sooner. Watch for this reading Layer B timings.
- `max_branch_depth` does not grow across unit-forces, so `is_diverging`'s
  `depth_saturated` clause may under-fire on unit-force-heavy searches. Perf-only.
