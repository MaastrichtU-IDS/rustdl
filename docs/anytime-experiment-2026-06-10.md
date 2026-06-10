# Anytime-under-deadline experiment — FALSIFIED → Resource/In-Use track

Date 2026-06-10. The one untested research-track claim (C3/F1): "under a tight
deadline, rustdl returns a sound partial hierarchy where a complete reasoner
killed at the same deadline returns nothing." Designed to falsify (advisor).

**Mechanism check:** rustdl's fastest sound partial = `--saturation-only` (the
EL closure, sound lower bound). Konclude is all-or-nothing (killed mid-run →
empty output, confirmed). So a win needs a deadline B with
`T_rustdl-partial < B < T_konclude-complete`.

**Measured `T_konclude` (complete) vs `T_rustdl-partial`:**
| ontology | Konclude complete | rustdl partial | window? |
|---|---|---|---|
| GALEN (EL, 27 997) | 0.31 s | 0.57 s (sat) | NO (Konclude faster + complete) |
| ore-10908 (SROIQ) | 0.12 s | — | NO |
| ore-15672 (hard SHOIN) | **0.04 s** | (full 140 s) | NO |
| ore_ont_12898 (564 MB) | 62 s | **crashes, rc=101** | NO (rustdl can't run it) |

**Verdict: FALSIFIED.** Konclude finishes *complete* faster than rustdl
produces *any* partial on every ontology rustdl can handle (including the
hardest, ore-15672 @ 0.04 s); on the giants Konclude can run but rustdl crashes,
so still no partial. There is no ontology class where rustdl uniquely delivers a
sound partial under a deadline. The anytime advantage has **no witness** —
Konclude is simply a far more optimized engine.

**Paper consequence: Resource / In-Use track, decided.** The defensible
contribution is NOT a speed/anytime research result. It is:
- **C1 soundness** — FP=0 corpus-wide + ORE-validated (the robust headline);
- **C5 embeddability** — native, in-process, ~2 ms cold start / 5–30 MB on the
  EL/Horn niche (vs JVM startup + heap), no subprocess/license;
- **C2 calibrated incompleteness** — genuine incompleteness characterized to 10
  pairs / 1 pattern on ORE; signalled, not silent (modulo the two channels
  found + fixed/closed this work);
- methodology note (JVM/docker startup confound).

Side finding: rustdl PANICS (rc=101) on the 564 MB ontology rather than erroring
gracefully — a robustness bug, not blocking, worth a guard.
