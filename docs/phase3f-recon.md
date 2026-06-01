# Phase 3f recon — `apply_max` internals

Source: post-Phase-3d SIO flamegraph
(`docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg`) + code-trace
of `crates/owl-dl-tableau/src/rules.rs:882-1011` and the helpers
`edge_satisfies` / `are_declared_inverses` / `is_sub_role` /
`are_distinct`. HEAD: 4500dbb.

## Flamegraph drill-down

Dominant `apply_max` frame on post-3d SIO: **2,043 samples, 11.42%** of total
(was 14.34% post-3c; Phase 3d hoist already shaved some). All direct children
of this frame sum exactly to 2,043 — clean decomposition, no ambiguity:

| Child of `apply_max` | Samples | % total | % of apply_max |
|---|---:|---:|---:|
| `edge_satisfies` | 1,315 | 7.35% | **64.4%** |
| `collect` (maxes `Vec::from_iter` / `spec_extend`) | 434 | 2.43% | 21.2% |
| neighbour-iter dispatch (`next` / `Map::next` / `neighbours`) | 187 | 1.04% |  9.1% |
| `drop_in_place<Vec<NodeId>>` (per-max `c_neighbours` drop) | 54 | 0.30% | 2.6% |
| `has_label` | 53 | 0.30% | 2.6% |

Below `edge_satisfies` (1,315 samples), the leaf split is:

| Path | Samples | % of `edge_satisfies` |
|---|---:|---:|
| `are_declared_inverses` → hashbrown `contains` (HashSet probe) | 671 | 51.0% |
| `is_sub_role` → `binary_search` on super-closure | 589 | 44.8% |
| (residual `edge_satisfies` self / branch overhead) | ~55 | ~4% |

The `are_declared_inverses` path is the Phase 3b O(1) HashSet — the cost shown
is the full hashbrown probe (`contains_key` → `get_inner` → `find` →
`find_inner` → `full`) with foldhash, **not** the pre-3b linear scan. Phase
3b's win is intact; this is the residual HashSet probe cost.

Notably **absent from the flame**: the `c_neighbours.contains(&w)` linear
dedup, the O(c²) pair loop, `are_distinct`, `merge_into_with_deps`, and
`compute_max_merge_deps`. None appear under `apply_max` at any measurable
width. This is direct evidence that `c_neighbours.len()` is typically small
(≤ ~4) on the SIO workload and merges fire rarely — candidates D, E, F are
empirically not hot.

## Identified dominant inner cost

**Candidate B: `edge_satisfies` per-edge-per-max — 64.4% of `apply_max` (7.35% of total).**

`apply_max` calls `ctx.edge_satisfies(seen, role)` inside `for max in maxes { for (seen, w) in node.neighbours() { ... } }`. So if a node has E neighbours and M max-constraints in its label, this is E × M calls. The 2× breakdown of edge_satisfies (51% inverse-pair HashSet probe, 45% sub-role binary search) confirms it: same-polarity edges hit `is_sub_role` (binary_search on a sorted RoleId vec from `RoleHierarchy::super_closure`), cross-polarity hit `are_declared_inverses` (hashbrown HashSet contains). Both are already individually optimal in their lookup mechanism — what's expensive is the call frequency.

Secondary cost: **Candidate C: maxes Vec collect — 21.2% of `apply_max` (2.43% of total).** Allocates `Vec<(u32, Role, ConceptId, DepSet)>` per call by FilterMap-walking `node.labels()`. The `DepSet` (`SmallVec<[u32; 1]>`) means each kept entry is ~24 bytes + spill. The collect exists because the inner loop needs `&mut ctx` (for `merge_into_with_deps` / `add_label_with_deps`) while the label walk borrows the graph immutably — a borrow-checker workaround, not an algorithmic necessity.

## §16 risk analysis

The dominant cost (B) has **HIGH §16 risk** if the fix is the natural shape (per-classify or per-call HashMap indexing on edges). Phase 3e (§16 ledger) tried exactly this shape on `apply_role_rules`'s `edge_satisfies` calls — SIO won, GALEN regressed +2.34%. Workload-dependent break-even: HashMap probe cost > edge_satisfies traversal cost when rule density is thin and edges are many (GALEN's shape).

