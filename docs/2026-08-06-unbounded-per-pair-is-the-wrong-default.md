# The DNF tail is not a weak-search problem: ~12 ontologies are search-bound and near-complete at a **1 ms** per-pair budget

> # ⚠️ TITLE AND THREE NUMBERS CORRECTED 2026-08-06. The filename is now wrong too.
>
> **The per-pair default is NOT unbounded — the CLI default is 1000 ms**
> (`owl-dl-cli/src/main.rs:155`, `README.md:183`); unbounded is only the *library* default.
> So the framing "unbounded is the wrong policy" is false, and the real statement is the much
> smaller "1000 ms loses where 1 ms wins". **The flag's own doc comment already said this** —
> it records `--pair-timeout-ms 25` being 7.5× faster on `wine` with an identical hierarchy
> and corpus-wide MISSED=0. The insight was pre-existing.
>
> **The addressable set is ~12, not 23.** My arms ran with all 32 cores; the gates run
> `--threads 1`. Verified: `ore_ont_14272` at pt=1 is **2.69 s parallel, 73.65 s
> single-threaded**. Re-run single-threaded at a 60 s cap the partition is **12 `dnf → ok`**,
> 25 still `dnf`, 2 already ok — and five ontologies scored "exact" below DNF under gate
> conditions, with survivors needing 5.7–20.4 s rather than 0.2–7.8 s.
>
> **"A restart is nearly free" was wrong by ~107×.** `prepare=5 ms` covers only
> `from_internal` + `abox_verdict`; a re-run also redoes convert, `saturate` and
> `label_cache_build` ≈ **536 ms of 2,850 ms**. I read `prepare=5` off a line printing
> `label_cache_build=525` beside it.
>
> **What survives:** a small per-pair budget really does convert no-answer into a
> near-complete answer on ~12 tail ontologies, and `--pair-timeout-ms` / `--global-timeout-ms`
> already ship for exactly that. The plan built on this document is **retracted**
> (`docs/superpowers/plans/2026-08-06-deadline-triggered-pair-budget.md`); the recommendation
> is to document the flags and build nothing.
>
> **Newly found, and the real prize here:** a tighter per-pair budget silently shrinks the
> label-cache budget (`lib.rs:2721-2735`, 30,000 ms → 178 ms), making `ore_ont_15010` go
> **5.6 s → 104 s for a byte-identical hierarchy**. That is a bounded defect worth its own
> work.
>
> **Fourth self-correction on this document.** All four were the same error in different
> clothes: a number measured under one configuration read as a property of the mechanism —
> Hasse-vs-closure, a budget-artifact `rescued=0`, 32-core-vs-single-thread walls, and a
> banner field read without its neighbours.

**Date:** 2026-08-06 · **Population:** the 39 Set-A tail members Konclude classifies in
**under 1 s** (`baselines/2026-08-04-triage-konclude-c120.jsonl`)

## The partition

Two arms per ontology, 60 s cap: default `classify`, and `classify --pair-timeout-ms 1`,
which caps per-pair search to ~zero. An ontology that still fails under the second arm
cannot be rescued by *any* per-pair search improvement.

| | n |
|---|---:|
| already `ok` at default (recovered since the 08-04 triage) | 3 |
| **`dnf` → `ok` once per-pair search is capped** | **23** |
| `dnf` even with per-pair search capped | 13 |

So per-pair search is the binding cost for **23 of the 36 still-failing** — a genuine
addressable set, against the 1–2 ontologies every other lever examined this week had.

## The finding: a 1 ms budget is near-complete, and the default is unbounded

Normalised transitive closures, rustdl at `--pair-timeout-ms 1` versus Konclude
(both through `normalise.py`, so this is closure-vs-closure):

| ontology | Konclude | rustdl @1 ms | |
|---|---:|---:|---|
| `ore_ont_10109` | 481 | **481** | exact |
| `ore_ont_934` | 365 | **365** | exact |
| `ore_ont_12723` | 9,250 | 9,249 | 100.0% |
| `ore_ont_15526` | 9,397 | **9,397** | exact |
| `ore_ont_10460` | 2,859 | **2,859** | exact |
| `ore_ont_2901` | 5,281 | **5,281** | exact |
| `ore_ont_14272` | 4,137 | **4,137** | exact |
| `ore_ont_1707` | 8,350 | 8,336 | 99.8% |
| `ore_ont_9864` | 4,443 | **4,443** | exact |
| `ore_ont_8429` | 5,049 | **5,049** | exact |
| `ore_ont_10807` | 4,789 | **4,789** | exact |
| `ore_ont_5764` | 5,678 | **5,678** | exact |
| `ore_ont_6485` | 240 | 235 | 97.9% |
| `ore_ont_10019` | 162 | 157 | 96.9% |
| `ore_ont_6333` | 351 | 328 | 93.4% |
| **total (12-ontology batch)** | **59,949** | **59,911** | **99.9%** |

