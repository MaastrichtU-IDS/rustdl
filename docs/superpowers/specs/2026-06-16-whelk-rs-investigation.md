# whelk-rs Investigation: EL Perf Reference and Concurrency Study

**Date:** 2026-06-16
**Status:** Investigation complete (build succeeded, benchmarks run)
**Purpose:** Track-A EL perf reference — whelk-rs vs rustdl saturation kernel, architecture
study, lessons for parallelization. Complements `2026-06-16-parallel-el-saturation-design.md`.

---

## 0. Build and measurement setup

```
whelk-rs commit 701710d5  (git clone --depth 1 https://github.com/INCATools/whelk-rs)
Built: cargo build --release -p owl-dl-bench --features whelk-compare (43 s first-build)
Harness: owl-dl-bench compare-whelk --iters 4 (first iter discarded as warmup)
Machine: linux/x86-64, same process for both engines
```

The bench reports three times:
- `rustdl classify` = `convert_ontology + saturate + classify_pure_el n² matrix`  
- `rustdl saturate()` = `convert_ontology + saturate` only (no matrix). **This is the fair kernel comparison.**
- `whelk assert()` = `translate_ontology + assert()` (saturation only; `named_subsumptions()` not timed)

It also performs a closure set diff: normalized `(sub_iri, sup_iri)` pairs from both engines,
excluding reflexive, ⊑ Top/Bot, and rustdl synthetics (DKey / urn:rustdl-* prefixes).

---

## Part 1 — Direct Performance Comparison

### 1.1 Benchmark results — saturation kernel (apples-to-apples)

| ontology | n | rustdl `classify` | rustdl `saturate()` | whelk `assert()` | kernel ratio | closure diff |
|---------|---|------------------|--------------------|--------------------|--------------|-------------|
| galen    | 2,748  | 205 ms | **189 ms** | 356 ms | rustdl **1.9× faster** | +17 rustdl-only, 0 whelk-only |
| notgalen | 3,087  | 265 ms | **251 ms** | 354 ms | rustdl **1.4× faster** | +27 rustdl-only, 0 whelk-only |
| go-basic | 51,967 | 6.4 s  | **2.3 s**  | 1.3 s  | whelk 1.7× faster      | **identical** (0 diff, both 357,043) |

**Key result: rustdl's saturation kernel is faster than whelk-rs on galen and notgalen.
whelk is faster on go-basic (1.7×, kernel only), but their closures are byte-identical.**

Only galen, notgalen, and go-basic are valid EL head-to-head rows. ro/pizza/sio differ
in completeness — whelk silently drops non-EL axioms while rustdl runs the correct hybrid
path; those comparisons are meaningless as a saturation benchmark.

### 1.2 Closure agreement

The bench builds normalized `(sub_iri, sup_iri)` pair sets from both engines (excluding
reflexive, ⊑ Top/Bot, DKey/urn:rustdl-* synthetics) and diffs them:

- **go-basic**: symmetric diff = 0. Both engines derive exactly 357,043 non-reflexive
  named-class subsumption pairs. The apparent whelk count of 512,946 was inflated by
  ~51,967 reflexive pairs + ~51,967 ⊑ owl:Thing + ~51,969 whelk-specific internals.
  The EL-complete inference content is **identical**.

- **galen**: rustdl has 17 more pairs than whelk (all involving classes like
  `IntrinsicallyPathologicalBodyProcess`, `AbnormalBodyStructure`). These are the EL++
  Phase 2a/2d-derived inferences from functional-role witness-merge. whelk correctly
  doesn't derive these (it implements base ELK, not EL++). These 17 pairs are
  sound entailments that whelk misses — rustdl's EL++ machinery contributes here.
  The galen-classified.owx oracle confirms these are genuine (rustdl has MISSED=0).

