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
- **Deeper fix (the real lever, scoped project):** **anywhere blocking** —
  block `y` against ANY older matching node (with an ordering to prevent mutual
  blocking), not just tree-ancestors. Standard in HermiT/Konclude; blocks far
  earlier → much smaller graphs → fixes BOTH the memory and the SROIQ wall.
  FP/completeness-sensitive (ordering + equality-pair condition for the `F`
  fragment must be exact), so it needs its own scoped effort + the full FP=0
  gate — not a quick patch.

## Scope note
This only affects the **out-of-EL hybrid-tableau path** (the SROIQ fragment).
The embeddable EL/Horn niche (saturation path: bibtex 5 MB, GALEN 30 MB) is
unaffected. So this is a SROIQ-performance/footprint item, orthogonal to the
EL/Horn embeddability story.
