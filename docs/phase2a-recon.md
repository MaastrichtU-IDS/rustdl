# Phase 2a recon — where does GALEN's 153.70s actually go?

Run 2026-06-03 at HEAD `b907daf` (Phase 2a T1 instrumentation
landed). Recon decides the shape of the Phase 2 Layer 2
implementation: replace the per-class wedge-build cost as spec §5
hypothesized, OR something else entirely.

## Headline

**GO, with re-framing.** Spec §5's hypothesis ("label-cache build is
~30% of wall") is **invalidated** by measurement: label-cache build is
0.2% of GALEN wall (323ms of 155.95s). The actual cost is the
**per-pair snapshot replay path**: 1.86M pair calls × ~1.44ms CPU
each = 2,686 CPU-seconds dominating wall at 17.2× concurrency.

**Empirical kicker**: `rustdl classify --saturation-only` on GALEN
produces 27,997 subsumptions (= Konclude closure) in **0.48 seconds**
— a **325× speedup** vs current default classify (155.95s). The
existing sound-but-incomplete fast path IS already complete on Horn
ontologies; the orchestrator just doesn't use it for them.

Phase 2 Layer 2 reframes as: **"for Horn ontologies, dispatch to
classify_saturation_only as the default"**. Tiny code change;
massive wall savings.

## Per-component wall breakdown (GALEN, default-on, HEAD `b907daf`)

```
# classes: 2748
# fragment: Horn (trust_sat sound by construction; hyper Horn fixpoint is complete)
# subsumption: saturation=27951 tableau=0
# label heuristic: pruned=164599 pass_through=0 misses=0
# wall breakdown ms: label_cache_build=323 snapshot_cache_build=3350 snapshot_replay=2685564 tier_walk=0
wall=155.95s
```

| Component | Measured value | Unit | % of total |
|---|---:|---|---:|
| label_cache_build | 323 ms | wall (single par_iter block) | 0.2% |
| snapshot_cache_build | 3,350 ms | CPU (AtomicU64 sum across workers) | ~2% wall-equivalent |
| **snapshot_replay** | **2,685,564 ms** | CPU (AtomicU64 sum) | **>100% wall-equivalent — dominates** |
| tier_walk | 0 ms | wall (clamped: components > total) | n/a |
| **Total wall** | **155,950 ms** | — | 100% |

### Unit caveat

`label_cache_build_wall_ms` measures wall time of one par_iter block (single Instant wrap).
`snapshot_cache_build_wall_ms` and `snapshot_replay_wall_ms` are CPU time accumulated across
rayon workers (AtomicU64 sums per call). Direct comparison requires dividing CPU by effective
concurrency. Effective concurrency = 2685.564s CPU / 155.95s wall ≈ **17.2× workers**.

CPU breakdown:
- label_cache: ~0.3s CPU (323ms × low concurrency on the par_iter)
- snapshot_build: 3.35s CPU
- snapshot_replay: **2,685s CPU**
- Other (tier walk, label-cache lookup, etc.): residual ~0.6s CPU

**Replay accounts for ~99% of total CPU work.**

## Why this matters for Phase 2 design

Spec §5 wrote (before Phase 1b/1b.5 measurements were available):

> Phase 7 already prunes 89-100% of pairs at the `(C, D)` query site.
> But: it builds a per-class wedge satisfiability call (cost: O(N)
> wedge runs); the label cache fires *during* the orchestrator's walk.
> Layer 2 produces a one-shot ontology-wide filter cheaper than N
> wedge calls. For GALEN the wedge-cache build is currently ~30% of
> the wall (per Phase 7 results); replacing it with one global
> saturation run could cut that further.

Two facts in this design have changed:

