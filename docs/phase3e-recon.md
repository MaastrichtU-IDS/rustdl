# Phase 3e recon — `apply_role_rules` internals

Source: post-Phase-3d SIO flamegraph
(`docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg`, 17,890 total
samples) + code-trace of `crates/owl-dl-tableau/src/rules.rs:313-407`. HEAD:
`adb8a52` (`plan(phase3e): apply_role_rules inner-cost performance plan`).

## TL;DR

The 16% post-3c "inner cost" of `apply_role_rules` is **still real at 16.03%**
on post-3d SIO (no big redistribution from Phase 3d). The dominant inner
sub-path is **`edge_satisfies` at 7.26%** (1,298 / 17,890 samples) — called
twice per edge per rule from the `matching_edges` closure. Underneath
`edge_satisfies`, costs split cleanly between `is_sub_role` (3.77%,
same-polarity arm) and `are_declared_inverses` (3.11%, cross-polarity arm).

The proposed fix shape (T3) is loop-nesting inversion + role-keyed
prefiltering: iterate edges outer, then look up matching rules via the
precomputed `super_closure` slice on the edge's role. This eliminates the
per-(rule, edge) `edge_satisfies` call entirely on the same-polarity arm
and reduces the cross-polarity arm to a single hash lookup per edge.

## Flamegraph drill-down

All counts from the **2,867-sample parent frame** of `apply_role_rules`
(fg:x=14191, fg:w=2867) on the post-Phase-3d SIO flame. Smaller
disconnected `apply_role_rules` frames (1,178 / 479 / 61 samples) are
secondary call sites (saturator pre-pass and a few search paths); the
2,867 frame is the dominant one and the share quoted in the plan.

| Sub-path | Samples | Share | Code region |
|---|---|---|---|
| **`apply_role_rules` (parent)** | 2,867 | 16.03% | `rules.rs:313-407` |
| ├─ `{closure#1}` = `matching_edges` | 1,686 | 9.43% | `rules.rs:346-359` |
| │  ├─ `edge_satisfies` | 1,298 | 7.26% | `lib.rs:598-609` |
| │  │  ├─ `is_sub_role` (same polarity) | 674 | 3.77% | `role_hierarchy.rs:133-137` |
| │  │  └─ `are_declared_inverses` (cross polarity) | 557 | 3.11% | `lib.rs:498` |
| │  ├─ `push<(Role, NodeId, DepSet)>` | 115 | 0.64% | `rules.rs:350,355` |
| │  ├─ `next<slice::Iter<(RoleId, NodeId)>>` | 45 | 0.25% | edge iter |
| │  ├─ `new<DepSet>` (clone on push) | 47 | 0.26% | `rules.rs:350,355` |
| │  └─ closure self/leaf | 181 | 1.01% | branch fixups |
| ├─ `collect<…, HashMap<ClassId, DepSet>>` = `guards_present` build | 713 | 3.99% | `rules.rs:334-342` |
| │  ├─ hashbrown `extend` closure `{closure_env#0}` | 422 | 2.36% | hash insert |
| │  └─ `{closure_env#0}` (filter_map on labels) | 291 | 1.63% | `rules.rs:338-341` |
| ├─ `get<ClassId, Vec<RoleRule>>` on `guarded_role_rules_by_guard` | 241 | 1.35% | `rules.rs:367` |
| │  └─ `hash_one<…, &ClassId>` | 239 | 1.34% | hash computation |
| ├─ `add_label_with_deps` | 170 | 0.95% | `rules.rs:398` |
| ├─ `union` (guard_deps ⨃ edge_deps) | 37 | 0.21% | `rules.rs:370` |
| └─ `push<(NodeId, ConceptId, DepSet)>` into `out` | 13 | 0.07% | `rules.rs:363,371` |

Sum of direct children: 2,860 of 2,867 samples (parent self < 0.05%).

## Identified dominant inner cost

**Candidate B — `edge_satisfies` per-edge-per-rule cost.** It accounts for
7.26% of total classify time, which is 45% of the `apply_role_rules` parent
and 77% of the `matching_edges` closure alone. No other single sub-path is
close — the next biggest (`guards_present` HashMap build) is 3.99%, and
candidate A's *Vec allocation* portion (`push` 0.64% + `new` 0.26% +
`next` 0.25% = ~1.15%) is small enough that eliminating it via an iterator
adapter would only reclaim ~1pp.

