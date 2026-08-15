# Root cause: why a small per-pair budget flips `ore_ont_16372`'s consistency verdict

**Date:** 2026-08-15 · **Status: root-caused and verified. Not fixed.** This is the blocker on
lowering the `--pair-timeout-ms` default (`docs/2026-08-15-pair5-default-blocked.md`).

## The chain, end to end

`ore_ont_16372` is genuinely inconsistent (Konclude, HermiT and rustdl's own `consistent`
agree). `classify` gets it right at ≥100 ms per pair and wrong below:

| `--pair-timeout-ms` | unsat found | verdict |
|---|---|---|
| 1000 (default) | 744 | `consistent=false` ✅ |
| 100 | 744 | `consistent=false` ✅ |
| 25 | 0 | `consistent=true` ❌ |
| 5 | 0 | `consistent=true` ❌ |

Deterministic (3/3 repeats per arm), so not the budget-boundary flapping `family.ofn` shows.

**The detector is `probe_says_inconsistent` (`classify.rs:1398`), and it is triggered by
exactly three per-class unsat proofs.** Isolated with its own flag at pair=100:

| | verdict | unsat |
|---|---|---|
| probe ON (default) | `consistent=false` | 744 |
| `RUSTDL_CLASSIFY_CONSISTENCY_PROBE=0` | `consistent=true` | **3** |

Those 3 are the whole signal. 3/744 = **0.403%**, which is precisely the number the function's
own comment records as the reason its fraction threshold must stay low. The 744 is the
*consequence* of detection (`classify_inconsistent` marks every class unsat), not its cause.

So the chain is:

1. A small per-pair budget starves the per-class unsat probes. Each timed-out probe defaults
   to **satisfiable** — sound for subsumption, and the documented behaviour.
2. Those 3 proofs each need **>25 ms** of tableau time. Below that, none survives.
3. `unsatisfiable_idxs` becomes empty.
4. `probe_says_inconsistent` early-returns at its `unsatisfiable_idxs.is_empty()` guard
   (`:1406`), so layers 2 (wedge) and 3 (bounded `⊤` probe) never run — and layer 3 is what
   actually decides this ontology, in 0.36 s.

**The detector's own trigger is budget-sensitive.** That is the defect. It is not that the
budget makes the reasoning wrong; it is that the evidence the detector waits for is the first
thing a small budget destroys.

## Ruled out by measurement, not assumption

* **The ABox pre-check** — `abox_precheck_probe` reports `precheck_ms=0, clash=false, opa=0`
  at any budget. The ABox saturation finds nothing here.
* **`classify_inconsistency_precheck`** — its `RUSTDL_TRACE` line fires in **neither** arm
  (checked without truncating the trace, after an earlier `head -6` nearly dismissed it
  wrongly), and its inputs do not depend on `per_pair` at all.
* **The documented label-cache coupling** — `RUSTDL_LABEL_CACHE_TIMEOUT_MS=30000` at
  `--pair-timeout-ms 5` still yields `consistent=true`.
* **Nondeterminism** — 3/3 repeats per arm agree.

## Why the obvious fixes are hazardous

**Do not simply drop the `is_empty()` / fraction gates.** They exist because layers 2–3 are
expensive, and removing them has already been measured to break things: a 1,920-ontology sweep
took five ontologies to `dnf` on exactly that shape — a huge ABox admitted on the strength of
one unsatisfiable class, where the `⊤` probe's cost scales with the ABox rather than the unsat
count (`ore_ont_14881`, `6108`, `7416`, `7803`, `1966`, all at 0.005–0.063%).

**Do not floor the whole unsat probe.** 744 classes × 100 ms = 74 s on this ontology alone.

### The `ClassAssertion`-scoped floor: BUILT AND REFUTED (2026-08-15)

The obvious targeted fix is to floor the unsat-probe budget only for classes named in a
`ClassAssertion` — layer 1 reads exactly those, and there are **7** on `ore_ont_16372`, not
744. It was implemented and it **does not work**:

| config, `--pair-timeout-ms 5` | verdict |
|---|---|
| floor off | `consistent=true` |
| `RUSTDL_UNSAT_PROBE_FLOOR_MS=100` | `consistent=true` |
| `RUSTDL_UNSAT_PROBE_FLOOR_MS=250` | `consistent=true` |

**Because the premise is false.** The 3 classes proven unsat at 100 ms are `IDO_0000473`,
`IDO_0000568`, `IDO_0000653`. The 7 asserted types are `IAO_0000078/0000225/0000409` and four
`oboInOwl` classes. **The two sets are disjoint** — so layer 1 cannot be what fires here, and
flooring the asserted types buys nothing.

What those 3 proofs actually do is satisfy the **fraction gate** (3/744 = 4.03‰ ≥ 2‰). The
detection itself is **layer 3**, the one bounded `⊤` probe, which the code comment already
says decides this ontology in 0.36 s. The reverted implementation is not in the tree.

**The lesson for the next attempt: the gate needs unsat evidence from ANY class, and which
classes supply it is not predictable from the axioms.** A scoped floor cannot work, because
there is no syntactic subset to scope it to. The sizing done for that scope (median 16
asserted types, p90 123, max 2,568) measured the wrong population and does not apply.

### Remaining candidates, none built

* **Budget-independent gate evidence.** The saturation closure is the obvious source and is
  useless here — it knows **0** of this ontology's classes are unsat, which is why the
  tableau-derived 3 are load-bearing.
* **Gate on "incomplete AND has an ABox" instead of on the unsat fraction.** If probes timed
  out we do not *know* the fraction, so treating 0 as "no evidence" is the actual error. Cost
  would be one bounded 200 ms probe per incomplete ABox-bearing ontology. Needs measuring —
  including whether `decide_with_deadline` honours its deadline promptly on large ABoxes,
  since the recorded 5-ontology DNF regression suggests setup cost may precede the first
  deadline check (the same shape as the documented unbounded `abox_saturation` prelude).
* **Do nothing and keep the default at 1000 ms**, accepting that the 16 recoveries and −15.8%
  wall stay behind an explicit flag.

The trap to avoid is already on record in the code: an earlier version verified every
asserted-instance class through the main tableau with *unbounded* probes — 58 on `wine` — and
ran the FP=0 net 8h47m at 32 cores without finishing.

## What this blocks, and what it does not

Blocks the `--pair-timeout-ms 5` default, which otherwise passes both pre-registered clauses
(`ok → dnf` = 0, ΔMISSED +0.78%) and buys **16 recoveries** and **−15.8%** wall.

Does **not** block using `--pair-timeout-ms 5` explicitly on a known ontology — the four
`unsat_probe`-cluster members are FP=0/MISSED=0 exact at that setting.

Does **not** indicate unsoundness: an inconsistent KB entails everything, so the 5,371
subsumptions reported at 5 ms are all vacuously entailed. The defect is the **verdict**.

## Guard that now exists

`owl-reasoner-harness/scripts/release-corpus-report.sh` fails a release on any consistency-verdict
change, and `ore_ont_16372` is in its sentinel list precisely because the stratified 400-ontology
population does **not** contain it. Validated end-to-end: re-running the same binary at
`--pair-timeout-ms 5` against the v0.4.18 baseline fails with exactly
`ore_ont_16372: consistent False → True`.
