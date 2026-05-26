# Model caching — implementation plan

Drafted 2026-05-26. Multi-session work; this file tracks the
design so the work survives session boundaries. Mirrors the
[`moms-plan.md`](moms-plan.md) format.

## Goal

Close the pizza default-mode gap to HermiT (currently ~6.5× behind:
rustdl 29 s vs HermiT 4.5 s). Per the diagnosis in
[`perf-2026-05-24-new-server.md`](perf-2026-05-24-new-server.md) §6
the bottleneck is the **per-pair tableau cost × 1172 timed-out
pairs**. The MOMS attempt (`moms-plan.md` §A) confirmed that
per-decision heuristics can't reduce the number of timeouts — the
search-tree explosion is structural.

Model caching is the lever HermiT uses. The idea:

> Whenever the tableau confirms that concept `C` is satisfiable
> (a completion graph with no clash), cache the satisfying labels
> at the root. Future queries of the form `C ⊓ D` start by
> assuming `C`'s cached model and only need to check whether `D`
> can be added without producing a clash. If yes, return Sat
> immediately without rebuilding the whole tree.

For pizza-shape classification (`is_subclass(C, D)` = `unsat(C ⊓
¬D)`), the same `C ⊓ ¬D_i` is tested against every `D_i` in `C`'s
candidate-parent walk. A cached model of `C` skips re-deriving
~250 binary disjunctions per probe.

## Cost / win estimate

If the cache hit rate is `h` and the cached-model probe is ~10×
cheaper than a cold tableau call (rough HermiT analog):

- Pizza wall: `29 s × (1 - h + h/10)` = 29 × 0.1 if `h ≈ 1.0`,
  29 × 0.55 if `h ≈ 0.5`. Even a 50% hit rate halves the wall.
- SIO: already 0.22 s with sat-only; full mode is timeout-bound
  same as pizza. Expect ~similar relative wins.

## Algorithm

### Cache key

The cache is keyed by the **root concept structure**, not the
exact `ConceptId`, because classify builds fresh `pool.atomic(C) ⊓
pool.not(pool.atomic(D))` for every pair. The atomic `C` is the
same `ConceptId` across pair-queries — that's our key.

Concrete key: `ConceptId` of the *first conjunct* in the test
concept (the "fixed" half). For `is_subclass(C, D)` the test
concept is `C ⊓ ¬D`, with `C` as the cached half.

### Cache value

The satisfying labels at the root of the last sat-confirmed
completion graph for that key. Specifically:

```rust
struct CachedModel {
    /// Labels asserted at the root (sorted by ConceptId for
    /// fast intersection / superset checks).
    root_labels: Vec<ConceptId>,
    /// Whether the model is "anchored" — i.e., the cached labels
    /// come from a Sat verdict and not an under-approximation.
    anchored: bool,
}
```

A `decide` call that returns Unsat or hits the deadline does **not**
populate the cache — only Sat verdicts produce reusable models.

### Lookup path

```rust
fn decide_with_cache(prepared, build, cache) {
    let test = build(&mut pool);
    // Test concept must be of shape `And([C, X])` for the cache
    // path to apply. Other shapes fall through to plain `decide`.
    let (key, extra) = match pool.get(test) {
        And([a, b]) => (*a, *b),
        _ => return decide(prepared, |p| test),
    };
    if let Some(model) = cache.get(&key) {
        // Replay `extra` against the cached model: if it can be
        // added without producing a clash, return Sat directly.
        if check_compatible(&model, extra, prepared) {
            return Ok(true);
        }
        // Replay produced a clash — fall through to full tableau.
    }
    let sat = decide(prepared, |p| test)?;
    if sat {
        // Snapshot the satisfying root labels and store under key.
        cache.insert(key, snapshot_root(...));
    }
    Ok(sat)
}
```

The `check_compatible` step is the soundness-critical part: it
must reject any model where adding `extra` would clash with the
cached labels. Conservative implementation: scan the cached
labels for `extra`'s complement; if present, declare incompatible
and fall through.

### Soundness

Two invariants:

