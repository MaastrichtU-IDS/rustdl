# Sound Label-Set (Un)Sat Caching: Design + GO/NO-GO
# Track C.2 continuation — post-defined-sup-sweep-gate measurement

**Date:** 2026-06-16  
**Commit measured:** f02ee4e (HEAD — FixedBitSet matrix + defined-sup sweep gate)  
**Host:** linux x86_64, 32 cores, rustdl v0.3.8 release binary  
**Mandate:** profile post-sweep residual; design the sound label-set cache; deliver GO/NO-GO

---

## 0. Context: what the sweep-gate commit did

Commit 97f5153 (`perf: gate defined-sup sweep with label-oracle`) wired the per-class
`LabelOracle` (built in `label_cache_build`) into the `defined-sup sweep`
(`classify.rs:1518–1647`). Before the gate, that sweep called
`subsumes_via_tableau` on every `(cand, defined-sup)` candidate without consulting the
oracle — 78,855 calls for SIO (50 `EquivalentClasses` × ~1577 candidates each) vs 197
for the tier walk. The three-arm oracle gate (prune `sup ∉ labels`; pass-through
`sup ∈ labels`; fall-through `NoVerdict`) reduced SIO's wedge calls from 78,855 to 265
and ORE-10908's from 6,881 to 2.

This was "Cheap C.2" from the 0.3 attribution — captured without any new caching
infrastructure. The question now is whether "Hard C.2" (label-set fact caching proper)
is still a worthwhile lever on the post-sweep residual.

---

## 1. Post-sweep-gate attribution (measured)

All measurements run as `classify --pair-timeout-ms 200` (wine `--pair-timeout-ms 25`),
commit f02ee4e, release binary.

### 1.1 SIO (`ontologies/real/sio.ofn`, 1585 classes, out-of-EL)

**Single-thread (RAYON_NUM_THREADS=1):**
```
# subsumption: saturation=10483 tableau=0
# label heuristic: pruned=110799 pass_through=265 misses=0
# wall breakdown ms: label_cache_build=9665  tier_walk=1785
# pairs-per-sub: n_subs=179 total=265 median=1 p90=2 p99=6 max=6
# wedge-cost-histogram (0|1|2-4|5-9|10-19|20-49|50-99|100-999|≥1000):
#   167 | 40 | 31 | 4 | 0 | 18 | 5 | 0 | 0
Elapsed: 11.5s  RSS: 33 MB
```

**32-thread:**
```
# label heuristic: pruned=110799 pass_through=265 misses=0
# wall breakdown ms: label_cache_build=922  tier_walk=1349
Elapsed: 2.46s  RSS: 377 MB
```

**Before sweep gate (Phase 0.3 doc):** 1T: 490s, 32T: 25.6s.  
**Speedup:** 42× (1T), 10.4× (32T).

**32T breakdown:**
- `label_cache_build`: 922 ms = **37.5%** of wall — 1585 per-class wedge probes
- `tier_walk`: 1349 ms = **54.8%** of wall — 265 wedge calls + BFS traversal overhead
- Other (overhead, I/O, saturation): ~7.7%

Within `tier_walk`:
- Wedge call cost (from histogram): 167×0 + 40×1 + 31×3 + 4×7 + 18×35 + 5×75 ≈ 1166 ms (1T) → ~41 ms amortized at 32T
- BFS/tier-traversal overhead: 1349 − 41 ≈ **1308 ms at 32T** — inherent to the
  `find_direct_parents_top_down` sequential-tier structure; not amenable to caching

---

### 1.2 ORE-10908 (`ontologies/external/ore-10908-sroiq.ofn`, 692 classes, out-of-EL)

**Single-thread:**
```
# label heuristic: pruned=33019 pass_through=2 misses=0
# wall breakdown ms: label_cache_build=1944  tier_walk=109
# pairs-per-sub: n_subs=2 total=2 median=1 p90=1 p99=1 max=1
Elapsed: 2.08s  RSS: 11 MB
```

