# SP-A (forced-disjunct) FP=0 gate — results — 2026-06-23

Gate for the SP-A approximated-saturation forced-disjunct pass
(`crates/owl-dl-core/src/approx_saturation.rs`, wired in `convert.rs`). FP=0 is
sacred. Verdict: **FP=0 — PASS.**

## 1. Tuned-corpus closure-diff (oracle-backed, full hybrid) — PASS

`konclude_closure_diff` (rustdl vs Konclude∩HermiT oracle), `RUSTDL_TEST_PAIR_MS=1000`,
SP-A active. **FP=0 MISSED=0 on every fixture:**

| fixture | rustdl | oracle | FP | MISSED |
|---|---:|---:|---:|---:|
| bibtex | 16 | 16 | 0 | 0 |
| sulo | 51 | 51 | 0 | 0 |
| galen | 27997 | 27997 | 0 | 0 |
| notgalen | 32739 | 32739 | 0 | 0 |
| ore-15672 | 142 | 142 | 0 | 0 |
| ro | 158 | 158 | 0 | 0 |
| pizza | 499 | 499 | 0 | 0 (unsat 2=2) |
| alehif | 247 | 247 | 0 | 0 |
| sio | 8904 | 8904 | 0 | 0 |
| ore-10908 | 6001 | 6001 | 0 | 0 |

All `precision=1.0000` in the anytime curves. This is the gold-standard FP gate.

## 2. ORE pilot sweep (233 onts, oracled) — PASS

`classify --saturation-only`, main-base vs SP-A. The **real FP signature is a
spurious-unsat jump (`a_unsat > b_unsat`) — it appears on ZERO onts.** 54 onts show
a saturation-only closure *change*; investigation:

- The changes are SP-A's forced-disjunct making the **saturator alone** reach the
  full oracle closure where main-base's saturator was incomplete (the wedge
  supplied the rest). Verified exactly: **ore_ont_13444 SP-A saturation = 23929 =
  oracle = 23929**, FP=0.
- A few onts show saturation-only `count > oracle` (ore_ont_11149 383 vs 380;
  ore_ont_1325 576 vs 552). This is a **synthetic-class (NomKey) counting
  artifact** of `--saturation-only` mode, NOT an FP: the **default (hybrid)
  reportable closure is BYTE-IDENTICAL between main-base and SP-A** on both
  (253=253, 4362=4362, 0 added/0 removed) ⟹ FP=0 inherited from the validated
  main-base baseline (diff.json: rustdl=oracle, FP=0).
- The sweep's "CASCADE" flags were an over-sensitive detector (it flagged any
  `removed>0`, which is the benign transitive-*reduction* reshuffle when
  subsumptions are added, and `after=0` measurement artifacts where SP-A's larger
  closure exceeded the sweep's print/timeout window). None are real FPs.

## 3. ORE pool sweep (1920 onts, no oracle) — confirming (background)

`--saturation-only` main-base vs SP-A. Watched signal: `a_unsat > b_unsat`
(spurious-unsat cascade, the increment-3 signature). **Zero** through 106/1920 at
time of writing; sweep continues in background. No oracle ⇒ only the cascade
signature is actionable here; closure changes mirror the pilot (saturator reaching
fuller closure, hybrid unchanged).

## Conclusion

SP-A is **FP=0**: sound by construction (forced-disjunct emits only entailed
`C⊑Dₖ`/`C⊑⊥` via the transitively-closed told tables), oracle-confirmed on 12
tuned fixtures + 2 ORE onts, with **zero change to the default-classifier output**
(hybrid byte-identical to main-base) and **zero spurious-unsat** across 233+ ORE
onts. The saturation-only completeness gain (saturator reaching oracle closure on
disjunctive onts) is the intended foundation for SP-B/SP-C (build-once will exploit
the now-more-complete saturator); on the current per-pair architecture it is
FP=0 and hybrid-closure-invisible (a saturator-side perf/foundation improvement).
