# Parallel EL Saturation — Design Document

**Date:** 2026-06-16
**Status:** Design only — no `.rs` edits. Profiling is live on the same crate; do NOT touch source files until profiling is complete.
**Author:** rustdl (Michel Dumontier + Claude)
**Track:** A (EL/Horn perf) from `docs/superpowers/plans/2026-06-16-konclude-parity-with-proofs.md`

---

## Motivation

rustdl's EL/Horn fast path is entirely single-threaded: `saturate()` in
`crates/owl-dl-saturation/src/lib.rs` runs as one sequential worklist loop.
The only rayon parallelism in rustdl today is the per-pair tableau loop, which
the EL/Horn fast path bypasses (the classifier reads the whole hierarchy from a
single saturation result). Concretely:

- **go-basic** (52k classes, 72k SubClassOf/EquivalentClasses axioms):
  rustdl 18.4 s vs ELK 2.1 s (8.5× behind the EL specialist that uses all cores)
- **galen** (2 748 named classes, EL + functional roles):
  rustdl ~0.59 s vs Konclude ~0.27 s (2.2× behind; small enough that
  sequential is defensible — measured per plan §perf-baseline)
- **notgalen** (~1.0 s vs ~0.27 s, 3.7×): similar

The go-basic gap, 8.5×, is too large to be pure per-core constant-factor loss.
ELK publishes near-linear scaling to ~8 cores on large EL ontologies; if rustdl's
saturator is also compute-bound rather than memory-bound, a parallel fixpoint
closes most of the gap on go-basic. 0.2's flamegraph attribution (per the plan)
is the formal go/no-go; this document designs *what to build* if attribution
confirms compute-boundedness.

---

## Part 1 — Konclude's Saturation and Classification Parallelism

### Sources examined

- `Source/Reasoner/Consistiser/CTotallyPrecomputationThread.cpp` (2714 lines)
- `Source/Reasoner/Kernel/Algorithm/CCalculationTableauApproximationSaturationTaskHandleAlgorithm.cpp` (8463 lines)
- `Source/Reasoner/Classifier/COptimizedClassExtractedSaturationSubsumptionClassifierThread.cpp`
- `Source/Scheduler/CTaskProcessorThread.h`, `CTaskProcessor.cpp`

### (a) Is the saturation fixpoint itself parallelized?

**Finding: No.** `createMarkedConceptSaturationProcessingJob` iterates over all
`CSaturationConceptDataItem*` items, calls
`extendApproximatedSaturationCalculationJobProcessing` on each, and then submits
**one** `CApproximatedSaturationCalculationJob` via a single `processCalculationJob`
call (`CTotallyPrecomputationThread.cpp:1842–1892`). The handler
`CCalculationTableauApproximationSaturationTaskHandleAlgorithm::handleTask`
(`Algorithm.cpp:243`) runs a **single nested worklist loop** over
`CIndividualSaturationProcessNode`s — no child-task spawning inside the loop
was found (absence of `createSubTask`, `addChildTask` in the ~8 k-line file). No
`QtConcurrent`, `QFuture`, or `QThreadPool` calls appear inside the saturation
task handler.

This finding is stated conservatively: the Konclude source is large and the
grep was limited to the primary handler. If deeper call-chains spawn tasks, they
were not surfaced by the traced paths. The architectural evidence points to a
sequential concept-fixpoint.

### (b) Task granularity for parallel work

Konclude uses parallelism at two levels that are **downstream** of concept
saturation:

1. **ABox individual-saturation batches** (`addRequiredSaturationIndividuals`,
   `CTotallyPrecomputationThread.cpp:1340–1410`): when `>5000` individuals exist,
   `QtConcurrent::blockingMappedReduced` parallelizes the linker-construction phase
   at ~5000-individual batch granularity. This is ABox/instance reasoning, not
   concept-TBox saturation.
2. **Post-saturation taxonomy extraction**
   (`COptimizedClassExtractedSaturationSubsumptionClassifierThread`): this
   classifier reads the precomputed saturation graph (`CPrecomputedSaturationSubsumerExtractor`)
   and infers direct-subsumption from subsumer-count comparisons (`isDirectSubsumer`,
   line 181). It does NOT dispatch per-class sat-tests to a thread pool on the
   pure-EL path — no `calculateJob` or `processCalculationJob` calls appear in the
   575-line file. The `QThreadPool` parallelism in the broader
   `CSubsumptionClassifierThread` framework (used by non-EL paths that do run
   satisfiability tests) is not invoked by the saturation-extraction classifier
   for pure-EL ontologies. **This is a negative result for the "Konclude parallelizes
   per-class tests on EL" hypothesis** — Konclude's EL fast path appears to run
   mostly single-threaded through both the fixpoint and the taxonomy extraction.

