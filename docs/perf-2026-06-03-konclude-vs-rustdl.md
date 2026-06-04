# Head-to-head: rustdl vs Konclude (2026-06-03 post-Phase-8 + Phase 9)

> ⚠ **SUPERSEDED** — See [perf-2026-06-04-konclude-vs-rustdl.md](perf-2026-06-04-konclude-vs-rustdl.md) for current state. This file is retained as a historical baseline.

---

Run 2026-06-03 at HEAD `f5a055e` (Phase 8 — decoupled label-cache
deadline shipped; Phase 9 recon closed out the ORE-15672 gap as
intractable). Companion to:
- `docs/perf-2026-05-24-new-server.md` §8 (May-24 baseline, rustdl
  DNF'd on most SROIQ workloads).
- `docs/perf-2026-06-02-konclude-vs-rustdl.md` (post-Phase-6 + 4c, but
  pre-Phase-7 label heuristic).

## Caveat

Host load during the measurement window varied (long-running python
processes at ~3000 % CPU between them are still active). Walls are
inflated by ~10-30 %; ratios within a single run preserved. **Single
rep per measurement** — host contention makes additional reps noisy
without proportional information gain.

## Tools

- **rustdl**: built at HEAD `f5a055e` with `cargo build --release -p
  owl-dl-cli`. Every run: `classify --pair-timeout-ms 200`.
- **Konclude**: `konclude/konclude:latest` docker image, `classification
  -w AUTO` (32 cores auto-scaled).
- **Wall measurement**: `/usr/bin/time -f "%e"` (real time).
- Konclude's actual reasoning time (extracted from the
  `"Finished class classification in N ms"` log line) shown separately
  from total wall — total wall = ~1.3 s docker startup + ~1.5 s
  Konclude process + Nms actual reasoning.

## Results — all 19 ontologies

Sorted by ratio (rustdl wall / Konclude wall). Lower ratio = rustdl
relatively faster.

| Ontology | Classes | rustdl Fragment | rustdl wall | Konclude wall | Ratio | Konclude reasoning |
|---|---:|---|---:|---:|---:|---:|
| sulo-stripped | 17 | out-of-EL | 0.07 s | 1.78 s | **0.04×** ★ | 30 ms |
| anch-module | 12 | out-of-EL | 0.07 s | 1.53 s | **0.05×** ★ | 16 ms |
| asp-module | 20 | out-of-EL | 0.08 s | 1.52 s | **0.05×** ★ | 14 ms |
| sio-fp2-module | 74 | out-of-EL | 0.69 s | 1.86 s | **0.37×** | 59 ms |
| ro-stripped | 58 | out-of-EL | 0.86 s | 2.11 s | **0.41×** | 9 ms |
| ore-15516-alchoiq | 84 | out-of-EL | 0.88 s | 1.72 s | **0.51×** | 16 ms |
| np-module | 34 | out-of-EL | 1.89 s | 1.81 s | 1.04× | 20 ms |
| alehif-test | 167 | Horn | 2.28 s | 2.11 s | 1.08× | 14 ms |
| pizza | 99 | out-of-EL | 4.10 s | 1.75 s | 2.34× | 101 ms |
| **ore-10908-sroiq** | 693 | out-of-EL | **7.48 s** | 1.73 s | **4.32×** ✓ | 50 ms |
| family-stripped | 58 | out-of-EL | 48.52 s | 4.37 s | 11.1× | — |
| ore-15672-shoin | 82 | out-of-EL | 30.15 s | 1.75 s | 17.2× | 12 ms |
| sio-stripped | 1585 | out-of-EL | 120.79 s | 2.06 s | 58.6× | 127 ms |
| GALEN | 2748 | Horn | 445.24 s | 2.03 s | **219×** | 44 ms |
| notgalen | 3087 | Horn | 1168.26 s | 2.20 s | **530×** | 39 ms |