1. **Cache hit `true` ⇒ test concept is satisfiable.** The cached
   labels were a witness for `C`; adding `extra` to that witness
   without clash extends it to a witness for `C ⊓ extra`. Sound
   iff `check_compatible` is conservative — false negatives are
   OK (we fall through to full tableau), false positives are not
   (would report unsat as sat).
2. **Cache hit `false` ⇒ no claim.** Falls through to full tableau.

The cache is **owned by `PreparedOntology`**. TBox is frozen for
the lifetime of the prepared instance, so cached models are valid
for every query against that same instance. Cross-instance cache
reuse is **out of scope** — preparing a new ontology invalidates
everything.

### Concurrency

Classify's pair-loop is rayon-parallel. The cache is
`Arc<DashMap<ConceptId, CachedModel>>`. Reads are lock-free;
writes are bucket-locked. False-positive cache misses under
contention are harmless (the worker just falls through to
tableau).

## Phases

**Phase 1 (this session):**
- Plan doc (this file).
- Cache data structure (`ModelCache`) with sound stubs.
- No integration yet — `PreparedOntology::decide` unchanged.

**Phase 2 (next session):**
- Wire `ModelCache` into `PreparedOntology`.
- Implement the `check_compatible` predicate (conservative scan
  for label-complement clashes).
- Replace `decide` with `decide_with_cache` at the classify
  call site.
- Measure: pizza wall, SIO wall, regression tests pass.

**Phase 3 (later session):**
- Smarter `check_compatible` — also replay simple rules (`apply_and`,
  `apply_concept_rules`) so the compatibility check catches more
  cases without doing a full saturate.
- Cache-size cap with LRU eviction (pizza has 99 keys max; SIO
  1585 — modest).
- Multi-conjunct test concepts (currently only `And([a, b])` shape).

**Phase 4 (further):**
- Anywhere-keyed cache: snapshot models at internal nodes too,
  for the cardinality / nominal-merge cases that share substructure
  across pair queries.

## Validation strategy

Every phase must pass:
- All ≥255 in-tree unit tests.
- 87-fixture differential corpus: zero verdict diff vs. baseline.
- Real-corpus regression (`tests/real_ontology_corpus.rs` under
  `--features real-corpus`): pizza, sio-stripped, family, RO
  unsat sets match HermiT-via-ROBOT reference.

Soundness is the highest-risk invariant. Each phase adds explicit
soundness assertions: if `check_compatible(model, extra)` returns
`true` and the cached labels + `extra` together would clash on
saturate, that's a bug — surface it loudly via `debug_assert!`.

## Acceptance criteria

- All 255 in-tree tests pass.
- 87-fixture corpus: zero verdict diff.
- Real-corpus: zero unsat-set diff vs. ROBOT-HermiT.
- Pizza default-mode wall: improvement.
- SIO default-mode wall: improvement (already 266 s; want < 100 s
  to claim the lever moved the headline).

If perf doesn't move on either pizza or SIO at Phase 2 (basic
integration), revert per the [`moms-plan.md`](moms-plan.md) §A
lesson — shipping a model caching implementation that doesn't
reduce timeouts is the same mistake as MOMS with more code.

## Open questions

- **Test-concept shape.** Pair queries build `And([C, Not(D)])`.
  Instance checks build `And([Nominal(a), Not(C)])`. Both fit the
  `And([key, extra])` pattern; classify can use the cache, instance
  can't (the nominal merge changes the root structure too much
  for a trivial compatibility check).
- **Disabling the cache.** Provide `--no-model-cache` flag for
  benchmarking and bisection? Initial pass: no — only add the
  flag if a workload regresses.
- **Negative caching.** Could we cache `unsat(C)` verdicts too?
  Yes, but they're cheaper to re-derive (closure already catches
  most). Defer until positive caching ships.
- **`PreparedOntology` lifetime.** The cache lives for the duration
  of one `PreparedOntology` instance. Classify creates one
  instance per call → cache resets between calls. This is correct
  for the classify-pair-loop but means batch-runs can't share
  models across separate classify invocations. Acceptable for now;
  cross-call sharing is Phase 4 territory.
