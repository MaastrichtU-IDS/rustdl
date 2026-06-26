# Stage-4 hard-class diagnostic — RESULTS + VERDICT (2026-06-26)

**VERDICT: GENUINE.** Wine's residual hard classes (the wall after ∃-seed + tight cap) are
**~19 genuinely-nondeterministic classes** — not capturable by richer all-model saturation.
The saturator already has the full ∀/≤1/nominal rule set, already seeds these classes their
derived ∃-facts, and they *still* blow up in the wedge's disjunctive search. So the only path
to ms for wine's residual is the **deep search-engine rearchitecture** (Konclude's integrated
nominal / per-test tree-shrinking — the no-cheap-entry frontier), not a bounded saturation
extension.

## Probe A — richer saturation is already present (premise moot)

Cherry-picking saturation increments 1 (`∀R.C`) + 2 (`≤1 R.C`) onto the current saturator
**fails to compile — duplicate `forall_atomic_operands_on_right`**: the current lineage
*already has* increment-1's ∀-machinery (the B2b ForallAtomicKey work) plus ForallKey
(`∀R.OneOf`), functional/≤1 witness-merge, MaxKey, NomKey. The increments were built on an
earlier saturator state (`1110e02`) and are subsumed by what's present. So "enrich the
saturation with ∀/≤1 rules" is a no-op here — the rules exist, and the 19 persist *despite*
them. (Cherry-pick aborted; not merged.)

## Probe B — the 19 hard classes are seeded yet hard (the decisive measurement)

`saturate_with_exists_facts(wine)` + per-class NoVerdict@2 s labeling (∃-seed on):

| | value |
|---|---|
| global derived ∃-facts | kept (named/NomKey) **194**, dropped (Tseitin/DKey) **64** |
| **HARD classes (NoVerdict @ 2 s)** | **19** |
| their ∃-facts | **kept 54** (avg ~3/class), dropped 17 |
| hard classes with ≥1 dropped ∃ | 17 / 19 |
| hard classes with ZERO ∃-facts | **2 / 19** |

**Reading:** the 19 hard classes are **not ∃-fact-starved** — 17 of 19 already have derived
∃-facts (54 total) that the ∃-seed *does* seed, and they collapse nothing: the classes stay
NoVerdict *with those facts seeded*. The 17 dropped ∃-facts are modest, and since 54 kept
facts don't collapse them, recovering 17 more won't. Only 2 are genuinely ∃-starved (a
marginal sliver, and the saturator's ∀/≤1 rules — already present — don't derive their
determinism either). **Conclusion: the 19 are hard from genuine disjunctive nondeterminism,
not uncaptured determinism.**

## What this means for "to ms"

The arc decomposes cleanly and is now fully measured:
- **The ∃-seed harvested the *capturable* determinism** → wine 49 s → 3.2 s (~15×), sound
  (FP=0/MISSED=0). This is the shippable win.
- **~19 classes are the genuine nondeterministic core** → the remaining ~28× to Konclude's
  114 ms. No all-model saturation collapses them (confirmed post-∃-seed, distinct from SP-0's
  pre-∃-seed backjumping finding). They need the **engine property** Konclude has: per-test
  completion-tree-shrinking on the nominal+disjunction fragment.

So **bounded saturation extension is closed for the residual** (the rules are present; the 19
are genuine). The deep engine rearchitecture is the only path to ms — a large, FP-critical,
no-cheap-entry program, but Konclude proves the *target* (114 ms) is achievable. That is a
fresh, ∃-seed-era go/no-go on the engine frontier, to be committed to with eyes open as its
own sub-project — not a quick continuation.

## Disposition

Diagnostic only; the Probe-A cherry-pick was aborted, the Probe-B instrumentation removed.
Branch `feat/stage4-hardclass-diagnostic` (spec + this verdict). `main` untouched. Wine's
sound result stands at **3.2 s (15×, FP=0/MISSED=0)**; the genuine 19-class core is the
engine frontier.
