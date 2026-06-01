# GALEN classify hot-path findings (Phase 3 prep)

Profiled 2026-06-01 against branch `plan/soundness-completeness-perf`
post-Phase-2b/2b.5 (commit b588b3a). Sampling: pprof-rs @ 199Hz,
`RUSTDL_PROFILE_SECONDS=120`, 32 rayon workers, on
`ontologies/external/galen.ofn` (GALEN, 2748 classes).

GALEN classify wall: ~24.7 min (vs 12.5 min Phase 2a baseline; the
~2× regression is the Phase 2b lever cost being paid for the 84%
MISSED recovery).

## Top-level split

The "saturate" frame at 72.32% is **the tableau's own `saturate()` in
`owl-dl-tableau/src/saturate.rs`**, called from `search.rs` on each
backtracking step — it is **not** the EL saturation engine
(`owl-dl-saturation` crate, `lib.rs:70`). The call chain is:

```
branch (y=4261, 73.01%) -> search (y=4245, 73.01%)
    -> [rayon + thread frames] -> saturate (y=2277, 72.32%)
        -> apply_deferred_concept_or_rules (31.40%)
        -> apply_max (17.08%)
        -> eq / clash_deps_at / first_clash (11–18%)
```

The EL saturation engine (`process_subsumer`) appears at only **0.04%** (11
samples). GALEN's workload is dominated by the SROIQ fragment, so nearly all
pairs fall through to the hypertableau rather than short-circuiting at EL
saturation.

## Top hot frames

| % incl | samples | function | notes |
|-------:|--------:|---------|-------|
| 73.01% | 18185 | `branch` | tableau backtracking driver (search.rs) |
| 73.01% | 18185 | `search` | called inside branch |
| 72.32% | 18013 | `saturate` | **tableau/saturate.rs** saturation loop (not EL engine) |
| 31.40% | 7820 | `apply_deferred_concept_or_rules` | deferred OR rule — Phase 2b cost |
| 18.13% | 4516 | `eq` | `PartialEq::eq` for `ConceptId` — **leaf frame** |
| 17.08% | 4255 | `apply_max` | max-cardinality rule |
| 11.27% | 2806 | `first_clash` | clash detection |
| 11.23% | 2796 | `clash_deps_at` | dependency set for clash |
| 10.73% | 2672 | rayon idle workers | `wait_until_cold` etc. |
| 6.91% | 1720 | `spec_extend` / `from_iter` (edge tuples) | heap alloc in `apply_max` |
| 5.63% | 1403 | `spec_extend` / `from_iter` (ClassId tuples) | heap alloc in `apply_deferred` |
| 4.48% | 1115 | `needs_deferred_or` + `binary_search<ConceptId>` | deferred-OR check |
| 4.08% | 1016 | `clone SmallVec<[u32;1]>` | DepSet clone in deferred-OR |
| 3.54% | 882 | `edge_satisfies` / `is_sub_role` | role membership in max rule |
| 3.44% | 857 | `decide` (`classify_top_down_internal`) | classify pair loop |
| 3.16% | 787 | `find_inner` / `get` (HashMap) | subsumer map lookup |
| 3.01% | 750 | `apply_concept_rules` | concept rule dispatch |
| 2.75% | 685 | `apply_exists` | existential rule |
| 2.41% | 600 | `eq<ConceptId>` (slice scan) | O(n) scan in apply_deferred |
| 1.86% | 463 | `neighbours` | edge-list access |
| 1.83% | 456 | `index<ConceptExpr>` | concept pool lookup |
| 1.79% | 446 | `has_label` | label membership check |
| 1.72% | 428 | `apply_and` | conjunction rule |
| 1.05% | 262 | `drop_in_place<Vec<NodeId>>` | dealloc in rule body |
| 0.86% | 214 | `apply_role_chains` | role chain rule |
| 0.04% | 11 | `process_subsumer` | **EL saturator** (owl-dl-saturation) |

## Quick interpretation

