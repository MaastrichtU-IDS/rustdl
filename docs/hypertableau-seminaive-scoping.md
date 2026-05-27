# Hypertableau — semi-naive Horn evaluation (scoping)

Drafted 2026-05-27. The clause-indexing increment cut SIO bare-sat
23.6 s → 6.0 s by trying only trigger-present clauses per node. The
profile of what remains is unambiguous:

| SIO bare-sat (after indexing) | |
|---|---|
| wall | ~6.6 s |
| match_attempts | 52 M |
| fixpoint_passes | 27 636 |
| node_clones | 2 153 |

A cheap-allocation experiment (a `match_body` fast path for role-free
bodies) produced **no wall change** — confirming the cost is the
*count* of match attempts, not per-call cost. The 52 M comes from the
pass loop re-attempting every trigger-present clause at every node on
**every** pass (~17 passes/class), even when nothing relevant changed
since the last pass. Semi-naive evaluation fires a clause only when a
body atom is *newly* derived, turning ~17 re-scans into ~1 drain.

## The hard part — back-propagation needs predecessors

Horn clauses split into two shapes:

1. **Local** (`A(x) ∧ B(x) → C(x)`, disjointness `A∧B→⊥`): body on
   `X` only. Fire when `x` gains a trigger label. The bulk of the 52 M.
2. **Role / back-prop** (`R(x,y) ∧ E(y) → F(x)`, domain/range, the
   `∀`-in-body clauses): body reads a role edge and/or a *successor*
   label. Fire when an edge `x→y` is added **or** when a successor `y`
   gains the relevant class. The second trigger is the crux: derive
   `F(x)` from `E(y)` means a change at `y` must wake `x` — its
   *predecessor*. The graph currently stores only forward edges.

So semi-naive is not just "index by `X`-trigger" (we have that). It
needs to route three event kinds to the right clauses.

## §1 — Events and the worklist

A worklist of derivation events, drained to fixpoint:

- `LabelAdded(node, class)` — `node` gained `class`.
- `EdgeAdded(src, role, tgt)` — a new role edge.

`horn_fixpoint` becomes: seed the worklist from the current graph
(every present label as a `LabelAdded`, every edge as an `EdgeAdded`),
then drain. Draining an event fires exactly the clauses that event can
newly enable; firings push new events. Terminates when the worklist
empties (a clash short-circuits to `Unsat`, as today).

## §2 — Three trigger indexes (built once per clause set)

- **`x_trigger[class] → [clause]`** (have it: `trigger_index`): local
  clauses whose representative `X`-class is `class`. Fired on
  `LabelAdded(n, class)` at `n`.
- **`succ_trigger[class] → [clause]`** (new): clauses with `class` as a
  class-on-successor atom `B(y)`. Fired on `LabelAdded(m, class)` at
  each **predecessor** of `m`.
- **`role_trigger[role] → [clause]`** (new): clauses with a body role
  atom on `R`. Fired on `EdgeAdded(n, R, _)` at `n`.

A clause may sit in several indexes; `match_body` re-verifies the full
body on every fire, so over-triggering is sound (just wasted work).

## §3 — Predecessor tracking

Add reverse edges so `LabelAdded(m, …)` can reach `m`'s predecessors.
Either a per-node `preds: Vec<(Role, HNode)>` maintained alongside
`edges`, or a global `Vec<Vec<HNode>>`. Per-node is simplest and the
graphs are small. Maintained in `fire_exists`/edge creation.

## §4 — Interaction with branching (the save/restore fork)

`solve` clones `self.nodes` per branch and restores on failure. The
worklist and any per-node "processed" state would also need
save/restore — **or** each `solve`/`horn_fixpoint` call re-seeds the
worklist from scratch from the current graph. **Decision: re-seed per
call.** Branching is rare (2 153 clones on SIO vs 52 M attempts), so
re-processing a branch's graph from scratch costs little, and it keeps
the worklist out of the cloned state entirely (the clone stays just
`nodes`). The within-class Horn saturation — where the 52 M lives — is
a single non-branching drain, which is the whole win.

## Result (shipped)

A first attempt at **node**-granularity (a dirty-node worklist reusing
`fire_clauses_at`) was **refuted by measurement**: it re-fired *all* of
a node's trigger-present clauses each time the node was re-dirtied, and
a class node gains many labels, so SIO came out at 52 M → 57 M attempts
(a slight regression). That confirmed the cost is the re-fire *count*,
which only **event** granularity prunes — firing exactly the clauses
the newly-derived label/edge enables, per this doc's §1–§3 design.

The event model delivered:

| SIO bare-sat | indexed (prior) | **event model** |
|---|---|---|
| wall | 6.6 s | **0.45 s** (15×) |
| match_attempts | 52 M | **2.0 M** (26×) |
| answers | 1585 sat / 0 unsat | identical |

Pizza unchanged (671 subsumptions, 24 misses, **0 false positives**).
Cumulative on SIO since H2b: **16.3 s → 0.45 s (~36×)**. The Konclude
gap closed from ~116× to **~3.2×** (0.45 s vs 0.14 s).

## §5 — Validation

- **Correctness is self-checking**: pizza must stay **671 subsumptions
  / 24 misses / 0 false positives** and SIO **1585 sat / 0 unsat**. Any
  semi-naive bug that drops a firing shows up immediately as more pizza
  misses. (This is why the rewrite is lower-risk than its complexity
  suggests — the harness catches incompleteness instantly.)
- **Win metric**: `match_attempts` should fall well below 52 M and
  `fixpoint_passes` (re-interpreted as worklist drains) collapse toward
  one per `horn_fixpoint` call. Target: another multiple-× on SIO wall,
  narrowing the Konclude gap (~43× today) toward single digits.

## §6 — Out of scope / risks

- **Don't** save/restore the worklist (re-seed per call, §4).
- **Don't** make the worklist itself clever (priority, ordering) — that
  is heuristics work, a separate increment, only after this lands.
- **Risk**: the `succ_trigger` + predecessor path is where an
  incompleteness bug would hide (a successor change that fails to wake
  the right predecessor). The pizza back-prop subsumptions
  (`∃R.E ⊑ F` shapes, already passing) are the regression guard.

After this, the remaining search-quality items are small
(`find_open_disjunction` indexing), and **H4** — flipping the engine on
behind `--hypertableau` — becomes the realistic next milestone.
