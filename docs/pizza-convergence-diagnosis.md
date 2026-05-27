# Pizza/SIO convergence diagnosis — blocking is fine, branching is the bottleneck

Drafted 2026-05-27, closing the blocking investigation requested
after the lazy-unfolding work. Supersedes the "needs hypertableau
(multi-month)" framing in
[`architecture-roadmap.md`](architecture-roadmap.md) with a more
specific — and more tractable — lever.

## The question

`is_blocked_true = 0` on pizza (every `is_blocked` call returns
false). Is that a blocking *bug* — and if fixing it would cap the
model and unlock everything downstream — or is it correct
behaviour on a genuinely-large model?

## Measurements (NamedPizza sat probe, 500 ms, `--features counters`)

```
apply_exists               = 118042
add_edge_calls             =    965
is_blocked_calls           = 708252
is_blocked_prefilter_rejects = 31866
is_blocked_subset_scans    =   2694
is_blocked_true            =      0   (absent from dump ⇒ 0)
```

## What the numbers say

1. **Blocking is not buggy.** The existing unit test
   `exists_terminates_on_cyclic_tbox_via_blocking` (`A ⊑ ∃R.A`)
   passes — blocking fires and caps that model at ≤ 4 nodes. The
   mechanism works.

2. **Blocking correctly does *not* fire on pizza.** The full
   subset scan runs 2694 times (a role-matching ancestor passed
   the signature prefilter) and `L(y) ⊆ L(x')` holds **zero**
   times. Pizza's `∃hasTopping` successors carry genuinely
   different labels (`{PizzaTopping, CheeseTopping}` vs
   `{PizzaTopping, MeatTopping}`) — neither is a subset of the
   other, so neither blocks the other. This is *correct*: those
   are distinct individuals in the model.

3. **The model is small.** `add_edge_calls = 965` across the
   entire 500 ms search — and that's the cumulative count over
   *all* explored branches, with rollback removing nodes from
   failed branches. The per-branch model is a few dozen nodes,
   not thousands. The earlier "92 nodes" trace figure is the peak
   across branches, not a single model.

4. **The bottleneck is search branching.** `apply_exists` fires
   118 k times over ~12 k saturate passes — i.e. the search
   explores on the order of ten thousand disjunction-branch
   points in 500 ms and still doesn't find the satisfying
   assignment for a concept that *is* satisfiable (HermiT agrees).

## Why so many branches for a satisfiable concept

Pizza's toppings interact through `DisjointClasses` axioms.
Choosing topping `X` can forbid topping `Y`. Finding a consistent
set of topping/base choices is a **constraint-satisfaction
problem**, and the naive depth-first tableau search re-discovers
the same local conflicts in many different sub-trees because it
doesn't *learn* from a clash — it just backtracks chronologically
(with dependency-directed back-jumping, but no clause learning).

This is exactly the problem modern SAT solvers solve with
**conflict-driven clause learning (CDCL)**. The DL-tableau analog
is conflict-driven no-good learning.

## The lever: fix and enable CDBL (already wired, currently disabled)

`TableauContext` already carries `learned_nogoods` and
`record_nogood` / `nogood_blocks` (lib.rs:164-360). The lookup is
**intentionally not wired into `search::branch`** (search.rs
~line 140) because the naive key is unsound:

> The naive "precond ⊆ active ⇒ skip" rule is unsound on pizza —
> verdict went from 2 unsat to 0 unsat — because the
> preconditions don't fully capture *which* node labels produced
> the clash; two no-goods recorded in different sub-trees can fire
> jointly at a node that's actually sat.

The fix is well-understood SAT-solver technology adapted to
tableau:

- **Track labels-as-evidence.** Today the trail records
  `TrailEntry::LabelAdded { node, concept }` per insertion. Extend
  it with the *immediate cause* (which earlier label/edge
  triggered the rule that added this one). That gives a "1-UIP
  cut" for clash extraction.
- **Key no-goods on the unsat-explaining label sub-set**, not on
  `(node, or_label, disjunct, branch-id-precond)`. A no-good then
  says "this *set of labels* at a node is jointly unsat" — which
  is sound to reuse anywhere that label-set recurs, exactly like
  a learned clause.

This is the principled implementation the search.rs comment and
`perf-2026-05-24-new-server.md` §5 both point at.

## Scope and expected impact

- **Scope:** multi-week. Touches the trail (cause tracking), the
  clash-extraction path (1-UIP cut), and the no-good
  store/lookup. Smaller than a hypertableau rewrite (which
  replaces the whole expansion algorithm); CDBL keeps the
  existing tableau and adds learning on top.
- **Expected impact:** directly targets the pizza/SIO branching
  explosion. SAT-solver experience says clause learning is the
  single highest-impact optimisation for constraint-interaction-
  heavy inputs — which pizza's disjoint-topping structure
  exactly is. No guarantee it makes SIO classes converge (their
  blowup may be existential-depth, not disjunction-interaction —
  needs the same per-class trace to confirm), but it's the most
  promising single lever for pizza.

## Revised lever ranking (supersedes architecture-roadmap §"Levers ranked")

1. **CDBL with labels-as-evidence** — multi-week, targets
   pizza's branching directly, reuses existing wiring. *New top
   recommendation.*
2. **Hypertableau** — multi-month, the general solution, but a
   full algorithm rewrite. Pursue only if CDBL plateaus.
3. **`--saturation-only`** — shipped; the production answer for
   mostly-EL workloads regardless.

The completed-model levers (model caching, snapshot/replay) stay
ruled out for pizza/SIO per
[`architecture-roadmap.md`](architecture-roadmap.md) — there's no
completed model to cache when the search times out.
