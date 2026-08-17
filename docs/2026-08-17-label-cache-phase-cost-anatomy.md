# What the label-cache phase actually spends its time on — and one unexploited finding

**Date:** 2026-08-17 · Item 3 of the post-session plan: attack the phase where the DNF tail
dies (`ore_ont_9944` spends its entire 600 s there; `11311` 231 s), given that five mechanisms
are already refuted against it.

## Profile

`samply` on `ore_ont_11311` with a 200 ms per-class label budget (so the run terminates),
single-threaded, symbols resolved via `addr2line`. Self-time, aggregated over the top-60
addresses (54% of all samples accounted):

| function | self-time |
|---|---|
| `HyperEngine::enumerate_matches` | **16.3%** |
| `HyperEngine::apply_head_atom` | 9.7% |
| `HyperEngine::fire_clause` | 7.9% |
| `HyperEngine::match_body` | 4.0% |
| `HyperEngine::clause_body_deps` | 3.6% |
| `HyperEngine::solve` | 2.6% |
| `process_event` | 1.7% |

**~46% is the wedge's clause fire/match loop.** This corroborates the standing in-tree note
that "the residual wedge-classify cost is `enumerate_matches`/`match_body` (the non-Horn fire
loop, ~25% self-time)" — measured here at 34% for the fire/match trio alone.

