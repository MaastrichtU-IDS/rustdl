# Rust-specific data structures to explore for rustdl

Companion to `outperform-hermit-plan.md`. The HermiT-parity plan
identifies *what* to optimize (lazy unfolding, model caching,
hypertableau). This doc identifies *which Rust-specific tools* to
reach for when implementing those phases.

Ordered by likely impact on the Phase B (per-call tableau speed)
budget.

## High impact

### 1. Bump arenas (`bumpalo`) for per-tableau-search allocations

The tableau's trail-based rollback is essentially "do a bunch of
allocations, then undo them all in reverse order at a checkpoint."
Bump arenas turn that into "allocate from a bump, drop the bump on
rollback" — eliminates every individual `Vec::insert` /
`Vec::remove` per label.

Phase 4's `Vec<DepSet>` parallel arrays would benefit a lot — each
`DepSet = Vec<u32>` would allocate from the arena, and tableau-end
frees them all in one stroke. The trail still needs to record
*what* logically changed for correctness, but the storage is
wholesale-freed.

Likely the single biggest "Rust-specific" win for the tableau hot
path. Pair with Phase A profiling — confirm allocation cost
dominates before refactoring.

### 2. `Rc<DepSet>` / `Arc<DepSet>` for refcounted sharing

After Phase 4 commit `c14ca56`, every rule derivation clones the
antecedent's `DepSet` into the conclusion. But almost every label
*added inside a branch* has `deps = [branch_id]` — the same
singleton over and over. Refcounted sharing turns "clone the Vec"
into "bump refcount." Combined with the lazy-unfolding work in
Phase B.2, this could close most of the corpus 34 → 76 ms
regression we currently pay for Phase 4 bookkeeping.

Concrete: change `DepSet` from `Vec<u32>` to `Arc<[u32]>` (or
`Rc<[u32]>` if we stay single-threaded inside a tableau). The
`union` helper would still allocate a new `Arc<[u32]>` for the
result, but the inputs are no-cost references.

### 3. Roaring bitmaps (`roaring` crate) for the saturation closure

We currently use `FixedBitSet` keyed by class index. For SIO at
n=1585 that's ~25 bytes/row × 1585 = ~40 KB dense per `Subsumers`
field. For Galen-EL or SNOMED at n>50000 it's tens of MB.

Roaring is compressed by run-length + container choice and on
sparse closures (most class pairs are *not* subsumption-related)
typically wins 5–10× on memory and 2–3× on union/intersection.
Direct replacement; same API surface.

Required for SIO-scale work regardless of which other Phase B/C
item is in flight.

### 4. `lasso` for IRI interning

We have a hand-rolled `Vocabulary` using `Arc<str>` + `HashMap`.
Lasso is a battle-tested interner that gives tighter packing and
`u32`-sized `Spur` handles instead of our `Arc<str>` (24 bytes →
4 bytes per IRI reference). Modest perf win, big code-clarity
win.

## Medium impact

### 5. `Cow<[u32]>` for `DepSet` in rule propagation

Many rules pass the same `DepSet` through unchanged (e.g.
`apply_and` propagates the `And` label's deps to each conjunct
unchanged). `Cow<[u32]>` lets us borrow when unchanged, own only
when we actually union. Pairs naturally with (2) above.

### 6. `DashMap` for the concurrent saturation worklist

Lock-free concurrent HashMap. Required infrastructure for the
Phase D.1 parallel saturation worklist; pointless to evaluate
before D.1 lands.

## Easy compile-time wins

### 7. LTO + codegen-units=1 in release

Currently `[profile.release]` in `Cargo.toml` is mostly defaults.
Adding:

```toml
[profile.release]
lto = "fat"
codegen-units = 1
```

typically buys 5–15 % on inner-loop-heavy code. Slower to compile
but our `--release` builds aren't the dev loop. Free win, should
land right after Phase A profiling baseline is captured.

### 8. Profile-Guided Optimization (`cargo pgo` or manual rustc PGO)

Train on the corpus + SULO + SIO classify, then optimize. Another
5–15 % typically. Worth doing once after Phase A lands a stable
baseline.

## Worth considering (not urgent)

### 9. `petgraph` for `CompletionGraph`

Battle-tested graph library, would replace our hand-rolled
adjacency lists. Code clarity win, ~zero perf change. Decide at
Phase C.1 (hypertableau rebuild) — if we're rewriting the tableau
anyway, may as well use the standard library.

### 10. `rkyv` / `bincode` for compiled-ontology caching

Parse + convert + absorb takes seconds on SIO; classify reuses
that prep across pair queries via `PreparedOntology`, but it's
rebuilt per process. Serializing the `PreparedOntology` to disk
would let the CLI skip the front half on warm runs. Probably
100–500 ms saved per CLI invocation on large inputs.

## What *not* to explore yet

- **`SmallVec<[u32; 1]>` for `DepSet`** — likely a wash vs
  `Vec<u32>` for our access pattern (we read more often than we
  mutate, and `Vec` derefs to `&[u32]` for free). Reconsider after
  profiling.
- **`std::simd` / `packed_simd` for bitset ops** — until profiling
  shows bitset ops are a hot spot. `FixedBitSet`'s internal `u64`
  strides already vectorize fine for our scale.
- **Trait specialization (nightly)** — adds nightly dependency,
  not worth the maintenance cost for the small gain.

## Where this lands in the outperform-HermiT plan

Items **1 (bumpalo arenas)** and **2 (Rc/Arc DepSet)** are good
candidates to interleave with Phase B (per-call tableau speed).
The Phase A profile would tell us which of them matters more — if
allocation cost dominates, bumpalo first; if dep-set clones
dominate, Rc first.

Item **3 (roaring)** is required infrastructure for SIO-scale
classify regardless of which other Phase B/C item is in flight.

Items **7 + 8 (LTO + PGO)** should both land in a single
~half-session config commit right after Phase A — they're free
10–20 % across everything.

## Decision rule when picking up this list

Don't reach for any of these without Phase A profiling output to
confirm the assumption. The Phase 4 attempt-1 dead-end (semantic
branching without DDB regressed corpus 2×) and the apply_max
precise-deps dead-end (regressed corpus from 58 to 70 ms with no
measurable SULO win) are both reminders that "this should be
faster in theory" routinely loses to "the existing path is already
cache-tuned in ways profiling reveals."

Evidence-first; allocate-second.
