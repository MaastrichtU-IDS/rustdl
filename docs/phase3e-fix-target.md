# Phase 3e — fix target

Per Phase 3e recon (`docs/phase3e-recon.md`), the dominant inner cost
of `apply_role_rules` is `edge_satisfies` invoked per-(rule, edge)
from the `matching_edges` closure: 7.26 % of total SIO classify time
(1,298 / 17,890 samples on the post-Phase-3d flame). That cost
decomposes into 3.77 % same-polarity (`is_sub_role`) + 3.11 %
cross-polarity (`are_declared_inverses`); both arms are cheap per
call (binary-search and O(1) hash-lookup respectively), so the
dominant lever is the **call count**, which is multiplicative in
(unguarded rules + Σ guarded rules) × (|n.edges| + |n.in_edges|).

This doc specifies the surgical fix: invert the loop nesting and
push the rule lookup behind two role-keyed indices built once at
`AbsorbedTBox::finalize()`. Eliminates the per-(rule, edge)
`edge_satisfies` call entirely.

## Target code

### Call-site under study

`crates/owl-dl-tableau/src/rules.rs:313-407` — the entire
`apply_role_rules` function. The hot core is the `matching_edges`
closure (lines 346-359) and its consumers (lines 360-375). Current
shape:

```rust
let matching_edges = |rule_role: Role| {
    let mut triples: Vec<(Role, NodeId, DepSet)> = Vec::new();
    for (pos, &(role, neighbour)) in n.edges.iter().enumerate() {
        if ctx.edge_satisfies(Role::Named(role), rule_role) {
            triples.push((Role::Named(role), neighbour, n.edge_deps[pos].clone()));
        }
    }
    for (pos, &(role, neighbour)) in n.in_edges.iter().enumerate() {
        if ctx.edge_satisfies(Role::Inverse(role), rule_role) {
            triples.push((Role::Inverse(role), neighbour, n.in_edge_deps[pos].clone()));
        }
    }
    triples
};
if use_index {
    for rule in &tbox.unguarded_role_rules {
        for (_, neighbour, edge_deps) in matching_edges(rule.role) {
            out.push((neighbour, rule.target_label, edge_deps));
        }
    }
    for (g, guard_deps) in &guards_present {
        if let Some(rules) = tbox.guarded_role_rules_by_guard.get(g) {
            for rule in rules {
                for (_, neighbour, edge_deps) in matching_edges(rule.role) {
                    let combined = union(guard_deps, &edge_deps);
                    out.push((neighbour, rule.target_label, combined));
                }
            }
        }
    }
} else { /* pre-finalize linear-scan fallback; retained verbatim */ }
```

### Supporting primitives

`edge_satisfies` — `crates/owl-dl-tableau/src/lib.rs:598-609`:

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

Three logical match cases for a `seen` edge polarity-tagged role and
a `wanted` rule role:
1. **Same polarity, hierarchy attached**: `is_sub_role(seen, wanted)`
   ≡ `wanted ∈ super_roles(seen)`.
2. **Same polarity, no hierarchy**: `seen == wanted` (RoleId equality).
3. **Cross polarity**: `are_declared_inverses(seen, wanted)` — O(1)
   HashSet lookup in `TableauContext::inverse_pairs_set`.

There is no fourth dispatch path (e.g. role-chain / transitivity /
reflexive role match) inside `edge_satisfies` itself. Transitive
roles are handled by *other* rules (`apply_role_chains`,
`apply_universal_via_transitive` etc.) that fold the chain closure
into the edge label *before* `apply_role_rules` reads it — they
mutate `n.edges` / `n.in_edges`, then `apply_role_rules` consults
the already-completed edge set. The recon's two-arm cost breakdown
is therefore exhaustive.

`super_roles` — `crates/owl-dl-core/src/role_hierarchy.rs:115-117`:
sorted-ascending slice of `RoleId`s such that `r ⊑ s`, reflexive
(always contains `r` itself).

`AbsorbedTBox` — `crates/owl-dl-core/src/absorb.rs:60-142`. Already
holds:
- `role_rules: Vec<RoleRule>` (canonical).
- `unguarded_role_rules: Vec<RoleRule>` (partition by `guard.is_none()`).
- `guarded_role_rules_by_guard: HashMap<ClassId, Vec<RoleRule>>`
  (partition by guard).

`RoleRule { role: Role, guard: Option<ClassId>, target_label: ConceptId }`
— `absorb.rs:156-165`. `Role` is the two-variant enum
`Named(RoleId) | Inverse(RoleId)` (`ir.rs:75-79`).

## Fix shape

