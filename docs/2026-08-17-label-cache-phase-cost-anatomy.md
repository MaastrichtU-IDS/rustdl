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
