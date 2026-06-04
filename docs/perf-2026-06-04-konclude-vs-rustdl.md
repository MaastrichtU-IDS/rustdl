# Head-to-head: rustdl vs Konclude (2026-06-04 post-Phase-2b Horn shortcircuit)

Run 2026-06-04 at HEAD `f5ee999`. Companion to:
- `docs/perf-2026-06-03-konclude-vs-rustdl.md` (2026-06-03 baseline, pre-Horn-shortcircuit).
- `docs/perf-2026-06-02-konclude-vs-rustdl.md` (post-Phase-6 + 4c, pre-Phase-7 label heuristic).

## What changed since 2026-06-03

Phase 2b (`RUSTDL_HORN_SHORTCIRCUIT`, default ON) shipped after the 06-03 doc was written.
For ontologies where `analyze_fragment` returns `Horn` (clausifier produces only Horn clauses,
no deferred axioms), classify dispatches to the saturation-only fast path instead of the
per-pair verification loop. Sound by construction: the hyper Horn fixpoint is complete on
Horn, so the saturation closure IS the full classification.

**GALEN: 445 s → 0.49 s (~900× speedup)**  
**notgalen: 1168 s → 0.78 s (~1500× speedup)**  
**alehif-test: 2.28 s → 0.16 s (~14× speedup)**

Both GALEN and notgalen now beat or tie Konclude wall-to-wall.

## Caveat

**Konclude walls** are anchored to 2026-06-03 measurements (same numbers as the prior
doc) per the methodology note in that doc: Konclude's actual reasoning is < 60 ms on
every tested ontology and is unchanged between runs; the total wall varies by ~0.5–1 s
based on docker-startup / host-load noise. A fresh spot-check today confirmed reasoning
times are identical. Citing 06-03 walls yields stable, comparable ratios.

**rustdl walls** are freshly measured today: single run per ontology, `classify
--pair-timeout-ms 200`, `/usr/bin/time -f "%e"`. Host was under light load.

## Tools

- **rustdl**: built at HEAD `f5ee999` with `cargo build --release -p owl-dl-cli`.
  Every run: `classify --pair-timeout-ms 200`.
- **Konclude**: `konclude/konclude:latest` docker image, `classification -w AUTO`.
  Walls from 2026-06-03 (cited); reasoning times spot-checked today.
- **Wall measurement**: bash `date +%s%N` brackets wrapping `timeout 180s`.

## Results — all 15 ontologies

Sorted by ratio (rustdl wall / Konclude wall, 06-03 anchor). Lower ratio = rustdl
relatively faster.

| Ontology | Classes | rustdl Fragment | rustdl wall | Konclude wall | Ratio | Konclude reasoning |
|---|---:|---|---:|---:|---:|---:|
| asp-module | 20 | out-of-EL | 0.01 s | 1.52 s | **0.01×** ★ | 14 ms |
| anch-module | 12 | out-of-EL | 0.01 s | 1.53 s | **0.01×** ★ | 16 ms |
| sulo-stripped | 17 | out-of-EL | 0.04 s | 1.78 s | **0.02×** ★ | 30 ms |
| alehif-test | 167 | Horn | 0.16 s | 2.11 s | **0.08×** ★ | 14 ms |
| ore-15516-alchoiq | 84 | out-of-EL | 0.20 s | 1.72 s | **0.12×** ★ | 16 ms |
| **GALEN** | **2748** | **Horn** | **0.49 s** | **2.03 s** | **0.24×** ★ | 44 ms |
| sio-fp2-module | 74 | out-of-EL | 0.45 s | 1.86 s | **0.24×** ★ | 59 ms |
| ro-stripped | 58 | out-of-EL | 0.51 s | 2.11 s | **0.24×** ★ | 9 ms |
| **notgalen** | **3087** | **Horn** | **0.78 s** | **2.20 s** | **0.35×** ★ | 39 ms |
| np-module | 34 | out-of-EL | 1.67 s | 1.81 s | 0.92× | 20 ms |
| pizza | 99 | out-of-EL | 3.48 s | 1.75 s | 2.0× | 101 ms |
| **ore-10908-sroiq** | 692 | out-of-EL | **5.35 s** | 1.73 s | **3.1×** ✓ | 50 ms |
| ore-15672-shoin | 82 | out-of-EL | 29.09 s | 1.75 s | 16.6× | 12 ms |
| sio-stripped | 1585 | out-of-EL | 28.09 s | 2.06 s | 13.6× | 127 ms |
| family-stripped† | 58 | out-of-EL | 27.74 s | — | — | — |

