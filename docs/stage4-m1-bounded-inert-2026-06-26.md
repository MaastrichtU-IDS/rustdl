# Stage-4 — M1 (bounded absorption) is provably ≤ the det-resolution ceiling → the lever is integrated propagation

**Status:** analysis (durable). After the branch-count decider confirmed determinism is real
(case A) and named M1 (disjunction→deterministic-implication absorption) as the lever, this
reconciles M1 against rustdl's *already-measured* gates before building — and finds that a
**bounded** M1 is provably inert, because the real lever is propagation *strength*, not absorption.

## The dominating bound

- **det-resolution gate (prior, measured):** apply each ⊔ disjunct → `horn_fixpoint` (rustdl's
  FULL Horn deterministic closure, which uses all disjointness/`body→⊥` clauses) → count
  survivors. Wine collapse ratio **0.18–0.34** (Wine 0.18, AlsatianWine 0.18, SweetWine 0.34,
  Zinfandel 0.25); **66–82% of ⊔ points stay nondeterministic after complete Horn propagation.**
- **Konclude (decider, measured):** resolves wine's hard classes in orders-of-magnitude fewer
  branches ⇒ its deterministic expansion collapses ~all of them (~100%).

**Any exclusion-based bounded rule is dominated by `horn_fixpoint`.** M1-absorption ("force the
last disjunct when n−1 are excluded"), M2 (NomKey-disjoint edges), and the prior SP-B (told-disjoint
of a derived subsumer) all supply *disjointness/exclusion edges* that `horn_fixpoint` already
consumes. So their ⊔-collapse cannot exceed the det-resolution ceiling of **18–34%** — below the
70% GO bar, far below Konclude's ~100%. A bounded M1 is provably ≤ this ceiling.

## Why edges don't help (M2 confirmed it: 645→645)

The wine value-disjunction (`∀hasColor.OneOf{Red,White,Rosé}`) lives on the **generated
successor** node, reached via **∀-propagation**. rustdl's `horn_fixpoint` has **no ∀-rule** (∀ is
handled by ForallKey markers in the saturator's completeness path and by the wedge's ∀-rule during
*generation*, not in the deterministic closure consulted at the ⊔ frontier). So the exclusions that
would collapse the value-disjunction (e.g. "this color is forced because ≤1 + the asserted value
exclude the others, propagated through ∀ to the successor") are **not derived by the propagation
that feeds the ⊔ live-filter.** Adding more disjoint *edges* (M2/SP-B) gives `horn_fixpoint` nothing
new to fire on for these disjuncts — hence M2's 645→645 and SP-B's pruned=0.

## The actual lever (now precisely characterized)

To exceed 18–34% requires **non-Horn deterministic propagation**: ∀ through generated successors +
nominal-merge + ≤1-driven exclusion, computed as part of the deterministic closure that feeds the
⊔ choice. That is Konclude's **integrated deterministic expansion** (its `requiresNonDeterministic-
Expansion` cache reaching ~100% because its precompletion saturation runs the full ∀/nominal/≤n
deterministic rules, not a Horn-only fixpoint). It is **not a bounded rule** — it is enriching
rustdl's deterministic propagation to Konclude's strength.

## Verdict — do NOT build a bounded M1; the engine target is the integrated propagation

- A bounded M1 (absorption rule + disjointness edges) is provably ≤ 18–34% ⊔-collapse on wine
  (dominated by the measured det-resolution ceiling) → it will be inert/insufficient like M2 and
  SP-B. Building it would repeat those NO-GOs.
- The de-risking program's net, *measured* result: **determinism is the real lever (Konclude,
  decider); the specific mechanism is integrated ∀+nominal+≤n deterministic expansion feeding the
  ⊔ choice; rustdl's Horn-only deterministic closure maxes at 18–34%, so the gap is the non-Horn
  deterministic propagation.** This is a substantial, entangled build (the decider's "identified
  but not cleanly portable" caveat made concrete), but it is now well-targeted and proven-to-work
  (Konclude reaches ~100%), NOT a blind wholesale rewrite.

## Decision point

The engine lever is fully characterized and de-risked: **integrated deterministic propagation
(∀ + nominal-merge + ≤1) feeding the ⊔ live-filter.** It is the larger build the user committed to —
now with (a) confirmation it works (determinism real, not the dense wall), (b) the precise mechanism,
and (c) measured proof that bounded shortcuts (M1-absorption/M2/SP-B) cannot reach it. The fork:
commit to the integrated-propagation build, or bank the sound 15× ∃-seed (wine 49s→3.2s, FP=0,
default-ON main) as terminal. `main` pristine throughout.
