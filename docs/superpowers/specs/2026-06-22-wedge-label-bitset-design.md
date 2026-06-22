# Wedge node-label bitset representation — design (scoping)

**Date:** 2026-06-22
**Status:** approved (brainstorming) → ready for implementation plan
**Branch (impl):** `perf/wedge-label-bitset`

The last non-dead-end perf lever for the hypertableau **wedge** after two shipped
SmallVec allocation wins exhausted the allocation frontier (allocator self-time
35% → 1.1%). Profiling a wedge-heavy ORE ontology's label-cache build showed the
residual matcher cost is genuine compute: `select_unpredictable` + `binary_search_by`
≈ 11% self-time, all from the node class-label **membership lookup** (`.has(class)`),
plus the O(n) sorted-insert in `add_label` (`push`/`push_mut` ~8–15%). Replace the
sorted-`Vec` label representation with a **bitset** to make `.has` and `add_label`
O(1) and blocking `subset`/`disjoint` bitwise.

**This is a pure representation change — no semantics change.** FP=0 is SACRED; the
gate is byte-identical closures corpus-wide.

## Current representation (explored)

`HyperNode` (`crates/owl-dl-tableau/src/hyper.rs:137`):
- `labels: Vec<ClassId>` — sorted by id, deduped. `.has` = `binary_search_by_key`
  (O(log n)); `add_label` = binary-search + `insert(pos, …)` (O(n) memmove).
- `label_deps: Vec<DepSet>` — **parallel to `labels` by index**; `label_deps[i]` is the
  backjumping dependency set of `labels[i]`. "Empty before any branching."
- Sorted order is load-bearing for double-blocking (`subset_sorted`, hyper.rs:2847,
  called at 1066/1069/1091) and `labels_disjoint` (1927).
- Snapshot: `pre_capture_labels: Vec<Vec<ClassId>>`; replay reconstructs
  `label_deps = vec![birth_deps; labels.len()]` (1478).
- ~76 `.labels`/`label_deps` access sites total.

The class-id universe includes **nominals + Tseitin synthetics above `num_classes`**
(hyper.rs:307/325) — the bitset MUST be sized to the full id universe, not
`num_classes`, or membership for synthetic ids is silently wrong (a correctness bug).

## Design

### Labels → `FixedBitSet`
`fixedbitset 0.5` is already a workspace dep (saturator + reasoner). `labels: FixedBitSet`
sized to the full class-id universe (compute the max id once at engine build and size
all nodes uniformly so `is_subset`/`is_disjoint` operate on equal-width sets).
- `has(c)` → `labels.contains(c.index())` — O(1).
- `add_label` → `labels.put(c.index())` — O(1), eliminates the sorted-insert memmove.
- `subset_sorted(a,b)` → `a.is_subset(b)` (bitwise).
- `labels_disjoint` → `a.is_disjoint(b)` (bitwise; or `(a & b).count_ones()==0`).
- iteration (`for c in …labels…`, 1133/1356) → `labels.ones().map(ClassId::new)` —
  ascending, so any order-dependent consumer is preserved.

### Deps → sparse `HashMap<ClassId, DepSet>` (default `EMPTY`)
A bitset has no per-label slot for the parallel `DepSet`. Replace the parallel
`Vec<DepSet>` with a sparse map, lookups for an absent key returning `DepSet::EMPTY`.
- **Semantically exact:** `label_deps` is empty before branching, so the entire Horn
  fixpoint (the bucket-B label-cache build) carries an empty map → cheap clone.
- Under branching, only labels added under a decision get an entry.
- Snapshot replay (1478) rebuilds entries for `labels.ones()` with `birth_deps`.
- **Soundness:** deps drive backjump pruning; an under-broad dep could wrongly skip a
  branch → a missed subsumption. The byte-identical-closure gate catches this — it is
  the load-bearing tripwire for the deps migration.

### Sites
Mechanical migration of the ~76 sites: `has`/`add_label`/remove (210/943/234),
the blocking `subset_sorted`×3 + `labels_disjoint`, iteration (1133/1356), and snapshot
(`pre_capture_labels` + the 1478 reconstruction). `pre_capture_labels` may stay
`Vec<Vec<ClassId>>` (it feeds a membership test; convert to a set lookup) or migrate to
`Vec<FixedBitSet>` — decided during impl by which is simpler, both sound.

## P0: branchiness × density profiling study (go / no-go / hybrid gate)

The wedge **clones whole nodes per branch** (save/restore). A width-W bitset clone is
`W/8` bytes regardless of density; today's `Vec<ClassId>` clone is `4 × present_labels`.
So the bitset is **cheaper to clone on dense-label nodes, costlier on sparse-wide-label
nodes**, and the net is governed by **branchiness × label density**:

- low branch count × dense labels → bitset wins (few clones, each cheaper);
- high branch count × sparse-wide labels → bitset loses (many clones, each fatter).

**Before completing the migration, run a profiling study** (using the wedge's existing
`SearchStats`: `disj_branches`, `merge_branches`, `max_branch_depth`, node-clone count;
plus per-node label-density / class-universe-width) across a **broad ontology set**
(whole corpus + an ORE-pilot slice). Characterize the regime each ontology falls in.
Decision from the study:
1. **Pure bitset** if net-positive (or neutral) across the regimes that matter.
2. **Adaptive/hybrid repr** if the bitset wins only in the low-branch/dense regime:
   choose representation by the measured regime — e.g. keep the sorted `Vec` when a
   workload is high-branch and labels are sparse relative to a wide universe, use the
   bitset otherwise. The selector keys off cheap, available signals (class-universe
   width, observed branch count) — NOT a per-node runtime guess that adds overhead.
3. **Abort B** if it's net-negative or only neutral everywhere (bank the two shipped
   SmallVec wins; record the negative result in perf memory).

This P0 study is the first implementation phase; the full mechanical migration only
proceeds on a (1)/(2) verdict.

## Evaluation (acceptance)

- **Soundness (the hard gate):** byte-identical classification closures **corpus-wide**
  (md5 of sorted `direct`/`equiv` edges) — galen/notgalen/sio/wine/ore-10908/ore-15672/
  ore-15516/alehif/ro/sulo/pizza/shoiq-knowledge/bibtex/family/go-basic — plus the
  ORE-pilot recovered + still-DNF slice. Any diff = abort.
- `cargo test --workspace` green; `cargo fmt`/`clippy` clean.
- **Perf:** broad A/B (before binary vs after binary), high-N interleaved repeats with
  median+min on the fast onts, single capped runs on the slow ones, **galen as an EL
  control** (must stay flat — it routes to the saturator, untouched). Keep only if the
  aggregate beats the ~±2% noise floor on the wedge onts; flat/negative → revert.
- **No DNF expectation:** this is a constant-factor lever; it is NOT expected to recover
  any of the 13 DNF ontologies (that frontier is algorithmic). Success = a measurable
  broad constant-factor speedup on wedge-heavy SROIQ, FP-safe.

## Out of scope

The saturator's label storage (separate engine, EL path); the snapshot cache format;
edge-by-role indexing (Phase-3e dead-end); any semantics/calculus change.