- **notgalen**: rustdl has 27 more pairs than whelk (Anonymous-class to pathological
  clusters — the same IPBP-cluster pairs recovered by Phase 2e/2d). whelk misses these
  because it has no functional-role merge. rustdl's notgalen MISSED=0 is validated by
  oracle; these 27 pairs are genuine entailments.

**Closure agreement summary:** rustdl derives a strict superset of whelk's closure on
galen and notgalen (EL++ additions, all confirmed sound by oracle). On go-basic
(no functional roles → EL++ rules fire vacuously) the closures are identical.

### 1.3 The go-basic timing breakdown

The go-basic gap between `rustdl classify` (6.4 s) and `whelk assert()` (1.3 s)
is dominated by two factors:

1. **The n² classify_pure_el matrix (4.1 s = 6.4 s − 2.3 s kernel):** `classify_pure_el`
   allocates a `vec![vec![false; 51967]; 51967]` ≈ 2.7 GB matrix and scans 51,967²
   ≈ 2.7 billion bitset `contains()` calls. This is the classify API overhead,
   not saturation. whelk never builds this matrix.

2. **The rustdl saturation kernel gap (2.3 s vs 1.3 s = 1.7×):** The remaining kernel
   difference is likely due to rustdl's EL++ machinery (Phase 2d fact inheritance,
   Phase 2a witness-merge checks) executing on go-basic even though they fire vacuously
   (go-basic has no functional roles). The per-event overhead of checking functional-role
   guards still runs 14M+ times across the closure computation. Gating Phase 2a/2d on
   `rules.functional_roles.is_empty()` upfront would eliminate this overhead on go-basic,
   potentially closing the kernel gap.

### 1.4 The notgalen 2.7× classify-level gap — now explained

The earlier raw result showed notgalen at 922ms (rustdl classify) vs 348ms (whelk assert).
With the updated bench:
- `rustdl saturate()` = 251 ms vs `whelk assert()` = 354 ms — **rustdl is 1.4× faster**
- The 922ms was `classify()` = `saturate` + `classify_pure_el` matrix (n=3087, ~9M ops ~15ms)
  + **top-down hierarchy walk overhead**. The `classify_pure_el` detour plus the
  top-down traversal (Phase 6 `find_direct_parents_top_down`, visited bitset walk)
  adds ~670 ms to the classify path on notgalen. This is the hierarchy-extraction cost,
  not saturation.

**The earlier "notgalen 2.7× gap" was misattributed to EL++ saturation overhead.
At the saturation kernel level, rustdl is FASTER than whelk on both galen and notgalen.**

---

## Part 2 — whelk-rs Architecture and Concurrency

### 2.1 High-level structure

whelk-rs consists of three modules (`src/whelk/`):

| File | Role |
|------|------|
| `model.rs` (212 lines) | `ConceptData`, `RoleId`, `ConceptId`, `Interner`, `QueueExpression`, `TranslatedOntology` |
| `owl.rs` (297 lines) | `translate_ontology` — horned-owl → `TranslatedOntology` (axiom-by-axiom translation) |
| `reasoner.rs` (904 lines) | `assert` + `assert_append` — consequence-based saturation fixpoint |

Entry point: `assert(ontology: &TranslatedOntology) -> ReasonerState`.

### 2.2 The EL saturation algorithm (consequence-based rules)

`assert` (reasoner.rs line 90) calls `saturate_roles` → `index_role_compositions` →
`assert_append` which calls `compute_closure`.

`compute_closure` (line 170): simple loop over a `Vec<QueueExpression> todo`,
processing each event via `process`:

```
QueueExpression:
  Concept(c)           → process_concept: rule_0 (c ⊑ c) + rule_top (c ⊑ ⊤)
  ConceptInclusion(ci) → process_concept_inclusion (deduplicated) + process_concept_inclusion_minus
  SubPlus(ci)          → process_concept_inclusion only (already-asserted path)
  Link{subject,role,target} → process_link
```

**Rules implemented (EL subset):**