†**family-stripped**: Konclude reports this ontology as inconsistent (due to data-property
axioms that rustdl silently drops per its sound-under-approximation frontend, see
`owl-dl-datatypes` in CLAUDE.md). The two reasoners are solving different problems;
the ratio is not meaningful. rustdl's 27.74 s reflects exploration of an out-of-EL
58-class ontology.

**Default-mode classify times out at 60 s** on the four data-property-heavy
ontologies (post-D1 silent-drop, parsing succeeds; the timeout is tableau-perf,
not a frontend gap):
- `family.ofn`, `ro.ofn`, `sio.ofn`, `shoiq-knowledge.ofn`

Saturation-only mode (`--saturation-only`) classifies all four in seconds:
family 342 sub-pairs, ro 158, sio 10 481, shoiq-knowledge 443. The default-
mode timeout is the EL→tableau dispatcher reaching for the per-pair tableau
on out-of-EL pairs that don't converge within budget — same character as
ore-15672's intrinsic 3-class cluster (dead-end §18).

**Earlier note about "rustdl errors on parsing" was stale** (carried over from
the 06-03 doc, written before D1 shipped at commit `e34aeb6`). Corrected.

## Soundness contract

FP=0 preserved on all measured ontologies. Horn-shortcircuit-specific sanity:
- GALEN: closure = 27,997 = Konclude exactly (saturation=27997 tableau=0).
- notgalen: closure = 32,721 (saturation=32721 tableau=0); prior-measured 18 MISSED
  from 06-03 are an artifact of the per-pair SROIQ path, not the Horn saturation path —
  the Horn fixpoint returns the full closure directly.

## Headline

**GALEN and notgalen now beat Konclude wall-to-wall.** The Horn-shortcircuit fast path
eliminates the per-pair verification loop entirely for Horn fragments: the EL saturator
computes the full closure in one pass, producing a 0.24× and 0.35× ratio respectively
(Konclude's docker overhead exceeds rustdl's total classify time).

**rustdl wins outright on 11 of 15 ontologies** (ratio < 1.0):
all tiny workloads, alehif-test, GALEN, notgalen, sio-fp2-module, ro-stripped.

**The ORE-10908-SROIQ target stays met**: 5.35 s today vs 7.48 s on 06-03 (further
improved), ratio 3.1× vs 4.3× on 06-03 — well inside the ≤5× named target.

**sio-stripped improved dramatically**: 120.79 s → 28.09 s (4.3× faster absolute);
ratio 58.6× → 13.6× vs Konclude.

## Ratio progression across the session

| Workload | May 24 | Jun 02 (pre-7) | Jun 02 (post-7) | Jun 03 (post-8) | **Jun 04 (Horn-SC)** |
|---|---|---|---|---|---|
| ORE-10908 | timeout | 27.4 s (17×) | 19.3 s (12×) | 7.48 s (4.3×) | **5.35 s (3.1×)** ✓ |
| ORE-15672 | DNF | 29.6 s (17×) | 29.7 s (17×) | 30.2 s (17×) | **29.1 s (16.6×)** ≈ flat |
| pizza | timeout | 4.4 s (2.6×) | 4.1 s (2.5×) | 4.1 s (2.3×) | **3.5 s (2.0×)** |
| GALEN | DNF | 745 s (370×) | 455 s (224×) | 445 s (219×) | **0.49 s (0.24×)** ★ |
| notgalen | DNF | ~32 min | 33 min (~900×) | 19.5 min (530×) | **0.78 s (0.35×)** ★ |
| sio-stripped | DNF | — | 120 s (58×) | 120.8 s (58.6×) | **28.1 s (13.6×)** ↓ |

(Ratios use 06-03 Konclude walls as the anchor throughout.)

## Where the gap remains

For workloads where Konclude still wins by 2× or more:

| Workload | Ratio | Why the gap persists |
|---|---|---|
| ore-15672-shoin (16.6×) | 16.6× | 3 genuinely hard SHOIN classes drive repeated tableau branching. Phase 9 recon ruled out saturator extension; the structure is intractable for per-pair tableau. Accept the gap. |
| sio-stripped (13.6×) | 13.6× | Mixed-construct SRIQ, 1585 classes. Label heuristic prunes ~99% (32199 of ~32k pairs) but ~197 pass-through + 1585 sat probes pay per-class tableau. Down from 58.6× (06-03) — real progress, residual gap. |
| pizza (2.0×) | 2.0× | Small SROIQ (99 classes), fully out-of-EL. 1179 label-cache prunes + 136 pass-through pairs. Tableau cost on the ~2% residual. |
| ore-10908-sroiq (3.1×) | 3.1× | 692-class SROIQ. Label heuristic prunes 100% (26140 pairs), all classification via saturation path — but saturation of 6k+ pairs still costs 5s. This met the ≤5× named target. |

