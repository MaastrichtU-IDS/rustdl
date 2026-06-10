# Perf fix: per-class unsat-probe via the label cache (2026-06-10)

Branch `perf/unsat-probe-via-label-cache`. Implements the lever the tier-walk
profile located (`docs/superpowers/specs/2026-06-10-global-model-rewrite-design.md`
§ TIER-WALK PROFILE): the top-down classifier's per-class unsat-probe ran the
**main tableau** (`prepared.decide_with_deadline`) once per class — profiled as
the dominant classify wall — even though the Phase-7 label cache (the **wedge**)
already decided satisfiability for those classes during its build.

**Change (`classify.rs` unsat-probe + `lib.rs::unsat_via_labels_enabled`,
default ON, opt-out `RUSTDL_UNSAT_VIA_LABELS=0`):** before the main-tableau
probe, consult `label_cache[i]`: `LabelOracle::Unsat → unsat`,
`Sat → sat`, `NoVerdict`/absent → fall through to the main-tableau probe
(unchanged). Closure-unsat short-circuit unchanged.

**Soundness:** `LabelOracle::Unsat` is a wedge `Unsat` — sound for any ontology
(the trusted direction) and already trusted in `find_direct_parents_top_down`;
`Sat` matches the established `trust_sat` model the label cache + pruning rely
on; `NoVerdict` keeps the main-tableau path. Verified verdict-IDENTICAL (ON vs
OFF) + FP=0/MISSED=0 vs the oracle.

## Results (OFF = pre-fix main-tableau pass; ON = fix, default)
| ontology | OFF wall | ON wall | speedup | verdicts |
|---|---|---|---|---|
| alehif | 6.46 s | **0.30 s** | **21×** | identical |
| ore-10908 | 23.1 s | **1.03 s** | **22×** | identical |
| pizza | 8.43 s | 4.47 s | 1.9× | identical |
| GALEN (EL) | 0.59 s | 0.58 s | — (EL path) | identical |
| ore-15672 | 140 s | ~138 s | — (walk-probe-bound) | (oracle gate) |

Big wins where the per-class main-tableau unsat-probe dominated (alehif,
ore-10908); partial on pizza (also has walk-probe cost); neutral on EL (GALEN)
and on the walk-probe-bound ore-15672 (its cost is the hard SHOIN per-pair
probes, untouched by this fix). lib tests 100/0; clippy + fmt clean.
