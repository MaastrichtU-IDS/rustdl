# Wedge cardinality-clash pre-check (algebraic cardinality, increment 1)

## Motivation

On the owlcs pizza ontology, the entire classify cost concentrates in
`X ⊑ InterestingPizza` pairs, where `InterestingPizza ≡ Pizza ⊓ (hasTopping
min 3)`. Deciding these refutes `X ⊓ (hasTopping max 2)`, which fires the
wedge's `≤n` merge rule (`hyper.rs::solve`): with the `≤2` violated, the
engine merges every pair of the node's distinct successors, one per branch,
recursing. When the successors are pairwise **disjoint** (pizza toppings
are), every merge clashes — but the engine discovers this empirically, by
merging-then-clashing-then-restoring. `SpicyPizza ⊑ InterestingPizza` takes
72 519 branches / 72 519 restores and does not terminate inside the wedge's
deadline (it stalls, falls through to the classic tableau, and times out).

Diagnosis evidence: `rustdl hyper-classify-probe pizza.owl` — every top
branching pair is `_ ⊑ InterestingPizza`; `match_attempts = 102.7M`.

## Increment 1: sound clash pre-check (this change)

Before branching over merges for a `≤n R.C` violation, recognise the clash
directly. Two successors are **must-distinct** when merging them must clash:
they are `≠`-forced, or they carry labels `a`, `b` with `a ⊓ b ⊑ ⊥`. If more
than `n` successors are pairwise must-distinct, the `≤n` is violated with no
possible merge → conclude `Unsat` with no branching.

- **Disjoint-pair oracle** — built once at engine construction from the DL
  clauses: a clause is a disjointness `(a,b)` iff it is **⊥-headed**
  (`head.is_empty()`) with a body of **exactly two `Class` atoms on the same
  variable** (`{A(v), B(v)} → ⊥`), `a ≠ b`. Nothing else (unary `{A(X)}→⊥` is
  "A unsat", a role-spanning body is not a pairwise disjointness).
- **Candidate set** — the *exact* set `find_open_at_most` already counts:
  `distinct_role_succ(node, role, qual)`, which includes a successor only if
  the qualifier `C` is in its saturated label set (definitely-`C`,
  representative-resolved). So the qualifier semantics are correct by
  construction; we never count a not-forced-`C` successor.
- **Clique** — a greedy lower bound suffices: any clique of pairwise
  must-distinct successors larger than `n` is a sound clash certificate.
  Missing the maximum clique just falls through to the existing merge loop
  (still sound).
- **clash_deps = `DepSet::ALL`** — matches the existing merge-Unsat path;
  conservative deps are mandatory (under-reporting would let
  dependency-directed backjumping prune a SAT sibling → false Unsat).

The merge loop also skips must-distinct pairs (not just `≠` pairs), avoiding
the wasted merge+clash+restore in partial-disjointness cases.

## Soundness

This path **only ever adds a clash** (declares `Unsat` earlier) — it never
concludes `Sat` and never skips a successor from the count. Monotonically
detecting a clash the merge search would also reach is sound; this is the
same reason early-`Unsat` is safe where deferred early-`Sat` (model caching)
is not. Verification gate: full corpus closure-diff must show **no new
subsumption edges** on any fixture (FP=0; closure identical, never a
superset), not merely a pizza speed-up.

## Scope / non-goals

Clears the `Unsat` (subsumption-holds) InterestingPizza pairs
(PolloAdAstra, Cajun, AmericanHot, FourSeasons, Capricciosa, Siciliana…)
without branching. Does **not** fix the `Sat` stalls (SpicyPizza-style):
proving `Sat` without exhaustive merge-branching needs the upper-bound /
model-finding half of algebraic reasoning — a later increment.
