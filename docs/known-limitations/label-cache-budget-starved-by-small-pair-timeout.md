# A small `--pair-timeout-ms` starves the label-cache budget, costing up to 18× for identical output

**Found:** 2026-08-06 · **Status: FIX EXISTS, default OFF (2026-08-19) —
`RUSTDL_LABEL_CACHE_PROBE`, see `docs/2026-08-19-label-cache-probe.md`.** A FLOOR remains the
wrong fix (measured: 112% aggregate cost on the small-`n` population, one `ok → DNF` with total
row loss). The working fix is a **differential escalation probe**: retry a class that FAILED at
the current budget at 1000 ms, and escalate only if that rescues it — bad-case cost is one
escalated build, independent of `n`. `ore_ont_5107` 6.65 s → 1.92 s (3.46×), guard case
`ore_ont_9540` 0.88× (vs 2.1× under naive escalation), 0 row diffs over 39 ontologies.
**Prior status line said "NOT WORTH FIXING", from a frame that could not see the defect
(40-slowest vs a small-`n` precondition).** At the DEFAULT `--pair-timeout-ms`, granting every class the 30 s ceiling helps
**0 of 40** slowest completers at ≥1.5× and costs **2.3% aggregate wall**. The `n × F` objection
below is now a measured number rather than a projection. **The COUPLING is real; its TRIGGER does
not fire.**
Forcing the 50 ms floor takes `ore_ont_15108` 43.1 s → **DNF**, and fires on 12 of 40 slowest
completers — so the cache is load-bearing. But a small `--pair-timeout-ms` no longer triggers it
on that frame (`15108` moves 1.13× at `pt=1`). **An intermediate version of this header claimed
"5 members"; those 5 are a DIFFERENT, cache-INSENSITIVE defect** — see
`docs/2026-08-19-label-cache-starvation-census.md` § THE ATTRIBUTION WAS WRONG.
**Workaround:** `RUSTDL_LABEL_CACHE_TIMEOUT_MS=30000`

> ## CENSUS RE-RUN 2026-08-19 — THE 2026-08-18 "CLOSED" HEADLINE WAS WRONG
>
> An earlier edit of this file (same session) marked the defect CLOSED because both *named*
> instances stopped reproducing. **That inference was too broad, and re-running this document's
> own census refutes it.** Membership CHANGED; prevalence went UP.
>
> Frame: the 40 slowest v0.4.19 completers (`wall > 0.5 s`), single-threaded, three arms —
> **A** default, **B** `--pair-timeout-ms 1`, **C** `--pair-timeout-ms 1` + a forced
> `RUSTDL_LABEL_CACHE_TIMEOUT_MS=50` (the floor) as a **positive control**. Thresholds and the
> verdict rule were **pre-registered** at this document's own 1.5×.
>
> | | recorded 2026-08-06 | census 2026-08-19 |
> |---|---:|---:|
> | `pt=1` ≥1.5× **SLOWER** | **1** | **5** |
> | `pt=1` ≥1.5× faster | 12 | 3 |
> | within 1.5× | 27 | 32 |
> | aggregate wall | 1499.5 → **1267.0 s** (net *faster*) | 1377.3 → **1624.8 s** (net **+18% SLOWER**) |
>
> ### The 5 live members — all byte-identical output
>
> | ontology | default | `pt=1` | ratio | arm C | rows |
> |---|---:|---:|---:|---:|---|
> | `ore_ont_14272` | 21.76 s | 73.26 s | **3.37×** | 3.37× | identical (835) |
> | `ore_ont_4827` | 37.14 s | 123.26 s | **3.32×** | 3.32× | identical (1006) |
> | `ore_ont_9864` | 24.53 s | 79.53 s | **3.24×** | 3.24× | identical (904) |
> | `ore_ont_8429` | 29.50 s | 90.50 s | **3.07×** | 3.07× | identical (1001) |
> | `ore_ont_6923` | 38.36 s | 102.93 s | **2.68×** | 2.69× | identical (1038) |
>
> **Every arm-B ratio matches its arm-C ratio to within 0.01×.** That is mechanistic proof rather
> than correlation: for these five, `--pair-timeout-ms 1` starves the per-class build *exactly as
> completely* as pinning the budget to the 50 ms floor. The coupling this document describes is
> intact and biting.
>
> ### The instrument is validated, so the numbers above are trustworthy
>
> Arm C fires on **12 of 40**, five of them to outright DNF at a 240 s cap (`15491` 8.96×,
> `9151` 7.43×, `9299` 6.81×, `5617` 7.20×, `15066` 6.74×). A null in arm B would therefore have
> been meaningful — and arm B is not null. Without this control, "5 members" and "0 members"
> would have been indistinguishable from a blind instrument.
>
> ### Why the two ORIGINAL instances dropped out — and why that is not a fix
>
> `ore_ont_15108` is arm-B **flat (1.01×)** yet arm-C **2.10×**: the cache is still load-bearing
> there, but a small per-pair budget no longer starves it. `ore_ont_15010` is no longer in the
> 40-slowest frame at all (5.98 s). Six more behave the same way — arm-C-only members (`5617`,
> `15066`, `9151`, `9299`, `13071`, `15491`).
>
> So the population split: on **7** ontologies the coupling has broken while the cache still
> matters, and on **5** it has not. **Nothing was fixed; the membership moved.**
>
> ### THE COST/BENEFIT THAT JUSTIFIED "NOT FIXED" HAS INVERTED
>
> § Why not fixed rests on "the pathology is ~2 ontologies against 12 large wins in the same
> sample", with the aggregate wall *improving* under `pt=1`. Both halves are now false: 5 against
> 3, and the aggregate is **18% worse**. That does not by itself make raising
> `LABEL_CACHE_FLOOR_MS` correct — the per-class `n × F` objection in that section still stands —
> but the empirical argument for inaction is gone and the section needs re-deciding on current
> numbers.
>
> ### A guess of mine, refuted
>
> I predicted the original's "12 large wins" were partly **truncation** — `--pair-timeout-ms 1`
> is a sound under-approximation, so a faster arm may simply have given up. **Wrong:** all 3
> faster members have **identical row counts** (`9429` 2706, `934` 107, `4796` 203), so they are
> clean wins. Only 2 ontologies change output at all under `pt=1`, and both are ~flat in wall
> (`15066` 8986 → 8952 rows, `9151` 11478 → 11477) — i.e. incompleteness and the wall pathology
> are **disjoint** phenomena here, not two faces of one.
>
> Raw data: `docs/benchmarks/data-2026-08-19-label-cache-starvation-census40.tsv`
>
> ### Threats to validity
>
> * Frame drawn from the **v0.4.19** sweep, not the 2026-08-06 one, so the two censuses do not
>   cover identical ontology sets. The comparison is of *rates and aggregates*, not paired.
> * All 40 files are content-distinct, but `ore_ont_10689` and `ore_ont_868` return identically
>   981,144 rows, which suggests **logical** duplication a content hash cannot detect — effective
>   *n* may be slightly below 40.
> * Arm-C DNFs are censored at 240 s, so those ratios are **lower bounds**.