1. **Label-cache build on GALEN is NOT ~30% of wall** (it's 0.2%).
   Phase 7's per-class wedge runs are cheap because each individual
   wedge call on GALEN is ~1-2ms (per Phase 1b.5 recon's wedge-cost
   histogram). 2748 wedge calls × 1.5ms / 17.2× concurrency ≈ 0.24s
   wall — matches the measured 0.323s.

2. **Snapshot-replay dominates wall**, not label-cache build. The
   1.86M pair calls (per Phase 1b.5 recon's pairs-per-sub
   distribution) × ~1.44ms each = 2,685s CPU = the full wall budget
   at 17.2× concurrency.

If Layer 2 cuts the pair count from 1.86M to, say, 200k (by proving
89% of subsumptions directly from saturation closure), CPU work drops
~89% → projected wall ~17s. If Layer 2 cuts to zero per-pair verification
on Horn (using saturation as both candidate filter AND verification
oracle), projected wall approaches the saturator-only baseline
(~milliseconds-to-seconds — Konclude does GALEN in 44ms).

## What Layer 2 should actually be

**Reframe: "Horn short-circuit via saturation closure" instead of
"replace per-class wedge build".**

For ontologies classified as `Horn`:
- The hypertableau Horn fixpoint is sound + complete (per the existing
  `# fragment: Horn (trust_sat sound by construction; hyper Horn
  fixpoint is complete)` banner line).
- A global saturation pass produces the complete subsumer closure
  per class.
- The orchestrator can skip the entire per-pair verification loop —
  no label cache build, no snapshot cache build, no replay calls.
  Just emit the closure directly.

For non-Horn ontologies (SROIQ workloads — ore-15672, pizza, etc.):
- Saturation closure is a sound under-approximation (some real
  subsumptions may be missed).
- Use saturation closure as a CANDIDATE filter: subsumptions in the
  closure are confirmed; subsumptions NOT in the closure may still
  hold and need per-pair verification.
- This matches spec §5's original "candidates(C) ⊆ classes" framing.

**On GALEN/notgalen/alehif (Horn), the Horn short-circuit gives
the headline win.** On SROIQ workloads, the candidate-filter gives
incremental savings (smaller pair count for the per-pair loop).

## Break-even projection

**Horn short-circuit (Layer 2-Horn variant)** on GALEN:

- Pre-project saturator-only wall (per `docs/perf-2026-06-03-konclude-vs-rustdl.md`):
  not measured for GALEN directly; Konclude reports 44ms total
  reasoning. Rustdl's saturator should be in the same order.
- Even pessimistic: if rustdl's Horn closure takes 5-10 seconds on
  GALEN's 27,997-class closure, that's a 15-30× wall reduction vs
  current 156s.
- Soundness: provably complete on Horn (already established).

**Layer 2 candidate filter (SROIQ variant)** on ore-10908:

- Current wall: 7.5s.
- Cache-build cost is bounded by Konclude head-to-head reference
  (ore-10908 Konclude classifies in 1.6s).
- Candidate filter could potentially reduce per-pair verification by
  ~50%; wall savings projected ~3-4s if it works.

## Empirical validation: saturation-only on GALEN

Sanity-check: run `rustdl classify --saturation-only` (existing
sound-but-incomplete fast path) on GALEN:

```
# classes: 2748
# fragment: Horn (trust_sat sound by construction; hyper Horn fixpoint is complete)
# subsumption: saturation=27997 tableau=0
# label heuristic: pruned=0 pass_through=0 misses=0
# wall breakdown ms: label_cache_build=0 snapshot_cache_build=0 snapshot_replay=0 tier_walk=0
wall=0.48s
```

**The saturator alone produces 27,997 subsumptions = the full
Konclude-parity closure in 0.48 seconds — a 325× speedup vs the
current default classify (155.95s).**

This validates the recon's recommendation with high confidence:
the Horn short-circuit isn't "develop a new global saturation
component"; it's **"for Horn ontologies, dispatch to the existing
`classify_saturation_only` path"** (which is already sound + complete
for Horn).

Phase 2b becomes a very small change: detect Horn at classify-start,
dispatch to the existing fast path. Soundness is proven by construction
(the path is already sound on Horn; the only change is making it the
default for Horn instead of opt-in).

