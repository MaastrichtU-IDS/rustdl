# rustdl internal timing breakdown (2026-06-17, commit ce19a4b)

Per-phase internal timings after this session's perf work (fix #1 role_super→Vec,
fix #3 n²-matrix→bitset, the defined-sup-sweep label gate, the BackPropRisk
diagnostic-loop gate, clause-index amortization). Measured on a 32-core host;
parallel walls unless noted. Wall = end-to-end (`/usr/bin/time -p`); phase figures
from the CLI `# wall breakdown ms` banner (per-pair path) + the 0.2/0.3 flamegraph
attribution (saturation path). FP=0/MISSED=0 on all (closure-verified).

## Two paths

rustdl routes per the fragment. **EL/Horn → saturation-only fast path** (no per-pair
tableau, no label cache). **Out-of-EL → hybrid** (EL saturation seeds the closure,
then a per-class label oracle + a top-down tier walk that calls the hypertableau
wedge only where the oracle is inconclusive).

## Corpus internal timings (current)

| fixture | classes | path | wall | dominant internal phases |
|---|---:|---|---:|---|
| galen | 2748 | saturation-only (Horn) | 0.30 s | parse ~93 ms, **saturate ~190 ms** (post fix #1), classify read-off ~20 ms |
| notgalen | 3087 | saturation-only (Horn) | 0.39 s | saturate (functional-merge) dominant |
| go-basic | 51967 | saturation-only (pure-EL) | 13.6 s | **parse ~7.4 s (external horned-owl)** + saturate + classify read-off (post fix #3) |
| ore-10908 | 692 | hybrid (out-of-EL) | 0.21 s | label_cache_build **159 ms**, tier_walk 32 ms; 2 wedge pass-throughs |
| sio | 1585 | hybrid (out-of-EL) | 2.04 s | label_cache_build **873 ms**, tier_walk **916 ms** (mostly the 265 wedge pass-throughs) |
| ore-15672 | 82 | hybrid (out-of-EL) | 28.7 s | per-pair wedge stalls (109 timed-out @200 ms) |
| wine | 137 | hybrid (out-of-EL) | ~54 s @25 ms | tier_walk ~94 % — 8666 non-subsumed pairs burning the deadline (wedge thrash) |

## Per-pair `# wall breakdown ms` (out-of-EL fixtures, fresh)

| fixture | label_cache_build | tier_walk | label-heuristic pruned / pass_through | BackPropRisk (gated) | wedge sat probes |
|---|---:|---:|---|---|---:|
| ore-10908 | 159 | 32 | 33019 / 2 | safe=0 unsafe=0 (off) | 692 (oracle) + 2 |
| sio | 873 | 916 | 110799 / 265 | safe=0 unsafe=0 (off) | 1585 (oracle) + 265 |

- **`label_cache_build`** = building the per-class label oracle (one wedge satisfiability
  probe per class — `692`/`1585`). This is genuine SROIQ reasoning, the largest
  out-of-EL cost now.
- **`tier_walk`** = the top-down direct-parents walk. The traversal itself is ~free
  (`RUSTDL_SKIP_BFS=1` → 0 ms); the time is the label-heuristic pass-through **wedge
  calls** it makes (sio: 265, a few in the 100–999 ms bucket — see the wedge-cost
  histogram).
- **label-heuristic pruned** (sio 110,799) = pairs the per-class oracle settled without
  a wedge call (the Phase-7/8 heuristic + the defined-sup-sweep gate shipped this
  session). pass_through = the residual wedge calls.
- **BackPropRisk safe=0/unsafe=0** = the diagnostic loop is now gated off (it ran
  unconditionally before this session, O(n×axioms), for the default-OFF snapshot cache;
  gating it cut sio `prepared` 491 ms → 73 ms).
- **snapshot_cache_build / snapshot_replay = 0** = the snapshot cache is default-OFF
  (FP-unsound on non-Horn; see the soundness contract).

## What the session's wins changed (before → after, this session)

| | before | after | lever |
|---|---:|---:|---|
| galen saturate | ~445 ms | ~190 ms | fix #1 (role_super HashMap→dense Vec) |
| go-basic reasoning | ~11.5 s | ~6.1 s | fix #3 (classify n²-bool-matrix → FixedBitSet read-off) |
| sio wall | 24 s | 2.04 s | defined-sup-sweep label-oracle gate (78,855 → 265 wedge calls) |
| sio `prepared` | 491 ms | 73 ms | gate the BackPropRisk diagnostic loop on snapshot-capture |
| sio label_cache_build | (922) | 873 ms | clause-index amortization (marginal — search-bound) |

## Where the remaining cost is (the frontier)

For out-of-EL, the residual is **genuine hypertableau-wedge reasoning**: `label_cache_build`
is per-class satisfiability (sio 873 ms = 1585 real probes), and the residual tier_walk
is the pass-through wedge calls. wine is the pathological case — the wedge's
disjunction-branching search thrashes (8666 non-subsumed pairs, ~12k branches each,
67.6 % disjunction) and times out. The cheap, FP-free levers are exhausted; closing the
hard-SROIQ residual (sio's per-class sat + wine's stall) requires a Konclude-style
clash-driven tableau-search rewrite (see the wine + Konclude verdict docs). EL/Horn is
near Konclude-wall parity (galen ~1.15×); go-basic is parse-dominated (external).
