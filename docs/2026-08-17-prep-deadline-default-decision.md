# `RUSTDL_PREP_DEADLINE`: OFF → **fixed and flipped ON** (2026-08-17)

> **SUPERSEDED BY ITS OWN FOLLOW-UP, SAME DAY.** The decision below ("keep OFF") was correct
> for the flag *as it then behaved*. The pathology it identified has since been fixed and the
> default flipped ON. Read this section, then the original analysis for the evidence that
> motivated the fix.
>
> **The fix** (`classify::prep_bounding_active`): bound prep only while the budget is still
> MEETABLE, and fall back to unbounded prep once it is not. That makes the bounded path
> **never worse than the unbounded one**.
>
> **Why prep could not simply be made interruptible** — the plan I started with, abandoned on
> measurement: the dominant pre-deadline cost is horned-owl PARSING, which is external and
> completes before rustdl is entered. Measured parse share: `ore_ont_7192` 10.4 s parse /
> 7.6 s convert (58%), `ore_ont_10926` 12.7 / 10.6 (54%), `ore_ont_2574` 2.8 / 1.4 (67%). So
> even a fully interruptible `convert_ontology` could not honour a budget smaller than the
> parse — it would return nothing, just sooner. Fixing the *policy* was the right lever;
> fixing the *interruptibility* was not.
>
> **Measured after the fix**, 45 ontologies, both arms, **row counts identical in every arm**:
>
> | budget | wall OFF → ON | row differences |
> |---|---|---|
> | 3 s (the pathological case) | 57.8 s → 61.7 s (+6.7%) | **0** (was 1 total loss) |
> | 20 s | 65.8 s → **41.2 s (−37.4%)** | **0** |
>
> `ore_ont_7192` at a 3 s budget: **50,753 rows** (was **0**).
>
> **Inert at defaults** — `--global-timeout-ms` defaults to `0`, so with no budget there is
> nothing to bound prep against; `pizza` is byte-identical. Blast radius is exactly the callers
> that set a budget, and for them ON is now a Pareto improvement.
>
> Guarded by 4 canaries in `classify::prep_bounding_tests`; **2 sabotages run, both caught**
> (removing the fallback fails `falls_back_once_the_budget_is_already_blown`; reverting the
> default fails `default_is_on`). One existing test changed contract:
> `from_internal_is_bounded_too` asserted that a `Duration::ZERO` budget aborts in
> `from_internal` — it no longer does, since a zero budget is never meetable. Its soundness
> assertions (flagged incomplete, full EL closure) still hold and are unchanged.
>
> ---
>
> ## Original analysis (2026-08-17, superseded above)

**Date:** 2026-08-17 · Closes the follow-up left by
`docs/2026-08-16-global-deadline-does-not-bound-wall.md`, which fixed the *parse* half of the
`--global-timeout-ms` contract and left conversion behind this flag.

## Scope first — the flag is inert at defaults

```rust
let prep_deadline = if crate::prep_deadline_enabled() { global_deadline } else { None };
```

`global_deadline` is `None` unless `--global-timeout-ms` is passed, and it defaults to `0`
(unbounded). **So the flag can only act on runs that set a global budget.** A full-corpus
two-arm sweep would have measured nothing; the correct gate is a deadline-set comparison, and
that is what was run. Establishing this first saved the sweep.

## What it does

`budget_origin` (`classify.rs:862`) starts the budget clock at the CALL rather than after
`convert_ontology`, and passes the deadline into saturation. So ON, `--global-timeout-ms N`
means "N ms of wall"; OFF it means "conversion + N ms".

## Measurement — 45 completing ORE ontologies, both arms

**Generous budget (20 s):**

| | OFF | ON |
|---|---|---|
| total wall | 63.0 s | **41.1 s (−34.7%)** |
| runs exceeding the budget | 1 | 1 |
| **ontologies with differing row counts** | **0 of 45** | |

A clean win: a third of the wall, no answer changes.

**Tight budget (3 s) — the discriminating arm:**

| `ore_ont_7192` | rows |
|---|---|
| OFF | **50,753** |
| ON | **0** |

1 of 45 diverges, and it diverges completely. `ore_ont_7192` spends **16.8 s** in
parse+convert. ON, the budget is exhausted before reasoning starts; OFF, the clock starts
after conversion so it gets its full 3 s and finishes the hierarchy.

## Why this argues for OFF, despite the −34.7%

**Parse and conversion are not interruptible.** So in the failing case ON does not "give up
early and save time" — it **pays the entire 16.8 s and returns nothing**. OFF pays ~20 s and
returns a complete 50,753-row hierarchy. The marginal 3 s buys the whole answer.

That is strictly worse behaviour, not a different point on a tradeoff curve. A budget that
cannot be honoured should either abort before doing the work or proceed best-effort; spending
the full cost and reporting nothing is the one option with no user.

The −34.7% is real but accrues exactly when the budget is **not binding** — i.e. when the
caller did not need it. Where it binds, the flag can zero the output.

## Decision

**Default stays OFF.** Keep the flag for callers who genuinely want a hard wall bound (a
batch harness under an external cap, which would rather get nothing at N ms than overrun) and
who know their inputs convert quickly.

**The `--global-timeout-ms` contract therefore remains half-honest by choice**, and that is
worth stating plainly: parse is charged (shipped 2026-08-16), conversion is not. The
asymmetry is deliberate — charging parse never zeroed an output in the measured sample,
charging conversion did.

**What would change this:** making conversion interruptible, or an early abort when
`elapsed > budget` before conversion begins. Either turns the failing case from "pay
everything, return nothing" into a fast, honest "budget exceeded", at which point ON becomes
the better default and the −34.7% is free. That is a bounded piece of work and the natural
next step if anyone wants the flag on.

Raw data: `docs/benchmarks/data-2026-08-17-prep-deadline-{20s,3s}.csv`.
