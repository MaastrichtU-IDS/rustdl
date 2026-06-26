# SP3 Phase-2 production ∃-seed — gate RESULTS + VERDICT (2026-06-26)

**VERDICT: GO.** Wiring the validated derived-∃-fact seed into the classify path delivers a
**44% wine classify speedup (49.3 s → 27.4 s), sound (FP=0/MISSED=0 byte-identical
corpus-wide)** — the coupled-saturation precompletion (named + ∃ seed) is the
Konclude-class wine lever, far beyond SP2's named-only ~7.5% ceiling.

## Soundness gate — FULL PASS (flag ON, 25 ms/pair, byte-identical)

`konclude_closure_diff`, `RUSTDL_SAT_SEED=1`, every oracled fixture FP=0/MISSED=0:
bibtex 16 · sulo 51 · alehif 247 · ro 158 · ore-10908 6001 · sio 8904 · galen 27997 ·
notgalen 32739 · ore-15672 142 · pizza 499 · **wine 653 (unsat:rustdl=0)** — all =
Konclude. The classify-scale ∃-coupling (per-pair `¬sup` × seeded ∃-structure across 137²
pairs + the label-cache build) is sound — the proof Phase-1 (per-class verdicts) could not
give.

## Wine classify wall — 44% (the production win)

`rustdl classify --pair-timeout-ms 25 wine.ofn`, fresh CLI binary:

| | misses | timed-out pairs | wall |
|---|---|---|---|
| flag OFF | 4009 | 4063 | **49.3 s** |
| **flag ON (named+∃)** | **1731** | **1861** | **27.4 s (−44%)** |

The ∃-seed (the deterministic value-assignment facts) makes the hard-tail classes
(CabernetFranc-type — DNF even under the named seed) **label within the deadline** →
the label cache prunes **57% more** pairs → the per-pair refutations that *are* the wine
wall collapse. SP2's named-only seed gave ~7.5%; named+∃ gives **44%**. Per-class evidence
(Phase-1): CabernetFranc DNF→209 ms (~250×); confirmed in the classify path
(`classify_labels(CabernetFranc) = Sat in 217 ms`).

## On the Chardonnay regression (Phase-1 caveat) — did not bite

The Phase-1 worry (Chardonnay named+∃ slower than named-only) is invisible at the production
deadline: Chardonnay's sat exceeds the label-cache deadline either way (miss in both), so it
never pays the regression while the collapsible hard-tail classes convert miss→labeled. Net
result confirms it: misses dropped 4009→1731 with no offsetting loss. **Selectivity not
needed** (the deferred fallback stays deferred).

## Method note (a measurement bug, caught)

An initial CLI wall read showed misses identical to named-only (∃ "inert") — traced to a
**malformed `cargo build -p A B` that silently skipped rebuilding the CLI binary** (stale
SP2.1 binary). The freshly-built binary shows the real 44%. Localized via a direct
`classify_labels(CabernetFranc)` timing (217 ms with ∃) + an `exists_seed` population check
(CabernetFranc: 5 ∃-seeds, 60 classes seeded, 194 total) — confirming the table + wiring were
always correct; only my build invocation was wrong.

## Verdict / consequence

GO: FP=0/MISSED=0 byte-identical corpus-wide AND a 44% net wine improvement (≫ SP2.1's 7.5%).
The coupled-saturation precompletion (SP2 named seed + SP3 ∃ seed) is a **sound, validated,
44% Konclude-class wine lever** — the culmination of the build-once arc, reversing its eight
prior NO-GOs. `RUSTDL_SAT_SEED` (named+∃) is ready to ship; recommend flipping default-ON (or
opt-in) per the controller, since it is sound corpus-wide and a large win on the SROIQ tail
with no measured regression.

## Disposition

`feat/sat-precompletion-sp3-prod` (Task 1 `663a675`). Default OFF pending the flip decision;
`main` untouched. The remaining headroom toward Konclude's ~114 ms is the residual ~1731
genuinely-too-slow misses (the SP1 saturation increments — richer ∀/≤n/nominal closure → more
∃-facts/class — would convert more, now with a measured payoff path).