### 1. New `AbsorbedTBox` fields

Two role-keyed indices, populated at `finalize()` time. Keying on
the **named `RoleId`** (the underlying property), not the `Role`
enum, because `edge_satisfies` always reduces both sides to
`role_id()` before its polarity check. Polarity is encoded in
*which* index a rule goes into (named-edge index vs inverse-edge
index), so the per-edge dispatch is a flat HashMap lookup with no
inner polarity branch.

```rust
// Indexed by the actual edge's RoleId at lookup time. Each entry
// lists the indices (into the canonical Vec) of the role rules
// that fire when an outgoing edge with this RoleId is encountered.
// Populated for BOTH same-polarity matches (rule.role is Named(r')
// with r ⊑ r') AND cross-polarity matches (rule.role is
// Inverse(s) with (r, s) ∈ inverse_pairs_set).
pub unguarded_role_rules_by_named_edge:  HashMap<RoleId, Vec<u32>>,
pub unguarded_role_rules_by_inverse_edge: HashMap<RoleId, Vec<u32>>,

// Guarded variant. Index is rule's position inside the inner Vec
// of `guarded_role_rules_by_guard[guard]` — but it is *easier* to
// flatten guarded rules into a single `Vec<GuardedRoleRule>` at
// finalize-time and key by index into that flat vec; or, simplest,
// store the (guard, target_label, role) triple directly in the
// index entries to avoid the indirection. T4 picks the cleanest
// concrete form. Recommended:
pub guarded_role_rules_by_named_edge:   HashMap<RoleId, Vec<GuardedRuleEntry>>,
pub guarded_role_rules_by_inverse_edge: HashMap<RoleId, Vec<GuardedRuleEntry>>,

#[derive(Copy, Clone, Debug)]
pub struct GuardedRuleEntry {
    pub guard: ClassId,
    pub target_label: ConceptId,
}
```

(The unguarded index could be a `Vec<UnguardedRuleEntry>` carrying
just `target_label`, since the rule's `role` is implicit in the
index key and the `guard` slot is `None` by construction. T4
chooses the concrete shape; the semantic content is what's
load-bearing here.)

### 2. `finalize()` population

After `unguarded_role_rules` and `guarded_role_rules_by_guard` are
populated (`absorb.rs:129-140`), append a third pass to build the
role-keyed indices. The pass walks each rule once and expands its
`Role` through the role hierarchy:

