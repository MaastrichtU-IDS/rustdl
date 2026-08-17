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