**The tableau is the bottleneck, not the EL saturator.** GALEN stresses the
SROIQ fragment with deep role hierarchies and max-cardinality restrictions; the
vast majority of class pairs require the full tableau, so the
`owl-dl-saturation` crate contributes negligible wall time.

Within the tableau's `saturate()` (called from `search.rs` on each
backtracking step, defined in `saturate.rs`), the two dominant costs are:

1. **`apply_deferred_concept_or_rules` (31.4%)** — this is Phase 2b's
   deferred OR trigger paying its runtime cost. The hot leaf inside it is
   `eq` (18.1%), which is `PartialEq::eq` for `ConceptId` — a scalar
   comparison, but called in a tight O(n) scan over the label vector in
   `needs_deferred_or`. About 5.6% goes to `SmallVec` / `Vec` allocation
   chains inside the rule body, and 4.1% to `clone SmallVec<[u32;1]>`
   (DepSet copies). These allocations were introduced by Phase 2b's
   conjunctive-trigger path for LHS-And expressions.

2. **`apply_max` (17.1%)** — max-cardinality rule. About 6.9% is heap
   allocation (`spec_extend` / `from_iter` building edge tuples into a new
   `Vec` per call), and 6.4% is `find_map` — an O(n) linear scan over the
   concept label vector looking for a matching filler.

Together `apply_deferred_concept_or_rules` + `apply_max` account for ~48% of
total wall time. Clash machinery (`first_clash` + `clash_deps_at`) accounts for
another ~22%. The idle rayon fraction (~10.7%) indicates scheduling
granularity: 32 workers are waiting when the single-threaded backtracking
search holds the work.

**`apply_role_chains`**, which dominated pizza at 99.8%, is only **0.86%** on
GALEN — GALEN's hot rules are cardinality and disjunction, not role chains.

## Comparison with spec's named perf targets

Per `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`
§"Phase 3", the spec named two targets:

- **Saturator hot path on SIO 68s / notgalen 10min.** The GALEN flamegraph
  shows the EL saturator (`owl-dl-saturation`) is negligible here (0.04%). The
  `saturate` frame at 72% is the tableau's own saturation sub-loop
  (`saturate.rs`). For
  SIO and notgalen — which are more EL-heavy — the EL saturator may be the
  bottleneck as the spec expects. **The GALEN flamegraph does not validate or
  refute the EL-saturator target; a SIO/notgalen profile is needed for that.**

- **Or-body trigger regression (commit `fddf2ee`).** `apply_deferred_concept_or_rules`
  at 31.4% confirms that the Phase 2b/2b.5 deferred-OR machinery pays real cost
  on GALEN. This is consistent with the spec's target. The hot leaves (the `eq`
  scan + SmallVec alloc chain) are the specific micro-targets within that rule.

## Phase 3 action items implied by this profile

1. **Reduce allocation in `apply_max` and `apply_deferred_concept_or_rules`**:
   replace per-call `Vec::collect()` with stack-allocated arrays or reuse
   buffers from the `TableauContext`. The `SmallVec` chaining overhead (6.9% +
   5.6%) should be addressable without algorithmic changes.

2. **Replace the O(n) scan in `needs_deferred_or` / `apply_deferred`**:
   `eq` at 18.1% (a `ConceptId` scan over the label set) suggests the label
   vector is searched linearly. A sorted label vector with binary search, or a
   bitmap for cheap membership testing, would collapse this.

3. **Clash machinery (first_clash + clash_deps_at at 22%)**: may benefit from
   caching the most recent clash rather than re-scanning on each rule iteration.

4. **EL-saturator targets**: profile SIO and notgalen separately — those
   workloads may tell a different story where `process_subsumer` / worklist
   iteration dominate.

5. **Idle rayon (10.7%)**: parallelism is underutilised during the
   backtracking phase. The classify pair loop is parallel but individual
   tableau invocations are single-threaded. Not a Phase 3 target but worth
   noting.