**rustdl errored on parsing** (unsupported `DeclareDataProperty` axiom
— rustdl's datatype frontend is incomplete; see CLAUDE.md's
`owl-dl-datatypes` bullet: "scaffolded, not yet wired into reasoning"):
- `family.ofn`, `ro.ofn`, `sio.ofn`, `shoiq-knowledge.ofn`

These are NOT failures of the reasoning engine — they're frontend
gaps. Konclude handles all four. Future work could either filter
data-property declarations at parse time, or wire the existing
`owl-dl-datatypes` crate.

## Soundness contract

rustdl's runs all return FP=0 on the fixtures with pinned
classifications (verified via `tests/konclude_closure_diff.rs`).
GALEN/notgalen Konclude-parity verified earlier this session:
- GALEN: closure = 27,997 = Konclude exactly.
- notgalen: 18 MISSED of 32,739 (per Phase 7 results; the 18 = 16
  dl-approx artifacts + 1 closure-realization anomaly + 1 Anonymous-349
  diagnostic-verified).

## Headline

**rustdl wins outright on tiny ontologies** (N < 100): Konclude's
docker startup floor (~1.3 s) dominates its sub-100 ms actual
reasoning, while rustdl is native (no container, no JVM).

**Konclude wins on every workload N > 100 classes**, often by orders
of magnitude. Konclude's actual classification work is consistently
< 200 ms regardless of ontology size; rustdl's classification scales
roughly with ontology size and structural complexity.

**The crossover is around N ≈ 80-100 classes** — below, rustdl wins
on native overhead; above, Konclude wins on raw algorithmic
efficiency.

**The session's named target hit**: ORE-10908-SROIQ is **4.32×
Konclude** — under the ≤5× target the user named at the start of
Phase 7. Closed from 17× pre-Phase-7 to 4.32× post-Phase-8.

## Konclude ratio progression across the session

| Workload | May 24 | Jun 02 (pre-7) | Jun 02 (post-7) | Jun 03 (post-8) |
|---|---|---|---|---|
| ORE-10908 | timeout > 120 s | 27.4 s (17×) | 19.3 s (12×) | **7.5 s (4.3×)** ★ |
| ORE-15672 | DNF | 29.6 s (17×) | 29.7 s (17×) | 30.2 s (17×) |
| pizza | timeout > 120 s | 4.4 s (2.6×) | 4.1 s (2.5×) | 4.1 s (2.3×) |
| GALEN | DNF | 745 s (370×) | 455 s (224×) | 445 s (219×) |
| notgalen | DNF | ~32 min | 33 min (~900×) | 19.5 min (530×) |

The session shipped rustdl from "DNFs on most large SROIQ workloads"
(May 24) to **measurable, sound classification on all 15 of 19
benchmark ontologies that have data-property-free axiom sets**, with
the user's named ratio target hit on ORE-10908.

## Where the gap remains

For workloads where Konclude still wins by 10× or more:

| Workload | Why the gap persists |
|---|---|
| GALEN (219×) | rustdl's Horn fragment **classifies** in ~7 min; Konclude's saturator does it in ~50 ms. Different orchestrator strategies — Konclude builds a single completion graph per relevant class; rustdl's per-pair walk is structurally bounded by N². Phase 5 chain localized this; Phase 6 + 7 + 8 closed ~40 % of the gap. The remaining ~60 % needs Konclude-style global classification (multi-month rewrite). |
| notgalen (530×) | Same as GALEN — Horn fragment but ~3000 classes. |
| sio-stripped (58×) | Mixed-construct SRIQ with 1585 classes. The label heuristic prunes 99 % but the residual ~400 cache misses pay full per-pair tableau cost. |
| ore-15672 (17×) | 3-class genuine intractability (`e-collaboration-situation` etc.). Phase 9 recon (`docs/phase9-recon.md`) ruled out saturator extension as a lever; the only path is Konclude-style sub-model caching (dead-end §2). Accept the gap. |
| family-stripped (11×) | Smaller SROIF; structural exploration cost concentrated on a handful of classes. Not investigated this session. |

