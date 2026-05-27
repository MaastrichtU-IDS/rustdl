# Hypertableau H3c — `≤n` merge for cardinality (scoping)

Drafted 2026-05-27. The last 24 pizza misses are min-cardinality (20,
`InterestingPizza`) + nominals (4). This scopes the cardinality slice:
the `≤n` successor-merge rule — the hardest SROIQ rule, and the first
engine change that mutates graph *structure* (identifying two nodes).
**Do not implement before this design is settled.**

## The pizza target

`InterestingPizza ≡ Pizza ⊓ ObjectMinCardinality(3 hasTopping)`. The 20
misses are `X ⊑ InterestingPizza` for pizzas `X` whose own axioms force
≥3 pairwise-disjoint `hasTopping` successors (e.g. American: Mozzarella
⊓ Tomato ⊓ Pepperoni, pairwise disjoint topping kinds).

Proof via the H3b `¬sup` refutation: `¬InterestingPizza = ¬Pizza ⊔
≤2 hasTopping`. The `¬Pizza` disjunct clashes (`X ⊑ Pizza`). The
`≤2 hasTopping` disjunct asserts an at-most-2 constraint on the root,
which has 3 `hasTopping`-successors; satisfying `≤2` requires merging
two of them; every pairwise merge unions two disjoint topping classes
→ clash; all merges clash ⇒ `≤2` unsatisfiable ⇒ subsumption holds.

So the mechanism is **`≤n` merge + disjointness clash**. No `≥n`
generation, no qualified `≤n.C`, no inequality tracking is needed for
these misses.

## §1 — `≤n` is an attached node constraint, not a clause

Clauses are fixed; `≤n` is a *fact at a node*. Add to `HyperNode`:

```rust
at_most: Vec<(Role, Option<ClassId>, u32)>   // (role, qualifier, bound)
```

`Option<ClassId>` qualifier is `None` for unqualified `≤n R` (the pizza
case). Asserted via a new head-atom kind on the `¬sup` side:

```rust
Atom::AtMost(Role, Option<ClassId>, u32, Var)
```

`encode_neg_disjunct` (H3b §3) gains a case: `¬(≥n R.C)` (NNF
`≤(n−1) R.C`) → `Atom::AtMost(R, C-or-None, n−1, X)`. `apply_head_atom`
for `AtMost` pushes onto the target node's `at_most` list (and emits an
event so the constraint is checked — see §3).

## §2 — Merge via a representative (union-find), lazy resolve

Merging `s_j` into `s_i` must not invalidate the event worklist (which
holds node ids). Standard fix: a union-find `representative: Vec<HNode>`
(`representative[s_j] = s_i`). Every event/edge dereferences through
`resolve(n)` on pop. `s_j` becomes unreachable; `s_i` is canonical.

Merge mechanics (all through event-emitting engine methods — never
write `nodes[..].labels` directly, or firings are missed):
- `representative[s_j.index()] = s_i`.
- for each label `c` on `s_j` not on `s_i`: `add_label(s_i, c)` (emits
  `Label(s_i, c)`, which wakes `s_i`'s — now also `s_j`'s — predecessors
  for back-prop, and fires `s_i`'s own clauses incl. its `at_most`).
- redirect `s_j`'s edges and preds onto `s_i` (append; emit `Edge`
  events for the moved out-edges).
- a disjointness clause `A ⊓ B → ⊥` firing on the unioned labels yields
  the clash that rejects an incompatible merge.

## §3 — Merge is a *branching* choice (lives in `solve`, not the drain)

A violated `≤n` is to `solve` what an open disjunctive clause is now.
After `horn_fixpoint`, `find_open_at_most` scans nodes: node `n` with
`at_most(R, q, k)` and more than `k` distinct `R`-successors (resolved
by representative, filtered by qualifier `q`) is *open*. Branching
enumerates **which pair to merge** (the `C(k,2)` choices); each branch
performs the merge over a saved graph, recurses; all-branches-clash ⇒
`Unsat`. Same save/restore shape as disjunctive-head branching.

**Save/restore must also clone `representative`** (alongside `nodes`).
Forgetting this corrupts the next pair's state — the canary is a false
positive or a wrong pizza miss-count.

## §4 — Scope cuts for the first phase (stated bluntly)

To dodge the hardest interactions and still clear the 20 misses:

- **Unqualified `≤n` only** (`None` qualifier). Qualified `≤n R.C`
  matching is deferred.
- **No `≥n` head generation.** The misses' successors come from `X`'s
  own `∃`. `≥n` (and its ≤-before-≥ rule-ordering termination story) is
  a later phase.
- **Merge only at the root binding** (the node `¬sup` was asserted on).
  No successor-side merges. This sidesteps the blocking-after-merge
  interaction (§5) entirely, since the root has no ancestors.
- **No inequality / `distinct` tracking.** Merges either clash on
  disjointness or succeed; no "must stay distinct" assertions arise in
  this cut.

A deeper `≤n` (merges on generated successors) will *not* work after
this phase — that's intentional; note it so future-you doesn't wonder.

## §5 — Known interaction deferred by the scope cut: blocking

Anywhere blocking (`n` blocked by an earlier `m` with `L(n) ⊆ L(m)`)
becomes subtle once merges change label sets and "earlier/ancestor" is
no longer a fixed identity. Merging only at the root (§4) avoids it for
now. When successor merges are added, blocking must be re-evaluated on
merge (recompute, or invalidate blocked-ness when an ancestor gains a
label). Out of scope for phase 1; flagged so it isn't forgotten.

## §6 — Validation

- **Pizza:** misses **24 → 4** (the 20 `InterestingPizza` unlocked),
  subsumptions 671 → 691, **0 false positives**. If the drop isn't 20,
  diagnose — don't paper over (the H3a/H3b discipline).
- **SIO:** 1585 sat / 0 unsat unchanged (no cardinality there to
  perturb; a regression would mean the `AtMost` path mis-fires).
- **Correctness self-check** as always: any merge bug that over- or
  under-derives shows up instantly as changed pizza miss count or false
  positives.
- New unit tests: `≤1 R` with two disjoint successors ⇒ Unsat;
  `≤2 R` with two successors ⇒ Sat (no merge needed); the three-disjoint
  `≤2` pizza shape ⇒ Unsat; save/restore across an at_most branch.

## §7 — Out of scope (later phases)

Qualified `≤n R.C`; `≥n` generation + ≤-before-≥ termination;
successor-side merges + blocking re-evaluation (§5); inequality
tracking; nominals (the other 4 pizza misses). After phase 1 the pizza
hierarchy is **≥99.4 %** of Konclude's closure (4 nominal misses left).
