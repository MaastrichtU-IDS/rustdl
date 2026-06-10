# Tableau memory investigation — alehif 1.47 GB (2026-06-10)

Triggered by the embeddability demo: alehif (ALC+H+I+F, **247 classes**)
classifies in rustdl at **6.5 s / 1.47–1.62 GB RSS** vs Konclude
**0.18 s / 60 MB** — 36× slower, 24× heavier. Investigated.

## Localization (rustdl on alehif-test.owx)
| config | RSS | wall |
|---|---|---|
| `--saturation-only` (no tableau) | 11 MB | 0.03 s |
| hybrid, `RAYON_NUM_THREADS=1` | **42 MB** | 170 s |
| hybrid, RAYON=4 | 135 MB | 43 s |
| hybrid, RAYON=8 | 258 MB | 21.6 s |
| hybrid, RAYON=16 | 882 MB | 11.5 s |
| hybrid, RAYON=32 (default, =nproc) | 1622 MB | 6.5 s |
| hybrid, `--pair-timeout-ms 25` | 357 MB | 0.46 s |

## Diagnosis
1. **Not a leak.** Single-threaded peak is 42 MB — per-pair completion graphs
   ARE freed between pairs. The blowup is **parallel fan-out**: the per-pair
   classify loop runs one tableau graph per rayon worker concurrently, so
   peak RSS ≈ `#cores × per-pair-graph`. Linear in thread count (42→135→258→
   882→1622 MB for 1/4/8/16/32 threads). It's the space-time cost model of
   parallel per-pair tableau, not a defect.
2. **Root cause of the large per-pair graph (~30 MB each).** The `Node` struct
   is compact (`SmallVec` labels/edges), so 30 MB ≈ **tens of thousands of
   nodes** for a 247-class ontology. That points at blocking effectiveness:
   `TableauContext::is_blocked` (lib.rs) is **ancestor-only pair (double)
   blocking** — `y` is blocked only by a *tree-ancestor* `x'` with matching
   parent-role and `L(y)⊆L(x') ∧ L(parent(y))⊆L(parent(x'))`. Sound for ALCHI
   (subset pair-blocking restores soundness under inverse roles), but
   conservative: blocking against ancestors only means it fires late and the
   completion tree grows large. The same weakness drives the 170 s
   single-thread wall.

## Actionable outcomes
- **Mitigation, zero-risk, available now:** `RAYON_NUM_THREADS` bounds the
  fan-out (8 → 258 MB / 21.6 s — 6× less memory for ~3× wall). EL/saturation
  ontologies are unaffected (single-pass, 11 MB). Document this for
  memory-constrained deployments; consider a memory-aware default cap for the
  tableau phase (a blanket cap would slow all hybrid classify, so leave it a
  knob unless a deployment needs the bound).
- **Deeper fix — CORRECTED 2026-06-10 (see
  `anywhere-blocking-scoping-2026-06-10.md`):** the original "add anywhere
  blocking" claim here was WRONG. alehif's 167 probes run through the hyper
  **wedge**, whose `is_blocked` (hyper.rs:780) **already does anywhere
  subset-pairwise blocking** with `double_blocking` ON by default. (The
  ancestor-only blocking I cited is the *main tableau's* `is_blocked`
  (lib.rs:726), which alehif does not use.) So anywhere blocking is already
  shipped. The residual blowup is **per-pair model duplication** — 167
  independent wedge models, each large on the inverse fragment because pair
  blocking is inherently conservative — × 32 parallel workers. The real lever
  is therefore **global model construction / sound sub-model reuse** (kill the
  167× duplication; the Konclude/HermiT approach), which is the previously
  identified "global model" rewrite — large and FP-critical. See the scoping
  doc for the L1/L2/L3 breakdown.

## Scope note
This only affects the **out-of-EL hybrid-tableau path** (the SROIQ fragment).
The embeddable EL/Horn niche (saturation path: bibtex 5 MB, GALEN 30 MB) is
unaffected. So this is a SROIQ-performance/footprint item, orthogonal to the
EL/Horn embeddability story.

## ROOT CAUSE (2026-06-10, final): glibc malloc-arena retention

The 1.47–1.6 GB is **allocator arena retention**, not a reasoner bug. Measured
on alehif:
- default (glibc, 32 threads): 1587 MB / 6.5 s
- `MALLOC_ARENA_MAX=2`: 503 MB / 15 s ; `MALLOC_ARENA_MAX=1`: 233 MB / 29 s
- mimalloc global allocator: **1873 MB** (WORSE — its own per-thread heaps
  retain too, plus baseline overhead)

`ARENA_MAX=1` (233 MB) ≈ the TRUE simultaneous-live peak across 32 threads; the
extra ~1.35 GB at default is glibc holding freed memory in up to `8×ncores`
per-thread arenas, churned by 16 k tiny per-probe alloc/free × 32 threads. The
clause-index hoist couldn't help because the retention is independent of
per-probe allocation *size*; swapping to mimalloc is worse. So there is **no
free fix** — every mitigation trades wall for RSS:
- `MALLOC_ARENA_MAX=2` (≈3× less RSS, ~2.3× wall) — env knob, zero code.
- `RAYON_NUM_THREADS=8` (≈6× less RSS, ~3× wall) — env knob.
The true working set (~233 MB) is itself modest; the headline 1.6 GB is an
allocator artifact. Pinning the residual per-probe alloc/free churn that
fragments the arenas needs a heap profiler (heaptrack/massif — not installed).
**This is an out-of-EL-niche SROIQ-path concern; the EL/Horn embeddable niche
is unaffected (GALEN 30 MB).**
