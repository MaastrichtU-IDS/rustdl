# Memory-tail localization on `ore_ont_9347`: four proposed mechanisms, all four refuted

**Date:** 2026-07-29
**Status:** Findings from a measurement-only localization pass. **Read before starting the
sparse-subsumer rewrite** — on the worst-measured ontology that fix would address ~2% of peak
RSS. The site is narrowed but not yet identified; the discriminating experiment is named in
"What to do next".

## Why this pass was run

`docs/benchmarks/2026-07-21-ore-memory/perf_ore_v0330.tsv` shows a memory tail: 289
`TIMEOUT120` ontologies, 10 above 14 GB RSS, worst `ore_ont_9347` at 35.7 GB. The root cause
on record (`2026-07-18-d4-saturator-memory-rootcause.md`, memory
`d4-saturator-dense-matrix-memory`) is the EL saturator's `WorklistEngine::new` eagerly
allocating **two dense `num_total_classes²` bit matrices**; for `ore_ont_3914`, Tseitin
synthetics inflate 12.4k named classes to 582,815 total ⟹ ~84 GB at construction. The proposed
fix is a sparse/lazy subsumer representation.

Before building that, this pass asked the cheaper question: **which stage actually holds the
memory?**

## Stage split — `ore_ont_9347` (8.6 MB input, **114 classes**)

Peak RSS via `/usr/bin/time -f %M`, one stage at a time:

| stage | command | wall | peak RSS |
|---|---|---|---|
| convert + absorb only | `rustdl tbox-stats` | 123.5 s | **14.2 GB** |
| + EL saturation | `rustdl classify --saturation-only` | 152.7 s | **19.1 GB** (completes, exit 0) |
| full classify | `rustdl classify` | 491.7 s | **238.2 GB → OOM-killed** |

## Knob sweep (single-threaded, 130 s budget)

| config | peak RSS | verdict |
|---|---|---|
| baseline | 36.549 GB | — |
| `RUSTDL_ANYWHERE_BLOCKING=1` | 36.549 GB | **inert** |
| `RUSTDL_MAX_NODES=50000` | 36.549 GB | **inert** |
| `RUSTDL_LABEL_HEURISTIC=0` | 36.549 GB | **inert** |
| `RUSTDL_HYPERTABLEAU=0` (wedge off) | **29.978 GB** | −6.6 GB (−18%) |
| `--pair-timeout-ms 50` | 36.549 GB | **inconclusive** — see below |

## Thread scaling

| threads | wall | peak RSS | outcome |
|---|---|---|---|
| 32 | 491 s | 238.2 GB | OOM-killed (signal 9) |
| 1 (`RAYON_NUM_THREADS=1`) | 904 s | 70.7 GB | timed out (exit 124), still growing |

---

## Findings

### 1. The D4 dense-matrix mechanism cannot explain this ontology

Saturation adds **~4.9 GB** (14.2 → 19.1) and *completes*. The ontology has **114 classes**,
so a dense `num_total_classes²` bitset over them is negligible, and whatever the Tseitin
multiplier is, the saturator's entire footprint is bounded above by the measured 19.1 GB.
**A sparse subsumer representation would address ~2% of this ontology's peak.**

D4's mechanism is real, and was measured on `ore_ont_3914` (a 12.4k-class Horn giant). This
does not refute it *there* — it shows the tail has at least two distinct sites and the
worst-RSS ontologies are not the D4 kind.

### 2. Parallel fan-out is not the mechanism

32× the threads yields only **3.4×** the memory (238 → 70.7 GB). The recorded
`tableau-memory-fanout` shape (`#cores × per-pair-graph`) would predict ~7.4 GB
single-threaded. That finding was correctly measured on `alehif` (~30 MB per pair-graph, 42 MB
single-threaded) and does not transfer here.

### 3. It is not the main tableau's completion graph, and not the label cache

Forcing anywhere-blocking on classify changes peak RSS by **0 bytes**, so the main tableau's
graph is not where the bytes are. `MAX_NODES` being inert is *expected* — it governs the
deadline-FREE search while classify pairs are deadline-bounded — so it confirms documented
behaviour rather than adding information. The label heuristic is likewise inert. The wedge
accounts for ~18%: real, not dominant.

### 4. The `--pair-timeout-ms 50` row is NOT a null result

It looks like "memory is not per-pair", but that does not follow. 114 classes is up to ~13k
pairs; at 50 ms each that is ~650 s of pair work against a 130 s window, so the cap never
bounds total pair work inside the measurement. Identical RSS is equally consistent with
per-pair accumulation that is never released. **This test does not discriminate — do not cite
it as if it did.**

### 5. The benchmark's RSS column is systematically truncated by its own timeout

`9347` reads 35.7 GB there only because the 120 s budget killed it *before* the blowup — its
conversion alone takes 123.5 s, so at 120 s it had not finished converting. Given 900 s it
reaches 238 GB. **Every `TIMEOUT120` row understates its true memory demand** by an unknown,
per-ontology amount. The table cannot rank memory work; "10 ontologies above 8 GB" is a floor.

### 6. Some "reasoning timeouts" are conversion timeouts

`9347`'s `TIMEOUT120` says nothing about the reasoner — it never got there. Part of the
289-ontology DNF tail is likely mis-attributed the same way, which also bears on
`2026-07-29-fragment-lever-selection-findings.md`, whose ceiling analysis assumed the tail was
reasoning-bound.

## What is established, and what is not

**Established:** 14.2 GB before any reasoning; 19.1 GB through saturation, which completes;
~6.6 GB is the wedge; growth is steady at ~40 MB/s (36.5 GB @ 132 s → 70.7 GB @ 904 s), not
explosive. Not the D4 matrices, not fan-out, not the main tableau graph, not the label cache.

**Not established:** where the remaining ~30 GB (and counting) lives. Steady linear
accumulation that survives bounding the *search* suggests something retained per unit of work
rather than one runaway structure. Remaining candidates: the once-per-classify
`PreparedOntology::from_internal` snapshot, and per-pair retention.

## The lesson worth keeping

Four mechanisms were proposed and all four died to measurement. Two of them came from existing
repo root-cause notes, **each correctly measured on a different ontology** and then generalised
by wording that does not carry its scope. **The memory tail is not one phenomenon.** Localize
per ontology before building, and read the existing notes as ontology-specific evidence rather
than general diagnoses.

## What to do next

1. **Settle the per-pair question with an experiment that can.** Instrument RSS at classify
   phase boundaries (after `PreparedOntology::from_internal`, after the label-cache build,
   every N pairs), or use a budget long enough that a small `--pair-timeout-ms` genuinely
   bounds total pair work.
2. **Confirm on `ore_ont_11085` (33.7 GB) and `ore_ont_5368` (26.3 GB)** before generalising.
   This document exists because that step was skipped for D4.
3. **Re-run the memory benchmark with a budget exceeding conversion time**, recording
   conversion separately, so the RSS column stops measuring the timeout and conversion-bound
   ontologies are separated from reasoning-bound ones.
4. **Do not start the sparse-subsumer rewrite** on the strength of the D4 note. It targets a
   real site — just not the one that dominates here.
