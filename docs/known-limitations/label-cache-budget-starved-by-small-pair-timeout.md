# A small `--pair-timeout-ms` starves the label-cache budget, costing up to 18× for identical output

**Found:** 2026-08-06 · **Status: CLOSED 2026-08-18 on both censused instances — cause
UNATTRIBUTED.** It was never fixed deliberately; re-measurement found the coupling gone.
**Workaround (no longer needed):** `RUSTDL_LABEL_CACHE_TIMEOUT_MS=30000`

> ## RETIRED BY RE-MEASUREMENT (2026-08-18)
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