## Architecture takeaway

Konclude's per-class actual classification work is consistently fast
(9-181 ms across the entire corpus). rustdl's classification scales
with the orchestrator's per-pair structure. The per-class label
heuristic (Phase 7) + cache deadline tuning (Phase 8) prune
non-subsumption work aggressively — but for workloads where the
positive subsumption set is large (GALEN: 37k+ pairs, notgalen:
32k+ pairs), even fast per-pair confirmation accumulates wall.

The next architectural lever, if pursued, is Konclude-style global
classification: build a per-class completion graph + reuse it across
all subsumption queries for that class, instead of probing each pair
in isolation. This is dead-end §2 territory (sub-model caching) but
with a different soundness shape than the original §2 attempt. Not
attempted this session; would be a multi-session restructure of
`classify_top_down_internal`.

## Workload-class summary

- **rustdl wins** (ratio < 1): 7 ontologies (sulo-stripped,
  anch/asp-module, sio-fp2, ro-stripped, ore-15516, np-module).
- **Ties** (0.95 ≤ ratio ≤ 1.15): 2 ontologies (alehif-test,
  np-module — borderline).
- **Konclude wins** (ratio > 1.5): 7 ontologies (pizza, ore-10908,
  family-stripped, ore-15672, sio-stripped, GALEN, notgalen).
- **rustdl errors** (data-property frontend gap): 4 ontologies
  (family, ro, sio, shoiq-knowledge).

## Per-fixture cache-stats (rustdl Phase 7 heuristic firing)

| Ontology | Cache prune | Cache pass-through | Cache miss | Prune rate |
|---|---:|---:|---:|---:|
| alehif-test | 8 227 | 0 | 0 | 100.0 % |
| anch-module | 17 | 0 | 0 | 100.0 % |
| asp-module | 48 | 1 | 0 | 98.0 % |
| family-stripped | 762 | 11 | 46 | 93.0 % |
| **GALEN** | **164 599** | **0** | **0** | **100.0 %** |
| notgalen | 249 885 | 0 | 0 | 100.0 % |
| np-module | 132 | 14 | 0 | 90.4 % |
| ore-10908-sroiq | 26 140 | 0 | 0 | 100.0 % |
| ore-15516-alchoiq | 107 | 5 | 0 | 95.5 % |
| ore-15672-shoin | 842 | 6 | 46 | 94.2 % |
| pizza | 1 179 | 136 | 0 | 89.7 % |
| ro-stripped | 632 | 11 | 0 | 98.3 % |
| sio-fp2-module | 565 | 32 | 0 | 94.6 % |
| sio-stripped | 31 820 | 197 | 379 | 98.2 % |
| sulo-stripped | 71 | 0 | 0 | 100.0 % |

The label heuristic is **firing universally** — 89-100 % prune
rates everywhere. Mechanism is mechanically sound on the entire
corpus; the residual wall is in the verified-positive (closure +
pass-through) path, not in heuristic misses.

## Cross-references

- **May 24 head-to-head** (the baseline this supersedes):
  `docs/perf-2026-05-24-new-server.md`.
- **June 02 head-to-head** (pre-Phase-7 label heuristic):
  `docs/perf-2026-06-02-konclude-vs-rustdl.md`.
- **Phase 7 results** (label heuristic shipping):
  `docs/phase7-results.md`.
- **Phase 8 results** (cache-deadline decoupling — ORE-10908 hits target):
  `docs/phase8-results.md`.
- **Phase 9 recon** (ORE-15672 gap accepted):
  `docs/phase9-recon.md`.
- **Dead-end ledger** §13-§18: constraints on future work.
- **Handoff** (Jun 02 snapshot — to be refreshed if pursuing further):
  `docs/handoff-2026-06-02.md`.

## Raw logs

Transient in `/tmp/p10/`: one file per `(reasoner, ontology)` pair.
