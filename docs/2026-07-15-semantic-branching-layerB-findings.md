# Fix #2 Layer B (semantic branching via exclusion set) — findings + GO/NO-GO (2026-07-15)

**Plan:** `docs/superpowers/plans/2026-07-15-wedge-semantic-branching-layerB.md`
**Spec:** `docs/superpowers/specs/2026-07-15-wedge-semantic-branching-design.md` §Layer B.
**Branch:** `feat/wedge-semantic-branching`. **Flag:** `RUSTDL_SEMANTIC_BRANCHING`, default OFF.

## Verdict: **NO-GO** on the `ore_ont_10019` success criterion → **bound-the-tail / defer** (the spec's pre-authorized fallback). Layer A + B are sound, land clean, and are banked default-OFF; the headline goal (decide ≥ ~half of `ore_ont_10019`'s 33 stalled classes) is **not** met.

## What shipped (commit `dd77605`)

Per-node `excluded: Vec<(ClassId, DepSet)>`: when a prior sibling `⊔` disjunct
returns a **clean `Unsat`**, exclude its class on the landing node before trying
later siblings (the sound case-split `D₁|D₂|D₃ → D₁|¬D₁∧D₂|¬D₁∧¬D₂∧D₃`). Excluded
classes are dead in the Layer A filter (→ prune/force) and clash if re-derived
(`process_event(Event::Label)` hook). Rides the whole-node-clone `Snapshot`
(no `trail.rs` change); frame-scoped.

**Soundness invariant (reuse-trap family) — honored:** exclude ONLY on a clean
`Unsat`, NEVER `Stalled` (structurally — exclusion added only in the `Unsat` arm).
Exclusion carries the sibling's Unsat clash deps `child_deps`; the manufactured
clash unions `deps_of(re-derived c) ∪ child_deps` (superset backjump). Landing node
resolved iff `inverse_func_merge`. Merge-transfer of `excluded` deliberately NOT
built (a lost exclusion is a MISS, never an FP — advisor; stalled classes are
`merge=0`).

## Soundness gate — GREEN

- **Non-Horn FP oracle `ore_ont_13723`** (Konclude∩HermiT): FP=0 / MISSED=0,
  closure 10166 = 10166 byte-identical, OFF and ON.
- **Curated byte-identity (classify OFF vs ON), unsat-class FP-sniff:** galen
  (3309, u0), notgalen (4111, u0), sio (1617, u0), ore-15672 (75, u0), ore-10908
  (759, u0), alehif (51, u0), pizza (314, u2), wine (u0) — all **byte-identical**.
- **Non-vacuity:** Layer B fires heavily where disjunctions live — **pizza: 31,376
  exclusions** (byte-identical, FP=0), **`ore_ont_10019`: 77,842 exclusions**. So
  FP=0 is a real result, not a vacuous pass. (`ore_ont_13723` fires only 1 — it is
  nearly-Horn, so pizza is the load-bearing FP fixture for Layer B.)
- **Canaries** (`tests/semantic_branching.rs`): `layer_b_exclusion_collapses_
  downstream_disjunction` (mover: exclusion drives a downstream unit-force) and
  `layer_b_never_excludes_a_stalled_sibling` (the FP tripwire — proven
  discriminating: injecting "exclude on Stalled" flips the verdict). Plus the 3
  Layer A canaries.

## `ore_ont_10019` GO/NO-GO measurement (47 classes; Konclude 90 ms / HermiT 360 ms for all)

| per-pair budget | flag | subsumption (sat/tab) | hyper-proven | timed-out pairs |
|---|---|---|---|---|
| 250 ms | OFF | 136 / 12 | 9 | 1253 |
| 250 ms | ON | 136 / 12 | 9 | 1253 |
| 2000 ms (agg 240 s) | OFF | 125 / 6 | 9 | 1626 |
| 2000 ms (agg 240 s) | ON | 125 / 6 | 9 | 1626 |

**Byte-identical OFF vs ON at every budget** — Layer B decides **zero** additional
classes, despite firing **77,842 exclusions**. The exclusions collapse local
disjunctions correctly but do not reduce the stall: the H2 disjunctive-DFS thrash
is whole-graph state-space re-exploration (`revisit_frac ≈ 1.0`, per the diagnosis),
which local semantic branching does not touch. This matches the advisor's ~40%
estimate landing on the No side and the established diagnosis (SP2 no-goods DEAD,
backjump-precision ruled out): **the remaining cure is whole-model caching / CDCL
clause-learning, which is out of scope for its reuse-trap FP surface.**

## Decision (per the spec's go-no-go)

1. **Fix #2 does not close `ore_ont_10019`.** Layer A + B are a genuine, sound,
   well-tested wedge capability (in-search BCP + semantic branching, FP=0 incl. the
   non-Horn oracle) but are **corpus-invisible** (no verdict change) and **do not
   move the dense-SROIQ disjunctive tail**. Keep them **default-OFF** — do not flip
   default-ON (zero benefit, added search cost from 78k exclusions).
2. **Take bound-the-tail:** make the `Stalled → NoVerdict → search.rs` fallthrough
   return sound-incomplete *fast* on the pathological dense-SROIQ tail (some ORE
   onts hang; the SP2 sweep found 4 timeout onts), so the classifier degrades
   gracefully instead of burning the per-pair/aggregate deadline. This is a separate,
   smaller piece of work; scope it next.
3. **Document the deferral:** the dense-SROIQ disjunctive tail (exemplar
   `ore_ont_10019`) needs Konclude-class whole-model caching / conflict-driven
   learning to close, deferred on FP-surface grounds (reuse-trap-A1 /
   snapshot-cache soundness fix). This is a legitimate, evidence-backed outcome —
   every cheap lever (SP1 throughput, SP2 no-goods, MRV, backjump-precision,
   Fix#2 A+B) is now measured out.

## Branch state

`feat/wedge-semantic-branching`: Layer A (sound, verdict-preserving) + Layer B
(sound, verdict-neutral) committed, default-OFF, full curated + non-Horn-oracle
FP=0. Nothing pushed (shared org repo — the user's call). Recommend: keep as a
banked, sound capability; do not merge to `main` as default-ON.