`mConfMaxTestParallelCount` is set to `processorCount * multiplier`
(`CTotallyPrecomputationThread.cpp:80–90`), governing how many parallel
precomputation jobs the thread queues; this controls *individual* ABox saturation
batches, not the concept-TBox fixpoint or EL taxonomy extraction.

### (c) Scheduler/work-distribution + synchronization model

Konclude's scheduler is an event-driven, actor-style model (`CTaskProcessorThread`,
`CTaskScheduler`): each `CTaskProcessorThread` owns a local task queue; the
`CPrecomputationThread` coordinates via `Qt` signals/events (not shared memory).
Tasks communicate via callback events
(`CSaturationPrecomputationCalculatedCallbackEvent`); the precomputation thread
receives completion events and enqueues follow-on work. Synchronization is
mostly via Qt event delivery, not fine-grained locks or atomics inside the
saturation worklist.

### (d) EL path specifically

For pure-EL ontologies Konclude runs `CPrecomputedSaturationSubsumerExtractor`
post-saturation (`COptimizedClassExtractedSaturationSubsumptionClassifierThread.cpp:107`):
it reads the already-computed saturation node for each concept and extracts
subsumer counts. The downstream taxonomy construction uses subsumer-count−1 comparisons
to infer direct-subsumption edges (`isDirectSubsumer`), avoiding per-pair tableau tests
entirely. No thread-pool fan-out was found for this extraction step. The saturation
fixpoint and the subsequent taxonomy extraction both appear to be **single-threaded**
on the pure-EL path; the traced code does not show where Konclude gains its per-core
advantage over rustdl on go-basic.

### Part 1 summary

**Both the concept-TBox saturation fixpoint and the post-saturation EL taxonomy
extraction appear to run single-threaded in Konclude** (as traced via the main
algorithm file and the saturation-extracted classifier). Multi-core benefit in Konclude
is confirmed only for ABox/individual-saturation batches (>5000 individuals,
`QtConcurrent::blockingMappedReduced`) and for the non-EL `CSubsumptionClassifierThread`
framework that dispatches sat-tests to a pool. Neither of these applies to the pure-EL
go-basic / galen path in the forms traced.

**The implication for the 8.5× go-basic gap:** Konclude's EL speed advantage is
likely per-core efficiency (mature C++/Qt, LLVM-optimized, tight data structures
for the saturation node graph) rather than parallelism. This means the primary lever
for closing the gap may be **Task A.1 (constant-factor: allocation, index structure)**
at least as much as parallel saturation. **ELK** (which does parallelize its fixpoint
and publishes near-linear scaling) remains the reference design for a parallel
approach — Konclude's architecture here is not the model to replicate.

---

## Part 2 — ELK's Concurrent EL Classification (Reference Design)

### Sources

Kazakov, Krötzsch, Simančík:
- "Concurrent Classification of EL Ontologies," ISWC 2011.
- "The Incredible ELK: From Polynomial Procedures to Efficient Reasoning with
  EL Ontologies," JAR 2014 (Section 3, concurrent algorithm).

### Algorithm structure

ELK maps one **context** to each class `X` in the ontology. Each context
holds:
- `S(X)` — the current set of derived subsumers of `X` (monotonically growing).
- `R(X, r)` — the set of role-filler classes reached from `X` via role `r`
  (the existential fact index).
- A **local conclusion queue** (or input queue): a list of newly derived
  conclusions for `X` that have not yet been processed.
- An **active bit** (one atomic boolean): set when the context is in the
  shared work queue; cleared when a worker takes it.

The **shared work queue** holds references to activated contexts. Workers
loop: dequeue a context (set `active := false`), drain its local conclusion
queue (processing each pending derivation by firing rules and generating
cross-context conclusions), then idle if the queue is empty.

