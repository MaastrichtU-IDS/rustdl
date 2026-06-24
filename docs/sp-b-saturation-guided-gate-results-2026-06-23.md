# SP-B saturation-guided viability gate — RESULTS + VERDICT — 2026-06-23

**Verdict: NO-GO** (against the pre-committed strict Konclude-class bar). The
saturation-guided ⊔ filter does NOT collapse wine's branch count. Bank B1–B2c as
sound foundations; wine stays an accepted perf gap. The throwaway gate code
(`spike/sat-guided-disjunction`) is NOT merged.

## What was measured

The B1–B2c saturation forcing was wired into the wedge's ⊔ choice as a
live-disjunct filter: a named-class head disjunct `Dₖ` is pruned at a node iff some
class in that node's label has a **derived** subsumer (per the now-complete-on-wine
saturation closure) told-disjoint with `Dₖ`. Gated by `RUSTDL_SAT_GUIDE`. Measured
on the four matched hard wine pairs via `decide_pair_probe` / `sat_class_probe`
(the production `HyperCache::decide` path), depth 256, 60 s deadline, adaptive
budget forced OFF (so the divergence early-cut does not mask the raw branch count).

## Results (OFF baseline vs ON)

| pair | OFF branches | ON branches | points_seen | pruned | forced_single | verdict |
|---|---|---|---|---|---|---|
| AlsatianWine ⊓ ¬AmericanWine | 67524 | 67110 | 28646 | **0** | **0** | Stalled (both) |
| sat(SweetWine) | 68585 | 67878 | 25315 | **0** | **0** | Stalled (both) |
| sat(Zinfandel) | 59654 | 59182 | 26308 | **0** | **0** | Stalled (both) |
| sat(RedWine) | 65898 | 65361 | 46532 | **0** | **0** | Stalled (both) |

OFF and ON both DNF at 60 s with ~60–68 k branches. ON branch counts are ~1 %
lower — within deadline jitter, NOT a collapse. The Konclude-class bar (hundreds,
not tens of thousands, on ≥2/3 pairs + single-digit-second wall) is missed on
0/4 pairs.

## Why — the decisive diagnostic: `pruned = 0` with `points_seen` in the tens of thousands

The hint **fired heavily** (25 k–46 k ⊔ branch points seen per pair) but **pruned
zero disjuncts and forced zero singles, on every pair**. So at none of wine's tens
of thousands of ⊔ branch points was any disjunct incompatible-by-told-disjointness
with the node's label. This is not "the hint rarely fired" (it fired everywhere)
and not "fired but didn't collapse the right subtree" — it is **"fired everywhere
but found nothing prunable."**

### Ruling out the two innocent explanations of `pruned = 0`

`pruned = 0` could be a no-op (empty/misaligned tables) or a true negative. Two
follow-up checks (advisor-prompted) settle it as a **true negative**:

1. **The guide is NOT a no-op.** `wine_guide_disjoint_table_is_populated`:
   `classes_with_disjoints = 35`, `total_disjoint_entries = 78`,
   `max_subsumers_of_any_class = 36`. Both the told-disjoint table and the
   saturation-closure subsumers are populated — `is_dead` has real data.
2. **The ⊔ disjuncts ARE the filterable kind (named classes).** Head-atom
   composition at guided ⊔ points (5 s diagnostic run):

   | pair | class_atoms | ∃/≤n atoms | pruned |
   |---|---|---|---|
   | AlsatianWine ⊓ ¬AmericanWine | 7346 | 34 | 0 |
   | sat(SweetWine) | 5101 | 5 | 0 |
   | sat(Zinfandel) | 5099 | 7 | 0 |
   | sat(RedWine) | 8376 | 69 | 0 |

   Wine's ⊔ disjuncts are **>99 % named classes** — the filterable kind, not ∃/≤n.
   So the mechanism is NOT too narrow in the "disjuncts aren't classes" sense.

So: named-class disjuncts dominate, the guide has real disjoint data, and still
**nothing is told-disjoint-dead**. The strong claim is earned: **told-disjointness
is too weak to resolve wine's disjunctive choices.** What makes those choices
determinate (the reason Konclude does wine in ~114 ms) is ∀-propagation, nominal
identity, and ≤n-forced merges — semantics that do NOT manifest as a told-disjoint
named-class disjunct at a wedge ⊔ point. The saturator's completeness on wine
(B1–B2c) comes from its own synthetic-marker machinery (NomKey / ForallKey / MaxKey
/ DKey) — which collapses the *closure*, but provides no told-disjoint signal the
wedge's clausal ⊔ points can exploit.

The load-bearing assumption — *the saturation forcing that made the saturator
complete on wine translates into prunable wedge ⊔ choices* — is therefore **false**:
not because the bridge was wired wrong, but because the pruning relation
(told-disjointness, the exact relation B1–B2c use) carries no information at wine's
branch points. This is consistent with [[wine-wall-bjgap1-genuine]] (wine's wall is
a nominal-merge-architecture property, not a search-guidance one).

## Verdict-preservation

No false verdict was produced: every ON run reached the same outcome as OFF
(Stalled/DNF — neither completed within 60 s, so no Sat/Unsat to flip). The filter
is sound by construction (only prunes told-disjoint-incompatible named-class
disjuncts); `pruned = 0` means it conservatively pruned nothing, which is trivially
verdict-safe. The gate's correctness guard holds; the result is a true negative,
not a measurement artifact.

## Scope of this NO-GO

This refutes the **specific mechanism** (named-class derived-subsumer × told-
disjointness ⊔ filtering), not the entire build-once architecture in the abstract.
But it refutes it at the load-bearing joint: the bridge from "complete saturation
closure" to "collapsed wedge branching" does not exist for wine via disjunct
forcing, **and the diagnostic localizes why** — not "disjuncts aren't classes"
(they are, >99 %) and not "guide is empty" (it has 78 disjoint entries over 35
classes), but **told-disjointness — the exact relation B1–B2c forcing is built on —
carries no pruning information at wine's ⊔ points.** A stronger bridge would need a
richer incompatibility relation than told-disjointness (∀-propagated / nominal-
identity / ≤n-merge-aware), i.e. it would have to reconstruct the deep semantics
inside the guide — which is the multi-month nominal-architecture work itself, not a
cheap hint. There is no evidence such a bridge reaches the Konclude regime, and
strong evidence (this gate + [[wine-wall-bjgap1-genuine]]) that "feed the saturation
forcing into the wedge" is simply not wine's lever.

## Consequence

- **NO-GO on the multi-month build-once core via this mechanism.** Do NOT commit
  the months.
- B1/B2a/B2b/B2c remain **sound FP=0 foundations** on `feat/build-once-redesign`
  (they made the saturator complete-in-output on the disjunctive+wine fragment;
  default-classifier output unchanged). They are banked, not merged to main.
- Wine stays an accepted perf gap (knob `--pair-timeout-ms`, MISSED=0).
- The gate code (`spike/sat-guided-disjunction`) is discarded; only this verdict
  doc lands on `feat/build-once-redesign`.

## Method note

The gate was decisive precisely because it wired the forcing into the real search
and measured `pruned`/`forced_single` — not a read-only "would-collapse" proxy. The
prior read-only SP-B gate's inconclusive 66–90 %-free figure is now explained:
those "free" ⊔ points are free because there is nothing told-disjoint to prune, and
the deep derived closure (B1–B2c) does not change that.
