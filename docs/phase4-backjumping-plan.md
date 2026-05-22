# Phase 4: dependency-directed back-jumping — implementation plan

Author: Claude (drafted during 2026-05-22 session). Lives in `docs/`
so it survives the conversation and the next session can pick it up.

## Why

`classify_internal_with_timeout` and every single tableau probe on
SROIQ-heavy ontologies (SULO, SIO) is bottlenecked by the search
loop's chronological backtracking. When the `⊔`-rule's `branch()`
fails on disjunct `d_i` due to a clash that was actually caused by
some ancestor branch decision (not by picking `d_i`), the current
code dutifully tries `d_{i+1}`, `d_{i+2}`, etc. — all of which will
clash for the same upstream reason. Dependency-directed back-jumping
(DDB) recognises that the clash didn't depend on the current branch
and propagates the failure straight up.

Combined with semantic branching (Phase 4 attempt that regressed
without DDB — see strategy memory), this is the unlock for `classify`
on real ontologies above ~50 classes.

## The contract

Every label and edge gets a **dependency set** (`DepSet`): the set
of `branch_id`s whose decisions were necessary to derive it. A clash
between complementary labels has `clash_deps = deps(L1) ∪ deps(L2)`.
`branch()` jumps back to the most recent `branch_id` in `clash_deps`,
skipping any branches whose `branch_id` isn't in there.

Soundness invariant: if a label `c` is in `L(node)` under some
combination of branch decisions `S`, then `deps(c) ⊆ S`. (We may
over-approximate `deps(c)` — that just means we back-jump less
aggressively, never wrongly.)

## Data structures

```rust
type DepSet = SmallVec<[u32; 4]>; // sorted, dedup'd

// owl-dl-tableau/src/graph.rs — Node
pub struct Node {
    // ... existing ...
    labels: SmallVec<[ConceptId; 8]>,
    label_deps: SmallVec<[DepSet; 8]>,   // NEW — parallel to labels
    edges: SmallVec<[(RoleId, NodeId); 4]>,
    edge_deps: SmallVec<[DepSet; 4]>,    // NEW — parallel to edges
    in_edges: SmallVec<[(RoleId, NodeId); 2]>,
    in_edge_deps: SmallVec<[DepSet; 2]>, // NEW — parallel to in_edges
}

// owl-dl-tableau/src/lib.rs — TableauContext
pub struct TableauContext<'pool, 'tbox, 'hier> {
    // ... existing ...
    /// Stack of branch_ids currently active (from outer to inner).
    /// Pushed by branch(), popped on return.
    active_branches: Vec<u32>,
    /// Next branch_id to hand out.
    next_branch_id: u32,
}
```

## Mutation API changes

```rust
impl TableauContext {
    /// Pure-fact rule additions (residual GCIs, told subsumptions
    /// in concept_rules etc.) that *don't* depend on any branch
    /// decision. Equivalent to `add_label_with_deps(node, c, [])`.
    pub fn add_label(&mut self, node: NodeId, c: ConceptId) -> bool;

    /// General rule additions: deps come from the antecedent label(s)
    /// or edge(s) that triggered this conclusion. The rule is
    /// responsible for unioning antecedent deps.
    pub fn add_label_with_deps(&mut self, node: NodeId, c: ConceptId,
                                deps: &DepSet) -> bool;

    /// Edge counterparts.
    pub fn add_edge(&mut self, from: NodeId, role: RoleId,
                     target: NodeId);
    pub fn add_edge_with_deps(&mut self, from: NodeId, role: RoleId,
                               target: NodeId, deps: &DepSet);

    /// Read deps of an existing label / edge.
    pub fn label_deps(&self, node: NodeId, c: ConceptId) -> Option<&DepSet>;
    pub fn edge_deps(&self, from: NodeId, role: RoleId,
                      target: NodeId) -> Option<&DepSet>;
}
```

