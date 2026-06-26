# Stage-3 build-once / KPSet — measurement-driven NO-GO for wine (2026-06-26)

**VERDICT: NO-GO for wine (caught by a cheap measurement before building the probe).**
Build-once/KPSet does not bridge wine's residual 3.2 s → ms, because that residual is
per-class **search-hardness** in rustdl's wedge, which a shared global model cannot
amortize away. The honest path to Konclude's 114 ms is the deep engine property (Stage-4),
not the classification-loop restructuring.

## The measurement that reframed it

`rustdl classify --pair-timeout-ms 1` with the ∃-seed, label-cache deadline swept:

| label-cache deadline | misses | label_cache_build | tier_walk | total wall |
|---|---|---|---|---|
| adaptive (~137 ms) | 2470 | 1.66 s | 1.41 s | **3.2 s** |
| 30 s | **891** | **30.6 s** | 0.86 s | 31.5 s |

- A generous deadline labels ~1579 more classes (deadline-starved) but costs **more in build
  than it saves in walk** (U-shape — the adaptive-label-cache finding) → net wall *worse*.
- **~891 classes don't label even at 30 s** — genuinely search-hard for the wedge. A single
  global model instantiating these would time out on them too (the joint model's per-class
  subtree is at least as hard as the isolated sat). Amortization shares *setup*, not the
  per-class disjunctive *search*; wine's classes are largely independent.

## Why build-once can't bridge it

- Build-once's only win is amortizing the label_cache_build (137→1 build). But the cost is
  dominated by the **hard-class search**, not setup — and the hard classes don't complete in
  any model. So build-once shrinks the wrong term.
- The 2470/891 misses are **correct-not-subsumed** (MISSED=0 — wine's positives are all
  saturation-derived). They don't need solving for soundness, only speed; at a 1 ms cap their
  walk is already ~0.5 ms/pair. **3.2 s is the sound near-optimum for the current engine.**

## The real bridge to ms (Stage-4, the frontier)

Konclude does wine in 114 ms because its **integrated nominal + completion-graph-caching
architecture** keeps each per-class test tiny — it never hits rustdl's disjunctive blowup on
those ~891 classes. They are hard *for rustdl's wedge*, not intrinsically. So the bridge is
an **engine property** (the deep nominal rearchitecture / per-test tree-shrinking), which
this arc repeatedly found has **no measured cheap entry** — not a classification-loop
optimization. Build-once is the wrong tool for wine.

## Disposition

Spec `docs/superpowers/specs/2026-06-26-build-once-kpset-probe-design.md` written, but the
probe is **not built** — the cheap deadline-sweep measurement pre-empted it (the measure-first
discipline: a one-run experiment caught a likely-NO-GO before an intricate 137-probe-model
build). Wine's sound result stands at **3.2 s (≈15× the session-start 49 s, FP=0/MISSED=0)**;
the remaining ~28× to Konclude is the Stage-4 engine frontier. Branch
`feat/build-once-kpset-probe` (spec only); `main` untouched.
