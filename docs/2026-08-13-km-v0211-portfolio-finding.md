# What KM v0.2.11 does that rustdl does not: it RACES engines with a preference order

2026-08-13. Prompted by the user's redirect — profile the latest KM rather than tune rustdl's
per-pair budget. The redirect was correct: it found a structural difference that the budget
work would only have approximated.

## Setup

Local KM checkout was 218 commits behind (2026-07-31 vs origin 2026-08-13). Built
**v0.2.11** from `origin/main` in a throwaway worktree (`/tmp/km-latest`) so the existing
checkout was untouched. Binaries: `km`, `elc`, `kobayashi-marust`, `ofn`.

KM does **not** shell out to Konclude — `engine/src/konclude_ht/` is a Rust
*reimplementation* of Konclude's hypertableau. So this is a genuine algorithmic comparison.

## The measurement that matters

`ore_ont_10019` — rustdl's starkest gap (47 classes, rustdl DNF at 90 s, Konclude 0.05 s):

| KM route | result |
|---|---|
| `auto` (learned selector) → picks `production_all` | **0.25 s** |
| `production_all` | **0.25 s** |
| `production_all1` (single-threaded) | **0.26 s** |
| `default` | **timeout** |
| `cb_plain16` | **timeout** |
| `cb_absorb16` | **timeout** |

**KM's own default route DNFs on this ontology.** Three of five routes DNF. So KM's advantage
is *not* a uniformly better engine — it is **which strategy it runs**.

## The architecture

Routes are named bundles of `KM_*` environment settings (`src/routing.rs:163-168`):

```rust
Route::Auto | Route::Manual | Route::Default => &[],   // bare defaults
Route::ProductionAll => PRODUCTION_ALL,                 // a bundle
Route::Default1 => &[("KM_THREADS", "1")],
```

The bundle that wins:

```
KM_MECHANISM=portfolio        KM_HT_ONLY=certified
KM_TRIGGER_ABSORB=1           KM_ABSORB=1
KM_BRIDGE_PROBE_BUDGET_S=30   KM_HT_SATURATION_BUDGET_S=180
```

And `KM_MECHANISM=portfolio` resolves to `race_cb_vs_ht` (`src/orchestrate/race.rs:575-591`),
whose own doc comment states the design:

> the HT arm runs INSIDE `race_cb_vs_ht` in fallback mode, where **CB is authoritative**: an HT
> arm's answer is taken **ONLY** when the certified CB engine errors or runs past its budget.
> Under that CB-preference the first-class cardinality arm is **monotone-safe** — it can only
> ever replace a CB timeout, and the number rules are sound (they never assert a subsumption CB
> would not) … recovers the SHQ/SHOQ number onts (`ore_ont_7499` / `9540`, both previously
> 240 s timeouts).

So: **two engines raced, with a preference order that makes the fallback monotone-safe.** The
weaker-but-faster arm can only ever *replace a timeout*, never override a verdict the
authoritative engine produced — which is what makes it sound without an oracle.

Note both named recoveries, `ore_ont_7499` and `ore_ont_9540`, are in rustdl's tail too.

## What rustdl has and lacks

| | KM v0.2.11 | rustdl v0.4.17 |
|---|---|---|
| feature flags | many `KM_*` | many `RUSTDL_*` |
| named flag bundles | **~14 routes** | none |
| per-ontology selection | **learned decision tree on a source profile** | none |
| engine racing / fallback | **`race_cb_vs_ht`, CB authoritative** | none — fixed saturation → wedge → tableau |

rustdl already has the *ingredients* (an EL saturator, a wedge, a main tableau, ~90 flags). It
lacks the **portfolio and the selector**. Its single global configuration must be right for
every ontology at once, and the DNF tail is where that fails.

## Why this supersedes the per-pair budget spec