> ## (SUPERSEDED by the census above — kept because its per-instance measurements are correct)
>
> ## Re-measurement of the two NAMED instances (2026-08-18)
>
> Both censused instances, single-threaded, current `main` (`0c1df06`):
>
> | ontology | arm | recorded | now |
> |---|---|---:|---:|
> | `ore_ont_15010` | default | 5.65 s | **5.99 s** |
> | | `--pair-timeout-ms 1` | **103.98 s** | **6.19 s** |
> | | `pt=1` + override | 5.64 s | 5.95 s |
> | `ore_ont_15108` | default | 44.65 s | **44.20 s** |
> | | `--pair-timeout-ms 1` | **200 s** | **45.40 s** |
> | | `pt=1` + override | — | 41.99 s |
>
> **The control is what makes this a retirement rather than a measurement difference:** both
> DEFAULT arms reproduce their recorded values (5.65→5.99, 44.65→44.20), so neither ontology
> merely got faster and neither host nor binary is confounding it. Only the *pathological* arm
> moved. The 18× and 4.5× couplings are gone; the override now makes no measurable difference.
>
> **Confirmed at the mechanism, not just the wall.** The defect was that a small per-pair budget
> starves the per-class build so the 96–100% pruning is lost. At `--pair-timeout-ms 1`
> `ore_ont_15010` now reports `# label heuristic: pruned=9268 pass_through=6 misses=751` — the
> cache is consulted and prunes heavily in exactly the regime that used to starve it.
>
> **Cause NOT attributed.** Nothing here was fixed on purpose. Something in v0.4.12–v0.4.19 or
> the 2026-08-17/18 work (`RUSTDL_PREP_DEADLINE` default-ON is a candidate) dissolved it. Do not
> invent a mechanism — the same "cured incidentally, unattributed" pattern as `ore_ont_10019`.
>
> **Scope, stated precisely:** closed on the **two instances this document censused**. The
> original census covered the 40 slowest completers; claiming the whole class is empty would
> need that census re-run. What is retired is this document's evidence, not a proof of absence.
>
> **DO NOT let this revive the "dead code" label on `RUSTDL_LABEL_CACHE_TIMEOUT_MS`.** That
> label was wrong for a *code* reason which still holds — the override always wins by
> construction (`lib.rs:2728-2730`). What has gone stale is the 18× *evidence* used to rebut it,
> not the rebuttal. A flag with no currently-known pathology to rescue is not dead code.