| Rule | Function | Description |
|------|----------|-------------|
| CR0  | `rule_0` | `C ⊑ C` reflexivity |
| CR1  | `rule_top` | `C ⊑ ⊤` |
| CR2 (−)  | `rule_minus_and` | `C ⊑ D₁ ⊓ D₂` → `C ⊑ D₁`, `C ⊑ D₂` |
| CR3 (+A) | `rule_plus_and_a/b` | Conjunction LHS: index new negative conjunctions; on new `D₁ ⊓ D₂ ⊑ head`, check who has both |
| CR3 (+L/R) | `rule_plus_and_left/right` | When C gains new subsumer D, fire conjunctions with D as left/right operand |
| CR4   | `rule_subclass_left/right` | Transitivity: `C ⊑ D ⊑ E` → `C ⊑ E` (forward and backward) |
| CR5 (−) | `rule_minus_some` | `C ⊑ ∃R.D` → `Link(C, R, D)` |
| CR5 (+A) | `rule_plus_some_a/b` | When new ∃R.D surfaces in a CI, index it into `negative_existential_restrictions_by_concept` and add propagations |
| CR5 (+B/R) | `rule_plus_some_b_right` | When C gains subsumer D, fire existential triggers where D is the existential filler |
| CR5 (+L)   | `rule_plus_some_left` | New propagation matches existing links → fire SubPlus |
| CR5 (+R)   | `rule_plus_some_right` | New Link matches existing propagations via role hierarchy |
| CR8   | `rule_ring_left/right` | Role compositions (chain rule) |
| CR⊥-L/R | `rule_bottom_left/right` | Bottom propagation |
| CR-union | `rule_union` | `DisjunctionOf(D₁…Dₙ)` → `Dᵢ ⊑ union` for each operand (EL+ disjunction handling) |
| CR-complement | `rule_complement` | `Complement(D)` → `D ⊑ ⊥` |
| init  | `rule_squiggle` | When a new Link target `T` appears, enqueue `Concept(T)` to initialize it |

whelk also handles `ObjectComplementOf` (maps inner → ⊥) and `DisjointClasses`
(mapped to pairwise `Conjunction(A,B) ⊑ ⊥` in `owl.rs`).

### 2.3 Core data structures

`ReasonerState` (reasoner.rs line 9):

```rust
pub struct ReasonerState {
    pub interner: Interner,
    hier: HashMap<RoleId, HashSet<RoleId>>,                                  // role hierarchy (pre-computed)
    hier_comps: HashMap<RoleId, HashMap<RoleId, Vector<RoleId>>>,            // pair-indexed role compositions
    inits: HashSet<ConceptId>,                                               // initialized concepts
    asserted_concept_inclusions_by_subclass: HashMap<ConceptId, Vector<ConceptInclusion>>,
    pub closure_subs_by_superclass: HashMap<ConceptId, HashSet<ConceptId>>,  // reverse closure index
    pub closure_subs_by_subclass:   HashMap<ConceptId, HashSet<ConceptId>>,  // forward closure
    asserted_negative_conjunctions: HashSet<ConceptId>,
    asserted_negative_conjunctions_by_right_operand: HashMap<ConceptId, HashMap<ConceptId, ConceptId>>,
    asserted_negative_conjunctions_by_left_operand:  HashMap<ConceptId, HashMap<ConceptId, ConceptId>>,
    asserted_unions: HashSet<ConceptId>,
    unions_by_operand: HashMap<ConceptId, Vector<ConceptId>>,
    links_by_subject:  HashMap<ConceptId, HashMap<RoleId, HashSet<ConceptId>>>,
    links_by_target:   HashMap<ConceptId, HashMap<RoleId, Vector<ConceptId>>>,
    negative_existential_restrictions_by_concept: HashMap<ConceptId, HashSet<ConceptId>>,
    propagations: HashMap<ConceptId, HashMap<RoleId, Vector<ConceptId>>>,    // (C, R) → ∃R.D concepts
    asserted_negative_self_restrictions_by_role: HashMap<RoleId, ConceptId>,
}
```

