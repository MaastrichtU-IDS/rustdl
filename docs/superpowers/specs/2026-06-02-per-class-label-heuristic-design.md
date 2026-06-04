# Per-class label heuristic + per-pair verify (design)

**Goal:** Cut the per-pair tableau call count by ~70-90% on non-Horn
workloads (ORE-10908, pizza, family) by adding a sound non-subsumption
pruner derived from each named class's first-found completion graph.
Targets Konclude-class wall (≤5× ratio) on SROIQ corpus, per the
2026-06-02 head-to-head doc (`docs/perf-2026-06-02-konclude-vs-rustdl.md`)
where rustdl is 17× slower than Konclude on ORE-10908/15672.

> **Outcome: SHIPPED (Phase 7).** ORE-10908 27.37 s → 19.32 s (−29%), ratio 17× → 12×. Phase 8 (commit `e31439c`) followed up by decoupling the label-cache deadline from per_pair_timeout, taking ORE-10908 to 7.48 s (ratio 12× → 4.32×). See `docs/phase7-results.md` and `docs/phase8-results.md`. Konclude-class ≤5× target achieved.

---

## Soundness contract

For each named class C, run the wedge engine ONCE in satisfiability
mode. On Sat, extract the **root node's label set** in the resulting
completion graph and store it as `labels(C): HashSet<ClassId>`.

When the orchestrator needs to test `C ⊑ D` for any D:

- **`D ∉ labels(C)`** → return `false` immediately. The completion
  graph IS a model of the ontology + C in which the C-root lacks D;
  this is a sound counterexample model for `C ⊑ D`. Skip the per-pair
  tableau call. Counts as `label_cache_pruned`.
- **`D ∈ labels(C)`** → fall through to the existing per-pair test
  (`subsumes_via_tableau`). The label could be coincidence-of-model;
  verification is necessary for soundness. Counts as
  `label_cache_pass_through`.

The verify-positives path preserves the existing soundness contract:
`subsumes_via_tableau`'s return is the authoritative subsumption verdict.
FP=0 stays guaranteed by construction.

On Unsat (C is unsatisfiable): the `LabelOracle::Unsat` variant
handles this case explicitly — the orchestrator returns `true` for
every C-vs-D test without consulting any set, since unsat classes
are vacuously subsumed by every D. No need to materialize an
"all classes" set.

On NoVerdict / timeout: skip the cache for that class — the
orchestrator falls through to existing per-pair behaviour. Sound under
the existing per-pair semantics.

## Architecture

### New wedge API

`crates/owl-dl-tableau/src/hyper.rs::HyperEngine` exposes one new
public method:

```rust
/// Run satisfiability on the given class; on Sat, return the
/// root node's label set. Used by the per-class label heuristic
/// in classify_top_down_internal.
pub fn classify_labels(
    &mut self,
    sub: ClassId,
    deadline: Option<Instant>,
) -> Result<LabelOracle, HyperError>;

pub enum LabelOracle {
    /// C is satisfiable; root-node labels (subset of atomic classes).
    Sat(HashSet<ClassId>),
    /// C is unsatisfiable in every model.
    Unsat,
    /// Deadline exceeded; no labels recorded. Orchestrator falls through.
    NoVerdict,
}
```

Implementation: re-uses existing `decide()` machinery. After the
fixpoint converges to Sat, walks the root node's `label` accumulator
and returns it as a `HashSet`. New API; existing `decide()` path
unchanged.

### Label cache (orchestrator side)

`crates/owl-dl-reasoner/src/classify.rs::classify_top_down_internal`
gains a build step BEFORE the tier-walk:

```rust
// Build the per-class label cache via PARALLEL wedge calls — one per
// named class. Each call is independent; rayon scales linearly.
// The deadline is the same per_pair_timeout already used by the unsat
// probes (classify.rs line 696-700); on NoVerdict the cache entry
// becomes None and the orchestrator falls through to per-pair.
let label_cache: Vec<LabelOracle> = (0..n).into_par_iter().map(|i| {
    let class_id = ClassId::new(u32::try_from(i).unwrap());
    let deadline = per_pair_timeout.map(|t| Instant::now() + t);
    prepared.classify_labels(class_id, deadline)
}).collect::<Result<Vec<_>, _>>()?;
```

The build cost is the same shape as a 1× pass of the existing
per-class unsat probe (already done at lines 689-727). The
implementation should FUSE the two passes: the existing unsat probe
already runs the wedge per class; extend its return type to include
the root-node label set on Sat, eliminating duplicate work. Net
cost-of-fusion: negligible (the wedge already computed the labels
internally — we just expose them).

### Orchestrator integration