If the label is already present, the existing semantics ("return
false, no-op") stays. We do **not** widen the existing deps on a
duplicate add — that would invalidate the soundness invariant on
rollback (the trail entry recorded the original deps, not the
widened set). Future refinement: track the union explicitly with a
trail entry that restores the prior dep set on undo.

## Per-rule dep propagation

Each rule in `crates/owl-dl-tableau/src/rules.rs` needs to compute
conclusion deps from antecedent deps. Listed by rule:

| Rule | Conclusion deps |
|------|-----------------|
| `apply_and` | `label_deps(node, And-expr)` |
| `apply_forall` | `label_deps(node, All-expr) ∪ edge_deps(node, edge)` |
| `apply_concept_rules` | `label_deps(node, trigger-atomic)` |
| `apply_nominal_rules` | `label_deps(node, nominal-literal)` |
| `apply_role_rules` (unguarded) | `edge_deps(node, edge)` |
| `apply_role_rules` (guarded) | `label_deps(node, guard) ∪ edge_deps(node, edge)` |
| `apply_residual_gcis` | `[]` — applies universally |
| `apply_exists` | `label_deps(node, exists-expr)` — new node's seed |
| `apply_min` | same as `apply_exists` for each generated witness |
| `apply_max` (merge) | union of merged labels' deps |
| `apply_role_chains` | `edge_deps(head) ∪ edge_deps(tail)` |
| `apply_self_restriction` | `label_deps(node, self-restriction-expr)` |
| `apply_role_axioms` (disjoint pair clash) | union of two edge_deps |

Edge deps on `new_successor` / `new_predecessor`: deps of the
triggering ∃-label.

`apply_max`'s choose branching (Q2 `≤n R.C` case) calls `branch`
with the implicit pair `(C, ¬C)` — both options get a fresh
`branch_id`, just like `⊔` branching.

## Clash detection

`first_clash(ctx)` becomes `first_clash_with_deps(ctx) ->
Option<(NodeId, DepSet)>`:

- Scan nodes for complementary label pairs `(c, ¬c)` or for `Bot`
  labels.
- Return `(node, label_deps(c) ∪ label_deps(¬c))`.

`SaturationResult::Clash(NodeId)` becomes `Clash(NodeId, DepSet)`.

## Search return type

```rust
pub enum SearchVerdict {
    Sat,
    Unsat(DepSet),         // clash deps
    DepthLimit,
}
```

`search()` and `branch()` return `SearchVerdict`. The reasoner-side
mapping is: `Sat → Ok(Some(true))`, `Unsat(_) → Ok(Some(false))`,
`DepthLimit → Ok(None)` (cooperative-deadline case) or
`Err(ReasonError::NoVerdict)`.

## branch() with back-jumping

```rust
fn branch(ctx, max_depth, node, options: &[ConceptId]) -> SearchVerdict {
    let my_id = ctx.next_branch_id;
    ctx.next_branch_id += 1;

    let mut combined_deps: DepSet = SmallVec::new();
    let mut depth_limited = false;

    ctx.active_branches.push(my_id);
    for d in options {
        let cp = ctx.checkpoint();
        ctx.add_label_with_deps(node, *d, &smallvec![my_id]);
        match search(ctx, max_depth - 1) {
            SearchVerdict::Sat => {
                ctx.active_branches.pop();
                return SearchVerdict::Sat;
            }
            SearchVerdict::Unsat(clash_deps) => {
                ctx.rollback_to(cp);
                if !clash_deps.binary_search(&my_id).is_ok() {
                    // This branch decision didn't contribute. Skip
                    // every remaining option — they'd hit the same
                    // upstream clash for the same deps.
                    ctx.active_branches.pop();
                    return SearchVerdict::Unsat(clash_deps);
                }
                // Decision mattered. Accumulate clash deps minus my_id
                // for the "all options failed" case.
                merge_deps_excluding(&mut combined_deps, &clash_deps, my_id);
            }
            SearchVerdict::DepthLimit => {
                ctx.rollback_to(cp);
                depth_limited = true;
            }
        }
    }
    ctx.active_branches.pop();

    if depth_limited {
        SearchVerdict::DepthLimit
    } else {
        // All options failed *and* each clash depended on my_id.
        // The disjunction itself is unsat under the ancestor deps.
        SearchVerdict::Unsat(combined_deps)
    }
}
```

`merge_deps_excluding(target, src, exclude)` does sorted merge,
skipping `exclude`. ~10 lines.

## Trail

`TrailEntry::LabelAdded` becomes:

```rust
LabelAdded { node: NodeId, concept: ConceptId, prior_deps: Option<DepSet> },
```

Where `prior_deps` is:
- `None` if this was a fresh insertion (rollback drops both label
  and `label_deps[pos]`).
- `Some(d)` if this was a duplicate-add that widened existing deps
  (rollback restores `label_deps[pos] = d`).

Same shape for `EdgeAdded`.

(For the *initial* implementation we can skip the widening case
entirely — duplicate adds don't update deps, just return `false`.
The `prior_deps: Option<DepSet>` field is then always `None` and
the `LabelAdded`/`EdgeAdded` undo path matches today's behaviour for
deps too. Widening is a follow-up optimisation.)

## Commits

1. **Infrastructure.** Add `label_deps` / `edge_deps` parallel
   `SmallVec`s on `Node`, `active_branches` + `next_branch_id` on
   `TableauContext`, the `with_deps` API methods, trail entry
   extension. Existing `add_label(node, c)` is a wrapper that passes
   empty deps. **No behaviour change.** Existing 220+ workspace
   tests stay green; this commit is purely additive.

2. **Per-rule dep propagation.** Update every `apply_*` rule per
   the table above. Each rule's behaviour is structurally
   unchanged — only the deps argument differs. Add unit tests that
   construct a `TableauContext` with hand-tagged label deps and
   assert the rule's conclusions get the expected deps.

3. **Clash detection deps.** Change `first_clash` and
   `SaturationResult` to carry the clash deps.

4. **Search return type.** Migrate `search()` and `branch()` to
   `SearchVerdict`. Reasoner facade adapts. No behavior change yet —
   `Unsat(_)` is treated like the current `Some(false)`.

5. **Back-jumping in branch().** Wire the deps check into the
   skip-remaining-options logic. **This is the commit that delivers
   speedup.** Benchmarks expected to move: SULO single-subclass
   query (from ~20 s timeout toward something finite), SIO classify
   (still bad but finite at higher class counts), corpus 86 should
   stay within noise.

6. **Semantic branching, take 3.** Now that DDB prunes subtrees,
   literal-only semantic branching should pay off. Bring back the
   restricted-complements variant from the dead-end and re-bench.

## Risks

- **Unsoundness from missed propagation.** Every `add_label` /
  `add_edge` site outside the rules (ABox seeding in `decide`,
  merge replay, etc.) needs deps too. Audit the call graph.
- **Performance regression from `SmallVec<DepSet>` bookkeeping.**
  Each node now stores 2-3 extra `SmallVec`s. Small-corpus impact
  measurable. Mitigation: tune SmallVec inline sizes; profile.
- **Trail size grows.** Every `LabelAdded` carries `prior_deps:
  Option<DepSet>`. Mostly `None`s but adds a few bytes per entry.
  Mitigation: defer the widening case to follow-up.

## What success looks like

After commit 5:

- SULO `is_subclass_of` on a tableau-required pair returns a
  verdict (sat or unsat) within ~1–2 s, down from ≥20 s.
- SIO consistency check returns within a few minutes (currently
  >60 s budget without finishing).
- Corpus 86 within ±10 % of the 34 ms baseline.

If any of those don't pan out, the plan needs revision — DDB might
not be the right lever, or per-rule propagation is incomplete. The
strategy memory's "Tried-and-dropped" section is the right place to
record the result.