`docs/superpowers/specs/2026-08-13-per-pair-budget-default-design.md` proposes lowering the
per-pair default, accepting a corpus-wide completeness loss (pre-registered at ΔMISSED < 5%) to
recover ~35 `tier_walk` ontologies.

The KM finding shows that is the **globally-applied approximation of a per-ontology decision**.
`--pair-timeout-ms 50` rescues those ontologies precisely because it is a *different strategy*;
applying it everywhere is what forces the completeness bill. As one arm of a race — where a
truncated result is taken only when the authoritative path times out — the same recovery costs
**no completeness on ontologies that already answer**.

Which reframes it: the budget change is a 1-line approximation of the right thing, and the right
thing is a mechanism rustdl has never had.

## Honest limits of this finding

* **Not verified: KM's correctness on these ontologies.** KM reports 162 subsumptions on
  `ore_ont_10019` against Konclude's 63 `SubClassOf`. That is very likely a closure-vs-direct or
  Top/Bottom convention difference — the design record already contains a **retracted** KM FP
  claim caused by exactly that (73% of a "~1795 spurious pairs" figure was a Top-equivalence
  artifact). Adjudicating needs the documented normalisation, and was not done here.
* **Not measured: KM across our whole tail.** One ontology, five routes. The route table above
  is evidence about `10019`, not about the 164.
* **Not assessed: the cost of a portfolio in rustdl.** Racing arms multiplies CPU, and rustdl's
  `label_cache_build` is already 6,412 s of CPU on `ore_ont_6134`. A race that runs two
  expensive arms could be worse than either.


## Sizing check: arm 2 recovers 3 of 164, not ~35 — do NOT build the portfolio yet

Before building the two-arm fallback, the pre-registered sizing check: how many of the 164 DNFs
complete at `--pair-timeout-ms 50`? Rule fixed in advance — ~35 recoveries justifies the
portfolio's testing tax, ~5 does not.

| | |
|---|---|
| completed **within 60 s** (the cap that DEFINED the 164) | **3 (2%)** |
| completed within the 90 s sweep cap | 14 (9%) |
| still DNF | **150** |

Recovered walls: median 27.1 s, max 30.9 s.

**The rule says do not build.**

### Why my estimate was 10× too high — the same error, twice

The "~35" came from four spot checks (`10019`, `6485`, `1707`, `14272`) run with **default rayon
parallelism (32 threads)**. The harness pins **`RAYON_NUM_THREADS=1`**, and the 164-ontology DNF
list was produced under that pin. So I extrapolated a 32-threaded sample onto a 1-threaded
population.

This is the **same thread-frame error** recorded earlier in this very document set — where I
explained a discrepancy between two of my own measurements by inventing a contention mechanism,
when the answer was `RAYON_NUM_THREADS=1` and one `cat /proc/<pid>/environ` would have shown it.
The lesson was written down and then repeated within the same session, which suggests the rule as
stated ("diff the conditions") is too passive. The operational form: **when a spot check and a
population disagree, assume a condition differs and go find which one before interpreting either.**

### What this does and does not settle

* **Does not invalidate the KM finding.** KM demonstrably races CB against HT with a
  preference order, its own `default` route DNFs where `production_all` succeeds, and rustdl has
  no equivalent. That architectural gap is real and stays recorded.
* **Does invalidate this particular arm.** A small per-pair budget is not the second arm worth
  racing — at the thread setting the tail is defined under, it rescues 3 ontologies.
* **Leaves the multi-threaded question open, and it is the more important one.** All census and
  sweep work in this arc is 1-threaded, but users run with default parallelism. If the tail is
  materially smaller at 32 threads, then the population everything here has been optimising
  against is partly an artifact of the measurement pin.

### The next measurement, and why it precedes any portfolio work

**How many of the 164 complete at DEFAULT settings with default parallelism?** That is one sweep,
and it determines whether the "164 DNF" figure is the user-facing tail or a 1-thread artifact. No
architectural work should be scoped against a population that has not survived that check.
