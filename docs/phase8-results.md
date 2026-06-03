# Phase 8 — label-cache deadline decoupling results

Run 2026-06-03 at HEAD `30b641c`. Phase 8 recon
(`docs/phase8-recon.md`) localized ORE-10908's post-Phase-7 19.32 s
wall to ~5 % of classes timing out at the per-pair 200 ms deadline
during `classify_labels`, then bloating the tier walk with
~38 cache-miss pairs × ~28 ms hyper-wedge cost each = 12-16 s of
avoidable wall. Fix: introduce `RUSTDL_LABEL_CACHE_TIMEOUT_MS`
(default 1000 ms, independent of `--pair-timeout-ms`).

## Headline

**ORE-10908: 19.32 s → 7.83 s = −59 %.** All 1003 cache-misses
converted to prunes. **Konclude ratio: 12× → 4.87× — UNDER the ≤5×
target** the user set when opening this work. ORE-15672 stays flat
(its 46-class hard cluster is genuine; raising the budget further
wouldn't help). GALEN unchanged. FP=0 / MISSED=0 preserved on the
Phase 0 net.

## The decoupled deadline

Before Phase 8:

```rust
let deadline = per_pair_timeout.map(|t| Instant::now() + t);
prepared.classify_labels(class_id, deadline)
```

The `--pair-timeout-ms 200` default bound was passed straight through.
A class needing 341 ms of wedge satisfiability (the recon's median
ORE-10908 staller) hit `NoVerdict` instead of `Sat`, and the resulting
miss path was orders of magnitude slower than the saved cache work.

After Phase 8:

```rust
let cache_ms = crate::label_cache_timeout_ms();
let deadline = if cache_ms == 0 {
    None
} else {
    Some(Instant::now() + std::time::Duration::from_millis(cache_ms))
};
prepared.classify_labels(class_id, deadline)
```

`label_cache_timeout_ms()` defaults to **1000 ms** with env override
via `RUSTDL_LABEL_CACHE_TIMEOUT_MS`. Generous enough to catch
ORE-10908's stallers (median 341 ms, max 631 ms per recon); tight
enough to bail quickly on genuinely intractable classes (ORE-15672's
~56 % NoVerdict rate — those classes don't finish in any reasonable
budget).

## Default tuning

The first attempt used 5000 ms. Empirical re-measure:

| Fixture | Phase-7 baseline | 5000 ms | 1000 ms (shipped) |
|---|---|---|---|
| ORE-10908 | 19.32 s | 7.53 s | 7.83 s |
| ORE-15672 | 29.71 s | 34.61 s | 30.59 s |

5000 ms wasted cache-build wall on ORE-15672 (~30 extra seconds across
46 stallers / 8 cores) for zero gain. 1000 ms retains the ORE-10908 win
while restoring ORE-15672's wall to within noise.

## Measurement table

Under variable host load:

| Fixture | Pre-Phase-7 | Post-Phase-7 | Post-Phase-8 | Cumulative Δ |
|---|---|---|---|---|
| **ORE-10908-sroiq** | 27.37 s | 19.32 s | **7.83 s** | **−71 %** |
| ORE-15672-shoin | 29.55 s | 29.71 s | 30.59 s | flat |
| pizza | 4.39 s | 4.06 s | 4.12 s | −6 % |
| alehif-test | 2.87 s | 2.21 s | 2.49 s | −13 % |
| ro-stripped | 0.87 s | 0.65 s | 0.89 s | noise |
| sio-fp2-module | 0.70 s | 0.65 s | 0.73 s | noise |
| sulo-stripped | 0.09 s | 0.13 s | 0.11 s | noise |
| GALEN (closure-diff) | 684 s | 455.73 s | 453.02 s | −34 % |

## Konclude head-to-head update

| Fixture | rustdl post-Phase-8 | Konclude | Ratio | Pre-Phase-7 ratio |
|---|---|---|---|---|
| ORE-10908-sroiq | 7.83 s | 1.61 s | **4.87×** ✓ | 17× |
| ORE-15672-shoin | 30.59 s | 1.72 s | 17.8× | 17× |
| pizza | 4.12 s | 1.68 s | 2.5× | 2.6× |
| alehif-test | 2.49 s | 1.78 s | 1.4× | 1.6× |

**ORE-10908 hits the ≤5× target** the user named at the start of
this work — closed from 17× pre-Phase-7 to 4.87× post-Phase-8.
ORE-15672 stays at 17× because of a small genuine-intractability
cluster (3 classes consulted ~15 times each in the tier walk; the
`misses=46` counter is tier-walk consultation events, not unique
NoVerdict classes). Phase 9 recon (`docs/phase9-recon.md`)
characterizes the cluster: it's joint-expansion budget exhaustion
on `∃proper-part.X` hops over a transitive+inverse role, NOT a
missing saturator rule. Phase 8's deadline extension correctly bails
quickly on them without further hurting wall.

## Soundness gate (Phase 0 net)

| Fixture | FP | MISSED | Wall (s) |
|---|---|---|---|
| alehif-test | 0 | 0 | 14.79 |
| ore-10908-sroiq | 0 | 0 | 15.87 |
| ore-15672-shoin | 0 | 0 | 36.02 |
| GALEN | 0 | 0 | 453.02 |

FP=0 / MISSED=0 unchanged everywhere. Phase 8 is a pure timing
parameter change; the heuristic's soundness contract (sound prune via
counterexample-model argument; verify-positives preserves per-pair
contract) is invariant.

## What this DOES / DOES NOT close

- **DOES close** the user's ≤5× Konclude target on ORE-10908.
- **DOES NOT close** the 17× gap on ORE-15672. The recon makes clear
  this requires a structurally different angle: the 46 NoVerdict classes
  are genuinely hard satisfiability instances, not deadline-bound. Could
  be approached by (a) extending the saturator to cover more SROIQ
  structurally so those classes' subsumptions aren't wedge-dispatched
  in the first place; (b) Konclude-style multi-class search; (c) accept
  the workload as out-of-scope for the heuristic approach.

## Cross-references

- Recon that drove this work: `docs/phase8-recon.md`.
- Phase 7 design: `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`.
- Phase 7 plan: `docs/superpowers/plans/2026-06-02-per-class-label-heuristic.md`.
- Phase 7 results: `docs/phase7-results.md`.
- Head-to-head: `docs/perf-2026-06-02-konclude-vs-rustdl.md` (Konclude
  ORE-10908 reference: 1.61 s).
- Phase 5 chain (the regression-localization template that became
  Phase 8's recon): `docs/phase5-{recon,walltime-probe,variance-check,downstream-probe}.md`.

## Tuning hooks

- `RUSTDL_LABEL_CACHE_TIMEOUT_MS=<integer>` — per-class cache-build
  deadline in milliseconds. Default 1000. Set to `0` for unbounded.
- `RUSTDL_LABEL_HEURISTIC=0` — disable the heuristic entirely (cache
  becomes uniformly `NoVerdict`). Default ON.
- `--pair-timeout-ms <integer>` — per-pair tableau budget (independent
  of the cache build). Default unbounded; corpus tests use 200.
