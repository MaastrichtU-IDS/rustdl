# Which timeout configuration maximises COMPLETE classifications? The defaults already do

**Date:** 2026-08-17 · Answers "what configuration of timeouts yields maximum classifications
with complete entailments?" **Answer: v0.4.19's shipped defaults — `--pair-timeout-ms 5`,
`--global-timeout-ms 0`, `RUSTDL_PREP_DEADLINE=0`.** The measured optimum is the status quo,
and the DNF tail is essentially budget-invariant.

## What was measured

110 random ORE ontologies × per-pair budgets {1, 5, 100, 1000} ms, **20 s wall cap**,
single-threaded, `classify --json`. Each run bucketed by rustdl's own truncation flag:

* **complete** — output produced, `incomplete: false` (no pair timed out)
* **incomplete** — output produced, `incomplete: true` (budget truncated it)
* **DNF** — no output within the cap

| `--pair-timeout-ms` | complete | incomplete | DNF | total wall |
|---|---|---|---|---|
| 1 | 95 | 3 | **12** | 367 s |
| **5 (default)** | **96** | 1 | 13 | 381 s |
| 100 | **96** | 0 | 14 | 383 s |
| 1000 | **96** | 0 | 14 | 383 s |

**Complete peaks at 96 and plateaus from 5 ms up.** A 200× larger budget adds no complete
results and costs one extra DNF. Dropping to 1 ms trades one complete for one fewer DNF.

## Only 3 of 110 ontologies change outcome at all

```
ore_ont_1966   pt1=incomplete → pt5,100,1000=complete   needs ≥5 ms to finish its pairs
ore_ont_1707   pt1,5=incomplete → pt100,1000=DNF        a small budget buys partial output
ore_ont_6333   pt1=incomplete → pt5,100,1000=DNF        only a tiny budget yields anything
```

**97% of the sample is budget-invariant**, because pure-EL and Horn ontologies take the
saturation fast path and never issue a per-pair probe — no budget applies to them. Any
recommendation therefore rests on a 3-ontology signal, and should not be over-read.

## The other two knobs need no measurement — the logic settles them

**`--global-timeout-ms` can never increase the complete count.** If the deadline fires, pairs
are cut and the result is by construction `incomplete`; if it does not fire, it is neutral. So
it can only convert DNF → incomplete. That is genuinely useful when you want *whatever is
available* under a hard external cap (measured: **113** ontologies go from no-output to a
hierarchy at a 55 s internal deadline), but it is strictly the wrong tool for maximising
*complete* results. Keep it at `0`.

**`RUSTDL_PREP_DEADLINE` stays OFF**, measured separately: ON it can pay a full 16.8 s
conversion and return *nothing* (`ore_ont_7192`: 50,753 rows → 0 at a 3 s budget), because
conversion is not interruptible. See `docs/2026-08-17-prep-deadline-default-decision.md`.

**Do not lower `--pair-timeout-ms` below the default in the hope of safety.** It feeds
`adaptive_label_cache_ms` as `clamp(n × per_pair, 50, 30_000)`, so a smaller per-pair budget
*starves the label cache* and can be **18× slower for byte-identical output**
(`ore_ont_15010`: 5.65 s → 103.98 s). A low budget can lose on both axes at once.

## The finding that matters more than the optimum

**The DNF population barely moves: 12 at 1 ms versus 14 at 1000 ms.** Across a 1000× budget
range, timeouts relocate **two** ontologies. So **no timeout configuration meaningfully
increases how many ontologies rustdl classifies** — the tail is not budget-limited.

That is consistent with the standing addressability check (`--pair-timeout-ms 1` eliminates
per-pair search, and 6 of 9 sampled hard ontologies still DNF at 60 s: their stall is in
`label_cache_build` / `saturate` / `prepare`) and with this session's phase measurements
(`ore_ont_9944` spends its entire 600 s in `label_cache_build`). Tuning per-pair budgets
cannot reach those phases.

## Limitations, stated plainly

1. **`incomplete: false` is a truncation flag, not a proof.** `trust_sat` is default-ON, so a
   run can report not-truncated while MISSING subsumptions the full tableau would find. This
   counts un-truncated results, not oracle-verified ones.
2. **The optimum is cap-relative.** At 20 s, budgets ≥100 ms cost DNFs; at a 300 s cap the
   plateau would extend and the DNF column would shrink. Re-run at your deployment cap.
3. **110 ontologies, 3 of them informative.** The direction (default is at the plateau's start)
   is solid; the one-ontology differences between adjacent rows are not.
4. Row counts, not oracle diffs — a change in *which* subsumptions are found at equal
   completeness would not show here.

## Recommendation

**Change nothing for this objective.** If the goal is instead *maximum ontologies answered at
all*, that is a different objective with a different answer: add `--global-timeout-ms` sized to
your cap (accepting `incomplete` results), which recovered 113 ontologies at 55 s — and note
the parse-charging fix (2026-08-16) is what makes that budget mean roughly what it says.
