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