**32-thread:**
```
# wall breakdown ms: label_cache_build=209  tier_walk=114
Elapsed: 0.32s  RSS: 51 MB
```

**Before sweep gate:** 1T: 21.0s, 32T: 1.09s.  
**Speedup:** 10.1× (1T), 3.4× (32T).

**32T breakdown:**
- `label_cache_build`: 209 ms = **65.3%** of wall — 692 per-class probes
- `tier_walk`: 114 ms = **35.6%** of wall — 2 wedge calls + BFS overhead
- Only 2 pass-through wedge calls remain (from 6,881 pre-sweep)

---

### 1.3 Wine (`ontologies/real/wine.ofn`, 137 classes, out-of-EL, 25ms timeout)

```
# label heuristic: pruned=5317 pass_through=53 misses=8661
# wall breakdown ms: label_cache_build=86307  tier_walk=439623
# timed-out pairs: 8666
# wedge-cost-histogram: 15 | 9 | 24 | 0 | 0 | 8666 | 0 | 0 | 0
Elapsed: ~526s (1T)
```

8666 of 8714 calls hit the 25 ms deadline. Wine is structurally distinct: the
`label_cache_build` cost is **630 ms/class** (search-dominated), not setup-dominated.
No caching approach addresses the hard SROIQ pairs that exhaust the deadline.

---

### 1.4 Summary attribution table

| Fixture | Wall 32T | Konclude | Ratio | label_cache% | tier_walk% | Wedge calls | Main tableau |
|---|---|---|---|---|---|---|---|
| SIO | 2.46 s | 59 ms | 42× | **37.5%** | 54.8% | 265 | 0 |
| ORE-10908 | 0.32 s | 23 ms | 14× | **65.3%** | 35.6% | 2 | 0 |
| Wine@25ms | 53.9 s | 33 ms | 1633× | 17% | 83% | 8,714 | 0 |

**Key shift from Phase 0.3:** `label_cache_build` is now the dominant or co-dominant cost
on SIO and ORE-10908. The sweep gate captured the defined-sup sweep cost entirely; the
residual tier_walk is mostly BFS overhead. Main tableau: 0 calls across all three
fixtures (all SROIQ cost is the hyper wedge).

---

## 2. Internal anatomy of `label_cache_build`

`label_cache_build` runs `PreparedOntology::classify_labels(class_id, deadline)` for
each named class in parallel (rayon). Each call lands in `HyperCache::classify_labels`
(`lib.rs:1079`):

```rust
pub(crate) fn classify_labels(&self, c: ClassId, deadline: ...) -> LabelOracle {
    let mut clauses = self.clauses.clone();          // (A) full clause clone
    clauses.push(DlClause { body: [q → c], head: [] }); // one Q-clause
    let mut engine = HyperEngine::new(&clauses, self.fresh_q); // (B) setup
    // ...
    match engine.decide_with_deadline(HYPER_WEDGE_DEPTH, deadline) { ... }
}
```