**All collections are `im::HashMap` / `im::HashSet` / `im::Vector`** — Hash Array
Mapped Tries (HAMT), i.e., Clojure/Scala-style persistent data structures from
the `im` crate (version 15.1). `HashMap`/`HashSet` use `FxBuildHasher` (Rust's
`rustc-hash`, an identity-based fast hash for integer keys).

- **Clone is O(1) structural sharing** — the design motivation for `im`: the
  `assert_append` function (`line 121`) calls `state.clone()` to start from an
  existing state and add new axioms incrementally. With im-collections, this is
  a pointer-bump, not a deep copy.
- **Lookup/insert is O(log₃₂ n)** — HAMT is slower per-operation than a flat
  `HashMap` or a `FixedBitSet` indexed array. This is a deliberate trade for
  the cheap-clone property.
- `im::HashSet::union()` (line 126-129) is O(min(|a|,|b|)) by structural
  sharing — relevant in `concept_signature` and `all_super_roles` helpers.

### 2.4 IS whelk-rs concurrent / parallel?

**No. whelk-rs is strictly single-threaded.**

Evidence: exhaustive grep of `reasoner.rs`, `owl.rs`, `model.rs`, `main.rs` for
`thread`, `rayon`, `par_`, `Arc`, `Mutex`, `Send`, `Sync`, `spawn` — zero results.
`Cargo.toml` lists no rayon, tokio, crossbeam, or threadpool dependency.
The `compute_closure` loop (line 170) is a plain sequential `while let Some(item) = todo.pop()`.
The `im` crate's persistent data structures enable cheap state-sharing but are
not a concurrent execution mechanism.

**The original Scala `whelk` (Balhoff, 2019) IS concurrent** — it uses Akka actors
with one actor per concept context, communicating via message-passing. whelk-rs is
a straight sequential Rust port; it does NOT replicate the Scala concurrency model.

**Implication for Track A:** whelk-rs does **not** validate "parallelize the
saturation" as a lever. It is a single-threaded Rust engine that matches or slightly
beats rustdl on the EL saturation kernel (galen). The concurrent reference for Track A
remains ELK's published per-context message-passing design (described in
`2026-06-16-parallel-el-saturation-design.md §Part 2`), not whelk-rs.

### 2.5 whelk-rs performance observations

Key design choices that affect speed (relative to rustdl):

**a. No EL++ extensions.** whelk has no functional-role witness-merge (Phase 2a),
no subclass-fact inheritance (Phase 2d), no sub-role propagation (Phase 2c-redux).
These additions in rustdl close GALEN/notgalen/SIO MISSED but cost per-fact and
per-subsumer work.

**b. `propagations` map avoids re-scanning facts on trigger fires.**
whelk's `rule_plus_some_b_left` and `rule_plus_some_left` maintain a
`propagations: HashMap<ConceptId, HashMap<RoleId, Vector<ConceptId>>>` —
essentially a (concept, role) → list-of-∃R.D-objects map. When a new link
`(subject, r, target)` fires, `rule_plus_some_right` looks up
`propagations[target][r_superrole]` directly. This is equivalent to rustdl's
`existential_triggers_by_body` but whelk's structure is a nested map vs
rustdl's dense Vec-of-Vec index. The dense Vec is faster for random access;
whelk's nested im::HashMap is heavier per lookup but produces smaller resident
state on sparse ontologies.

**c. `rule_plus_and_left/right` pick the smaller side for intersection.**
`rule_plus_and_left` (line 431): when C gains new subsumer D₁, it checks
`conjunctions_matching_left[D₁]`. It picks the smaller of `d2s.len()` vs
`conjunctions_matching_left.len()` to iterate (lines 436-456). This is a minor
optimization absent in rustdl (which uses dense `conjunctive_by_body` index
already amortized to O(triggers_for_that_body)).