Cross-context conclusions (e.g., deriving `Y ⊑ H` while processing `X`'s
rule CR5 firing) are **enqueued into `Y`'s local conclusion queue**, not
written directly. After enqueuing, if `Y`'s active bit is `false`, it is
set to `true` and `Y` is added to the shared work queue.

This design means:
- **At most one worker processes `X`'s local conclusion queue at a time.**
  This is the key invariant: `S(X)` and `R(X, r)` are only mutated by the
  worker that currently holds `X`. No per-context lock is needed for reads
  during rule firing (only the holder writes). Cross-context enqueue only
  requires the active bit to be an atomic compare-and-swap.
- **Shared state** is minimal: (1) the shared work queue (concurrent deque),
  (2) per-context active bits (one `AtomicBool` per class). Per-context subsumer
  sets are owned and written by a single worker at a time.
- **Monotone convergence**: subsumer sets and fact sets are add-only. Any
  interleaving of rule applications converges to the **same least fixed point**.
  There is no ordering dependence on conclusions.

### Termination detection

The fixed point is done when the work queue is empty AND no worker is
processing any context. ELK uses an atomic counter of in-flight workers
plus the work queue: when both reach zero simultaneously, saturation is
complete. A naïve "queue empty = done" would terminate prematurely (a worker
might be generating conclusions that will re-activate contexts). The standard
technique: each worker increments an "active worker" counter on dequeue and
decrements it on context re-quiescence; termination requires counter = 0 and
queue empty.

### Why it scales near-linearly (to ~8 cores, then tapering)

- Most rules for context `X` only read other contexts' `S(Y)` (read-only)
  and write into `X`'s own state (no lock needed) or enqueue into another
  context's input queue (one CAS on the active bit).
- No global lock; the only shared data structure under write contention is
  the work queue, which is a concurrent deque with scalable append.
- Tapering after ~8 cores: contention on the work queue and false-sharing of
  active bits; ontologies with deep dependency chains serialize even with
  many workers.

### Key difference from the ELK-as-described approach

The JAR 2014 paper describes the *algorithm*; ELK's implementation uses
a custom event-driven runtime (`CEWorker`) with per-worker input queues
(context-to-worker routing) to further reduce work-queue contention at
high core counts. The core semantic invariant — single-owner per context at
a time, conclusions routed as messages — is the essential structure to replicate.

---

## Part 3 — Designing rustdl's Parallel EL Saturation

### 3.1 Current data structure analysis

rustdl's `WorklistEngine` (the only mutable state during fixpoint) holds:

| Structure | Type | Single-writer? | Notes |
|-----------|------|---------------|-------|
| `subsumers.subsumers[c]` | `Vec<FixedBitSet>` | No — `process_subsumer(c,d)` writes `(x,d)` for ALL `x` that had `c` as subsumer | Central race surface |
| `subsumed_by[d]` | `Vec<FixedBitSet>` | No — `record_subsumer(c,d)` writes `subsumed_by[d].insert(c)` (the **superclass row**, not the active context); any worker processing an edge into `d` writes to that row | Symmetric race: **any popular class `d` (owl:Thing, GO:molecular_function) becomes a hot write row** |
| `facts`, `seen_facts` | `Vec`, `HashSet` | Single writer today | Lock or per-context needed |
| `facts_by_sub[c]` | `Vec<Vec<usize>>` | No — Phase 2d propagates to all subs | Same race |
| `facts_by_target[c]` | Same | Same | Same |
| `todo_subsumer/fact/unsat` | `VecDeque` | Single writer today | Need concurrent queue |
| `merged_atom_sets` | `HashMap<(ClassId, RoleId), BTreeSet>` | No | Phase 2a merge state |
| `tseitin_runtime` | `TseitinAllocator` | No — `introduce_runtime_synthetic` grows the universe | **Dynamic universe: the blocker** |

The fundamental incompatibility with a naive parallel approach: **`process_subsumer(c,d)`
writes into MANY classes' bitsets in one call** (all x with x ⊑ c, all facts inherited by c,
etc.). This is the inverse of ELK's local-write design. ELK writes only into the
current context's state and *routes* cross-context conclusions as messages; rustdl
performs the transitive-closure propagation eagerly in place.

### 3.2 The structural blocker: dynamic universe growth