The cost structure is `(|unguarded_rules| + Σ_{g ∈ guards_present} |guarded[g]|)
× (|n.edges| + |n.in_edges|)` `edge_satisfies` calls per `apply_role_rules`
invocation. Both arms of `edge_satisfies` are already efficient *per call*
(`is_sub_role` is a `binary_search` on a precomputed closure;
`are_declared_inverses` was Phase 3b'd to O(1) HashSet) — the cost is the
**call count**, not the per-call work. Reducing the number of calls
dominates everything else.

## Code-trace evidence

Function under study: `crates/owl-dl-tableau/src/rules.rs:313-407`.

Inner loop structure (lines 346-359, the `matching_edges` closure):

```rust
let matching_edges = |rule_role: Role| {
    let mut triples: Vec<(Role, NodeId, DepSet)> = Vec::new();
    for (pos, &(role, neighbour)) in n.edges.iter().enumerate() {
        if ctx.edge_satisfies(Role::Named(role), rule_role) {       // <-- hot
            triples.push((Role::Named(role), neighbour, n.edge_deps[pos].clone()));
        }
    }
    for (pos, &(role, neighbour)) in n.in_edges.iter().enumerate() {
        if ctx.edge_satisfies(Role::Inverse(role), rule_role) {     // <-- hot
            triples.push((Role::Inverse(role), neighbour, n.in_edge_deps[pos].clone()));
        }
    }
    triples
};
```

Called once per rule at lines 362 (unguarded) and 369 (guarded). With R
unguarded rules and G guarded-rule applications, this yields R+G full
passes over `n.edges + n.in_edges`, each pass invoking `edge_satisfies`
on every edge.

`edge_satisfies` is `crates/owl-dl-tableau/src/lib.rs:598-609`:

```rust
pub fn edge_satisfies(&self, seen: Role, wanted: Role) -> bool {
    let s = seen.role_id();
    let w = wanted.role_id();
    if seen.is_inverse() == wanted.is_inverse() {
        match self.hierarchy {
            Some(h) => h.is_sub_role(s, w),
            None => s == w,
        }
    } else {
        self.are_declared_inverses(s, w)
    }
}
```

The same-polarity arm walks the precomputed `super_closure` slice via
`binary_search` (`crates/owl-dl-core/src/role_hierarchy.rs:133-137`); the
cross-polarity arm is an O(1) hash lookup post-3b. Both are cheap per
call; the cost is the multiplicative call count.

`unguarded_role_rules` and `guarded_role_rules_by_guard` are populated in
`crates/owl-dl-core/src/absorb.rs:129-139` (partition of `role_rules` by
`rule.guard.is_some()` produced by `finalize`). They are stored as a
`Vec<RoleRule>` and `HashMap<ClassId, Vec<RoleRule>>` respectively. Each
`RoleRule.role` is a `Role` (named or inverse). The cardinality on SIO
is not directly visible from the flame but inferable: the
`guarded_role_rules_by_guard.get` cost (1.35%) being roughly proportional
to the number of guard classes present on node × number of
`apply_role_rules` invocations suggests dozens to hundreds of distinct
guards per call site over the run, with `unguarded_role_rules` non-empty
(otherwise the unguarded loop would have zero share and we'd see all
`edge_satisfies` cost under the guarded loop's `matching_edges`).

`guards_present` is rebuilt every invocation (`rules.rs:334-342`,
`HashMap::from_iter` via `.collect()`), but the per-call build cost
(3.99%) is roughly half the `edge_satisfies` cost, so even a perfect
fix would yield less.

## Discarded candidates

- **Candidate A (Vec allocation in `matching_edges`)** — only ~1.15% in
  push/new/next combined. Worth fixing as a secondary cleanup but not
  Phase 3e's primary target. Note: an iterator rewrite is best done
  alongside the loop-inversion fix from candidate B anyway, since both
  touch the same closure.
- **Candidate C (`guards_present` HashMap build, 3.99%)** — second
  biggest but half of candidate B. A small-N optimization (SmallVec or
  Vec<(ClassId, DepSet)>) might shave 1-2pp; caching across invocations
  is harder (labels change between calls). Defer to Phase 3f.
- **Candidate D (HashMap.get on `guarded_role_rules_by_guard`, 1.35%)** —
  small. Already O(1) hash; `hash_one` on `ClassId` (a `u32` wrapper) is
  near-optimal.
- **Candidate E (`add_label_with_deps`, 0.95%)** — already tiny; batching
  it would force a larger surface change for less than 1pp.

## Proposed fix shape (handoff to T3)

**Invert the loop nesting in the index path (lines 360-375) and use the
role hierarchy's precomputed `super_closure` to drive rule lookup.** In
T3 we will:

1. Add two role-keyed indices to `AbsorbedTBox` (or build them lazily in
   `apply_role_rules`): `unguarded_rules_by_role_id: HashMap<RoleId,
   Vec<RoleRuleIdx>>` keyed by `rule.role.role_id()` and partitioned by
   `rule.role.is_inverse()`; same for guarded rules.
2. Replace the `matching_edges` closure with the inverted loop: for each
   edge `(role, neighbour)` in `n.edges` and `n.in_edges`, walk
   `hierarchy.super_roles(role)` once to collect all `rule_role`s the
   edge satisfies (same polarity), then look up rules for each
   `rule_role` via the keyed index. For the cross-polarity arm
   (`are_declared_inverses`), do the symmetric lookup using inverse
   role declarations.
3. Each edge now does O(|super_closure[role]|) work for same-polarity
   lookup (a single slice walk, no `is_sub_role` calls) and O(1) hash
   lookups for cross-polarity, replacing R + G `edge_satisfies` calls.

Soundness: the change preserves the exact set of fired (rule, edge)
pairs because the role-keyed lookup enumerates the same set
`edge_satisfies` would have accepted (sub_roles closure is the inverse
of `is_sub_role`'s decision; `are_declared_inverses` is symmetric).
DepSet propagation is unchanged — same guard-deps union, same edge-deps
clone.

Predicted impact: the 7.26% `edge_satisfies` line should collapse to
<1% (mostly residual cross-polarity), and the `matching_edges` closure
should drop from 9.43% to ~3-4%. Total `apply_role_rules` share
estimated to fall from 16.03% to ~8-10%. GALEN wall reduction in the
3-8% range is plausible (calibrated against Phase 3b, which removed a
similar-magnitude `are_declared_inverses` linear scan).

T3 will produce `docs/phase3e-fix-target.md` with the exact code shape.

---

Confidence: **high** that a single surgical fix in the matching_edges
closure body (plus an `AbsorbedTBox` index field built at `finalize`)
delivers the predicted impact. No multi-fix split needed.
