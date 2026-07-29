# Memory-tail localization: the D4 attribution does not explain the worst ontology

**Date:** 2026-07-29
**Status:** Findings from a localization pass. **Read before starting the sparse-subsumer
rewrite** — on the worst-measured ontology that fix would address ~2% of peak RSS.

## Why this pass was run

`docs/benchmarks/2026-07-21-ore-memory/perf_ore_v0330.tsv` shows a memory tail: 289
`TIMEOUT120` ontologies, 10 of them above 14 GB RSS, worst `ore_ont_9347` at 35.7 GB. The
diagnosed root cause on record (`2026-07-18-d4-saturator-memory-rootcause.md`, memory
`d4-saturator-dense-matrix-memory`) is the EL saturator's `WorklistEngine::new` eagerly
allocating **two dense `num_total_classes²` bit matrices** (`subsumed_by` + `subsumers`);
for `ore_ont_3914`, Tseitin synthetics inflate 12.4k named classes to 582,815 total ⟹ ~84 GB
at construction. The proposed fix is a sparse/lazy subsumer representation.

Before building that, this pass asked a cheaper question: **on the worst ontologies, which
stage actually holds the memory?**

## Measurement — `ore_ont_9347` (8.6 MB input)

Peak RSS via `/usr/bin/time -f %M`, one stage at a time, generous budgets:

| stage | command | wall | peak RSS |
|---|---|---|---|
| convert + absorb only | `rustdl tbox-stats` | 123.5 s | **14.2 GB** |
| + EL saturation | `rustdl classify --saturation-only` | 152.7 s | **19.1 GB** |
| full classify | `rustdl classify` | 491.7 s | **238.2 GB → OOM-killed (signal 9)** |

`--saturation-only` reports `# classes: 114` and `# mode: pure EL (saturation-only)`.

## Three findings

### 1. The D4 dense-matrix mechanism cannot explain this ontology

The saturator stage adds **~4.9 GB** (14.2 → 19.1). The explosion — 19 GB to 238 GB —
happens **after** saturation, in the hybrid per-pair path. And the ontology has **114
classes**: a dense `num_total_classes²` bitset over 114 classes is negligible, and whatever
the Tseitin multiplier is, the saturator's whole footprint is bounded above by the 19.1 GB
measured. **A sparse subsumer representation would address ~2% of this ontology's peak.**

D4's mechanism is real and was measured on `ore_ont_3914` (a 12.4k-class Horn giant). This
finding does not refute it there — it shows the memory tail has **at least two distinct
sites**, and that the worst-RSS ontologies are not necessarily the D4 kind. Localize per
ontology before choosing a fix.

### 2. The benchmark's RSS column is systematically truncated by the timeout

`9347` is recorded at 35.7 GB because the 120 s budget killed it *before* the blowup;
conversion alone takes 123.5 s, so at 120 s it had not even finished converting. Given a
900 s budget it reaches 238 GB and is OOM-killed. **Every `TIMEOUT120` row understates its
true memory demand**, by an unknown and per-ontology amount. The table cannot be used to
rank memory work, and "10 ontologies above 8 GB" is a floor, not an estimate.

### 3. Some "reasoning timeouts" are conversion timeouts

Conversion + absorb on `9347` takes 123.5 s — longer than the whole 120 s benchmark budget.
So its `TIMEOUT120` verdict says nothing about the reasoner; it never got there. Part of the
289-ontology DNF tail is likely mis-attributed the same way. This is separately relevant to
the fragment-lever question (`2026-07-29-fragment-lever-selection-findings.md`), which
assumed the tail was reasoning-bound.

## Working hypothesis for the 238 GB (test pending)

The shape matches the already-recorded `tableau-memory-fanout` finding: peak RSS is
`#cores × per-pair-graph`, not a leak — 238 GB / 32 cores ≈ 7.4 GB per worker graph, with
huge per-pair completion graphs driven by ancestor-only pair-blocking. That predicts a
near-linear drop with thread count, testable in one run
(`RAYON_NUM_THREADS=1 rustdl classify`). If it holds:
- the immediate mitigation is a thread cap (already the recorded mitigation for `alehif`),
- the real fix is per-pair graph size (anywhere-blocking is already implemented and
  default-ON for deadline-free paths — check whether the classify path can use it here),
- and the sparse-subsumer rewrite is the wrong target for this class of ontology.

## What to do next

1. **Finish the thread-scaling check** on `9347`, then repeat the three-stage split on
   `11085` (33.7 GB), `5368` (26.3 GB), `1833` (19.3 GB) to see whether the ratio is stable
   or ontology-specific.
2. **Re-run the memory benchmark with a budget that exceeds conversion time** (or record a
   separate conversion-only column), so the RSS numbers stop being timeout artifacts and the
   conversion-bound ontologies are separated from the reasoning-bound ones.
3. **Only then** choose between: sparse subsumer representation (D4 site), conversion-stage
   allocation (the 14.2 GB site), or per-pair graph size / thread capping (the 219 GB site).
   On present evidence the third dominates on the worst ontology and the first does not
   apply to it at all.

**Do not start the sparse-subsumer rewrite on the strength of the D4 note alone.** It is
aimed at a real site, but not at the one that dominates here.