## The defect

`adaptive_label_cache_ms` (`crates/owl-dl-reasoner/src/lib.rs:2721`) sets the per-class
label-cache build budget to `clamp(n × per_pair, LABEL_CACHE_FLOOR_MS, LABEL_CACHE_CEILING_MS)`
with `FLOOR = 50`, `CEILING = 30_000` (`:2712-2713`). Its stated rationale is a break-even:
*"labeling C is worth it iff its `sat` costs less than refuting C's ~n pairs at the per-pair
cap"* (spec: `docs/superpowers/specs/2026-06-25-adaptive-label-cache-deadline-design.md`).

**The reasoning under-values the label cache.** It prices the cache at *the capped cost of the
refutations it replaces*, but a successful build **prunes 96–100% of pairs entirely**. Starve
it and the pruning is lost while the refutations still happen — merely capped. So a *smaller*
per-pair budget can make the whole classification dramatically **slower**.

## Evidence, measured single-threaded (gate conditions)

`ore_ont_15010`, **identical 171-row output in all three arms**:

| arm | label-cache budget | wall |
|---|---:|---:|
| default (`--pair-timeout-ms 1000`) | 30,000 ms (ceiling) | **5.65 s** |
| `--pair-timeout-ms 1` | 178 ms (`n`=178 × 1) | **103.98 s** |
| `--pair-timeout-ms 1` + `RUSTDL_LABEL_CACHE_TIMEOUT_MS=30000` | 30,000 ms | **5.64 s** |

**18× slower for byte-identical output, and the override restores it exactly** — which is what
identifies the coupling as the cause rather than a correlate.

Note `n × per_pair` = 178 ms clears the 50 ms floor while landing **170× below the ceiling**,
so the floor provides no protection at all in this regime.

## Prevalence: 2 known, censused

Over the **40 slowest completers** (`wall > 0.5 s`, from the 2026-08-06 arm-off sweep),
re-measured single-threaded at default versus `--pair-timeout-ms 1`:

| | n |
|---|---:|
| pt=1 ≥1.5× **slower** | **1** (`ore_ont_15108`, 44.65 s → 200 s, ≥4.5×) |
| pt=1 ≥1.5× **faster** | **12** (up to **134.8×**: `ore_ont_12653` 25.07 s → 0.19 s) |
| within 1.5× | 27 |
| aggregate wall | 1,499.5 s → 1,267.0 s |

So the pathology is **~2 ontologies** (`15010`, `15108`) against 12 large wins in the same
sample. **The sample is deliberately biased toward slow ontologies** and says nothing about
the fast majority (corpus median ~50 ms), where a small budget can only cost completeness.

## Why not fixed

The obvious fix — raise `LABEL_CACHE_FLOOR_MS` — only ever *raises* budgets, which sounds
free. It is not: **the budget is per class**, so a floor `F` costs up to `n × F` on any
ontology where the label cache genuinely cannot succeed. At n = 1,000 classes a 2 s floor is a
2,000 s worst case. That is precisely the cost the adaptive rule exists to avoid, so a floor
change needs a corpus-wide sweep over candidate `F`, not a chosen constant.

**Against a prize of ~2 ontologies, that sweep is not currently justified.** Recorded instead,
with a working one-variable escape hatch. If a user-facing ontology lands in this class, the
sweep is the route — and `ore_ont_15010` is the ready-made discriminating fixture, since the
override restores its wall exactly.

## Two record corrections this surfaced

1. **`CLAUDE.md:1405` lists `label_cache_timeout_ms` as "dead code" in the constant audit. It
   is live and load-bearing** — `RUSTDL_LABEL_CACHE_TIMEOUT_MS` "always wins" by construction
   (`lib.rs:2728-2730`), and setting it changed a wall by 18×. A "dead code" label would deter
   exactly the investigation that found this.
2. **A small per-pair budget is a large net win on slow completers** — 12 faster, 1 slower,
   aggregate 1,499 s → 1,267 s over the 40 slowest. This is **not a new finding**: the flag's
   own doc comment (`owl-dl-cli/src/main.rs:150-155`) already records `--pair-timeout-ms 25`
   being 7.5× faster on `wine` with an identical hierarchy and corpus-wide MISSED=0. It does
   support the standing recommendation to **document the flag** for the DNF tail rather than
   build automation around it.
