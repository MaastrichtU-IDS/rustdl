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

**The promising shape: floor the budget only for classes named in a `ClassAssertion`.** Layer 1
needs unsat status for exactly those, and there are **7** on `ore_ont_16372`, not 744. Sized
over the 424-ontology release population:

| distinct asserted types per ontology | median | p90 | max |
|---|---|---|---|
| count | 16 | 123 | **2,568** (`ore_ont_9694`) |
| cost of a 100 ms floor | 1.6 s | 12.3 s | **257 s** |

262 of 424 ontologies carry at least one `ClassAssertion`. So the median case is cheap and the
tail is not — a flat floor would cost `ore_ont_9694` 257 s and push it over any cap. **Any
implementation needs a cap on the number of floored probes, and that cap needs its own
measurement.** This is the same trap the code comment already records: an earlier version
verified every asserted-instance class through the main tableau with *unbounded* probes — 58 of
them on `wine` — and made the FP=0 net run 8h47m at 32 cores without finishing.

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
