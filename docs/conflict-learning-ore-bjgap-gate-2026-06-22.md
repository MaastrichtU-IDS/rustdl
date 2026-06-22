# Conflict-learning go/no-go gate on the ORE per-pair-bound onts (2026-06-22)

Re-examination of the clash-driven-search / conflict-learning lever, prompted by this
session's DNF re-measurement. The prior NO-GO (`docs/conflict-learning-design-2026-06-06.md`
§3b) was measured **on wine**: backjumping never fires (0 of 1.07M clashes), 96% of
decisions on branch-created successors, ~17.5% on stable (root/deterministic) nodes. The
new question: do the 8 per-pair-bound ORE-2015 sample onts share wine's pathology, or have
a regime where conflict-learning could pay?

## Method

Throwaway instrumentation (branch `diag/ore-bjgap-probe`, reverted) added 4 `SearchStats`
counters in the wedge's `solve` (`hyper.rs`): at each ⊔ decision, whether the node is
**stable** (`birth_deps == EMPTY`, i.e. root or deterministic successor); at each disjunct
Unsat clash, whether it **backjumps** (`d ∉ child_deps`, non-overflow). Dumped per slow/
branchy pair (`branches > RUSTDL_BJPROBE_MIN`), aggregated over a `classify --pair-timeout-ms
1000` run (90–120s cap). Two metrics vs wine's baselines.

## Pre-committed decision rule (written before looking)

Proceed to the multi-month rewrite only if **backjumpable-clash fraction > 5% AND
stable-node fraction > 40%** on a **majority** of the sampled onts. Else = same NO-GO.

## Data (7 onts of the 8)

| ont | branchy pairs | disj decisions | stable% | clashes | backjumpable% | verdict |
|---|---|---|---|---|---|---|
| ore_ont_5964 | 188 | 1 213 772 | **73.0%** | 2 569 137 | **16.98%** | PASS |
| ore_ont_13545 | 196 | 1 231 744 | **73.8%** | 2 578 502 | **17.68%** | PASS |
| ore_ont_10702 | 508 | 3 056 218 | **44.8%** | 5 358 529 | **23.80%** | PASS |
| ore_ont_2313 | 174 | 13 624 | 43.4% | 4 951 | **0.00%** | fail (wine-like backjump) |
| ore_ont_8273 | 561 | 181 704 | 31.6% | 65 438 | 3.36% | fail |
| ore_ont_6485 | 165 | 42 739 | 4.5% | 18 468 | **0.00%** | fail (worse than wine) |
| ore_ont_8666 | 155 | 422 490 | 0.6% | 771 044 | 18.94% | fail (backjumps fire, nodes not stable) |

wine baseline: stable ~17.5%, backjumpable **0.00%** (0 of 1.07M).

## Verdict: NO-GO (by the pre-committed rule) — but the prior "uniform NO-GO" is refuted

**3 of 7 pass — not a majority → NO-GO for a general conflict-learning rewrite.**

But the data overturns the assumption that the wine NO-GO extends uniformly: the ORE
per-pair-bound onts are **heterogeneous**. `5964`/`13545` (73% stable, ~17% backjumpable)
are a *categorically different regime from wine* — backjumping fires and most decisions are
on stable nodes, so conflict-learning would genuinely help them. `10702` likewise (45% / 24%).
The other four are wine-like-or-worse: `2313`/`6485` never backjump (0%); `8666` backjumps
but only 0.6% of nodes are stable (a nogood would almost never be recordable); `8273` is
marginal on both.

So the realistic prize for a conflict-learning rewrite is **~3 of the 8 obscure ORE-2015
sample onts** — and a single approach won't uniformly help even those (8666 backjumps but
lacks stable nodes; the others lack backjumps). A multi-month, soundness-critical engine
rewrite is not justified for ~3 obscure benchmark files that aren't in the user's working
corpus. **Bank the shipped perf wins; this stays shelved, now with ORE-specific evidence
(not just the wine measurement).**

The favorable subset (`5964`/`13545`/`10702`) is recorded here in case a future workload
makes a similar regime corpus-relevant — those are where conflict-learning has a foothold.

**Caveats that shrink the prize further (per advisor reconcile):**
- The gate tests structural **viability** (conflict-learning isn't *blocked* as on wine),
  not **efficacy** — even the 3 passers are "maybe-helps," not "will-help."
- `5964` and `13545` have near-identical regime metrics (disj_dec/clashes/stable%/backjmp%
  all within ~1.5%) — likely the same ontology family/regime (they are NOT byte-identical:
  462 KB vs 738 KB), so the distinct favorable subset is effectively ~2 regimes, not 3.
- `8666` (backjumps fire 18.9% but only 0.6% stable nodes) shows the onts fail for
  *different* reasons — no single conflict-learning design covers the heterogeneity.
- The 8th ont (`10019`) was deliberately NOT measured: 3/7 → at best 4/8 is still not a
  majority and still not a clean GO; completeness-seeking only delays the close.

**Final: NO-GO, committed.** Realistic prize = ~2–3 obscure ORE-2015 *sample* onts where
conflict-learning *might* help — not in the working corpus, not worth a multi-month
soundness-critical rewrite. The genuinely useful byproduct is refuting the "uniform wine
NO-GO": the regime is heterogeneous, and the foothold is recorded above.