`introduce_runtime_synthetic` (called from Phase 2a functional-witness-merge in
`process_fact`) can **grow the class-id universe mid-fixpoint**, resizing every
`Vec<FixedBitSet>` and every `Vec<Vec<_>>` per-class index. This happens when two
sub-role facts arrive at the same (sub, R_f) pair and the merged body has not been
seen before. Two concurrent workers deriving the same body `{A, B}` must receive the
**same** synthetic class id (or the closure forks → MISSED, violating byte-identity).
The dedup map `tseitin_runtime.by_body: HashMap<Vec<ClassId>, ClassId>` plus the
size-growth of all per-class arrays are the atom of the problem: this requires
serialization.

### 3.3 The gate that removes the blocker: static-universe fragment

**Key observation:** `introduce_runtime_synthetic` fires ONLY from Phase 2a
(functional-role witness-merge: `merged_atom_sets` path in `process_fact`,
lines 871–973 of `lib.rs`) and not from any of the told/CR5/role-chain/domain rules.
The static class universe (fixed after `collect_el_rules`) is sufficient for:
- All told subsumptions, conjunctive triggers, and existential facts/triggers.
- CR5 existential propagation and role hierarchy.
- Chain rules and transitivity.
- Domain axioms.
- Bot propagation.
- Disjointness → unsat.
- Nominal ABox reach (NomKeys allocated during `collect_el_rules`, not at runtime).
- MaxKey and ForallKey synthetics (also allocated during `collect_el_rules`).

Runtime synthetics are only needed for the Phase 2a functional-merge rule.

**go-basic has NO functional roles.** Confirmed: `grep -c "FunctionalObjectProperty"
ontologies/real/go-basic.ofn` → 0. go-basic is pure EL+ (subclass, equivalent,
existential, role hierarchy, transitivity/chains, domain/range) with a static
class universe through the entire fixpoint. **This is the target ontology for the
8.5× gap.**

**galen HAS 150 functional roles.** Confirmed: galen's Phase 2a rule fires
at runtime; the universe grows. galen runs in ~0.59 s sequentially — the 2.2×
gap against Konclude may well be per-core constant-factor rather than
parallelism. Even if galen's 2.2× gap is addressable, the structural complexity
of parallelizing the dynamic-universe engine is not justified by 0.59 s.

**Design decision: gate the parallel engine on the static-universe fragment.**
At fixpoint entry: if `functional_supers_of` is non-empty for any role
(equivalently, if `Phase 2a can fire`), fall back to the existing sequential engine.
Otherwise use the parallel engine. This condition equals
`rules.functional_roles.is_clear()` (computed in `collect_el_rules`). The condition
is checkable in O(1). No per-run cost.

This gate covers go-basic and all pure-EL ontologies. The correctness argument is
clean: with no functional roles, `merged_atom_sets` is never touched, `introduce_runtime_synthetic`
is never called, the class universe is exactly `num_total_classes` at `seed()` time
through the end of `run()`.

Nominals (NomKey, MaxKey, ForallKey) are allocated during `collect_el_rules`,
before the gate — they are static and do not affect the gate condition.

### 3.4 Data structure adaptation for the static-universe parallel engine

The parallel engine for the static-universe fragment needs:

#### Subsumer bitsets

Replace `Vec<FixedBitSet>` (one bitset per class) with a **2D bit matrix** where
each row is padded to a machine-word boundary. Under the static-universe gate the
total size is `N × ceil(N/64)` 64-bit words, fixed at construction. Each word is a
`std::sync::atomic::AtomicU64`. Row `c` is exclusively owned by whoever is processing
context `c` — under the ELK single-owner invariant, no two workers write to the
same row concurrently, so atomic stores to individual words use `Relaxed` ordering
(the happens-before relationship is established by the work-queue dequeue, which
uses at least `Acquire`). Other workers may read row `c` while processing their own
contexts (`contains(c, d)` reads); these are safe monotone reads (bits only set,
never cleared) — a `Relaxed` load suffices for correctness.

For go-basic: N = ~52k classes (post-Tseitin, but no runtime synthetics). Matrix
size = 52000 × 813 words × 8 bytes ≈ **338 MB**. This is large but within
workstation range. Note: the current sequential engine already allocates this
matrix (as `Vec<FixedBitSet>`) — the parallel version does not add heap.

Alternative if memory is a concern: **roaring bitmaps** or **compressed sparse rows**
(read-side scanning is heavier but compresses GALEN-style dense subsumers). The
flat bitset is the lowest-friction first step.

