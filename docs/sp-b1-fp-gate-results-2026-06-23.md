# SP-B1 (derived-closure forced-disjunct) FP=0 gate — results — 2026-06-23

Gate for the B1 in-saturator forced-disjunct rule (`process_subsumer`, using the
derived subsumer closure × disjointness). FP=0 is sacred. Verdict: **FP=0 — PASS.**
Method mirrors `docs/sp-a-fp-gate-results-2026-06-23.md`.

## 1. Tuned closure-diff (oracle-backed, full hybrid) — PASS

`konclude_closure_diff`, `RUSTDL_TEST_PAIR_MS=1000`, SP-A+B1 active:
**FP=0 / MISSED=0 on all 12 fixtures** (galen, notgalen, ore-10908, ore-15672, sio,
pizza, alehif, ro, sulo, bibtex) — closures byte-equal to the oracle, precision=1.0.

## 2. ORE pilot sweep (233 onts, oracled): SP-A vs SP-A+B1 — PASS

`classify --saturation-only`, before = SP-A, after = SP-A+B1, isolating B1.

- **Real FP signature (spurious-unsat jump, `a_unsat > b_unsat`): ZERO onts.** The
  increment-3 trap is structurally absent (B1 is atomic-only; no nominal disjointness,
  no functional-merge pooling).
- 37 onts show a saturation-only closure change. Every flagged ont with an oracle has
  **SP-A+B1 saturation closure ≤ oracle**, and **= oracle exactly** on many
  (ore_ont_13752 15947=15947, 14450 20204=20204, 4578 56331=56331, 13852 12341=12341,
  …). So the changes are B1's derived-closure forcing driving the *saturator* to the
  full oracle closure on disjunctive onts — sound recoveries, not FP.
- The sweep's "CASCADE" flags were over-sensitive: `removed>0` is the benign
  transitive-*reduction* reshuffle when subsumptions are added; `after=0` was a
  measurement artifact (B1's larger closure exceeded the sweep's 120s print/timeout
  window — direct re-runs show the full oracle closure, not 0). None are real FPs.
- "no-oracle" flagged onts (16666, 2313, 2749, 2792, 3077, 443) lack an oracle in
  `diff.json` (Konclude/HermiT failed on them) — not comparable, no FP evidence.

## Conclusion

B1 is **FP=0**: sound by construction, oracle-confirmed on 12 tuned fixtures, ≤-oracle
(often =oracle) on all oracled ORE-pilot flagged onts, zero spurious-unsat across 233
onts. Default-classifier output unchanged (hybrid FP=0/MISSED=0 byte-equal); the gain
is the *saturator* reaching the oracle closure on disjunctive ontologies via the
derived-closure forced-disjunct — the deep-saturation foundation B2/B3 extend.