```rust
// Same-polarity expansion: an edge with RoleId r satisfies a
// rule.role of `Named(r')` iff r' ∈ super_roles(r). To make the
// lookup O(1) per edge, invert the relation at finalize() time:
// for each rule with role Named(r'), index it under EVERY r in
// sub_roles(r'). (Equivalent to "for every edge RoleId r, list
// rules whose r' is in super_roles(r)".) Same shape for inverse.
//
// Cross-polarity: a Named-edge with RoleId r satisfies a rule
// with role Inverse(s) iff `are_declared_inverses(r, s)`. For
// every declared inverse pair (a, b) in inverse_pairs_set, a rule
// with role Inverse(b) is reached by a Named-edge of RoleId a
// (and vice-versa). Combined with the hierarchy: a Named-edge
// of RoleId r reaches a rule with role Inverse(s) iff there
// exists r' such that r ⊑ r' AND (r', s) ∈ inverse_pairs_set —
// but inverse_pairs are declared on the underlying RoleId, NOT
// folded through the hierarchy. Verify by reading edge_satisfies:
// it calls are_declared_inverses(seen_id, wanted_id) directly on
// the role_id of each side, with no super_roles walk in between.
// So the cross-polarity expansion in finalize() must mirror
// that: just inverse_pairs_set membership; no hierarchy walk.
//
// (See `Cross-polarity scope` below for the soundness rationale.)

// Concrete pass (pseudocode; T4 adapts to the actual RoleHierarchy
// passing convention — apply_role_rules currently reads it off
// `ctx.hierarchy` / `tbox.role_hierarchy`):
for (idx, rule) in self.unguarded_role_rules.iter().enumerate() {
    // Same polarity: index under every sub-role of rule.role.role_id()
    let sub_ids = hierarchy
        .map(|h| h.sub_roles(rule.role.role_id()))
        .unwrap_or(&[rule.role.role_id()]);
    let (same_polarity_index, cross_polarity_index) = if rule.role.is_inverse() {
        (
            &mut self.unguarded_role_rules_by_inverse_edge,
            &mut self.unguarded_role_rules_by_named_edge,
        )
    } else {
        (
            &mut self.unguarded_role_rules_by_named_edge,
            &mut self.unguarded_role_rules_by_inverse_edge,
        )
    };
    for &sub_id in sub_ids {
        same_polarity_index.entry(sub_id).or_default().push(idx as u32);
    }
    // Cross polarity: for every (a, b) where b == rule.role.role_id()
    // AND (a, b) ∈ inverse_pairs_set, an edge of RoleId a in the
    // OPPOSITE polarity from rule.role fires the rule.
    for &(a, b) in inverse_pairs {
        if b == rule.role.role_id() {
            cross_polarity_index.entry(a).or_default().push(idx as u32);
        }
    }
}
// Symmetric loop for guarded_role_rules_by_guard, populating
// guarded_role_rules_by_{named, inverse}_edge with GuardedRuleEntry.
```

A few engineering notes for T4:

- The role hierarchy lives on `AbsorbedTBox` (verify; if not,
  `finalize()` will need it passed in — the recon mentions it is
  attached to the tableau context, but the role-hierarchy slice
  required by Phase 3e is the *terminological* one, available at
  finalize time).
- `inverse_pairs` is on `TableauContext`, not `AbsorbedTBox`.
  Decide at T4 whether to (a) move/copy the inverse-pair list into
  `AbsorbedTBox` so `finalize()` can build the cross-polarity
  index, or (b) build the cross-polarity index in a separate
  init step that runs at tableau construction time. Option (a)
  is cleaner; option (b) defers the choice. T4 picks.
- The indices store rule indices, not rule references, to avoid
  redundant Copy of `RoleRule` (already `Copy`, so either is
  cheap; pick whichever keeps the call site clean).

### 3. Restructured `apply_role_rules` body

Replace lines 343-394 (the `let mut out = Vec::new(); …` block
including the `matching_edges` closure and both unguarded/guarded
consumer loops) with the edges-outer shape below. The
`use_index` outer gate is retained (the pre-finalize fallback at
lines 376-393 is kept verbatim).

```rust
let mut out: Vec<(NodeId, ConceptId, DepSet)> = Vec::new();
if use_index {
    // Outgoing edges: a Named-polarity edge with RoleId r.
    for (pos, &(role, neighbour)) in n.edges.iter().enumerate() {
        let edge_deps = &n.edge_deps[pos];
        if let Some(rule_idxs) = tbox.unguarded_role_rules_by_named_edge.get(&role) {
            for &idx in rule_idxs {
                let rule = &tbox.unguarded_role_rules[idx as usize];
                out.push((neighbour, rule.target_label, edge_deps.clone()));
            }
        }
        if let Some(entries) = tbox.guarded_role_rules_by_named_edge.get(&role) {
            for entry in entries {
                let Some(guard_deps) = guards_present.get(&entry.guard) else { continue; };
                let combined = union(guard_deps, edge_deps);
                out.push((neighbour, entry.target_label, combined));
            }
        }
    }
    // Incoming edges: an Inverse-polarity edge with RoleId r.
    for (pos, &(role, neighbour)) in n.in_edges.iter().enumerate() {
        let edge_deps = &n.in_edge_deps[pos];
        if let Some(rule_idxs) = tbox.unguarded_role_rules_by_inverse_edge.get(&role) {
            for &idx in rule_idxs {
                let rule = &tbox.unguarded_role_rules[idx as usize];
                out.push((neighbour, rule.target_label, edge_deps.clone()));
            }
        }
        if let Some(entries) = tbox.guarded_role_rules_by_inverse_edge.get(&role) {
            for entry in entries {
                let Some(guard_deps) = guards_present.get(&entry.guard) else { continue; };
                let combined = union(guard_deps, edge_deps);
                out.push((neighbour, entry.target_label, combined));
            }
        }
    }
} else {
    // Pre-finalize fallback — unchanged. Same body as today's
    // `else` arm at rules.rs:377-393, lifted verbatim.
}
```

Each edge now does **two HashMap.get's** (unguarded + guarded
indices on the matching polarity) plus a linear walk over a small
matched-rule slice. The per-(rule, edge) `edge_satisfies` calls
are gone entirely.

## Soundness invariant

The restructured loop must enumerate **exactly the same set** of
(rule, neighbour, deps) emissions as the pre-fix code. Two
equivalences to verify.

### Same-polarity equivalence (eliminates `is_sub_role` arm)

Old code: for each rule R with `R.role = Named(r')` (resp.
`Inverse(r')`), and each outgoing edge `(role=r, neighbour)`
(resp. each incoming edge), emit iff
`edge_satisfies(Named(r), Named(r'))` ≡ `is_sub_role(r, r')`
when a hierarchy is attached, or `r == r'` otherwise.

New code: at finalize time, for each rule R with
`R.role = Named(r')`, push `R`'s index into
`unguarded_role_rules_by_named_edge[r]` for every `r ∈ sub_roles(r')`.
At classify time, for each outgoing edge with RoleId `r`, walk
`unguarded_role_rules_by_named_edge[r]`.

Equivalence: rule R is emitted for outgoing edge `r` iff
`r ∈ sub_roles(r')` iff `r ⊑ r'` iff `is_sub_role(r, r')` iff
the old code emitted. ✓ (When no hierarchy is attached,
`sub_roles(r')` collapses to `{r'}` — equivalent to `r == r'`.)

### Cross-polarity equivalence (eliminates `are_declared_inverses` arm)

Old code: for each rule R with `R.role = Inverse(s)`, and each
*outgoing* edge `(role=r, neighbour)` (note polarity mismatch),
emit iff `edge_satisfies(Named(r), Inverse(s))` ≡
`are_declared_inverses(r, s)` ≡ `(r, s) ∈ inverse_pairs_set`.

New code: at finalize time, for each rule R with
`R.role = Inverse(s)`, scan `inverse_pairs` for pairs
`(a, b)` with `b == s` and push R into
`unguarded_role_rules_by_named_edge[a]`. (Symmetric for
`R.role = Named(r')` against incoming edges.) At classify time,
for each outgoing edge with RoleId `r`, the same index walk
picks up R.

Equivalence: R is emitted for outgoing edge `r` iff there exists
`(r, b)` in `inverse_pairs` with `b == s` iff
`(r, s) ∈ inverse_pairs_set` (since `inverse_pairs_set` is
populated symmetrically and from the same source — see
`crates/owl-dl-tableau/src/lib.rs:484-491` — both
`(r, s)` and `(s, r)` are inserted at declare time, so
membership coincides). ✓

Critically, the cross-polarity path does **NOT** fold through the
role hierarchy: `edge_satisfies` calls
`are_declared_inverses(seen.role_id(), wanted.role_id())`
directly, with no `is_sub_role` walk on either side. So the
finalize-time expansion must mirror that — for cross polarity, do
**not** expand `r'` to `sub_roles(r')`. This matches the recon's
3.11% attribution to cross-polarity (smaller than 3.77% same-
polarity, consistent with no hierarchy expansion).

### DepSet propagation

Unchanged. The pre-fix code clones the edge's `DepSet` for the
unguarded path (`edge_deps.clone()` at `rules.rs:350,355`) and
`union`s with `guard_deps` for the guarded path
(`rules.rs:370`). The new code does the same:
- Unguarded: emit `(neighbour, rule.target_label, edge_deps.clone())`.
- Guarded: look up `guard_deps = guards_present[entry.guard]`,
  emit `(neighbour, entry.target_label, union(guard_deps, edge_deps))`.

The `guards_present` map is built identically (lines 334-342;
unchanged by this phase). The guarded-path skip on missing guard
(`guards_present.get(...).is_none()` → continue) is identical to
the pre-fix behaviour — the pre-fix code only ever consults
`guarded_role_rules_by_guard[g]` for `g ∈ guards_present`; the
new code defers the check to the inner per-entry test
(`guards_present.get(&entry.guard)`), reaching the same skip
outcome because every entry in `guarded_role_rules_by_named_edge[r]`
came from some `RoleRule` whose guard *might or might not* be in
`guards_present` for this node.

### Emission-order equivalence

The pre-fix code emits in **rule-then-edge** order; the post-fix
code emits in **edge-then-rule** order. The downstream consumer
is `add_label_with_deps`, which is order-insensitive (it iterates
`pending`, calls `add_label_with_deps` per entry; the only
order-sensitive side effect is which DepSet "wins" if the same
`(target, c)` arrives twice from different rules — but
`add_label_with_deps` already handles label-already-present as a
no-op without comparing dependencies). T4 should add a structural
canary that exercises a node with two rules firing on the same
target to verify emission order doesn't affect verdicts.

### Pre-finalize fallback

The `use_index = false` branch (`tbox.unguarded_role_rules.is_empty()
&& tbox.guarded_role_rules_by_guard.is_empty()`) retains today's
linear scan unchanged. Hand-built TBoxes used in unit tests that
bypass `absorb::finalize()` keep working.

## What this design does NOT change

- **`guards_present` HashMap rebuild (3.99 %)** — the second-largest
  inner cost. Out of scope for Phase 3e; a future Phase 3f could
  cache it per-node or convert to a `SmallVec`/sorted Vec. The
  recon's discarded-candidate C.
- **`apply_max` (14.34 %)** — already Phase 3b'd on the inverse-roles
  path; remaining cost is the non-inverse arm.
- **`from_iter / collect` heap-alloc cluster (~6 %)** — orthogonal
  Phase 3f/3g target.
- **Role-hierarchy semantics** — `super_roles` / `sub_roles` /
  `is_sub_role` are read-only consumers in this design; no
  changes to `RoleHierarchy` or `RoleHierarchyBuilder`.
- **DepSet representation** — unchanged; same clone-on-push and
  `union` calls.
- **`add_label_with_deps`** — unchanged; receives the same
  `(target, c, deps)` triples in a possibly-different order.
- **Wedge / hypertableau / saturation engines** — unchanged;
  Phase 3e is a tableau-only refactor of one rule body.
- **Env-flag defaults** (`RUSTDL_HYPERTABLEAU*`, etc.) — unchanged.

## Predicted impact

- **SIO `apply_role_rules` flame frame**: 16.03 % → **~8-10 %**
  (≈6-8 pp drop). The `edge_satisfies` 7.26 % collapses to
  zero (the per-(rule, edge) call is eliminated); the
  `matching_edges` closure body (the surrounding 9.43 %)
  collapses to a thin index-lookup + push loop estimated at
  1-2 %; the remaining `apply_role_rules` cost is
  `guards_present` (3.99 %, untouched) + `add_label_with_deps`
  (0.95 %) + small leaf costs.
- **GALEN classify wall**: **3-8 % reduction** off the post-3d
  baseline (~11.87 min → ~10.9-11.5 min). Recon's calibration
  against Phase 3b is the rationale: Phase 3b removed a
  similar-magnitude per-call `are_declared_inverses` linear scan
  and delivered a sub-flame-proportional wall drop because of
  rayon overlap and concurrent work; Phase 3e is closer to the
  classify hot path than Phase 3d (the work is inside-tableau
  rather than at residual snapshot time), so the per-pp wall
  response should be slightly stronger.
- **FP=0 + MISSED=17 unchanged on GALEN.** Pure semantic-
  preserving refactor; the soundness section above gives the
  invariant.
- **Phase 0 net unchanged** (`alehif_closure_matches_konclude`,
  `ore_10908_sroiq`, `ore_15672_shoin`): FP=0 / MISSED=0 across
  all three.

## Why this fix shape is right

The current code shape is **rules-outer × edges-inner with a
predicate**: for each rule, for each edge, ask
"does this edge satisfy this rule's role?" That asks the same
question O(rules × edges) times per node. Each question is cheap
post-Phase-3b (binary search or HashSet lookup), but the
multiplicative factor on SIO yields 7.26 % of total wall.

The fix is the same algorithmic shift Phase 3d applied at the
dispatch level (replace per-trigger linear-scan with O(1)
indexed lookup), now applied at the matching-edges level:
precompute the rule-set per (edge-RoleId × polarity) pair at
`finalize()` time — once, before any classify call — so the hot
loop becomes **edges-outer with HashMap-lookup-inner**, doing
O(edges) lookups instead of O(rules × edges) predicate calls.

The finalize-time work is bounded by
`Σ_{rule} (|sub_roles(rule.role)| + |{(a,b) ∈ inverse_pairs : b == rule.role.id}|)`
which is linear in the role-hierarchy size and the inverse-pair
declarations combined with rule count — cheap (paid once per
TBox; classify amortizes it across O(n²) class pairs).

## Cross-references

- Phase 3e plan: `docs/superpowers/plans/2026-06-01-phase3e-apply-role-rules-inner.md`
- Phase 3e recon: `docs/phase3e-recon.md`
- Sibling phase fix-target template: `docs/phase3d-fix-target.md`
- `edge_satisfies` source: `crates/owl-dl-tableau/src/lib.rs:598-609`
- `are_declared_inverses` source (Phase 3b): `crates/owl-dl-tableau/src/lib.rs:495-510`
- `RoleHierarchy::sub_roles / super_roles / is_sub_role`: `crates/owl-dl-core/src/role_hierarchy.rs:115-137`
- `AbsorbedTBox::finalize` (currently does partition-by-guard at lines 129-140): `crates/owl-dl-core/src/absorb.rs:110-141`
- `RoleRule` struct: `crates/owl-dl-core/src/absorb.rs:156-165`
- `Role` enum: `crates/owl-dl-core/src/ir.rs:75-79`
- Phase 3d results (prior baseline): `docs/phase3d-results.md`
