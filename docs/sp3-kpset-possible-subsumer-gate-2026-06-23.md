# SP3 (saturation-based KPSet possible-subsumer pruner) — gate verdict: NO-GO — 2026-06-23

The one Konclude technique that targets rustdl's *actual* cost (refutation, not
completeness). Gated SP0-style — verdict reached from existing code semantics + the
already-measured wine banner, **no build needed**.

## The idea

Konclude's KPSet classifier derives, per class C, an over-approximation of its
**possible subsumers** `possible(C) = {D : C ⊓ D satisfiable}`, computed cheaply from
the saturation. Any `D ∉ possible(C)` ⟹ `C ⋢ D` for certain (sound non-subsumption,
no tableau). The hope: prune wine's 8251 timed-out refutation pairs cheaply.

## Pre-committed verdict rule

GO only if a saturation-based `possible(C)` could prune pairs that rustdl's existing
label heuristic does not. Else NO-GO.

## Verdict: NO-GO — strictly dominated by the existing label heuristic

rustdl's Phase-7 label heuristic (`LabelOracle::Sat`, `reasoner/src/lib.rs:1442`) is
**already a tighter, model-based form of the same pruner**. Its contract (verbatim):

> `Sat(labels)`: C is satisfiable; root-node labels are the candidate subsumer set.
> `D ∈ labels` → verify via per-pair test; `D ∉ labels` → sound non-subsumption (this
> completion graph is a counterexample).

So `labels(C)` = the root labels of **one satisfying model** of C, and it prunes
`C ⋢ D` whenever that single model omits D. Three independent consequences, each
fatal to SP3:

1. **Domination.** KPSet prunes `C ⋢ D` only when `C ⊓ D` is UNSAT (D not a possible
   subsumer). But `C ⊓ D` UNSAT ⟹ D is absent from *every* model of C ⟹ absent from
   the label heuristic's sampled model ⟹ **the label heuristic already prunes it.**
   So KPSet's prune set ⊆ the label heuristic's prune set. It can never prune a pair
   the label heuristic missed.

2. **The hard pairs are co-satisfiable (unprunable).** Wine's 8251 timed-out pairs
   *passed* the label heuristic, i.e. `D ∈ labels(C)`. The root satisfies C and D
   together ⟹ **`C ⊓ D` is satisfiable** ⟹ D is a genuine possible subsumer ⟹ **no
   sound possible-subsumer pruner can ever fire on them.** The refutation that
   `C ⊑ D` is false (`C ⊓ ¬D` SAT) is irreducible — wine's non-subsumptions are
   between compatible classes. (Same wall the whole arc hit: wine's cost is
   refutation, not pruning.)

3. **A saturation bound is *looser*, not tighter.** `labels(C)` comes from a concrete
   model — a specific small set. A sound saturation over-approximation of
   `possible(C)` must include everything possibly-derivable across all branches, so
   `possible_sat(C) ⊇ labels(C)`: it would test *more* pairs, not fewer. Strictly
   worse for the O(n²) frontier too.

## Cost attribution kills the perf angle too

The one place a cheap saturation bound could help is build cost: the label heuristic
builds `labels(C)` from a per-class wedge satisfiability call. Wine banner:

```
# wall breakdown ms: label_cache_build=3014 ... tier_walk=100598
# label heuristic: pruned=5715 pass_through=81 misses=8235
# timed-out pairs: 8251
```

`label_cache_build` is **3s of ~104s**; `tier_walk` (the 8251 refutations) is **100s**.
Even a free (saturation-built) possible-subsumer set saves the 3s and cannot touch the
100s — the refutations are between co-satisfiable classes (point 2).

## Conclusion

The saturation-based KPSet pruner is strictly dominated by a mechanism rustdl already
ships, on a cost that isn't the bottleneck, against pairs that are provably unprunable.
**NO-GO**, consistent with the whole saturation arc: every saturator-side lever
(completeness SP1, deterministic seeding SP2, possible-subsumer pruning SP3) is
inert on rustdl's corpus because the saturator already answers 100% of positives
(`tableau=0`) and wine's residual cost is irreducible refutation between compatible
classes. The only remaining wine lever is the deferred non-deterministic model-reuse
re-architecture — not a saturator innovation.
