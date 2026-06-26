# Stage-4 engine program — terminal verdict (M1 falsified by recon → bank the 15×)

**Status:** terminal verdict (durable). The recon the advisor insisted on — dumping what Gamay's
hard ⊔ points actually branch on, and whether a side is derivable — **falsified the M1 framing** and
resolved the engine program to a data-backed bank.

## The recon (decisive)

`sat(Gamay)` ⊔ survivor dump + node labels + vocabulary resolution:
- The recurring per-successor branch `[4,25]` = **`food:ConsumableThing ⊔ food:NonConsumableThing`**
  fires on successors whose labels are **`WineDescriptor / WineColor / WineBody / WineFlavor /
  WineSugar / WineTaste / Region / Winery`** — i.e. the descriptor/region/maker successors Gamay
  generates via `∃hasColor / ∃hasBody / ∃hasFlavor / ∃hasSugar / ∃locatedIn / ∃hasMaker / …`.
- The home-node ⊔ points are mostly **Tseitin synthetics** (compound wine-type-definition
  disjunctions), arity 2–5.
- Ontology check: `NonConsumableThing ≡ ObjectComplementOf(ConsumableThing)` (the partition is
  **excluded-middle**, `A ⊔ ¬A`), and **WineDescriptor/WineColor/Region/Winery have NO axiom
  constraining their Consumable-side.**

## Why this ends M1 (and the bounded-lever search)

By the advisor's pre-committed criterion — *does deferral/absorption help only if a side is
derivable at branch time?* — the answer is **neither side is derivable**: a WineColor / Region /
Winery is genuinely unconstrained w.r.t. Consumable membership (a model may place it on either
side). So:
- **M1 (disjunction→deterministic-implication absorption) is falsified**: it was scoped for
  nominal-value `∀R.OneOf` disjunctions; the actual hard ⊔ points are an unconstrained excluded-
  middle partition on generated descriptor successors + interacting compound-Tseitin definitions.
  Absorption defers *when* you branch; with no derivable side it cannot collapse these.
- The explosion is **rustdl generating an anonymous successor per `∃` and branching each on an
  unconstrained (largely irrelevant) excluded-middle partition**, compounded by the interacting
  wine-type-definition disjunctions. That is **successor generation / blocking / nominal handling**
  — the nominal-architecture/generation wall named in `wine-wall-bjgap1-genuine`, re-found from the
  disjunction side.

## The full engine-program result (this session)

Determinism **is** real in Konclude (branch-count decider: it classifies wine in ~132ms; rustdl
`sat(Gamay)` is 451k branches/30s-Stalled; the total-time bound rules out the per-branch-cost
escape). But **every transplantable bounded mechanism was measured out**, each by the corpus/probe,
not by inference:
- sound completion-graph reuse — minimal-sound-key gate (no sparse sound key; whole-graph = no reuse).
- algebraic cardinality — would-prune gate (marginal=0; Konclude has none either).
- precise merge-dep backjumping — SP-0 (deps genuinely dense) + FP=232.
- cardinality-exclusion edges (M2) — built, sound, **inert** (645→645; no consumer).
- disjunction absorption (M1) — **falsified by recon** (genuinely-open excluded-middle target).

Konclude's speed is its **integrated architecture** (generation + nominal collapse + lazy/absorbed
expansion + the deterministic-expansion cache acting together), not any single transplantable rule.
That is the repeatedly-NO-GO'd large rewrite with **no cheap/bounded entry** — now confirmed from
every angle this session.

## Verdict — BANK

**The sound 15× ∃-seed (wine 49s→3.2s, FP=0/MISSED=0 corpus-wide, default-ON `main`) is the engine
program's terminal deliverable.** The residual wall is genuine disjunctive model-search over wine's
descriptor/definition space (generation-bound), whose only lever is Konclude's integrated
generation/nominal architecture — a large rewrite with no bounded entry, which this session
characterized exhaustively (≥10 measured NO-GOs/falsifications). Continuing to relocate the lever
(M2→M1-nominal→M1-GCI) without a firing measurement was the anti-pattern; the recon ended it.

`main` is pristine throughout (ee6904c). M2 (`feat/nomkey-diff-disjoint`, sound but inert) and all
probes/docs are unmerged research record. If the integrated generation-rewrite is ever undertaken,
it is a fresh, large sub-project committed to with eyes open — not a continuation of the bounded
search, which is now closed.

## One honest open thread (not a now-action)

rustdl branches eagerly on the *tautological* `ConsumableThing ⊔ ¬ConsumableThing` partition on
descriptor successors where the polarity is irrelevant to Gamay's satisfiability. Whether a
tautology/irrelevance-skip (don't branch a disjunction whose polarity nothing depends on) is a
cheap sound win is **untested** — but proposing it now would be another infer-relocate step. It is
logged as a future hypothesis requiring its own firing measurement (does it actually reduce
branches, FP=0), not a justification to keep the program open.
