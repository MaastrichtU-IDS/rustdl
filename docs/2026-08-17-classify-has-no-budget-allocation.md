# The label cache spends the whole budget and is then never consulted — classify has no budget allocation

**Date:** 2026-08-17 · Found because the user said *"the label cache sounds faulty or
incorrectly interpreted — look deeper."* It was. **This is a real defect, and it is more
general than the label cache.**

## The observation

`ore_ont_11311` and `ore_ont_9944` under a 60 s global budget, single-threaded:

```
ore_ont_11311: label_cache_build = 58,139 ms → label heuristic: pruned=0 pass_through=0 misses=0
               unsat_probe = 1 ms   tier_walk = 8 ms   sweeps = 3 ms
ore_ont_9944:  label_cache_build = 58,222 ms → label heuristic: pruned=0 pass_through=0 misses=0
               unsat_probe = 1 ms   tier_walk = 6 ms   sweeps = 1 ms
```

**58 seconds building a cache that is consulted ZERO times.** `pruned=0 pass_through=0
misses=0` is not "pruned badly" — the counters are the *read* sites, so it was never read.
The build exhausts the budget; `tier_walk`, its only substantial consumer, then aborts in
8 ms.

This is provably wasted work, not merely redundant work. The earlier finding that
`labels(C) == closure(C)` for 100% of classes is beside the point on these two ontologies:
the output is never examined at all.

## Capping the phase does make it get used — and reveals the real problem

`RUSTDL_LABEL_CACHE_TOTAL_MS` bounds the phase in aggregate. It **exists but is opt-in.**

| `ore_ont_11311`, global 60 s | build | pruned | misses | **unsat_probe** | tier_walk |
|---|---|---|---|---|---|
| no cap (**default**) | 58,087 ms | **0** | 0 | 1 ms | 9 ms |
| cap 15 s | 15,001 ms | **2,493** | 4,276 | **37,212 ms** | 5,929 ms |
| cap 5 s | 5,001 ms | 993 | 5,137 | **46,644 ms** | 6,490 ms |

| `ore_ont_9944`, global 60 s | build | pruned | misses | unsat_probe | tier_walk |
|---|---|---|---|---|---|
| no cap (**default**) | 58,288 ms | **0** | 0 | 1 ms | 6 ms |
| cap 15 s | 15,001 ms | **0** | 0 | **43,236 ms** | 6 ms |
| cap 5 s | 5,000 ms | 591 | 2,616 | **47,991 ms** | 5,289 ms |

Two things fall out:

1. **Capping works as intended** — `pruned` goes 0 → 2,493 on `11311`. The cache starts
   earning its keep.
2. **A different phase immediately absorbs the freed budget.** `unsat_probe` goes from
   **1 ms to 37–48 seconds**. On `9944` at a 15 s cap the cache is *still* never consulted,
   because `unsat_probe` takes 43 s instead.

## The actual defect: no budget ALLOCATION

**Classify phases consume the global deadline greedily, in sequence. Whichever runs first
starves everything after it.** There is no allocation, no reservation, and no notion of
"this phase should get at most X% of the run".

Phase order and observed appetite on these inputs:

| phase | appetite when it runs first |
|---|---|
| `label_cache_build` | 58 s of a 60 s budget (unbounded by default; per-class cap is `clamp(n × per_pair, 50, 30_000)`, which with n=8,008 bounds nothing in aggregate) |
| `unsat_probe` | 37–48 s when the cache is capped |
| `tier_walk` | 5–6 s at best — and it is the phase that produces the answer |

This explains a result recorded earlier and not understood at the time:
`docs/2026-08-17-timeout-config-for-complete-classifications.md` found the DNF tail
**budget-invariant** — 12 DNFs at a 1 ms per-pair budget versus 14 at 1000 ms. Of course:
the knobs do not *allocate* budget, they only change **which phase starves**.

## Honest bound on the fix

**No allocation tried produces a hierarchy.** Every combination of
`RUSTDL_LABEL_CACHE_TOTAL_MS` ∈ {2 s, 5 s, 15 s} × `RUSTDL_UNSAT_PROBE_MS` ∈ {unset, 1 s, 2 s}
yields **0 `direct` rows** on both ontologies at a 60 s budget. The total work genuinely
exceeds 60 s; reallocating it does not conjure an answer.

So this is **not** a DNF rescue. What it is:

* **A provable waste bug** — 58 s producing an artefact that is never read, on any ontology
  where the label phase can outrun the budget. That is worth fixing on its own merits.
* **A structural gap** — the pipeline needs budget allocation across phases, not one deadline
  that each phase races to consume. That is a design change, not a constant.