*(Tooling note: `samply` must be invoked by absolute path under `timeout`, and its output needs
`addr2line` against the binary — the profile's `funcTable` carries raw addresses.)*

## Both obvious levers are ALREADY shipped and default-ON

Before proposing anything, the two candidates that follow from the profile turned out to exist:

1. **Seed each per-class run with the shared saturation closure** so the wedge does not
   re-derive it — this is `RUSTDL_SAT_SEED`, **default ON**, which "seeds `Q → D` for every
   entry" of a table computed once via `owl_dl_saturation::saturate`.
2. **Reuse the clause index across classes** instead of rebuilding per class — this is
   `RUSTDL_CLASSIFY_LABELS_AMORTIZE`, **default ON since 0.4.10** (and mis-documented as OFF
   until today; see that commit).

So the 46% is what remains *after* both. The wedge is not re-deriving the closure.

## New finding: the closure seed TAXES this phase, for identical output

Measured min-of-3/5 interleaved, `--pair-timeout-ms 1000` (non-truncating), single-threaded.
`label_cache_build` only:

| ontology | SEED=1 | SEED=0 | delta | output |
|---|---|---|---|---|
| `ore_ont_5303` | 535 ms | **65 ms** | **+723%** | identical |
| `ore_ont_16847` | 140 ms | 107 ms | +31% | identical |
| `ore_ont_11378` | 1,276 ms | 1,079 ms | +18% | identical |
| `ore_ont_10908` | 202 ms | 183 ms | +10% | identical |
| `ore_ont_15010` | 5,909 ms | 5,484 ms | +8% | identical |
| `ore_ont_7877` | 7 ms | 11 ms | −36% | identical |

On `ore_ont_11378` the split is tight and reproducible — 1276–1292 ms versus 1079–1096 ms
across five interleaved runs, **no overlap** — and the closures hash identically
(`79a185c1…`). So within this phase the seed costs up to **8×** and buys nothing.

That is not a contradiction of the flag: `RUSTDL_SAT_SEED`'s documented win is the per-**pair**
oracle (wine 49 s → 3.2 s, ~15×). What is new is that its cost lands squarely on the phase
where the DNF tail dies.

## But it is NOT a flip — total wall splits both ways

The seed's other effect is *pruning*: seeded models are tighter, so `D ∉ labels(C)` refutes
more pairs. Drop it and the build is cheaper but more pairs reach the oracle.

| ontology | wall SEED=1 | SEED=0 | delta |
|---|---|---|---|
| `ore_ont_5303` | 2.79 s | **0.97 s** | **+186%** |
| `ore_ont_15010` | 6.02 s | 5.58 s | +8% |
| `ore_ont_16847` | 0.64 s | 0.61 s | +5% |
| **`ore_ont_11378`** | **2.92 s** | 3.95 s | **−26%** |

So the seed costs 2.9× on one ontology and *saves* 26% on another. **Instance-dependent.**

**Soundness of the candidate change is favourable**, which is why it is worth recording rather
than discarding: omitting entailed seed facts can only make a model MORE permissive ⇒ larger
label sets ⇒ *fewer* refutations ⇒ pairs get tested rather than pruned. That is
completeness-neutral-or-better and FP-safe; the only exposure is wall.

## FOLLOW-UP (same day): the candidate is instance-level, and my mechanism story was WRONG

Three further measurements, in order.

**1. The seed's effect on the target population is LARGE and goes BOTH ways.** Same per-class
budget (300 ms), counting classes that end in `NoVerdict`:

| | `SAT_SEED=1` | `SAT_SEED=0` |
|---|---|---|
| `ore_ont_11311` | 341 timeouts / phase 231.3 s | **480** / 263.5 s |
| `ore_ont_9944` | **1,065** timeouts / phase **583.3 s** | **64** / **107.2 s** |

On `9944` the seed *causes* 16× more timeouts and a **5.4× longer phase**; on `11311` it
*prevents* 139. Both reproducible. **The earlier six-ontology table measured the wrong
population** — all easy, all completing, where the phase is not the problem. The seed-site
comment says why it exists: it lets hard classes' `sat` calls terminate inside the deadline
(wine had ~4,638 label-cache misses). It does that on `11311` and the opposite on `9944`.

**2. It does NOT rescue the DNF, because the seed and the BUDGET are entangled.** `ore_ont_9944`
DNFs at a 300 s cap under every combination tried — `SAT_SEED` ∈ {0,1} × label budget ∈
{default, 300 ms, 1000 ms}. With `--pair-timeout-ms 5` and 8,008 classes the adaptive rule sets
the per-class budget to the **30 s ceiling** (`clamp(8008 × 5, 50, 30_000)`), so even 64
timing-out classes cost half an hour. This is the same coupling as the 18×
`ore_ont_15010` starvation defect, approached from the other side.

**3. The role-richness mechanism is REFUTED.** Hypothesis, pre-registered before measuring:
`exists_seed` derives ∃-facts through chains / transitivity / inverses and each entry becomes a
per-class clause feeding `enumerate_matches`, so role-rich ontologies should pay more. Tested
on a role-stratified sample of **completers** (the first attempt drew from the DNF tail and
`ore_ont_3575` DNF'd in both arms, yielding no delta):

| ontology | roles | SEED=1 | SEED=0 | seed cost |
|---|---|---|---|---|
| `ore_ont_9151` | 188 | 29,982 ms | 26,821 ms | +12% |
| `ore_ont_7532` | 156 | 858 ms | 624 ms | +38% |
| `ore_ont_1509` | 180 | 1,930 ms | 1,987 ms | −3% |
| `ore_ont_8911` | 177 | 10,060 ms | 10,061 ms | −0% |
| 4 more | 133–140 | | | −3% … +1% |

**Median seed cost −0%**, Pearson r = +0.202 (n=8, meaningless). These carry **6–8× more role
machinery than `9944`** (188 vs 22) and show **no effect** — if richness drove it they would
show it more, not less. The mechanism is wrong.

A further confound worth recording: only **8 of 38** sampled completers had a measurable label
phase at all, and **none** were role-poor — because role-poor usually means pure-EL, and
pure-EL takes the saturation fast path and never builds a label cache. "Role-poor" and "has the
phase under test" are anti-correlated **by construction**, so this design cannot produce the
contrast it needs.

**Status: one strong instance-level finding, no structural predictor.** `SAT_SEED=0` is worth
knowing about as a per-instance escape hatch on `9944`-like inputs, and it is NOT a default
change and NOT a rule. Three test designs failed before this conclusion (DNF-tail sample,
pure-EL confound, refuted mechanism) — recorded so the next attempt starts past them.

## Where this leaves item 3

**The candidate is "skip the closure seed in `classify_labels` while keeping it in the per-pair
oracle"** — a phase-scoped split of a flag that is currently all-or-nothing. Its prize is up to
8× of the phase that defines the DNF tail; its risk is losing pruning on ontologies like
`11378`.

**It needs a corpus sweep, not a decision from six ontologies** — and note that this is exactly
the shape the learned-cap assessment warned about
(`docs/2026-08-17-learned-cap-tuning-assessment.md`): a per-instance-optimal setting whose
population effect is unknown and whose two phases pull in opposite directions. The sweep is the
honest next step; a default flip on this evidence would be premature.

**The residual after that is constant-factor work on `enumerate_matches`** — already the
in-tree residual, and this codebase has a history of reverted attempts at exactly that kind of
indexing change (Phase 3e edge-keyed role-rule indexing, reverted at +2.34% GALEN).