`HyperEngine::new` (`hyper.rs:623`) does:
- `build_disjoint_pairs(clauses)` — O(#clauses) scan, builds `HashSet<(u32,u32)>`
- `build_clause_indexes(clauses)` — O(#clauses) scan, builds `Vec<Vec<usize>>` trigger
  indexes (by class, by role, by successor-class) (`hyper.rs:557`)

**Per-probe cost = (A) clone + (B) index build + (C) search.**

### 2.1 Setup-dominance: three-fixture scaling

For each out-of-EL fixture, the per-class probe cost (= `label_cache_build_ms / n_classes`,
1T measurement):

| Fixture | Classes | Axioms (proxy for #clauses) | Per-probe cost (1T) |
|---|---|---|---|
| Pizza | 99 | 292 | 0.70 ms/class |
| ORE-10908 | 692 | 926 | 2.81 ms/class |
| SIO | 1585 | 2348 | 6.10 ms/class |

Axiom ratios (pizza→ore, pizza→sio): 3.2×, 8.0×.  
Cost-per-probe ratios: 4.0×, 8.7×.

**Cost-per-probe scales approximately linearly with axiom count** across three fixtures
(k ≈ 0.0024–0.0030 ms/axiom). Because models are tiny (median 2 nodes from the alehif
L2 probe; SIO's `pass_through=265, misses=0` confirms all 1585 classes produce small
satisfying models), **the search cost (C) is near-constant per probe.** The
clause-clone + index-build (A+B) dominate.

Wine breaks this: 630 ms/class >> 0.86 ms predicted from axiom count alone. Wine's SROIQ
nominals/`≤n`/inverse structure forces deep disjunction-branching search — wine is
**search-dominated**. This is a separate regime.

### 2.2 What this means for caching

The dominant label_cache_build cost is **per-probe fixed setup** (clone + index-build),
not repeated sub-problem search. Caching satisfiability results of specific label sets
addresses (C), which is ~10–20% of cost. The 80–90% is (A+B), which is amortizable by
a different mechanism: **build the clause indexes once and reuse across all n probes**
(clone only the search-mutable state, not the clause slice). This is a FP-free
engineering fix, not a label-set cache.

---

## 3. Why sound label-set fact caching is the wrong lever here

### 3.1 Two cost layers; the cache targets only one

The pre-sweep cost was in the **query layer** (per-pair subsumption calls). The sweep
gate cut that layer by >99%. The residual is in the **oracle-build layer** (per-class
satisfiability probes). A label-set fact cache targets the search cost within individual
probes — the wrong layer.

### 3.2 No repeated (sub, sup) pairs

Classic memoization of `hyper_decide(sub, sup)` → verdict yields zero hits: classify
asks each `(C, D)` pair at most once. The 265 SIO pass-through pairs are 265 distinct
pairs with distinct sups.

### 3.3 Inter-probe label-set reuse: context-independence requirement

A node in probe (C₁) labelled `{X, Y}` and a node in probe (C₂, D) labelled `{X, Y}`
look identical, but the two probes run with **different clause databases** — probe (C₂, D)
adds `q ⊑ ¬D` (the negated-sup clause) while (C₁) has only `q ⊑ C₁`. The satisfiability
of `{X, Y}` under the full clause DB + `q ⊑ C₁` is not the same as under the full DB
+ `q ⊑ C₂ ∧ q ⊑ ¬D`. Reusing the cached result from one probe in the other is unsound:
it could treat `{X, Y}` as satisfiable when the additional `¬D` constraint closes it, or
vice versa.

More precisely (two directions, asymmetric):
- **Unsat reuse:** valid when `V_curr ⊇ V_proven` (the current DB has MORE constraints
  than the one used to prove unsat — adding constraints can only maintain or create new
  clashes). Proven-unsat in a weaker context ⟹ unsat in any stronger one.
- **Sat reuse:** valid when `V_curr ⊆ V_proven` (the current DB has FEWER constraints
  than the one used to find a satisfying model — removing constraints opens up models).
  Proven-sat in a stronger context ⟹ sat in any weaker one.

The per-pair clause context varies by `(sup, negated-sup-atoms)` across every probe.
Conservative dependency-tracking would invalidate the vast majority of entries — the hit
rate is negligible for SIO/ORE (2 nodes median; each probe's one non-root node carries a
distinct label from a distinct clause push).

### 3.4 The tier-walk BFS overhead is structural, not a cache target

Of SIO's 32T tier_walk 1349 ms, ~1308 ms is BFS traversal (the `find_direct_parents_top_down`
sequential-tier structure). This is inherent to the top-down hierarchy-placement algorithm
and does not involve repeated search subproblems.

### 3.5 Wine: different regime, no caching escape

Wine's cost is 8666 pairs each exhausting the 25 ms wedge deadline. These are genuine
SROIQ-hard pairs requiring deep disjunction branching. Caching their intermediate label
sets does not help: the full branching tree is the cost, not re-expansion of shared nodes.

---

## 4. Sound label-set fact cache: design sketch (for completeness)

Despite the NO-GO verdict on the current workload, the design is recorded for the next
time the question arises.

### 4.1 What to cache

A global table keyed by `(label_set: FrozenHashSet<ClassId>, role_context: RoleCtx) →
SatFact`, where:
- `label_set` is the set of class labels at a node
- `role_context` encodes the role path to the node (depth, role chain) — necessary for
  inverse-role and `≤n` context-sensitivity
- `SatFact` is either `Unsat(proven_under: ClauseSetVersion)` or
  `Sat(model_minimal_ctx: ClauseSetVersion)`

### 4.2 Soundness contract

The monotonicity principle is: unsatisfiability is preserved when adding constraints;
satisfiability is preserved when removing them.

**Unsat reuse is safe when `V_cached ⊆ V_curr`** (the cached proof used FEWER clauses
than the current probe). Formally: if `{L}` was proven unsat under clause DB V_cached
and `V_curr ⊇ V_cached` (the current DB has more constraints, e.g., an additional `¬D`
clause), then `{L}` is also unsat under V_curr. Every model of `{L}` that fails in the
weaker context also fails under the additional constraints. This matches §3.3:
"proven-unsat in a weaker context ⟹ unsat in a stronger one."

A cached `Unsat` entry from a probe with a SMALLER clause set (fewer constraints) is safe
to reuse under a larger set. A cached entry from a probe with MORE constraints (e.g.,
a per-pair probe that added `¬D`) is NOT safe to reuse in a probe with fewer constraints.

**Sat reuse is safe when `V_curr ⊆ V_cached`** (the cached sat proof used MORE constraints
than the current probe). Formally: if `{L}` was proven sat under V_cached and
`V_curr ⊆ V_cached` (the current probe has fewer constraints), then `{L}` is also sat
under V_curr — removing constraints can only open up new models. This matches §3.3:
"proven-sat in a stronger context ⟹ sat in a weaker one."

A cached `Sat` entry from a probe with a LARGER clause set (more `¬D` constraints) is
safe to reuse under a smaller set; the reverse is NOT safe. For the per-pair classify loop
where each probe adds a distinct `¬D`, the only sound `Sat` reuse is when the current
probe's DB is a subset of the one that produced the cache entry — rarely true in practice.

In short: the dependency check is ASYMMETRIC — Unsat entries are useful when the cached
probe was cheaper (fewer clauses), Sat entries when the cached probe was more constrained.

### 4.3 Failure mode of the snapshot cache (the predecessor)

The model-snapshot cache (`RUSTDL_SNAPSHOT_CAPTURE`, default OFF) stored ONE satisfying
model for class C and replayed it for all (C, D) pairs. Failure mode: the snapshot was a
`Sat` result for `C` alone, but probing `(C, D)` adds `¬D` — so if `D ∈ the-one-model`,
the snapshot shows "D is in this model" → correctly says "subsumed", but if `D ∉
the-one-model` → says "not subsumed" even when C ⊑ D in every model (the model was
not canonical). On disjunctive ontologies, a class may have multiple models, some
containing D, some not; replaying the one model that doesn't contain D gives a false
non-subsumption. The `BackPropRisk::Safe` gate excluded `inverse/nominal/card` but NOT
`disjunction` — ORE 2015 exposed 30+ FP per ontology on disjunctive-but-card-free inputs.

The label-set fact cache avoids the model-reuse trap because it caches **facts**
(boolean sat/unsat of a label set), not models. But it still inherits the
`Sat`-reuse dependency problem: "sat under context V₀" ≠ "sat under context V₁ ≠ V₀".

### 4.4 Dependency tracking cost

To safely reuse `Sat` entries, each cache lookup must verify `V_curr ⊆ V_cached`
(current DB ⊆ the one that produced the cached Sat result). For `Unsat` entries the
check is `V_curr ⊇ V_cached` (current DB ⊇ the one under which unsat was proven).
Encoding either check as a clause-set hash or version vector adds O(#clauses) overhead
per lookup — comparable to just re-running the probe. `Unsat` sub-node hits are also
rare on EL-ish SIO/ORE, where unsat is decided at the root by the saturation closure.

### 4.5 FP-soundness gate (mandatory before any implementation)

Any future implementation must pass:
1. **Adversarial review (opus-level):** clause-version dependency proof for every reuse
   site (Unsat and Sat paths separately)
2. **SROIQ fuzz:** 200 randomly generated SROIQ ontologies, cache-on vs cache-off
   verdict-identical (this is the snapshot cache's retroactive test that would have
   caught the 30+ FP)
3. **Corpus-wide closure-diff:** FP=0/MISSED=0 on galen, notgalen, sio, wine,
   ore-10908, ore-15672, shoiq-knowledge, alehif, pizza, ro, bibtex

---

## 5. Positive alternative: clause-index amortization

The **real lever** on SIO/ORE label_cache_build is amortizing the per-probe setup cost.

**What to do:** in `HyperCache::classify_labels`, build `disjoint_pairs` and
`ClauseIndexes` **once** from the base clause slice (which is fixed for all probes on a
given ontology). Expose pre-built indexes from `HyperCache`; each per-class probe copies
or COW-references them instead of rebuilding from scratch. The per-probe cost drops to
(A) a single Q-clause append + (C) search, eliminating (B) the O(#clauses) rebuild.

**Expected gain:**
- The index-build fraction of per-probe cost is ~80% on SIO/ORE (from the setup-dominance
  evidence above)
- SIO 32T label_cache_build: 922 ms → ~184 ms (ceiling: 5× reduction on that component)
- SIO 32T total wall: 2.46 s → ~1.7 s (label_build cut + tier_walk unchanged)
- ORE 32T total wall: 0.32 s → ~0.13 s

**Soundness:** trivially FP-free. The indexes are read-only derived data; no verdict
is cached. Note that `HyperEngine::new` already takes `&'c [DlClause]` (a borrow, not
owned); the `clone()` at `lib.rs:1086` exists solely to append the one Q-clause
(`q ⊑ c`) before passing to the engine. So the fix is: build `ClauseIndexes` and
`disjoint_pairs` once from the base clause slice inside `HyperCache`; per probe, start
from those pre-built indexes and apply only the Q-clause's index delta (one entry in
`x_trigger[q.index()]`). The 5× label_build estimate is an **unverified ceiling** —
if the clone itself (not index-build) dominates the 80%, the gain will be smaller; a
benchmark before+after the refactor is required.

**Implementation:** medium — refactor `HyperEngine::new` to accept pre-built indexes.
The clause-index structs (`ClauseIndexes`, `disjoint_pairs`) live in `hyper.rs:527–593`;
`HyperCache` fields (`lib.rs:977`) would hold the pre-built copies.

**This is a Track-A fix, not a Track-C fix** — it touches `owl-dl-tableau/src/hyper.rs`
and `owl-dl-reasoner/src/lib.rs` but involves no new caching semantics.

---

## 6. GO/NO-GO verdict

### 6.1 On sound label-set (un)sat caching (Track C.2 Hard)

**NO-GO.** Do not implement now.

**Reasons:**

1. **The sweep gate captured the target cost.** The pre-sweep 78,855 defined-sup sweep
   calls (99.75% of SIO's wedge cost) are now 265. The post-sweep residual is
   dominated by per-probe setup cost within `label_cache_build`, not by repeated
   sub-problem search within the wedge. The original C.2 motivation — "1585 classes ×
   50 sups each, all re-expanding the same sub-class" — no longer exists.

2. **The residual cost is in the wrong layer.** After sweep gate:
   - SIO 32T: label_cache_build=922 ms (37.5%), tier_walk=1349 ms (54.8% mostly BFS)
   - ORE 32T: label_cache_build=209 ms (65.3%), tier_walk=114 ms
   - Wedge search within label_cache_build: ~10–20% of build time (setup-dominated)
   - A fact cache addresses that 10–20% of one 37–65% component → < 10% total gain

3. **Zero hits from pair-verdict memoization.** Each `(C, D)` pair in classify is asked
   exactly once. Caching `hyper_decide(C, D)` → verdict is a no-op (no repeated pairs).

4. **Inter-probe label-set reuse requires context independence that doesn't hold.**
   Different probes run with different clause DBs (each pair appends a distinct `¬D`
   clause). Reusing a `Sat` result from one probe in another requires the current DB to
   be a subset of the cached DB — satisfied almost never in the per-pair loop.

5. **FP risk is non-trivial.** The snapshot cache's failure (30+ FP on ORE 2015) arose
   from exactly this pattern: a per-class model was reused across per-pair probes with
   different clause contexts. A label-set fact cache inherits the same FP surface for
   `Sat` reuse, with conservative dependency-tracking potentially ruling out all hits
   anyway (making the cache a no-op that adds overhead).

6. **The recurrence rate of identical label sets across probes is unverified.** An
   instrumented counter measuring "how often does the same `{L}` appear at a non-root
   node in two different probes?" does not yet exist. No implementation should be
   attempted before that counter produces a measured hit rate. (If the hit rate is < 5%,
   the cache is a no-op with overhead; if it's > 50%, the verdict should be revisited.)

**Gate for future consideration:** implement a non-caching counter
(`LabelSetRecurrenceCounter`) that tracks unique label sets seen across all probes in a
classify run and reports `recurrence_rate = 1 - unique/total`. If `recurrence_rate > 0.3`
on SIO/ORE (i.e., > 30% of node-label observations are repeats), revisit the NO-GO.
Implement only after that gate. Cost: one `HashMap<FrozenSet<ClassId>, u32>` in the
classify loop; no verdict change; trivially sound.

### 6.2 Positive recommendation

**Pursue clause-index amortization** as the Track-A fix for `label_cache_build`. Estimated
SIO 32T gain: 0.9 s → 0.18 s on the label_build component; total wall 2.46 s → ~1.7 s.
FP-free. Implementation is medium-cost (refactor `HyperEngine::new` to accept pre-built
indexes). Mandatory gates: FP=0/MISSED=0 corpus-wide; `cargo test --workspace` clean.

Wine (53.9 s at 32T, measured) is not addressable by either caching or amortization —
its cost is 8666 genuine SROIQ-hard pairs each exhausting the 25 ms deadline. Only CDCL
conflict learning or a global-model approach addresses wine.

---

## 7. Key file references

| What | File | Line(s) |
|---|---|---|
| `HyperCache::classify_labels` (per-probe entry) | `crates/owl-dl-reasoner/src/lib.rs` | 1079–1108 |
| `self.clauses.clone()` per probe | `lib.rs` | 1086 |
| `HyperEngine::new` (setup cost) | `crates/owl-dl-tableau/src/hyper.rs` | 623 |
| `build_clause_indexes` O(#clauses) | `hyper.rs` | 557–593 |
| `build_disjoint_pairs` O(#clauses) | `hyper.rs` | 602–617 |
| `ClauseIndexes` struct | `hyper.rs` | 527 |
| Defined-sup sweep with oracle gate | `crates/owl-dl-reasoner/src/classify.rs` | 1518–1647 |
| Tier walk with oracle gate | `classify.rs` | 1813–1907 |
| Snapshot cache soundness comment | `lib.rs` | 740–757 |
| `BackPropRisk` soundness gate | `lib.rs` | 743–746 |

---

## 8. Pre-sweep vs post-sweep at a glance

| Fixture | Wall 1T (pre) | Wall 1T (post) | Speedup | Dominant cost (post) |
|---|---|---|---|---|
| SIO | 490 s | 11.5 s | 42× | label_cache_build (84%) |
| ORE-10908 | 21.0 s | 2.08 s | 10× | label_cache_build (93%) |
| Wine@25ms | ~526 s (1T) / 53.9 s (32T) | ~526 s / ~55 s | ~1× | tier_walk hard-SROIQ (84%) |

The sweep-gate has already extracted the only economical caching win. The residual is
structural overhead (clause-index rebuild per probe; BFS traversal) — amenable to
engineering fixes, not semantic caching.
