# Backjumping / no-good learning — the wall lever (scoping)

Drafted 2026-05-28. The HF1–HF4 calculus is complete for the corpus and
`Unsat`-sound. HF5 (wire the engine as the complete classifier to *move
the classify wall*) was gated on an integration bet: is the engine's
per-pair cost low enough to beat the timing-out tableau? **A focused
measurement says: not yet — and the lever is backjumping, not wiring.**

## §0 — The decisive measurement (pizza)

`rustdl hyper-classify-probe pizza.ofn` now prints a branched-pair wall
histogram. Pizza, 9702 pairs, 3935 branched, 4:44 total:

| wall | pairs |
|---|---|
| <10 ms | 2777 |
| <100 ms | 862 |
| <500 ms | 197 |
| <1 s | 5 |
| <2 s | 1 |
| **2–5 s** | **91** |
| ≥5 s / stall | 2 |

**92.5 % of branched pairs finish <100 ms.** The 4:44 wall is ~93 slow
pairs — **almost all `SpicyPizza`-as-sub**, each with an *identical*
`branches = 167968, depth = 12, wall ≈ 2 s` (and 2 stalls at 5 s:
`SpicyPizza ⊑ NonVegetarianPizza` / `InterestingPizza`).

## §1 — Diagnosis: no learning, not redundant recomputation

The identical branch counts across *unrelated* sups (`SpicyPizza ⊑
MeatTopping`, `⊑ Mushroom`, `⊑ PizzaBase` …) are the signature: the
engine explores `SpicyPizza`'s entire disjunctive tree before the `¬sup`
constraint ever discriminates, because `¬sup` is trivially satisfied at
every leaf. There is **no mechanism to remember that a sub-tree's
disjunction choices didn't contribute to any clash**, so the same
167968-branch tree is re-walked every time.

The fix is **dependency-directed backjumping / no-good learning**
(DPLL-style over the disjunction trail): when a branch closes, compute
the set of decisions actually responsible for the clash; backjump past
irrelevant intervening decisions; record the no-good so the same dead
sub-tree isn't re-entered. This collapses `SpicyPizza`'s 167968 branches
to the few that matter — fixing the 91 slow pairs *and* the 2 stalls.

**Not sub-model caching.** Caching `SpicyPizza`-Sat and testing sups
against it is a soundness trap: any sup that propagates back into the
sub (inverse / nominal / cardinality coupling) invalidates the cache,
and detecting that reliably is the problem the classifier already solves
by top-down dispatch. Backjumping gets the same speedup without the
soundness gymnastics.

## §2 — Scope

- **Trail + dep-sets:** each disjunctive decision carries a decision
  level; each derived atom/clash carries the set of decision levels it
  depends on (the existing `≠`/clash machinery already threads some of
  this — the H3c back-jumping in the *other* tableau is the reference).
- **Backjump:** on a closed branch, jump to the deepest decision in the
  clash's dep-set, not the chronologically previous one.
- **No-good (optional, second increment):** record the clashing
  decision set to prune re-entry. Start with backjumping; add learning
  if the wall isn't moved enough.
- **Gate:** pizza `hyper-classify-probe` total wall drops sharply (the
  91 slow pairs collapse) and the 2 stalls resolve to `Sat`; corpus
  agreement unchanged (695/158/51, 0 FP); SIO unaffected.

## §3 — Then HF5

With backjumping landing the hard pairs in milliseconds, the HF5 wiring
bet becomes confident: wire the engine's three-valued verdict
(`Unsat→subsumed`, `Sat→not-subsumed` under the agreement-check gate,
`Stalled→fall back to tableau`) into the production classify's residual
path, lifting the H4 wedge's `Unsat`-only restriction. **HF5 is gated on
this phase**; without it, wiring leaves the 93 hard pairs slow and moves
the wall only partially.

## §3.5 — Shipped (disjunction backjumping)

Dependency-directed backjumping over the disjunction trail, dep-sets as
a `u128` bitset (`DepSet`, overflow ⇒ conservative). Result on pizza:
**4:44 → 13.2 s (~21×)**, the 91 slow `SpicyPizza` pairs collapsed to
`<10 ms`, one of the two stalls resolved; corpus unchanged (695 / 158 /
51, **0 FP**); SIO 0.95 s. The `≤n`-merge branch stays conservative
(`DepSet::ALL`, chronological) — backjumping through merge decisions is
future work.

**The bug that proves the discipline:** the first cut tracked dep-sets
on labels only and **regressed pizza to 753 with 58 false positives**
(`Topping ⊑ VegetarianPizza` — unsound backjump). Cause: a clause
matching a successor via a role atom (domain-style `R(x,y) → D(x)`)
depends on that successor *existing* — created under a decision — but
the dep-set missed it (no class atom on `y` to carry it). Fix: a
per-node **`birth_deps`** (the decision dep-set a node was created
under), unioned in `clause_body_deps` for every bound node. The corpus
0-FP check caught it; the 85 hand-built branching tests did **not** —
the corpus is the soundness net for dep propagation.

## §4 — Risk / honesty

Backjumping is a real implementation effort (trail + dep-set
bookkeeping over the branch search), not a one-commit increment — the
classic place for off-by-one dep-set bugs that cause *unsound* pruning
(backjump past a decision that *was* load-bearing → miss a clash → false
`Sat`). Drive it with crafted canaries where the naive engine over-
explores and a known clash must still be found, plus the corpus 0-FP
guard. This is the first phase since HF1 that is squarely a
*search-quality* problem, not a calculus rule.
