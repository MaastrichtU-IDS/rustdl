# Phase-0 combination spike — RESULTS + VERDICT — 2026-06-23

**Verdict: GO RETRACTED → diagnosing.** The initial GO was REFUTED by the wine full-closure FP
gate: the "sound" combo (det-pruning + MRV, precise OFF) produces **FP=232 on wine**
(rustdl_closure=885 vs konclude 653; 232 spurious SUBSUMPTIONS; MISSED=0; unsat rustdl=0) —
det-pruning DROPS LIVE DISJUNCTS on wine's nominal/cardinality pairs (sat(C⊓¬D) wrongly clashes).
FP=0 on pizza/bibtex but FP=232 on wine ⟹ det-pruning's failed-literal soundness has the
nominal-context hole (deterministic Horn closure is branch-dependent under nominals+merge). The
~70×/2.3s collapse was real PERFORMANCE but achieved UNSOUNDLY. **The GO was presented prematurely
(on pizza/bibtex + 2 probes, before the wine capstone). The performance signal stands; the
soundness claim does not. Now disentangling which lever FPs and whether it is fundamental
(nominal-context) or a recoverable implementation leak.** See the "FP REFUTATION + diagnostics"
section at the end.

---

## (Superseded) initial GO writeup

The combination of levers — deterministic-look-ahead **pruning** + cheap-**MRV** ⊔ ordering +
the shipped **sound ⊔ backjump** — collapses a hard wine model from its ~67k-branch DNF thrash to
a `Sat` answer in single-digit seconds (Alsatian⊓¬American 867 br / 2.3 s; SweetWine 10856 / 24 s).
The precise ≤n backjump (forced on) caused a spurious-Unsat collapse; dropping it gave the correct
per-probe verdict. **BUT the full-wine closure FP gate (below) shows the precise-OFF combo is STILL
FP-unsound on wine (FP=232) — the per-probe Sat verdicts were not representative of the full
classify.** Read the FP-refutation section as the operative verdict.

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

---

## FP REFUTATION + diagnostics (THE OPERATIVE VERDICT — supersedes the GO writeup above)

The initial GO was presented on pizza/bibtex FP=0 + 2 individual `Sat` probes, before the full
wine closure-diff landed. The wine capstone refuted it: **FP=232** (rustdl 885 vs konclude 653,
MISSED=0, unsat rustdl=0) — 232 spurious subsumptions (`sat(C⊓¬D)` wrongly clashing).

**Disentangling** (throwaway sub-flags `RUSTDL_COMBO_NO_DETPRUNE` / `RUSTDL_COMBO_NO_MRV`; wine
closure-diff at `RUSTDL_TEST_PAIR_MS=25` — spurious subsumptions complete fast, so a tight
deadline reproduces the FP in ~56 s):

| config (25 ms) | wine FP | verdict |
|---|---|---|
| full combo (det-pruning + MRV) | 156 | unsound |
| **det-pruning only** | **7** | **unsound (the culprit)** |
| **MRV only** | **0** (653=653, MISSED=0) | **sound** |

**Root cause of det-pruning's unsoundness:** `horn_fixpoint` is *deterministic* — it does NOT
perform the ≤n **merge** / nominal-identification (branching moves `solve_at_most` makes). On
wine's `≤1`+nominal-value-partition pattern the look-ahead sees ">n successors" and **clashes**
where the real search would **merge and proceed**, reading a *merge-resolvable* clash as "`Dₖ`
dead" and **dropping a live disjunct**. Fundamental to the look-ahead's determinism on the
≤n/nominal fragment (FP=0 on pizza/bibtex, which lack it). MRV *amplifies* it ~22× (7 → 156).

**The corrected, SOUND result — MRV alone:**

| model | OFF | **MRV-only (sound)** |
|---|---|---|
| sat(Alsatian⊓¬American) | 66683 / 60 s DNF | **1227 br / 1.2 s / Sat** |
| sat(SweetWine) | 67459 / 60 s DNF | **12366 br / 15.6 s / Sat** |

MRV alone collapses the hard models 5–54× to the correct `Sat`, **FP=0/MISSED=0 on the full wine
closure (653=653)**. The full combo's collapse (867 / 10856) is barely better than MRV-only
(1227 / 12366) — **det-pruning added ~nothing to the collapse and all of the unsoundness.**

**OPERATIVE VERDICT: GO via MRV (sound variable ordering); DROP det-pruning (unsound +
~unnecessary, and there is no amortization to build — MRV has no look-ahead).** MRV is sound by a
stronger argument than det-pruning ever had — reordering which open ⊔ to branch first is
verdict-invariant (same search space, better order), no nominal-context hole — *and* it is
empirically FP=0/MISSED=0 on wine. **First build phase: promote MRV to a sound wedge feature
(cheap per-branch most-constrained-⊔ scan, no look-ahead), gated on full-corpus FP=0 +
no-regression** (the build-phase proof; not yet run — soundness-by-construction + FP=0-on-wine is
strong, but the corpus-wide gate is the proof, per this session's repeated lesson). The
"amortized deterministic-expansion" path in the GO writeup above is moot: MRV needs no look-ahead.