#### Work queues and context activation

Replace the three `VecDeque`s with:
```
active_contexts: crossbeam_queue::SegQueue<(EventKind, EventPayload)>
```
where `EventKind` is one of `Subsumer(ClassId, ClassId)`, `Fact(FactId)`,
`Unsat(ClassId)`. Alternatively, per-context local queues with an
`AtomicBool active[N]` bit (pure ELK style). The per-context activation bit design
has better cache locality on large N; the global SegQueue is simpler to implement.

**Recommended for a first parallel pass:** a single shared `crossbeam_queue::SegQueue`
(wait-free multi-producer multi-consumer bounded queue). Workers pop events, apply
the corresponding rule, and push any derived events back. The queue is already in
scope (crossbeam is a transitive dep through rayon); if not, `dashmap` is in Cargo.toml
and `dashmap::DashMap` provides a sharded map. The `crossbeam` suite is the canonical
Rust parallel data structure library.

`rayon::scope` with a dynamic task spawn is **not** the right primitive here: rayon's
scope expects a static task graph; the fixpoint generates tasks dynamically (new events
from rule applications). Use `rayon::ThreadPool` + a shared concurrent queue instead:

```rust
let pool = rayon::ThreadPoolBuilder::new().num_threads(N).build()?;
pool.install(|| {
    rayon::scope(|s| {
        for _ in 0..N {
            s.spawn(|_| worker_loop(&queue, &state));
        }
    });
});
```

where workers drain the queue and call `process_event(&state)` which returns newly
derived events, pushed back to the queue. Each context's subsumer row is owned during
its processing by the worker that holds the event for that context.

#### Per-context single-owner invariant

To preserve the ELK invariant (at most one worker mutates context `C`'s state at a
time), add `active_bit: AtomicBool` per class (a flat `Vec<AtomicBool>`). The rule
`process_subsumer(c, d)` writes to subsumer row `c` and also to all `x` with `x ⊑ c`
(the transitivity-backward direction). The backward transitivity write is the
complication: it violates single-context ownership because it writes to other classes'
rows.

**Resolution:** in the parallel engine, **do not perform eager backward-transitivity
propagation during `process_subsumer`.** Instead, when `(c, d)` is recorded, push
`(x, d)` events for all `x` currently in `subsumed_by[c]` *into the work queue* (as
new `Subsumer(x, d)` events). Those events will be processed sequentially by whoever
holds context `x`. This converts the eager in-place write to a message-passing step —
exactly the ELK approach.

**Important caveat:** `record_subsumer(c,d)` also writes to `subsumed_by[d].insert(c)`
(the **reverse index**, keyed on the superclass `d`). This write happens under
context `c`'s ownership, but it touches a **foreign row** — `d`'s row — that other
workers may simultaneously write (via other edges into `d`) or read (via
`subs_of_class(d)` in their own processing). This is NOT eliminated by message-passing
and requires `subsumed_by` to be an `AtomicU64`-word matrix (same as `subsumers`).
For popular superclasses — `owl:Thing`, top-level GO terms like `GO:molecular_function`,
`GO:biological_process` — all 52k go-basic classes eventually propagate an edge into
these rows, making them **hot write targets** (`fetch_or` contention). This is a
fundamental scaling limiter for the reverse index and cannot be fully avoided by
message-passing alone. It tempers the expected speedup for Option A (not just Option B)
on dense lattice ontologies. See §3.8.

Similarly, `process_fact` Phase 2d inheritance (push fact to all subs of `fact.sub`)
must become queued `Fact` events for each sub, not recursive `push_fact` calls.

#### Facts array

In the static-universe engine, replace `Vec<ExistentialFact>` with an **atomic
append-only arena**: a `Mutex<Vec<ExistentialFact>>` or a lock-free growable
array. Because `seen_facts: HashSet<(ClassId, RoleId, ClassId)>` requires
checked insert (dedup), replace it with `DashMap<(ClassId, RoleId, ClassId), ()>`
(already in Cargo.toml). `DashMap::entry().or_insert()` provides the
try-insert-only-if-absent pattern needed for seen_facts.