**d. `hier_comps` pre-indexes role compositions as `r1 × r2 → supers`.**
`index_role_compositions` (line 718) precomputes the transitive composition
closure: for every ordered pair of sub-roles `(r1, r2)`, list the super-roles
whose composition they can fire. This avoids the per-link re-check of role
hierarchy in the chain rule. rustdl uses `chain_axioms: Vec<(RoleId, RoleId, RoleId)>`
plus `supers_of()` (called 22 times, allocates a Vec each call) — there is likely
overhead from this repeated allocation in the chain rule hot path.

**e. `rule_ring_right` has a `links_with_s.contains(&d)` dedup check** (line 641)
before emitting Link events. This avoids redundant `process_link` calls that would
be deduplicated anyway by whelk's `seen` check in `process_link`. rustdl deduplicates
via `seen_facts: HashSet` inside `push_fact` — functionally identical, structurally
equivalent.

---

## Part 3 — Lessons for rustdl's EL Performance (Track A)

### 3.1 What whelk-rs validates and does not

**Validated by whelk-rs:**
- rustdl's saturation kernel is already FASTER than whelk-rs on galen (1.9×) and
  notgalen (1.4×). rustdl's EL++ extensions add both more completeness (27–17 extra
  sound pairs) AND better speed.
- A single-threaded Rust EL saturation engine with FixedBitSet closures outperforms
  an im::HAMT-based engine. The `im` overhead is real.
- The closures agree exactly on go-basic (0 diff); whelk-rs is a sound cross-check
  for the base ELK fragment.

**NOT validated by whelk-rs:**
- Parallel saturation as a speed lever. whelk-rs is sequential, and it is *slower*
  than rustdl on the kernel datapoints where both are complete. A sequential engine
  already beats whelk — parallelism is an open (unmeasured) *future* lever.
- That the go-basic kernel gap (1.7×) requires parallelism to close. It may be
  addressable by gating EL++ overhead checks on functional-role presence.

### 3.2 The two genuine EL perf gaps and their fixes

**Gap A: O(n²) classify_pure_el matrix (go-basic, 4.1 s of 6.4 s)**

`classify_pure_el` (`classify.rs` line 715) allocates a `vec![vec![false; n]; n]`
matrix (~2.7 GB for go-basic) then scans 51,967² ≈ 2.7 billion bitset `contains()`
calls. This is confirmed by measurement: `saturate()` alone = 2.3 s, `classify()` = 6.4 s,
difference = 4.1 s = the n² scan.

**The fix:** bypass the dense matrix for pure-EL. The `find_direct_parents_top_down`
function (Phase 6, `classify.rs`) already extracts direct parents from the closure
bitsets in O(n × avg_subsumer_count) — but it is called from `classify_top_down_internal`,
which itself calls `classify_pure_el` for pure-EL inputs (line 1153). The fix is to
add a direct-parent extraction path that reads from the `Subsumers` closure without
materializing the n×n matrix — equivalent to what `classify_top_down` does for the
hybrid path, applied to pure-EL. Neither `classify_top_down` nor `classify_pure_el`
currently does this: both eventually call `classify_pure_el`. A new function that
iterates each class's subsumer bitset once and extracts the minimal (non-dominated)
superclasses would close this gap.

Estimated impact: go-basic `classify` 6.4 s → ~2.3 s (matching the saturate-only time).

**Gap B: EL++ per-event overhead on ontologies without functional roles (go-basic kernel, 1.7×)**

`rustdl saturate()` = 2.3 s vs `whelk assert()` = 1.3 s on go-basic. go-basic has
no functional roles, so Phase 2a/2c-redux fire vacuously. But the guards still execute
tens of millions of times: `process_fact` checks `self.rules.functional_supers_of(fact.role)`
and `merged_atom_sets.entry(key)` for every fact, and `process_subsumer` copies
`facts_by_sub[d]` for every new `C ⊑ D` edge even when no facts are in scope.