## Recommendation

**GO with the Horn short-circuit framing for Phase 2b.**

Phase 2b implementation plan should:

1. Detect Horn fragment at classify-start (existing
   `analyze_fragment` already does this, per
   `crates/owl-dl-reasoner/src/classify.rs:100`).
2. For Horn ontologies, take a new fast-path:
   - Run the existing saturator to produce the full closure (the
     `crates/owl-dl-saturation` crate already does this).
   - Skip `classify_top_down_internal`'s per-pair loop entirely.
   - Emit closure as the classification result.
3. Verify on GALEN + notgalen + alehif: FP=0/MISSED=0, wall
   massively reduced.
4. Non-Horn ontologies stay on the existing per-pair path
   (Phase 1b/1b.5/1c behavior unchanged).

The Horn short-circuit is **architecturally simpler than spec §5's
Layer 2 design** because the saturator is already complete on Horn —
no need for a new saturation-over-TBox component. It's literally
"detect Horn, dispatch to saturator, skip the rest".

**Risk:** the existing saturator may not produce the full closure
in the format the orchestrator expects, OR it may have edge cases
on GALEN-style Horn-with-existentials. The implementation needs to
verify: (a) saturator handles GALEN's complete closure (27,997
subsumptions), (b) closure can be emitted directly as
`Classification`, (c) GALEN soundness gate stays clean.

## What instrumentation kept

The four wall-breakdown fields from T1 (commit `b907daf`) are useful
diagnostic telemetry going forward — keep them. They formalize as
profiling fields rather than "Phase 2a recon-only instrumentation".
Future ontology profiling sessions can reuse the banner output without
re-instrumenting.

Phase 2b implementation should explicitly NOT revert these fields.

## What's deferred

- **notgalen + sio-stripped breakdowns**: if Phase 2b lands on GALEN
  with the projected savings, run the same instrumentation to validate
  notgalen + sio-stripped show similar Horn-short-circuit wins.
- **Per-class wedge-cost ratio measurement**: a microbenchmark of
  snapshot-build vs replay-call cost to validate the lazy expansion
  ratio assumption. Not blocking Phase 2b.
- **Layer 2 SROIQ-variant**: the candidate-filter design for SROIQ
  workloads. Scope to a separate Phase 2c plan if Phase 2b Horn
  variant doesn't also handle SROIQ.

## Why the spec's hypothesis was wrong

Spec §5 assumed per-class wedge calls dominate wall because pre-snapshot,
the wedge ran ON EVERY PAIR (no caching). With Phase 7 label cache,
the wedge runs ONCE per class (for label-cache build) and the per-pair
loop hits the wedge only when label cache passes through. With Phase 1b
snapshot cache, the per-pair wedge work IS the snapshot replay —
not a separate cost.

The current GALEN breakdown shows: label-cache build (the only
per-class wedge call) is cheap; replay (1.86M per-pair calls) dominates.

Spec §5 was written before this state existed. The recon's job was
to verify the spec's assumption against reality; it's wrong, so Phase 2b
re-targets.

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md` §5.
- Phase 1b.5 recon (showed pairs-per-sub flat distribution + wedge cost histogram):
  `docs/phase1b5-recon.md`.
- Phase 1c results (GALEN 153.70s baseline): `docs/phase1c-results.md`.
- Phase 2a recon plan (this work): `docs/superpowers/plans/2026-06-03-konclude-snapshot-cache-phase-2a-recon.md`.
- Pre-project Konclude head-to-head (GALEN at Konclude's 44ms): `docs/perf-2026-06-03-konclude-vs-rustdl.md`.
- Existing fragment detection: `crates/owl-dl-reasoner/src/classify.rs:100` (`analyze_fragment`).
- Existing pure-EL short-circuit (similar pattern Phase 2b extends):
  `crates/owl-dl-reasoner/src/classify.rs:624` (`classify_pure_el`).