Variants that move the §16 risk profile:

1. **Per-classify HashMap on edges** (Phase 3e shape, applied to apply_max): HIGH RISK. SIO has 84 inverse-pair declarations + a rich role hierarchy; GALEN has a sparse role hierarchy and edge-heavy nodes. Same break-even asymmetry as 3e.

2. **Per-node-visit local bucket of neighbours-by-role** (build during the neighbour scan, then index into for each max): LOWER RISK but still workload-dependent. Cost amortization: O(E) build + O(M × bucket_lookup) per node visit. Beats current O(E × M) only when M ≥ 2. If GALEN's typical M (`Max`-constraint count per node) is 1 or 0, the bucket alloc is pure overhead. M=0 is already short-circuited by `maxes.is_empty()`. The unknown is M=1 frequency, which is unmeasured. WORKLOAD-DEPENDENT (low–medium).

3. **Loop inversion** — `for (seen, w) in neighbours { for (max, role, body, deps) in &maxes { if edge_satisfies(seen, role) && has_label(w, body) ... } }`: same call count, just different ordering. Allows `edge_satisfies(seen, role)` results to short-circuit `has_label` per neighbour-max pair (already done in current code). No improvement; reject.

4. **Cache `edge_satisfies` results in a small inline structure** (e.g. `SmallVec<[(Role, bool); 8]>` keyed by `seen` only): WORKLOAD-DEPENDENT but with small constants. Saves repeat `is_sub_role` / `are_declared_inverses` calls when the same edge is checked against multiple `role`s across maxes. Same M ≥ 2 break-even.

**Candidate C is workload-neutral.** Removing the maxes `Vec` collect via direct iteration / inline traversal saves ~2.43% on SIO with no workload-dependence. The fix is straightforward (re-shape the borrow: clone `Max` triples into a `SmallVec` to release the graph borrow before the mutating inner loop, or iterate labels indexed-only without holding the borrow).

## Code-trace evidence

`crates/owl-dl-tableau/src/rules.rs:882-966` — `apply_max`. Key trace:

- L887-900: `maxes: Vec<(u32, Role, ConceptId, DepSet)> = labels.iter().enumerate().filter_map(...).collect();` — `DepSet = SmallVec<[u32; 1]>` (per `crates/owl-dl-tableau/src/trail.rs` def). Vec capacity grows via `spec_extend` (305 samples = 70% of the 434 collect cost). FilterMap walks ALL labels (typically dozens-to-hundreds) and only retains `ConceptExpr::Max(...)` — most calls return None (the 264-sample `find_map/try_fold` cost is this miss-walk).
- L905-916: outer `for (n, role, body, max_deps) in maxes` × inner `for (seen, w) in node.neighbours()`. The `ctx.edge_satisfies(seen, role)` is the first short-circuit gate.
- L912: `!c_neighbours.contains(&w)` — linear; invisible in flame ⇒ `c_neighbours.len()` is small.
- L917-918: `if c_neighbours.len() <= n` continue — typical fast-out.
- L926-941: O(c²) pair loop — invisible in flame ⇒ rarely entered or c is tiny when it is.

`crates/owl-dl-tableau/src/lib.rs:598-609` — `edge_satisfies`. Branch:
- Same polarity: `hierarchy.is_sub_role(s, w)` (or `s == w` fallback).
- Cross polarity: `are_declared_inverses(s, w)` (HashSet contains).

`crates/owl-dl-tableau/src/lib.rs:498-510` — `are_declared_inverses`: early `is_empty()` short-circuit + `HashSet::contains(&(r, s))`. The flame's 671 samples are entirely the hashbrown probe.

`crates/owl-dl-core/src/role_hierarchy.rs:133-137` — `is_sub_role`: `super_closure[sub.index()].binary_search(&sup).is_ok()`. The flame's 589 samples are the binary search on the sorted RoleId slice.

`crates/owl-dl-tableau/src/lib.rs:902-904` — `are_distinct`: `node(a).inequalities().contains(&b)` (linear scan on a `Vec<NodeId>`). Invisible in flame ⇒ inequalities sets are tiny or `are_distinct` is rarely called.

