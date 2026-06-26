# SP-B2a (synthetic-conjunction forced-disjunct) FP=0 gate — results — 2026-06-23

Gate for B2a: `Sᵢ = C⊓Dᵢ` synthetics + `process_unsat` hook forcing the disjunction when
an `Sᵢ` becomes derivably unsat (via the saturator's own rules). FP=0 is sacred.
Verdict: **FP=0 — PASS.** Method mirrors the B1 gate.

## 1. Tuned closure-diff (oracle, full hybrid) — PASS

`konclude_closure_diff`, `RUSTDL_TEST_PAIR_MS=1000`, SP-A+B1+B2a active: **FP=0 / MISSED=0
on all distinct fixtures** (galen, notgalen, ore-10908, ore-15672, sio, pizza, alehif, ro,
sulo, bibtex) — closures byte-equal to oracle, no FP anywhere.

## 2. ORE pilot sweep (233 onts, oracled): B1 vs B1+B2a — PASS

`classify --saturation-only`, isolating B2a's effect.
- **Real FP signature (`a_unsat > b_unsat`): ZERO onts.** No spurious-unsat cascade.
- 13 onts differ (B2a's deep forcing). Every flagged ont with an oracle has
  **SP-A+B1+B2a saturation ≤ oracle**, = oracle exactly on many (11484 10526=10526,
  11502 27620=27620, 11623 27997=27997, 566 5310=5310, …). Sound recoveries.
- 9 "CASCADE" flags are the known artifacts (`removed>0` reduction reshuffle / `after=0`
  print-window) — none are real FPs (all `a_unsat==b_unsat`).

## Validation

- B2a differentiator canary forces `X⊑B` via a `C⊓Dᵢ`-unsat caught by functional-merge
  (`∃r.P ⊓ ∃r.Q`, `Disjoint(P,Q)`) — a deep incompatibility B1 cannot detect.
- B1's 6 canaries still pass (B2a complements, no regression).

## Conclusion

B2a is **FP=0**: sound by construction (`C⊓Dᵢ` unsat ⟹ entailed exclusion), oracle-confirmed
on the tuned fixtures, ≤-oracle on all oracled ORE-pilot flagged onts, zero spurious-unsat.
It generalizes B1's exclusion to any incompatibility the saturator proves (functional-merge,
existential, domain, …) — the deep-saturation foundation that B3 (nominal disjointness feeds
the same `Sᵢ` synthetics) extends. Default-classifier output unchanged.