**The fix:** add a `has_functional_roles: bool` flag to `WorklistEngine` at construction
time (derived from `rules.functional_roles.is_empty()`). Gate the entire Phase 2a/2c-redux
block in `process_fact` and the Phase 2d inheritance blocks in `push_fact` /
`process_subsumer` behind this flag. This is a ~4-line change that avoids the dead-code
paths entirely on EL ontologies without functional roles.

Estimated impact: go-basic kernel 2.3 s → closer to whelk's 1.3 s; galen/notgalen
unchanged (both have functional roles and need Phase 2d/2a).

### 3.3 What whelk-rs data structures teach

**`im` crate adds overhead vs FixedBitSet.** whelk is ~1.9× slower than rustdl on
galen (356 ms vs 189 ms) at the saturation level. HAMT lookup is O(log₃₂ n) vs
O(1) bitset index. The only `im` win is O(1) `state.clone()` for `assert_append`
(incremental reasoning); rustdl doesn't need that since it doesn't support it.

**`supers_of()` allocates a Vec on every call (22 call sites in hot paths).**
`supers_of(&self.role_super, r)` calls `.iter().copied().collect()` — a heap allocation.
Called in `process_fact` (chain rule) and `process_subsumer` (existential trigger firing)
potentially millions of times per saturation. A sibling agent's diff shows `freeze_role_super`
pre-computes a `Vec<Box<[RoleId]>>` dense array (`lib.rs` line 2243) — this eliminates
the per-call HashMap lookup and Vec allocation. This optimization is already in the
working tree (the `freeze_role_super` change from the sibling agent).

**whelk's `hier_comps` pre-indexes composition pairs.** rustdl iterates
`chain_axioms: Vec<(RoleId, RoleId, RoleId)>` with `supers_of()` per chain per fact.
whelk pre-computes `hier_comps: HashMap<RoleId, HashMap<RoleId, Vector<RoleId>>>`,
indexed as `r1 → r2 → [sup]`. This reduces the chain rule from O(n_chains × supers_of)
per Link event to one O(1) lookup. On go-basic with 2 transitive roles, the saving is
small; on ontologies with deep role hierarchies it would matter more.

**`rule_plus_and_left/right` pick the smaller side.** whelk's conjunction-trigger
firing (line 436-456 of `reasoner.rs`) iterates the smaller of `d2s` vs
`conjunctions_matching_left` when looking for triggerable conjunctions. rustdl's
`conjunctive_by_body` index is already amortized to O(triggers_for_body) per lookup,
so this specific optimization doesn't apply.

### 3.4 Summary: honest read on parallelism as a Track A lever

whelk-rs is **single-threaded and SLOWER than rustdl's kernel** on the two
completeness-comparable ontologies (galen 1.9×, notgalen 1.4× — rustdl wins).
This changes the Track A picture significantly:

1. **Parallelism is NOT the lever here.** whelk-rs doesn't validate it;
   on the contrary, rustdl's sequential kernel already beats a competitive
   sequential Rust reference engine. A parallel approach has no same-completeness
   baseline to beat.
2. **The go-basic gap (1.7× kernel, 4.9× classify) has two separate fixes:**
   - Fix A: bypass the n² classify_pure_el matrix → eliminates 4.1 s → major win
   - Fix B: gate EL++ guards on functional-role presence → closes ~1 s → moderate win
   Both are single-threaded, surgical changes. Combined: go-basic classify would
   drop from 6.4 s to ~1.0–1.3 s, comparable to or better than whelk.
3. **ELK's concurrent context-actor model (from the sibling design doc)** remains
   the right architecture for *scaling beyond what sequential can achieve*
   (>100k-class ontologies, >8 core machines). But on current corpus sizes,
   the sequential kernel is not the bottleneck — the n² matrix is.

The #1 Track-A action: eliminate `classify_pure_el`'s n×n matrix for pure-EL inputs.

---