`facts_by_sub[c]` and `facts_by_target[c]` become per-context owned structures
(written only by the worker holding context `c`'s event), so they can remain
`Vec<Vec<usize>>` — but the usize index into the global fact arena must be agreed
upon at insert time. This is the motivation for an atomic counter:

```rust
// Global facts arena
facts: Arc<Mutex<Vec<ExistentialFact>>>  // or a concurrent arena
seen_facts: DashMap<(ClassId, RoleId, ClassId), usize>  // value = index
```

A simpler alternative for a first pass: keep the entire `push_fact` logic in a
mutex-protected "fact registrar" called by workers; dedup inside the lock; only lock
briefly to register a new fact and get its index. Since `seen_facts.contains`
(the fast-no-op path) is already handled by the DashMap's per-shard read lock, the
common case (already-seen triple) is nearly lock-free.

### 3.5 Two-option comparison

| Approach | Description | Effort | Scaling | Risk |
|----------|-------------|--------|---------|------|
| **Option A: ELK-style per-context restructure** | Rewrite `process_subsumer/fact/unsat` to use per-context local queues + active bits; convert all eager backward-propagation writes to enqueued messages; workers over shared `SegQueue`; `AtomicBool[N]` active bits. Gated on static universe. | High (1–2 wks; touches all three `process_*` methods) | Near-linear to ~8 cores on large static EL | Correct-by-construction for the gate; preserves FP=0/MISSED=0 by monotone-convergence argument |
| **Option B: Sharded global queues** | Keep global work queues; replace `Vec<FixedBitSet>` with `AtomicU64` matrix; use N rayon workers draining a shared `SegQueue`; `DashMap` for `seen_facts`; `Mutex` around `push_fact`'s registration step. Gated on static universe. | Medium (1 wk); smaller rewrite surface | Moderate (contention on `seen_facts` DashMap + backward-propagation writes); likely 3–4× not near-linear | More contention from eager backward-transitivity (workers fight to write into the same classes' rows during large transitive closures) |

**Recommendation: Option A.** The per-core contention argument is decisive on large
ontologies with wide transitive closures (go-basic has heavy chain axioms). Option B's
eager backward-transitivity writes create hot rows (highly-subsumed classes like `owl:Thing`
become bottlenecks). Option A's message-passing model eliminates the write-contention.

Option B is a valid lower-effort prototype to validate the gate and concurrency
machinery before investing in Option A's full restructure.

### 3.6 Soundness and completeness invariants

**The parallel closure equals the sequential closure** under the following conditions,
both provable for the static-universe gate:

1. **Monotone accumulation.** All three event types add facts (bits set, facts
   appended, `unsatisfiable` bits set); nothing is ever removed. Under monotone
   semantics, any interleaving of rule applications converges to the same least
   fixed point. This is a standard result (see ELK JAR 2014 §3 Theorem 1).

2. **Correct termination detection.** The parallel engine terminates when the work
   queue is empty AND all workers are idle (no in-flight events). Implemented via
   an atomic `active_worker_count: AtomicUsize`; a worker increments before taking
   an event and decrements after pushing all derived events. Termination: queue
   empty AND counter = 0 (checked under a brief barrier). A wake-up mechanism
   (condvar or rayon scope completion) fires when both conditions hold.

**Races that would break the invariant — and how they are excluded:**

| Race | Mechanism | Resolution |
|------|-----------|-----------|
| Two workers write bits into `subsumers[c]` simultaneously | Eager backward-transitivity | Eliminated in Option A (message-passing); in Option B, each word is `AtomicU64` with `fetch_or` (CRAM-compatible; bitset sets are commutative) |
| Premature termination (event in flight when counter = 0) | Worker decrement before push | Fix: worker pushes all derived events before decrementing counter |
| `seen_facts` dedup race (two workers register the same triple) | `HashSet::insert` is not thread-safe | Replaced with `DashMap::entry().or_insert()` (atomic per-shard) |
| `tseitin_runtime.by_body` dedup (two workers allocate same synthetic) | Phase 2a only | Excluded by static-universe gate (`functional_roles.is_clear()`) |
| `subsumed_by[d]` written while another worker reads it | `record_subsumer(c,d)` writes `subsumed_by[d].insert(c)` where `d` is the **superclass** (a foreign row, not the active context `c`'s row); a concurrent worker processing a different edge into `d` writes the same row | Both options: make `subsumed_by` an `AtomicU64`-word matrix identical to `subsumers` (already mandated for Option A's consistent atomics; Option B requires the same). The write is `fetch_or` — commutative, monotone, safe under `Relaxed` ordering. See hot-row caveat in §3.8. |