**Every one of these produces NO ANSWER AT ALL at the current default**, and produces a
96.9–100% closure in 0.2–7.8 s at a 1 ms per-pair budget. 9 of 12 in the batch match the
oracle exactly.

**Raising the budget buys nothing.** 1 ms → 50 ms adds **0–2 pairs** per ontology
(157→159, 235→237, 481→481), while wall grows with the budget until the ontology DNFs
again at 500 ms. **A pair that does not resolve in 1 ms essentially never resolves**, so
unbounded waiting is pure loss.

## Why this reframes the frontier

The standing account is that this tail needs a clash-driven search rewrite — a large
project. That may still be true for the 13 ontologies where search is *not* the cost. But
for these 23 the search is not weak: it already finds ~all entailments almost immediately.
**The defect is the POLICY of unbounded per-pair search**, which spends the entire wall on
a handful of pairs that never converge and therefore returns nothing.

That is a defaults problem, not an algorithms problem.

## Proposed design — and why the obvious version is worse

The obvious move, a small global default per-pair budget, has a **measured** cost: the
record already contains a pre-registered MISSED-net arm at 1 ms per-pair showing
**ΔMISSED +80** corpus-wide. That taxes the ~1,750 ontologies that already answer
completely, to help 23.

**The better shape has no cost on completers: keep the default unbounded, and add a
deadline-triggered FALLBACK.** If a classify run exceeds a wall threshold, restart (or
continue) with a small per-pair budget. Ontologies that complete never see a budget, so
ΔMISSED on them is **0 by construction**; only would-be-DNFs pay, and for them the
alternative is no answer at all. rustdl already reports `incomplete: true` when pairs are
cut, so the signal is preserved.

**Unmeasured and required before building:** the restart's cost (a fallback that re-does
preparation may not fit inside a useful budget), the threshold's basis, and the two gates.
The `RUSTDL_CLASSIFY_INCONSISTENCY_MS` history is the cautionary precedent for picking a
time constant by intuition.

## An attractive-looking lever, KILLED by a confound check before it was built

Phase timings answer the restart-cost question decisively — on `ore_ont_14272` at pt=1,
`prepare=5 ms` of a 2,808 ms wall (`saturate=3`, `label_cache_build=525`, `tier_walk=1608`,
`sweeps=652`). **A fallback that re-does preparation costs ~5 ms**, so a restart is
affordable.

The same banner surfaced something that looked far better than a budget: the
**wedge-stall → main-tableau fallthrough**. Measured at pt=1 across the 15 ontologies:

| | |
|---|---:|
| fallthrough probes run | **251,959** |
| **rescued** | **0** |
| rescue rate | **0.0000%** |

That reads as pure waste, and suggests a clean sound lever: after N fallthroughs with no
rescue, stop falling through — subtractive, so a MISS at worst, never an FP.

**It is an artifact of the budget, and the check that found this is one command.** At pt=1
the *tableau* probes are cut at 1 ms too, so of course they rescue nothing. Re-measured on
`ore_ont_10019` at a 500 ms budget:

```
# fallthrough (wedge-stall→tableau): ran=156 rescued=2 (of which diverged-stall=2)
```

**The fallthrough does rescue — it is just slow.** So `rescued=0` measures the budget, not
the mechanism, and the fallthrough-cut lever is **NO-GO**: it would discard genuine
entailments. The 2 rescues are also exactly the size of the observed gap (pt=1 gives 157 of
162 on this ontology; pt=50 gives 159), so the numbers are mutually consistent.

**This does not weaken the main finding** — a small budget still buys 96.9–100% of the
oracle closure where the default buys nothing. It relocates the cost: the missing pairs are
precisely the ones the slow fallthrough would have rescued, which is a *completeness* price
to be priced by the MISSED net, not a free win.

Also recorded: `pizza`, `sio` and `ro` emit **no fallthrough banner at all** at default, so
this path is specific to the hard population and a change to it cannot regress them.

## Scope and honesty

- The completeness table is **15 of the 23** (12 in the widened batch, 3 earlier). Not all 23.
- **ΔMISSED +80 is quoted from the existing record, not re-measured here.**
- The fallback design is **proposed, not measured**. No implementation exists.
- An earlier version of this analysis compared rustdl's **Hasse** output against Konclude's
  **transitive closure** and read 58-vs-162, i.e. catastrophic incompleteness. That was an
  invalid comparison and it inverted the conclusion; the table above normalises both sides.
  Worth stating because the wrong version was briefly convincing.
- The **fallthrough-cut lever was killed by a confound check, not by a build** (above). Both
  errors in this document were of the same kind: a number measured under one configuration
  read as a property of the mechanism. That is now the most frequent failure mode in this
  session's record, and a confound check is the cheapest guard against it.