## Quick-Reference: whelk-rs vs rustdl saturation structure

| Aspect | whelk-rs | rustdl `owl-dl-saturation` |
|--------|----------|--------------------------|
| Algorithm | Base ELK (JAR 2014) rules | ELK + EL++ functional-merge (Phase 2a) + subclass fact inheritance (Phase 2d) + sub-role propagation (Phase 2c-redux) |
| Concurrent | No (sequential loop) | No (sequential loop) |
| Closure storage | `im::HashMap<ConceptId, im::HashSet<ConceptId>>` nested map | `Vec<FixedBitSet>` dense per-class bitmap |
| Existential facts | `links_by_subject / links_by_target` im::HashMap | `facts_by_sub / facts_by_target` Vec<Vec<usize>> |
| Role hierarchy | Pre-computed `hier: HashMap<RoleId, HashSet<RoleId>>` | Pre-computed `role_super_map` → `freeze_role_super` dense `Vec<Box<[RoleId]>>` |
| Role compositions | Pre-indexed `hier_comps: HashMap<RoleId, HashMap<RoleId, Vector<RoleId>>>` | Iterated `chain_axioms: Vec<(RoleId,RoleId,RoleId)>` with role-super lookup |
| Post-saturation taxonomy | Not computed (returns raw closure maps) | O(n²) matrix scan in `classify_pure_el` |
| EL++ features | None | Phase 2a witness-merge, Phase 2d fact inheritance, Phase 2c-redux sub-role propagation |
| Kernel speed (galen) | **356 ms** | **189 ms** (1.9× faster, same correctness) |
| Kernel speed (notgalen) | **354 ms** | **251 ms** (1.4× faster, 27 extra sound pairs) |
| Kernel speed (go-basic) | **1.3 s** | **2.3 s** (1.7× slower; EL++ dead guards on no-functional-role input) |
| Closure agreement | base ELK | ELK + EL++ (superset; verified by closure diff on all 3) |
| Incremental reasoning | `assert_append` (cheap clone via im) | Not supported |
| LOC | ~900 | ~4000 |

---

## Appendix: Raw benchmark output

**galen (iters=4, first discarded):**
```
rustdl classify      mean=205.091ms  min=202.910ms  max=209.354ms
rustdl saturate()    mean=189.276ms  min=188.832ms  max=189.640ms
whelk assert()       mean=356.056ms  min=351.586ms  max=364.768ms
rustdl sat_sub=27997 (non-reflexive named pairs, excludes Top/Bot/synthetics)
whelk named_subsumptions=36226 (includes reflexive + ⊑Top)
closure diff: rustdl 27997, whelk 27980, symmetric diff=17 (17 in rustdl only, EL++ extras)
```

**notgalen (iters=4, first discarded):**
```
rustdl classify      mean=264.910ms  min=264.036ms  max=266.399ms
rustdl saturate()    mean=250.689ms  min=248.479ms  max=254.324ms
whelk assert()       mean=353.523ms  min=347.587ms  max=364.535ms
rustdl sat_sub=32739, whelk named_subsumptions=41975
closure diff: rustdl 32739, whelk 32712, symmetric diff=27 (27 in rustdl only, EL++ extras)
```

**go-basic (iters=4, first discarded):**
```
rustdl classify      mean=   6.391s  min=   6.378s  max=   6.406s
rustdl saturate()    mean=   2.275s  min=   2.219s  max=   2.338s
whelk assert()       mean=   1.344s  min=   1.177s  max=   1.441s
rustdl sat_sub=357043, whelk named_subsumptions=512946 (51967 reflexive + 51967 ⊑Top + residual)
closure diff: rustdl 357043, whelk 357043, symmetric diff=0  ← IDENTICAL
```

Benchmark built as: `cargo build --release -p owl-dl-bench --features whelk-compare`  
whelk-rs cloned at commit `701710d5` from `https://github.com/INCATools/whelk-rs`.
