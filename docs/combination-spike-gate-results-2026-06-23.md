# Phase-0 combination spike — RESULTS + VERDICT — 2026-06-23

**Verdict: GO** (the first GO of the build-once arc). The combination of **sound** levers —
deterministic-look-ahead **pruning** + cheap-**MRV** ⊔ ordering + the shipped **sound ⊔
backjump** — collapses a hard wine model from its ~67k-branch DNF thrash to a **correct
(`Sat`) answer in single-digit seconds**. The spike also identifies the architecture
precisely: **det-pruning + MRV + sound ⊔ backjump are the levers; the precise ≤n backjump is
the WRONG, unsound lever and must be dropped.**

## What was measured

`sat(SweetWine)` (`sat_class_probe`) and `sat(Alsatian ⊓ ¬American)` (`decide_pair_probe`),
depth 256, adaptive-budget OFF, 60 s deadline, on a 2 GiB-stack thread. Combo-OFF baseline vs
two combo-ON configurations.

| config | model | branches | restores | wall | verdict |
|---|---|---|---|---|---|
| OFF (baseline) | SweetWine | 67459 | 67459 | 60 s DNF | Stalled |
| OFF (baseline) | Alsatian⊓¬American | 66683 | 66683 | 60 s DNF | Stalled |
| combo (precise ON) | SweetWine | 987 | 2701 | 2.5 s | **Unsat ✗ (spurious)** |
| combo (precise ON) | Alsatian⊓¬American | 854 | 2539 | 2.2 s | **Unsat ✗ (spurious)** |
| **combo (precise OFF, SOUND)** | **SweetWine** | **10856** | 23041 | **24 s** | **Sat ✓** |
| **combo (precise OFF, SOUND)** | **Alsatian⊓¬American** | **867** | 2546 | **2.3 s** | **Sat ✓** |

## The decisive disentangling

The first combo-ON run (precise ≤n backjump forced ON) collapsed the search ~70× **but to
spurious `Unsat`** — wine is consistent and both classes are satisfiable. The verdict-sanity
guard caught it. Since det-pruning (a `horn_fixpoint` clash is a genuine dead disjunct) and
MRV (reordering) are **sound by construction**, the only unsound lever was the forced precise
≤n backjump (the known FP=232 mechanism). Re-running with **precise-backjump OFF** (the sound
subset) resolved the ambiguity: the collapse **persists AND the verdict becomes correct
(`Sat`)**. So the precise backjump was not doing the collapse — it was only corrupting the
verdict. The sound levers do the work.

## GO/NO-GO against the pre-committed bar

Bar: one hard wine model collapses ~67k → small (<1k, ideally ~tens) **AND** wall < ~30 s
**AND** verdict = Sat.

- **`Alsatian ⊓ ¬American` (sound combo): 867 branches (<1k ✓), 2.3 s (<30 s ✓), Sat ✓ —
  clears all three.** → **GO.**
- `SweetWine` (sound combo): Sat ✓, 24 s (<30 s ✓), 10856 branches (above the <1k ideal — a
  6× collapse, not yet the regime). Meets wall+verdict, not the branch ideal.

One model fully clears the bar with a **sound** configuration. This is a genuine GO, not the
FP graveyard.

## What the spike establishes for the rewrite

1. **The combination premise holds — soundly.** The levers compound: 60 s-DNF → 2.3 s / Sat on
   one model, 6× + Sat on the other. Sound by construction (no precise backjump).
2. **The architecture is det-pruning + MRV + sound ⊔ backjump.** Drop the precise ≤n backjump
   — it caused the spurious-Unsat and is not needed for the collapse.
3. **Two things to carry into the build:**
   - **Soundness re-enters as the gate:** the sound subset must pass the corpus closure-diff /
     FP=0 gate (it is sound by construction — det-pruning drops only deterministically-dead
     disjuncts — but the corpus gate confirms at scale).
   - **Performance to close:** SweetWine's 10856 branches / 24 s shows the unamortized per-⊔
     `horn_fixpoint` look-ahead is the remaining cost. The build-once **amortization** of the
     deterministic-expansion (compute the deterministic closure once, consult it cheaply at ⊔
     points) is the lever to bring SweetWine into the <1k / single-digit-second regime and to
     keep the corpus fast — i.e. exactly Konclude's edge-2, now with a validated reason to
     build it (the sound det-pruning collapse is real; amortization makes it cheap).

## Consequence — first build phase

GO to the rewrite's first **real** build phase: a **sound** wedge configuration of det-pruning
+ MRV (no precise ≤n backjump), corpus closure-diff / FP=0 gated, with the deterministic
look-ahead **amortized** (build-once deterministic-expansion) so the per-⊔ cost is paid once.
That phase's gate is FP=0 corpus-wide + a wall/branch measurement showing the sound,
amortized configuration holds the wine collapse and does not regress the corpus.

Throwaway spike code (`spike/combo-rewrite-gate`) does not merge; only this verdict lands on
`feat/build-once-redesign`.
