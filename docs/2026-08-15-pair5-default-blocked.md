# `--pair-timeout-ms 5` passes both pre-registered clauses and must NOT ship

**Date:** 2026-08-15 · **Recommendation: do not flip the default.** The blocker is an
answer change the pre-registered gate is structurally unable to see.

## The gate passes

Same v0.4.18 binary (`sha 17aeec66e978`) on both arms, 1,920 ontologies, 60 s cap, 1 thread,
sequential. Arm A is the v0.4.18 release sweep; only the budget differs.

| clause | result |
|---|---|
| `ok → dnf` regressions | **0** |
| ΔMISSED (400-ont net, same-binary control) | **+6 (+0.78%)** vs a 5% threshold |
| FP (all three MISSED-net arms) | **0** |
| recoveries | **16** |
| wall over both-arm completers | 3,456 s → 2,911 s (**−15.8%**) |

The 16 recoveries include **`ore_ont_10019` at 2.3 s** — the ontology behind the
surrogate-atom design, the CB-reopen question, and the standing "dense SROIQ needs an
architecture" framing. A constant fixes it.

`--pair-timeout-ms 1` was screened and **fails**: ΔMISSED **+360 (+46.75%)**, ~9× the
threshold. The cliff sits between 1 ms and 5 ms.

## The blocker: a consistency verdict flips

`ore_ont_16372` is genuinely **inconsistent** — Konclude, HermiT and rustdl's own
`rustdl consistent` all agree independently
(`docs/2026-08-09-ore16372-misclassification-rootcause.md`).

| budget | `classify --json` |
|---|---|
| default (1000 ms) | `consistent=false`, 744 unsat, 0 direct — **correct** |
| 100 ms | `consistent=false`, 744 unsat — correct |
| **25 ms** | `consistent=true`, 0 unsat — **wrong** |
| **5 ms** | `consistent=true`, 0 unsat, 747 direct — **wrong** |

Deterministic: three repeats per arm, no flipping. So this is not the
budget-boundary nondeterminism that `family.ofn` exhibits.

**Severity.** Not a false subsumption: an inconsistent KB entails everything, so the 5,371
reported subsumptions are all vacuously entailed. The defect is the **verdict** —
`consistent: true` on an ontology with no models, which is the exact contradiction v0.4.8's
`RUSTDL_CLASSIFY_INCONSISTENCY` was introduced to eliminate. Shipping a default that
reintroduces it on a corpus ontology would undo that work.

The run is at least flagged `incomplete: true`, so it is not silent — but "incomplete"
understates "the consistency answer is wrong".

## Why the gate could not see it

Both arms **complete**, so it is not `ok → dnf`. And the MISSED net cannot price it either:
`oracle_diff::aligned_closures` excludes unsatisfiable classes **on both sides** before
diffing, so an ontology whose entire class set is unsat contributes ~nothing to ΔMISSED.

**The gate measures completeness and outcome. It does not measure verdict correctness.**
That is a real hole, and it is the reason the full-corpus output differential — not the two
clauses — caught this.

## Mechanism: NOT resolved

Stated plainly rather than guessed at. What has been ruled out by measurement:

* **Not the ABox pre-check.** `abox_precheck_probe` on this ontology reports
  `precheck_ms=0, clash=false, opa=0` — the ABox saturation finds nothing, at any budget.
* **Not the label-cache coupling.** The documented
  `label-cache-budget-starved-by-small-pair-timeout` interaction does not explain it:
  `RUSTDL_LABEL_CACHE_TIMEOUT_MS=30000` with `--pair-timeout-ms 5` still yields `unknown`.
* **Not `classify_inconsistency_precheck` firing differently.** Its inputs
  (`closure.globally_inconsistent() || closure.top_is_unsat() || abox_saturation_…`) do not
  depend on `per_pair` at all, and its `RUSTDL_TRACE` line fires in **neither** arm.
* **Not nondeterminism** — 3/3 repeats per arm agree.

So `stats.inconsistent` is being set on the default path by something that a smaller per-pair
budget suppresses, and the responsible site has not been identified. **Anyone continuing here
should start by finding what sets `stats.inconsistent` on this ontology at 1000 ms**, since
every obvious candidate is eliminated above.

## Recommendation

1. **Do not flip the default.** One wrong consistency verdict on a corpus ontology outweighs
   16 recoveries, and the recoveries remain available today via an explicit
   `--pair-timeout-ms 5`.
2. **Fix the verdict defect first**, then re-run this exact comparison. The evidence that the
   budget is otherwise cheap is strong and will keep.
3. **Add a verdict clause to the gate.** The pre-registered rule should become:

   > Ship iff `ok → dnf` = 0 AND ΔMISSED < 5% AND **no ontology changes its `consistent`
   > verdict**.

   The third clause is cheap — it is one field per ontology in output already captured — and
   this exercise shows the first two do not imply it.
4. `--pair-timeout-ms 1` is a **documented reject**: +46.75% MISSED.

## Method note

Four ontologies showed *added* `direct` rows at 5 ms, which reads as a soundness alarm. Three
were artifacts: `direct` is a transitive **reduction**, so losing an intermediate subsumption
promotes an endpoint to a direct edge. Their closures were proper subsets — each lost exactly
2 subsumptions and gained 0. Only `ore_ont_16372` was real, and its closure went 0 → 5,371
because the *default* arm reports the ontology inconsistent and elides every row.

Comparing `direct` rows would have raised three false alarms and mis-framed the true one.
**Compare closures, not reductions.**