**Byte-identical output requirement:** the output `Subsumers` bitset is a set of
(class, class) bits. Sets are order-independent by definition; any interleaving of
add operations produces the same final set. The `is_unsatisfiable` bits are similarly
a monotone set. Therefore **byte-identical output is guaranteed by monotone convergence**
without requiring deterministic scheduling, provided all derived implications are
eventually processed (termination detection ensures this).

### 3.7 Interaction with Track B proof recording

Track B adds a side-table `ProofTrace: HashMap<(ClassId, ClassId), ProofNode>` mapping
each derived subsumer pair to the rule + premises that produced it
(`docs/superpowers/specs/2026-06-16-saturator-proof-recording-spec.md`).

Under parallel saturation, the same `(c, d)` pair may be derivable via multiple paths
(e.g., transitivity from two different intermediate classes). A parallel engine will
record **whichever derivation wins the CAS** — the proof is still valid (the premises
exist, the rule is correct) but non-deterministic across runs. This is acceptable for
correctness (a proof is a proof) but breaks byte-identical proof traces.

**Recommendation: `RUSTDL_PROOF=1` forces single-thread.** The plan already sequences
Track A before Track B ("Track B goes onto the stabilized loop"). Proof recording is an
opt-in diagnostic mode; the performance of proof extraction is not latency-critical.
Pinning to single-thread when proof recording is active is a one-line gate in `saturate()`:

```rust
if cfg.record_proofs || !rules.functional_roles.is_clear() {
    // sequential engine (handles proof recording and dynamic-universe ontologies)
    engine.run_sequential_with_recording(cfg);
} else {
    engine.run_parallel();
}
```

This isolates proof recording from the parallel engine entirely: no concurrent map
overhead in the proof side-table, no non-determinism in proofs.

### 3.8 Expected speedup: honest conditional assessment

**0.2 attribution is the formal go/no-go** (per the plan). The parallel engine is only
worth building if attribution shows the fixpoint itself is compute-bound, not
parse/intern/output-bound.

Conditional assessment assuming compute-bound fixpoint on go-basic:

| Ontology | Class count | Fragment | Sequential | ELK parallel | Expected rustdl parallel |
|----------|-------------|---------|-----------|---------------|--------------------------|
| go-basic | ~52k | Pure EL (static universe) | 18.4 s | 2.1 s (8 cores) | **2–4× speedup** at 8 cores, ~5–9 s; closes some of the gap vs ELK but likely not to constant-factor (see hot-row caveat below) |
| galen | ~2 748 | EL + functional roles | ~0.59 s | N/A (sequential gate) | No change — sequential path preserved |
| notgalen | ~7k | EL + functional roles | ~1.0 s | N/A | No change |
| wine | ~700 | SHOIN (nominal) | ~412 s | N/A (tableau path) | No change |

**Why not near-linear for go-basic?** Three sources of overhead vs ideal:
1. **Reverse-index hot rows.** `record_subsumer(c,d)` writes `subsumed_by[d].insert(c)`
   where `d` is the superclass. For near-root classes (all 52k go-basic classes ultimately
   subsume `GO:molecular_function`, `GO:biological_process`, `owl:Thing`), every worker
   derives edges into the same few `subsumed_by[d]` rows — `AtomicU64::fetch_or`
   contention is unavoidable in both Option A and B. This is a **structural limiter on
   the reverse index** independent of the backward-transitivity messaging design.
2. **Wide backward-transitivity propagation.** Each new `(c,d)` edge generates
   `|subs_of_class(c)|` new events. In Option A these go on the work queue; in Option B
   they are eager writes. In either case the total event volume is `O(|closure|)`
   which for go-basic is large. Queue throughput becomes the bottleneck at high core counts.
3. **Per-core constant-factor loss vs ELK.** ELK is a JVM-native binary with 15+ years
   of JIT optimization on the ELK algorithm; rustdl uses HashMap-based trigger lookup
   while ELK uses pre-built inverted index arrays. Task 0.2 will quantify how much of
   the gap is per-core vs parallelism — if per-core dominates, Track A.1 has larger ROI.