`crates/owl-dl-tableau/src/graph.rs:207-212` — `neighbours()`: chained iter over `edges` + `in_edges`, building Role from RoleId. The 187 sample residual (Map::next / chain dispatch) is unavoidable iterator overhead.

**Estimated typical cardinalities on SIO** (inferred from flame, not measured):
- `node.neighbours().count()` (E): moderate (50+ likely, since 2043 samples represent many invocations).
- `maxes.len()` (M): probably 1–3 typical (the collect frame is 21% of `apply_max`, suggesting the collect itself is not trivial — multiple matched Max constraints per call, OR many label walks per call).
- `c_neighbours.len()` (c): small (≤4 typical) given absent contains/pair-loop in flame.

These cardinalities are **not measured by counter** — recommend T3 add a one-shot histogram counter to confirm M ≥ 2 frequency before committing to a fix that requires it.

## Discarded candidates

- **A: `c_neighbours` per-max edge scan.** Subsumed by B — the edge scan cost
  IS the `edge_satisfies` cost. No independent attack surface.
- **D: O(c²) pair-mergeability loop.** Invisible in flame; c is small. ~0%.
- **E: `compute_max_merge_deps`.** Invisible (no `compute_max_merge_deps`
  samples found in the SVG). Fires only on `!are_distinct(a, b) && merged`,
  which is rare per the absent pair-loop cost. ~0%.
- **F: `c_neighbours.contains(&w)` linear dedup.** Invisible. ~0%.

## Proposed fix shape (handoff to T3)

**Recommendation: TWO-stage approach, with workload-dependence gate.**

**Stage 1 (low-risk, workload-neutral, modest impact): attack Candidate C.**

Remove the `maxes` Vec by either:
(a) iterating `node.labels()` indices once, materialize the (count, role, body) tuple inline at the call site by re-resolving via `pool.get`, and use a separate `DepSet` clone path that doesn't require an upfront allocation; or
(b) push the per-max body of the loop into a separate `apply_max_for_constraint(ctx, node, n, role, body, max_deps)` function so the outer loop only needs `(usize, ConceptId)` (label position + concept) which can be collected into a `SmallVec<[(u32, ConceptId); 4]>` (no DepSet clone in the collect — clone deps inside the inner function after re-borrowing).

Predicted SIO win: ~1.5–2.0% of total (most of the 2.43% collect cost; some allocator churn remains for the `Vec<NodeId> c_neighbours`).
Predicted GALEN risk: NIL (the alloc is workload-neutral; removing it costs nothing on any shape).

**Stage 2 (conditional on T3 measurement): attack Candidate B with workload-adaptive dispatch.**

Add a `RUSTDL_COUNTERS` histogram tracking `maxes.len()` distribution per `apply_max` call. Run on SIO + GALEN. If M ≥ 2 frequency is high on both → safe to add a per-node-visit `SmallVec<[(Role, NodeId, bool /* is_sub_of_some_max_role */); 8]>` bucket. If M ≥ 2 is high on SIO but rare on GALEN → DO NOT SHIP the bucket; falls into §16 trap (gains on SIO, regress GALEN). If M ≥ 2 is rare on both → defer to a different phase target (`from_iter/collect` 6.51% cluster across the codebase).

**If T3 prefers a single surgical fix**: ship Stage 1 only. It's the workload-neutral free win; Stage 2's gain is conditional and risky.

**Alternative pivot if T3 wants more impact than Stage 1 alone:** target the **`from_iter/collect` 6.51% cluster** identified in the post-3c findings doc as the Phase 3e alternative. That cluster aggregates heap allocs across multiple callers and is workload-neutral in shape.

## Confidence

- The 2043-sample / 7-children decomposition is exact (children sum to parent — no missing depth).
- Candidates D, E, F are decisively ruled out by absence from the flame.
- Candidate B's workload-dependence is empirically established by §16.
- Candidate C's workload-neutrality is structural (alloc removal cannot regress).
- The M (max-per-node) cardinality assumption is **not** measured — Stage 2 requires a counter probe before committing.

**Verdict for T3**: ship Stage 1 (workload-neutral collect removal); attempt Stage 2 only after a counter-confirmed M ≥ 2 frequency check on GALEN. If Stage 2 doesn't survive that gate, pivot to the `from_iter/collect` 6.51% cluster.