`find_direct_parents_top_down` (Phase 6's HEAD location) inserts a
label-cache check before each subsumption test:

```rust
let subsumed = match label_cache.get(c) {
    Some(LabelOracle::Sat(labels)) => {
        if labels.contains(&d) {
            // Verify (might be coincidence of model)
            subsumes_via_tableau(...)?  // existing path
        } else {
            stats.label_cache_pruned += 1;
            false  // sound non-subsumption
        }
    }
    Some(LabelOracle::Unsat) => true,  // unsat C trivially subsumes anything
    Some(LabelOracle::NoVerdict) | None => {
        subsumes_via_tableau(...)?  // fall through
    }
};
```

### Counters + diagnostic

`ClassificationStats` gains:
- `label_cache_pruned: usize` — pairs the heuristic rejected (sound non-sub).
- `label_cache_pass_through: usize` — pairs where D ∈ labels(C); verification path taken.
- `label_cache_misses: usize` — labels(C) had NoVerdict or wasn't built.

Diagnostic ratio: `pruned / (pruned + pass_through)` is the effective
prune rate. Target on ORE-10908: ≥ 70%.

## Edge cases

- **C unsatisfiable**: `labels(C) = ALL` sentinel; every D check returns true.
- **C's wedge call NoVerdict**: cache misses; existing per-pair path.
- **D not an atomic class**: cache key is `(ClassId, ClassId)` — atomic only. Non-atomic D goes through existing path.
- **Equivalence (C ≡ E)**: each has its own cache entry; both verify positives independently.
- **Parallel construction race**: cache is built once, immutably; no race after build.

## Implementation surface estimate

| Component | Files | LoC est. |
|---|---|---|
| `HyperEngine::classify_labels` API | `crates/owl-dl-tableau/src/hyper.rs` | ~30 |
| `LabelOracle` enum + re-export | `crates/owl-dl-tableau/src/lib.rs` | ~10 |
| `PreparedOntology::classify_labels` wrapper | `crates/owl-dl-reasoner/src/lib.rs` | ~20 |
| Label cache build (parallel rayon) | `crates/owl-dl-reasoner/src/classify.rs` | ~30 |
| Orchestrator integration in `find_direct_parents_top_down` | `crates/owl-dl-reasoner/src/classify.rs` | ~15 |
| `ClassificationStats` fields + display | `crates/owl-dl-reasoner/src/classify.rs` + `crates/owl-dl-cli/src/main.rs` | ~10 |
| Structural canary tests | `crates/owl-dl-reasoner/src/classify.rs::mod tests` | ~50 |
| Soundness gate sweep (Phase 0 net + GALEN + ORE) | — | (measurement only) |
| **Total production code** | | **~115 LoC** |

Comparable scale to Phase 2d (148 LoC) or Phase 6 (~10 LoC). Well
within single-phase scope.

## Measurement gates

| Gate | Criterion | Action on fail |
|---|---|---|
| Phase 0 net (FP=0) | unchanged | REVERT |
| GALEN MISSED | unchanged at 0 | REVERT |
| ORE-10908 wall | ≤ 14 s (≥2× current 27 s improvement) | accept as marginal; tighten or revert per data |
| ORE-15672 wall | ≤ 15 s (≥2× current 29 s) | as above |
| pizza wall | ≤ 2 s (≥2× current 4.4 s) | as above |
| GALEN wall | within ±10% (684 s) | accept regression up to 10% if non-Horn workloads benefit |
| `label_cache_pruned / (+pass_through)` on ORE-10908 | ≥ 50% prune rate | low rate → heuristic doesn't help this workload; investigate |

The GALEN ±10% tolerance accepts that Horn workloads pay the new
cache-build cost without benefit (cache helps non-Horn). If GALEN
regresses >10%, we'd need workload-adaptive dispatch (§16 lesson:
skip the cache build for `fragment == Horn`).

## Risks + mitigations

1. **First-found model is label-sparse**: heuristic prune rate is low,
   wall doesn't improve. Mitigation: T5 measures the prune rate; if
   <30%, the heuristic isn't useful for that workload and the cache
   build is wasted. Could pivot to enumerating multiple models.

2. **Cache build cost dominates for small/cheap classifications**:
   on alehif-test (~2.9 s), per-class wedge calls × 167 classes
   could exceed the 2.9 s budget. Mitigation: workload-adaptive —
   skip the cache for ontologies with N < threshold OR pure-EL
   fragment (cheap saturator-only path already covers them).

3. **Wedge satisfiability isn't deterministic** (rayon branch
   ordering) — the cache labels could vary between runs. Same
   soundness (any model is a counterexample), but prune rates may
   wiggle ±5%. Acceptable.

4. **Equivalence-class verification overhead**: if C ≡ E, both fail
   the prune (D ∈ both labels) and both verify. Could deduplicate
   via early equivalence detection. YAGNI for first cut.

## What this DOES NOT change

- The saturator's closure (already does Horn-fragment subsumption sound + complete).
- The wedge's existing `decide()` API (used elsewhere).
- The `subsumes_via_tableau` verification path.
- The `trust_sat` env-flag semantics.
- Fragment classification (Phase 4b/4c).
- Phase 6's `find_direct_parents_top_down` walk dedup.

## Cross-references

- Head-to-head measurement that motivated this: `docs/perf-2026-06-02-konclude-vs-rustdl.md`.
- Phase 5 chain (the GALEN regression localization that taught us instrumentation discipline): `docs/phase5-{recon,walltime-probe,variance-check,downstream-probe}.md`.
- Phase 6 (most recent perf change): `docs/phase6-results.md`.
- Dead-end §2 (the per-class model approach we're NOT doing): `docs/hypertableau-dead-ends.md`.
- Dead-end §16 (workload-dependent break-even pattern to avoid): `docs/hypertableau-dead-ends.md`.
- HermiT label heuristic original: Glimm/Horrocks/Motik/Stoilos "Optimising Pseudo-Model Caching in HermiT" (the literature inspiration).
