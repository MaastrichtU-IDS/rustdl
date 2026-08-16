# `--global-timeout-ms` does not bound the wall, and the overshoot scales with class count

**Date:** 2026-08-16 · Found while scanning whether an internal global deadline should be a
default. **Defect, not yet fixed.**

## The measurement

Same binary, `--pair-timeout-ms 5`, `--global-timeout-ms 20000`, 1 thread:

| ontology | classes | wall | overshoot |
|---|---|---|---|
| `ore_ont_7192` | 71,033 | **58.1 s** | **2.9×** |
| `ore_ont_2574` | 81,270 | 35.3 s | 1.7× |
| `ore_ont_11311` | 8,022 | 20.8 s | 1.0× |
| `ore_ont_9944` | 8,008 | 20.2 s | 1.0× |
| `ore_ont_12567` | 232,084 | 17.7 s | 0.8× (finished naturally) |

**The overshoot appears only when the deadline FIRES**, and then grows with class count:
~1 s at 8k classes, 15–38 s at 71–81k. `ore_ont_12567` has 232k classes and no overshoot,
because it completes before the deadline — so this is not a function of size alone, it is
post-deadline work.

## Where the time goes

`ore_ont_2574` at a 30 s deadline, wall 46.6 s:

| segment | ms |
|---|---|
| parse + convert (before any phase timer) | 4,063 |
| measured phases, summed | 30,072 — exactly the deadline |
| **unaccounted, after the phases** | **~12,500** |

Ruled out by measurement:

* **Not output writing** — redirecting to `/dev/null` gives 49.7 s, no better.
* **Not the Hasse reduction** — `RUSTDL_FAST_DIRECT_SUBSUMERS=1` vs `=0` is 46.6 s vs
  46.3 s.

The remaining candidates are the per-class structures built after the reasoning phases for
an 81,270-class ontology (class vector, index map, entailment matrix, and teardown of the
`PreparedOntology`), none of which is inside the deadline's accounting. **Not yet localised
further** — the phase instrumentation stops before this region, which is precisely why it
was invisible.

## Why it matters

1. **It is a user-facing contract violation.** `--global-timeout-ms 20000` producing a 58 s
   run is surprising, and there is no flag to bound the difference.
2. **It manufactured 2 of the 3 "regressions"** in the global-deadline scan. `ore_ont_2574`
   and `ore_ont_7192` completed *without* a deadline in ~55 s, and were killed *with* a 55 s
   deadline inside a 60 s cap — the deadline fires, then the unbounded tail exceeds the
   remaining 5 s. `ore_ont_2574` at a 30 s deadline completes at 48.9 s with all 57,851
   rows, confirming the mechanism: the answer was ready, the tail spent the budget.
   (The third, `ore_ont_10689`, was cap-boundary noise — 3/3 repeats complete at 57.2–59.2 s.)
3. **It is the deadline's ONLY real cost** — see the retraction below.

## What the scan showed anyway

Arm A = v0.4.19 defaults, no internal deadline. Arm B = the same plus
`--global-timeout-ms 55000`, inside a 60 s cap, 1,920 ontologies:

| | |
|---|---|
| produced nothing before → a hierarchy now | **113** |
| `ok → dnf` | 3 (**2 real**, both caused by the defect above) |
| complete in both arms | 1,774 |
| wall over both-arm completers | 3,012 s → 3,260 s (+8.3%) |

## RETRACTED: the +8.3% wall cost

**It does not reproduce, and the deadline is net FASTER.** A 90-ontology random sample of
the `both` set, re-run in both arms:

| group | n | no deadline | with 55 s deadline | |
|---|---|---|---|---|
| **all sampled** | 90 | 256.8 s | **230.8 s** | **−10.1%** |
| deadline fired | 4 | 153.5 s | 123.6 s | −19.5% (truncation saves work) |
| never fired | 86 | 103.3 s | 107.2 s | +3.8% (single runs) |

And a deadline that *cannot* fire (600,000 ms) is free: over 6 ontologies at min-of-3
interleaved, −1.9% to +4.0%, mean ~0.4%.

**Why the +3.8% and my follow-up "+15.3%" are not real either.** I localised an apparent
40% cost to the `sweeps` phase from one run per arm. Repeating it 4× per arm on
`ore_ont_7803`, single-threaded:

```
run 1:  no-dl sweeps=8582   dl sweeps=5308
run 2:  no-dl sweeps=3557   dl sweeps=7187
run 3:  no-dl sweeps=4231   dl sweeps=4299
run 4:  no-dl sweeps=4684   dl sweeps=5759
```

**The sweeps phase varies 2.4× within a single arm.** The "+40%" was noise, and a min-of-5
wall comparison sitting on top of that variance does not establish 15% either. The
mechanism is not the truncating budget — `--pair-timeout-ms 5` and `1000` both spread 1.4×.

Confirmed independently by code: `effective_deadline` (`classify.rs:2296`) computes
`gd.min(Instant::now() + t)` and calls `Instant::now()` in the no-global branch too, so
with `gd = start+55 s` against a 5 ms per-pair budget the effective deadline is *identical*
either way. There is no extra clock check to pay for.

**Method note worth keeping:** the sweeps phase has 1.4–2.4× run-to-run variance
single-threaded at a non-truncating budget. Per-ontology wall comparisons on sweep-heavy
ontologies need many repeats or an aggregate over a large sample; one run per arm is not
usable, and neither is min-of-5 for a 15% effect.

## Recommendation

**The deadline is a win; the tail is the defect.** On the measured sample it recovers 113
ontologies from no-output-at-all and is 10% *faster* in aggregate. Its only genuine cost is
the unbounded post-deadline region, which can push a run that was going to finish just under
the cap over it — 2 ontologies out of 1,920.

So the order is:

1. **Bound the tail** — bring the post-phase region inside the deadline, or subtract a
   class-count-proportional reserve when arming it. This is the actual bug and it is worth
   fixing on its own merits: `--global-timeout-ms 20000` producing a 58 s wall is a broken
   contract regardless of what the default becomes.
2. **Then flip the default**, gated as usual on the three-clause release gate. The evidence
   so far is favourable, but the flip needs a full-corpus two-arm sweep — the 113 gains come
   from the DNF tail, which a sample drawn from completers structurally cannot see.

Repeat-runs are mandatory for cap-boundary cases: one of the three apparent regressions was
noise, and only 3× repeats distinguished it.