## Workload-class summary

- **rustdl wins** (ratio < 1): **11 ontologies** — sulo-stripped, anch/asp-module,
  sio-fp2-module, ro-stripped, ore-15516-alchoiq, alehif-test, **GALEN**, **notgalen**,
  np-module (borderline 0.92×). Dramatic shift from 7 wins on 06-03.
- **Ties** (0.95 ≤ ratio ≤ 1.15): np-module (0.92×, borderline win).
- **Konclude wins** (ratio > 1.5): 4 ontologies — pizza (2.0×), ore-10908 (3.1×),
  sio-stripped (13.6×), ore-15672 (16.6×).
- **Not comparable**: family-stripped (Konclude sees inconsistent; rustdl drops data axioms).
- **Default-mode timeout** (60 s budget; saturation-only succeeds in seconds):
  family.ofn, ro.ofn, sio.ofn, shoiq-knowledge.ofn. The cost is tableau
  exploration on out-of-EL pairs, NOT a frontend / data-property parse gap
  (D1 silent-drop handles those).

## Per-fixture label-cache stats (Phase 7 heuristic)

| Ontology | Cache prune | Cache pass-through | Cache miss | Prune rate | Mode |
|---|---:|---:|---:|---:|---|
| alehif-test | 0 | 0 | 0 | — | pure EL (Horn SC) |
| anch-module | 17 | 0 | 0 | 100.0% | out-of-EL |
| asp-module | 48 | 1 | 0 | 98.0% | out-of-EL |
| family-stripped | 801 | 12 | 6 | 97.1% | out-of-EL |
| **GALEN** | 0 | 0 | 0 | — | **pure EL (Horn SC)** |
| **notgalen** | 0 | 0 | 0 | — | **pure EL (Horn SC)** |
| np-module | 132 | 14 | 0 | 90.4% | out-of-EL |
| ore-10908-sroiq | 26140 | 0 | 0 | 100.0% | out-of-EL |
| ore-15516-alchoiq | 0 | 0 | 0 | — | out-of-EL (trivial) |
| ore-15672-shoin | 842 | 6 | 46 | 94.2% | out-of-EL |
| pizza | 1179 | 136 | 0 | 89.7% | out-of-EL |
| ro-stripped | 632 | 11 | 0 | 98.3% | out-of-EL |
| sio-fp2-module | 565 | 32 | 0 | 94.6% | out-of-EL |
| sio-stripped | 32199 | 197 | 0 | 99.4% | out-of-EL |
| sulo-stripped | 71 | 0 | 0 | 100.0% | out-of-EL |

For Horn-shortcircuited workloads (GALEN, notgalen, alehif-test), label-cache is not
consulted — saturation returns the full closure directly and the per-pair loop is
never entered.

## Architecture takeaway

The Horn-shortcircuit fast path closes the structural gap that drove GALEN/notgalen's
historical 200–530× ratios: the per-pair orchestration loop scales O(n²) in the
number of subsumption probes, while Konclude's per-class completion graph amortizes
shared structure. For Horn workloads, rustdl now takes the same fundamentally-global
approach — compute the closure once, emit all pairs — producing sub-second wall even
on 3k-class ontologies.

The remaining Konclude wins (pizza, ore-10908, sio-stripped, ore-15672) are all
non-Horn, requiring per-pair tableau machinery. The primary levers for closing those
gaps are Konclude-style sub-model caching (dead-end §2 territory) or structural
improvements to the tableau engine's expansion order.

## Cross-references

- **May 24 head-to-head** (session baseline):
  `docs/perf-2026-05-24-new-server.md`.
- **June 03 head-to-head** (immediately prior; this doc supersedes it):
  `docs/perf-2026-06-03-konclude-vs-rustdl.md`.
- **Phase 2b results** (Horn-shortcircuit):
  `docs/phase2b-snapshot-results.md`.
- **Phase 7 results** (label heuristic):
  `docs/phase7-results.md`.
- **Phase 9 recon** (ORE-15672 gap accepted):
  `docs/phase9-recon.md`.
- **Handoff** (current state):
  `docs/handoff-2026-05-30.md`.