* **An explanation** of why the timeout-configuration sweep found nothing: it was tuning
  knobs that cannot allocate.

## PRIOR ART: this defect was already found one phase upstream, fixed, and MEASURED OUT

`unsat_probe_cap` (`classify.rs:2808`, `RUSTDL_UNSAT_PROBE_MS`) documents the SAME defect at
the `unsat_probe` stage, in the same terms:

> "`unsat_probe` runs one satisfiability probe per class … so the phase costs `n × per_pair`
> and then `tier_walk`, the phase that actually computes the hierarchy, never runs. Measured on
> `ore_ont_934` (108 classes): `unsat_probe` = 103,541 ms, `tier_walk` = 0, and classify DNFs."

And its verdict:

> "**MEASURED NEGATIVE RESULT — this flag rescues NOTHING. Default OFF.** The mechanism works
> exactly as designed and buys nothing … `tier_walk` from 0 ms to 73,309 ms — the phase really
> is unblocked, and it decides 27 subsumptions it previously never reached. The ontology still
> DNFs, because `tier_walk` at ≥50 ms/pair cannot finish either."

**So starvation-unblocking has already been built once and measured to rescue nothing**, because
the starved consumer cannot finish even when handed the whole budget. That prior result predicts
the outcome of the analogous fix here, and the prediction held (below).

## ATTEMPTED FIX, AND WHY IT WAS REVERTED

Implemented `accelerator_share_deadline`: with a global budget and no explicit
`RUSTDL_LABEL_CACHE_TOTAL_MS`, cap the label phase at **half the remaining budget** — the
accelerator-versus-consumer rule, the same mechanism as `prep_bounding_active`.

It did exactly what it was designed to do and did not achieve the goal:

| `ore_ont_11311`, global 60 s | label_cache_build | unsat_probe | tier_walk | pruned |
|---|---|---|---|---|
| before | 58,139 ms | 1 ms | 8 ms | 0 |
| **with the half-share** | **29,043 ms** | **29,043 ms** | 6 ms | **0** |

The freed 29 s went straight to `unsat_probe`; `tier_walk` still got 6 ms and the cache was
still never consulted. **Fixing one greedy phase hands the budget to the next.** A pairwise
split cannot fix a chain — it needs a reservation for the output-producing phase across ALL
preceding phases, which is a scheduling redesign, not a local cap.

**Reverted.** Keeping an ineffective cap would add a constant, change behaviour under budgets,
and deliver nothing measurable.

## What remains TRUE and unaddressed

The waste is real even though unblocking does not rescue:

* **26 of 45** sampled budget-recovered ontologies spend **~55 s to return a hierarchy that is
  byte-identical to `--saturation-only`**, which produces it in a median of **2.01 s** (~27×).
  Verified per-ontology by output hash: `ore_ont_9944` 50×, `ore_ont_3080` 275×;
  `ore_ont_5438` differs, so it is not universal.
* The right shape is therefore **not** "share the budget" but "**do not start accelerators that
  cannot pay for themselves, and return the degraded answer now**". That needs a predictor for
  "can this cache complete", which is unavailable a priori — the same circularity that killed
  the certification line.

## Candidate fix, and its trap

The obvious move — make `RUSTDL_LABEL_CACHE_TOTAL_MS` default to a fraction of the global
budget — introduces a new tunable constant, which is exactly what
`docs/2026-08-17-learned-cap-tuning-assessment.md` argues must come from mechanism rather than
fitting. There is a mechanism available here and it does not need a fitted fraction: **a phase
should not be allowed to consume budget that its own consumer will need.** A defensible
version is to reserve budget for `tier_walk` (the phase that produces output) before the
optional accelerators run, rather than picking a percentage.

Note also that any cap trades prunes for **misses**: at a 5 s cap `11311` shows
`misses=5,137`, and each missed class turns its pairs into per-pair probes. The 15 s arm
(2,493 pruned / 4,276 misses) versus the 5 s arm (993 / 5,137) shows the trade directly.

## Method note: a redirection bug hid these counters all session

The counters are printed to **stdout** with the taxonomy, not to stderr. Several probes this
session used `2>&1 >/dev/null | grep …`, which redirects stdout to `/dev/null` and greps
**stderr only** — so `# label heuristic:` was silently invisible and returned empty output
twice before the mistake was spotted. Phase timings came through only because those probes
merged with `2>&1 |`.

**Any conclusion in this session that rested on a stderr-only grep should be re-checked
against stdout.** The wall-breakdown and TIMING lines were read correctly (they are stderr);
the label-heuristic counters were not readable at all until this was fixed.
