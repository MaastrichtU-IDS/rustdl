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

## 4. It is NOT parallel fan-out — it is ONE unbounded graph (hypothesis REFUTED by measurement)

The obvious hypothesis was the already-recorded `tableau-memory-fanout` shape: peak RSS =
`#cores × per-pair-graph`, which would put 238 GB / 32 cores ≈ 7.4 GB per worker. Tested:

| threads | wall | peak RSS | outcome |
|---|---|---|---|
| 32 | 491 s | 238.2 GB | OOM-killed (signal 9) |
| 1 (`RAYON_NUM_THREADS=1`) | 904 s | **70.7 GB** | timed out (exit 124), still growing |

**32× the threads yields only 3.4× the memory, so fan-out is not the mechanism.** A single
worker's completion graph passes 70 GB and had not converged when the budget expired. For
scale, `tableau-memory-fanout` measured `alehif` at ~30 MB per pair-graph and 42 MB
single-threaded — this is ~1700× that, on a **114-class** ontology.

So the mechanism is **unbounded completion-graph growth on a single pair**: a
blocking/termination failure, not an allocation-strategy one. A thread cap only slows the
approach to OOM; it cannot bound the graph.

That is a more tractable target than any allocation rewrite, and there is existing machinery
to test against:
- **`RUSTDL_MAX_NODES`** (default 50000) caps the deadline-FREE tableau search and yields a
  `NodeCap` verdict ⟹ `Ok(None)` (sound MISS). Per `CLAUDE.md`, classify pairs run on the
  **deadline-bounded** path, so the cap likely does not apply here — meaning a 114-class
  ontology can grow an unbounded graph with nothing to stop it.
- **`RUSTDL_ANYWHERE_BLOCKING=1`** forces anywhere-blocking on classify. It is default-ON
  only for deadline-free paths; the 152-ontology bake-off found it verdict-identical to
  ancestor-blocking, and ancestor-only blocking is known to be unable to cut certain
  generating cycles.
- **`--pair-timeout-ms`** bounds each pair's search and is the existing practical mitigation.

## 5. Neither knob bounds it, and the label cache is not it either (measured)

Single-threaded, 130 s budget, `ore_ont_9347`:

| config | peak RSS | verdict |
|---|---|---|
| baseline | 36.549 GB | — |
| `RUSTDL_ANYWHERE_BLOCKING=1` | 36.549 GB | **inert** |
| `RUSTDL_MAX_NODES=50000` | 36.549 GB | **inert** |
| `RUSTDL_LABEL_HEURISTIC=0` | 36.549 GB | **inert** |
| `RUSTDL_HYPERTABLEAU=0` (wedge off) | **29.978 GB** | −6.6 GB (−18%) |
| `--pair-timeout-ms 50` | 36.549 GB | inconclusive, see below |

So the "unbounded tableau graph" framing in §4 is **also wrong**: forcing anywhere-blocking on
classify changes nothing, so the main tableau's completion graph is not where the bytes are.
`MAX_NODES` being inert is expected (it governs the deadline-FREE search; classify pairs are
deadline-bounded) and confirms the documented behaviour rather than telling us anything new.

The wedge accounts for ~18% — real, not dominant.

**The `--pair-timeout-ms 50` row does NOT show that memory is non-per-pair.** 114 classes is
up to ~13k pairs; at 50 ms each that is ~650 s of pair work, far exceeding the 130 s window.
A 50 ms cap therefore does not bound total pair work inside the measurement, so identical RSS
is equally consistent with per-pair accumulation that is never released. This test does not
discriminate; do not cite it as if it did.

## What is actually established

- **14.2 GB accrues before any reasoning** (convert + absorb), 123.5 s.
- **19.1 GB through EL saturation**, which COMPLETES (exit 0, `# classes: 114`).
- **~6.6 GB is the wedge.**
- Growth is **steady, ~40 MB/s** (36.5 GB at 132 s → 70.7 GB at 904 s), not explosive.
- Not the label cache, not the main tableau graph, not parallel fan-out, not the D4 matrices.

Steady linear accumulation that survives bounding the *search* looks like something retained
per unit of work rather than a single runaway structure — but the discriminating experiment
has not been run.

## Hypotheses refuted, and the lesson

Four mechanisms were proposed for this ontology and **all four died to measurement**: D4's
dense `num_total_classes²` matrices, parallel per-pair fan-out, unbounded tableau
completion-graph growth, and the label cache. Two of them (D4, fan-out) came from existing
repo root-cause notes — each **correctly measured on a different ontology** (`ore_ont_3914`,
`alehif`) and then generalised by wording that does not carry its scope.

**The memory tail is not one phenomenon.** Localize per ontology before building anything, and
treat the existing notes as ontology-specific evidence rather than general diagnoses.

## What to do next

1. **Settle the per-pair question properly**: instrument RSS at the classify phase boundaries
   (after `PreparedOntology::from_internal`, after the label-cache build, after every N pairs),
   or run with a budget long enough that a small `--pair-timeout-ms` genuinely bounds total
   pair work. The remaining candidates are the once-per-classify `PreparedOntology` snapshot
   and per-pair retention.
2. **Confirm on `ore_ont_11085` (33.7 GB) and `ore_ont_5368` (26.3 GB)** before generalising —
   this document exists because that step was skipped for D4.
3. **Re-run the memory benchmark with a budget exceeding conversion time**, and record
   conversion separately, so the RSS column stops measuring the timeout (see §2/§3).
4. **Do not start the sparse-subsumer rewrite** on the strength of the D4 note. It targets a
   real site, but ~2% of this ontology's peak.

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

(superseded by "What to do next" above.)
aimed at a real site, but not at the one that dominates here.
