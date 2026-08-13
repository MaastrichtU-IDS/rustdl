# Per-pair budget default — design

**Date:** 2026-08-13 · **Status:** design, approved for planning · **Objective agreed with the
user:** *prefer an answer over no answer*, with the completeness cost quantified rather than
minimised.

## Problem

`--pair-timeout-ms` defaults to **1000** (`crates/owl-dl-cli/src/main.rs:155`,
`default_value_t = 1000`). Its documented justification is *"the empirical knee on pizza"* —
tuned on one ontology. On the `tier_walk` bucket it is actively harmful: a pair that will
never conclude burns the full budget, so the run produces **nothing**.

Measured (single runs, quiet host, `fsesrv-g1`):

| ontology | default 1000 ms | `--pair-timeout-ms 50` |
|---|---|---|
| `ore_ont_10019` | 98.1 s | **6.5 s** |
| `ore_ont_6485` | **dnf** | **13.2 s** |
| `ore_ont_14272` | **dnf** | **33.1 s** |
| `ore_ont_1707` | **dnf** | 46.1 s |

`tier_walk` is the second-largest DNF bucket (**35 of 164**, and growing across caps:
11 → 27 → 35), so the addressable set is substantial.

### The recovery is caused by the per-pair budget — isolated, not assumed

`adaptive_label_cache_ms` (`lib.rs:2817`) computes
`clamp(n × per_pair, LABEL_CACHE_FLOOR_MS=50, LABEL_CACHE_CEILING_MS=30_000)`, so
`--pair-timeout-ms 50` moves **two** budgets at once. On `ore_ont_10019` (47 classes) the
label-cache budget goes 30,000 ms → 2,350 ms, a 12.8× cut. A four-arm test separates them:

| ontology | A: default | B: pair50 | C: pair50 + `LC=30000` | D: pair1000 + `LC=2350` |
|---|---|---|---|---|
| `ore_ont_10019` | 98.1 s | 6.5 s | **6.4 s** | 96.4 s |
| `ore_ont_6485` | dnf | 13.2 s | **13.2 s** | dnf |
| `ore_ont_14272` | dnf | 33.1 s | **32.7 s** | 52.4 s |

**B ≈ C** — restoring the label budget does not undo the recovery. **D ≈ A** — cutting only
the label budget does not produce it. The per-pair search budget is the cause.

## Part 1 — decouple the label-cache budget from `per_pair`

`adaptive_label_cache_ms`'s dependence on `per_pair` must go before Part 2 lands, for two
reasons in priority order:

1. **It is a confound.** Otherwise every Part 2 arm varies two budgets and the result is
   uninterpretable. Two claims in this arc were already invalidated exactly this way (a
   `RUSTDL_DATA_PROPERTIES=0` ablation that changed the reasoning problem as well as the
   check; a "4.2× scaling" figure computed across runs that did different amounts of work).
2. It independently closes an open defect —
   `docs/known-limitations/label-cache-budget-starved-by-small-pair-timeout.md`, "costing up
   to **18×** for identical output", whose current workaround is
   `RUSTDL_LABEL_CACHE_TIMEOUT_MS=30000`.

**Change:** base the budget on a constant instead of `per_pair`, keeping
`RUSTDL_LABEL_CACHE_TIMEOUT_MS` as the override and the existing floor/ceiling clamp.

**Expected to be near-inert at today's default**, which is the argument that it is safe:
`n × 1000` already saturates the 30 s ceiling for any `n ≥ 30`, so only ontologies with fewer
than ~30 classes see any change at the current default. That prediction is itself a gate — if
the Part 1 arm moves many ontologies, the reasoning is wrong and Part 1 stops.

**Gates:** FP=0 net; full 1,920 two-arm sweep; byte-identity expected on the curated
fixtures. Ships or is rejected **on its own**, before Part 2 exists.

## Part 2 — lower the per-pair default

**Change:** `default_value_t = 1000` → the value the screening selects. No new flag; the
existing `--pair-timeout-ms` is the revert path (`--pair-timeout-ms 1000` restores prior
behaviour exactly).

**Value selection by screening, not judgement.** Candidates **25 / 50 / 100 / 200** against
1000. Screen with the **MISSED net** first (400-ontology population, ~10 min per arm, prices
ΔMISSED), then run the **full 1,920 two-arm sweep** on the survivor only (~2 h, the sole gate
that can observe `ok → dnf`). The cheap gate can reject three candidates before the expensive
one runs once.

**Pre-registered decision rule** — recorded before measuring, because pre-registration has
overturned a post-hoc reading twice in this arc:

> **Ship iff `ok → dnf` = 0 AND ΔMISSED < 5%** (< ~260 against the 5,198 baseline over the
> 400-ontology population).

`ok → dnf` must be **zero** because it is a strict regression with no compensating gain. A
bounded completeness loss is the price the objective explicitly accepts.

**Why the completeness loss is acceptable and not silent.** A truncated pair defaults to
`not-subsumed`, a *sound* under-approximation — never a false subsumption. The run prints a
prominent `INCOMPLETE` warning to stderr plus a `# timed-out pairs: N` banner line, so the
loss is signposted. Precedent: the CLI's own help already advises `--pair-timeout-ms 25` for
nominal-heavy ontologies, where a lower budget was verified **MISSED=0** on wine.

## Consequence to build in: the wine freshness canary

`CLAUDE.md` uses wine's **default** wall (~74 s) as the stale-binary canary. Lowering the
default changes that number, so the canary must be re-measured and the doc updated **in the
same commit**. Otherwise the next reader diagnoses a real change as a stale binary — a failure
this design record has already recorded once.

## Testing

* **Part 1:** unit test that `adaptive_label_cache_ms` no longer varies with `per_pair`, and
  that `RUSTDL_LABEL_CACHE_TIMEOUT_MS` still overrides; floor/ceiling behaviour retained.
* **Part 2:** unit test pinning the new default constant, and that `--pair-timeout-ms 1000`
  reproduces prior behaviour.
* **Both:** FP=0 soundness net (the failure direction is a MISS, not an FP, so this is
  non-regression evidence), ΔMISSED for completeness, full sweep for outcomes.

No new canary *shape* is needed: these are constant changes whose entire risk lives in the two
corpus gates.

## Non-goals

* **No adaptive/predictor logic.** This arc repeatedly found shape predictors do not predict
  cost (residual-absorbability AUC **0.480**, below chance; "guard-manufacturable" *anti*-correlated
  with peer solvability).
* **No escalation tier** (small budget, then re-run undecided pairs at 1000 ms).
  Completeness-preserving by construction but measured to lose: `ore_ont_10019` leaves **154**
  pairs undecided at 50 ms, so round 2 costs ~154 s against the 6.5 s a flat 50 ms achieves.
* **No change to `--global-timeout-ms`** or to the label-cache ceiling beyond Part 1's
  decoupling.

## Scope check

Two sequenced, independently gated constant changes with one doc update. Single plan.