**On galen/notgalen:** the 2.2×–3.7× gap against Konclude is likely
constant-factor (per-core loop cost) rather than parallelism, because:
(a) galen/notgalen complete in <1 s sequentially — the overhead of rayon thread-pool
startup + work-queue synchronization at <100 ms task granularity would yield worse
scaling than at the 18.4 s go-basic scale;
(b) both hit the sequential fallback anyway (functional roles present).

**If 0.2 shows parse/output dominates:** parallelizing the fixpoint gives no benefit;
the correct lever is faster parsing (horned-owl bottleneck) or faster output
serialization, not a parallel engine.

### 3.9 Effort estimate and risk

| Option | Estimated effort | Primary risk | Fallback |
|--------|-----------------|-------------|---------|
| Option B (sharded-global, lower fidelity) | ~3–5 days: replace `seen_facts` with DashMap, bitset with AtomicU64, add rayon workers | Hot rows from backward-transitivity on dense lattices; contention scaling cap ~3–4× | Sequential engine untouched |
| Option A (ELK-style, full restructure) | ~8–12 days: rewrite process_* to message-passing, per-context local queues, active bits, termination detection | More complex termination detection; more test surface | Sequential engine untouched |

Both options keep the existing sequential `WorklistEngine::run()` fully intact as the
fallback for the dynamic-universe gate. The parallel engine is a **new code path**,
not a rewrite of the existing one — this is the safest possible approach given the
SACRED FP=0 gate.

**Ordering with Track A.1 (constant-factor):** per the plan, Task 0.2 (flamegraph)
drives Track A. If 0.2 shows the per-core constant factor is the dominant cost,
A.1 (reduce allocations, improve index data structures) should precede parallelism —
Option B is a low-effort prototype that can be built in parallel with A.1 to validate
the gate and concurrency harness without blocking on A.1's completion.

---

## Summary

| Section | Headline finding |
|---------|-----------------|
| Part 1: Konclude | Concept-TBox saturation fixpoint is single-threaded (one `handleTask` loop, no intra-fixpoint task spawning found). Post-saturation EL taxonomy extraction is also single-threaded (no job dispatching found in `COptimizedClassExtractedSaturationSubsumptionClassifierThread`). Multi-core benefit confirmed only for ABox individual-saturation batches (>5000 individuals, `QtConcurrent`). Konclude's EL speed advantage appears to be **per-core efficiency** rather than parallelism. ELK, not Konclude, is the reference for a parallel fixpoint. |
| Part 2: ELK | Per-context ownership (single worker mutates `S(X)` at a time), shared work queue, active bits, cross-context conclusions as enqueued messages. Near-linear scaling to ~8 cores. Monotone convergence guarantees same least fixed point regardless of interleaving. |
| Part 3: Design | **Gate**: parallel engine activates only when `functional_roles.is_clear()` (static class universe; covers go-basic, all pure-EL). **Design**: ELK-style per-context message-passing (Option A, recommended) or sharded-global (Option B, lower effort). **Soundness**: monotone accumulation + correct termination detection → byte-identical output provably guaranteed. **Dynamic-universe race (Tseitin allocator dedup)**: eliminated by gate. **Reverse-index hot rows** (`subsumed_by[d]`): structural bottleneck on dense lattices; requires `AtomicU64::fetch_or` matrix (both options); tempers speedup to 2–4× on go-basic rather than near-linear. **Proof recording (`RUSTDL_PROOF=1`)**: pins single-thread to preserve deterministic proofs. **Expected speedup**: 2–4× on go-basic at 8 cores (conditional on 0.2 showing compute-bound fixpoint); no change on galen/notgalen (sequential gate). |

---

## Next steps (gated on 0.2)

1. Run Task 0.2 flamegraph on go-basic. If fixpoint `run()` is ≥ 60% of wall,
   proceed to Option B prototype first (validate gate + concurrency harness).
2. Build Option B: add `AtomicU64` bitsets, `DashMap` seen-facts, rayon workers.
   Gate condition: `functional_roles.is_clear()`. Run corpus closure-diff.
3. If Option B shows ≥ 3× on go-basic: invest in Option A's full restructure for
   better scaling on larger EL ontologies.
4. Track A.1 (constant-factor) runs in parallel with the above — independent of
   the parallel engine design.
5. After Track A stabilizes, Track B (proof recording) builds the `ProofTrace`
   side-table onto the stable sequential engine.
