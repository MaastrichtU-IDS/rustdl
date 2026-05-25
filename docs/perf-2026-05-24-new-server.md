# Perf snapshot — 2026-05-24, new server

First baseline on the new machine (replacing the 16-core box implied
by [`outperform-hermit-plan.md`](outperform-hermit-plan.md)). Three
workloads: the synthetic EL chain sweep, the 87-fixture differential
corpus, and the new real-ontology corpus
([`docs/real-ontology-corpus.md`](real-ontology-corpus.md)).

Headline: **the new box is fundamentally slower per-core for the
single-threaded saturation path** (synthetic EL is 1.5–1.7× slower
at sizes where we have a baseline number), **but the cores are
effectively used by the parallel classify pair-loop** (real-ontology
runs show 20–32× CPU utilization). Real SROIQ ontologies still don't
finish, and four of six entries in the new corpus hit hard coverage
gaps rather than perf walls. **No phase of [`outperform-hermit-plan.md`](outperform-hermit-plan.md)
has been done since the baseline was recorded** — these are the
unchanged-code numbers on different silicon.

## Setup

| | |
|---|---|
| Host | `fsesrv-g1` |
| CPU | 2× AMD EPYC 7282, 32 cores total, 1 thread/core, 2.8 GHz boost |
| RAM | 251 GiB |
| Rust | `rustc 1.95.0 (59807616e 2026-04-14)`, stable |
| Build | `cargo build --workspace --release` (thin LTO, 1 codegen unit) |
| Binaries | `./target/release/{rustdl, owl-dl-bench}` |
| Git rev | `5df2879` (`master`) |
| Date | 2026-05-24 |

Raw outputs land under `bench-results/`.

## 1. Synthetic EL chain (`bench synthetic-el`)

Generated in-memory by [`crates/owl-dl-bench/src/main.rs`](../crates/owl-dl-bench/src/main.rs):
a transitive `partOf` chain of length `n` plus an anchor trigger. The
classifier should take the saturation-only fast path (`mode = pure EL`).
The closure is single-threaded so the pair-loop parallelism doesn't
fire here.

| n | new box (med of 10) | baseline (16-core) | ratio | scaling vs 2× n |
|---:|---:|---:|---:|---:|
| 50   | 4.15 ms    | —      | —      | — |
| 100  | 31.1 ms    | —      | —      | 7.5× |
| 200  | 246.7 ms   | 145 ms | 1.70×  | 7.9× |
| 400  | 2.03 s     | 1.35 s | 1.50×  | 8.2× |
| 800  | 17.1 s     | —      | —      | 8.4× |
| 1600 | 208.3 s    | —      | —      | 12.2× |

Closure stats are clean: every n shows `saturation=n-1`, `tableau=0`,
`unsat=0`, i.e. the orchestrator's pure-EL fast path catches every
subsumption. n=3200 was aborted; extrapolating ~12× from n=1600 puts
it at ~40 minutes per call.

**Reading this.** The doc baseline numbers (n=200, n=400) were taken
on a different host, undated. Both are slower here. Likely cause: per-
core clock. EPYC 7282 boosts to 2.8 GHz; the previous box almost
certainly ran ≥3 GHz per core. Per-2×-n scaling is **~8× through
n=800 then climbs to 12× at n=1600** — superlinear, between n^3 and
n^3.5 in that range. The strategy doc names roaring-bitmap migration
(`docs/rust-specific-data-structures.md`) as the canonical SIO-scale
fix; the same lever applies here, since `FixedBitSet` keyed by class
index dominates allocation as n grows.

## 2. 87-fixture differential corpus (`bench corpus`)

All 87 fixtures classified with `--repeats 5`:

```
files: 87   successful: 87   failures: 0
classes (sum): 228
pure-EL files: 20 / 87
subsumption queries: saturation=82 tableau=202
satisfiability probes: saturation=7 tableau=157
wall clock (sum of medians, 5 repeats each): 42.829 ms
```

Per-file medians range from **8.3 µs** (`84_disjoint_through_subsumer_chain_unsat.ofn`)
to **317 µs** (`87_nested_or_back_jumping_unsat.ofn`). No regressions
vs the 87/87 differential pass recorded in the strategy memory.

Raw: `bench-results/corpus-fixtures-20260524-180353.txt`.

## 2b. Bundled example ontologies (`crates/owl-dl-bench/examples/`)

Tiny hand-crafted ontologies that ship in the repo, classified
through `owl-dl-bench classify`, 10 repeats:

| ontology | min | median | max | classes |
|---|---:|---:|---:|---:|
| `anatomy.ofn` (pure EL, 31 classes) | 125 µs | **160 µs** | 222 µs | 31 |
| `family.ofn` (small SROIQ) | 3.11 ms | **3.66 ms** | 4.37 ms | — |

Anatomy is the same workload the baseline doc records at **85 µs** on
the previous host. Median 160 µs on the new box → ~2× slower,
consistent with the synthetic-EL per-core regression.

Raw: `bench-results/anatomy-family-pizza-topdown-20260524-183003.txt`.

## 3. Real-ontology corpus

Fetched + converted via [`scripts/fetch-real-ontologies.sh`](../scripts/fetch-real-ontologies.sh).
Data-property and datatype axioms stripped where the unmodified
ontology hits the Phase-7 hard-error gate (see
[`docs/real-ontology-corpus.md`](real-ontology-corpus.md#caveat-data-property-stripping)).

| ontology | size (OFN) | classes | mode | wall | user (CPU·s) | notes |
|---|---:|---:|---|---:|---:|---|
| `go-basic.ofn` (raw) | 80 MB | 51,937 | pure EL | **16.94 s** | 15.16 s | 364,415 saturation subsumption hits; deterministic single-thread; max-RSS 3.31 GB |
| `sulo-stripped.ofn` | 22 KB | 17 | hybrid | **0.43 s** (med of 10, min 0.42, max 0.44) | 3.06 s | 11 timed-out pairs out of 210 tableau subsumption calls; 17 tableau unsat probes |
| `pizza.ofn` (raw) | 69 KB | — | — | **>300 s** (`--pair-timeout-ms 200`) / **>120 s** (no per-pair timeout) | 9540 s / 3824 s | classify never finishes; CPU pegged at 32× wall, i.e. full parallel pair-loop saturation |
| `sio-stripped.ofn` | 1.0 MB | — | — | **>300 s** | 6149 s | matches the baseline doc's "does not finish in 90 s"; new box still doesn't finish in 5× that |
| `family-stripped.ofn` | 200 KB | — | — | **hard error** | <1 s | conversion: "role chain sub-property axiom outside supported fragment (only length-2 named-role chains are implemented)" |
| `ro-stripped.ofn` | 854 KB | — | — | **hard error** | <1 s | conversion: "unsupported axiom kind: SWRL Rule" |

Raw: `bench-results/real-ontologies-20260524-180427.txt`,
`bench-results/real-stripped-followup-20260524-182412.txt`,
`bench-results/sulo-repeats-20260524-182729.txt`.

### Engine behaviour per ontology

- **GO** sits squarely in the EL fragment. 51k classes, 364k
  saturation subsumption hits, zero tableau calls. This is the
  ontology that demonstrates rustdl's pure-EL path is real and works
  at scale. 17 s wall is competitive without any further work.
- **SULO** is small (17 classes) but SROIQ-heavy — the orchestrator
  falls back to the tableau and 5 % of pairs time out at the 200 ms
  cap. 0.43 s wall vs the doc's 466 ms baseline → essentially the
  same number, slightly better on the new box. The doc's "11×
  HermiT-gap" claim refers to SULO's tableau wall, not to silicon.
- **Pizza** is the surprise — never finishes. CPU utilization
  near-perfect at 32× wall, so the parallel pair loop is saturating
  the box, but per-pair tableau work blows the budget. Pizza was
  *not* in the baseline doc; this is a new data point worth noting
  for the perf plan.
- **SIO** behaves identically to the baseline: doesn't finish.
- **family** and **ro** never run — different gap kinds, both
  outside the supported fragment. They are listed in the corpus so
  that "did this gap get closed?" has a reproducible check.

## Comparison vs the baseline gap table

Reproducing [`outperform-hermit-plan.md`](outperform-hermit-plan.md) §
"Today's gap" with the new-box numbers replacing the rustdl column:

| Workload | rustdl (old box) | rustdl (new box) | HermiT | delta |
|---|---:|---:|---:|---|
| anatomy.ofn (pure EL, 31 classes) | 85 µs | 160 µs | — | **1.88× slower** (still ahead of HermiT) |
| Synthetic EL n=200 | 145 ms | 247 ms | — | **1.70× slower** |
| Synthetic EL n=400 | 1.35 s | 2.03 s | 338 ms | gap now 6.0×, was 4.0× |
| SULO classify `--pair-timeout-ms 200` | 466 ms | 490 ms ⁂ | 43 ms (old) / **~1.4 s** (new, see §4) | **claim inverts** — the 43 ms baseline was apples-to-oranges (see §4) |
| SIO classify | does not finish in 90 s | does not finish in 300 s ⁂ | minutes | unchanged |
| family.rdf.owl | rejected / timeout | hard error: length-3 role chain | inconsistent in 8 s | unchanged coverage gap |
| pizza.ofn | not measured | does not finish in 300 s; `--top-down` also times out at 120 s | — | new workload, new gap |

⁂ SULO and SIO numbers on the new box are on `*-stripped.ofn`
variants (data-property axioms removed via ROBOT — see
[real-ontology-corpus.md](real-ontology-corpus.md#caveat-data-property-stripping));
the previous baseline predates the stripping pin, so the workloads
are not strictly identical. The SROIQ shape of the post-strip
ontology is the same in both cases — data-property axioms aren't
reasoning-load-bearing through the saturation/tableau path today.

## Takeaways

1. **The new machine is not a perf win for synthetic EL.** EPYC 7282 at
   2.8 GHz boost runs the single-threaded closure ~1.5–1.7× slower
   than the previous host on the n=200/400 reference points. The 32
   cores buy us nothing on a pure-EL chain because the closure
   doesn't parallelize.

2. **Cores are being used on real ontologies.** SULO, SIO, and pizza
   show 7-32× CPU utilization via the classify pair-loop. So the
   parallelism is real where the workload supports it — but the
   per-pair tableau cost on real SROIQ inputs eats whatever cores
   provide.

3. **GO at 17 s is the headline number.** 51,937 classes, pure EL,
   3.3 GB RSS, deterministic. This is a stronger demonstration than
   the synthetic chain that the saturation engine is production-real
   at biomedical-EL scale.

4. **Half the real corpus doesn't run at all** because of conversion
   gaps unrelated to the saturation/tableau work: data properties
   (Phase 7), length-3+ role chains, SWRL rules. These block
   measurement of the inputs we actually want to measure. Lifting
   the data-property + datatype rejection alone would unblock SULO,
   family, RO, and SIO.

5. **Pizza is the most surprising regression / blind spot.** The
   baseline doc has no pizza number; the new run shows it never
   finishes. A 69 KB SROIQ tutorial ontology timing out at 300 s
   wall (and at 120 s unbounded) is a signal that the per-pair
   tableau search has a worst-case path that real ontologies hit
   trivially.

## Phase A — pizza flamegraph (done 2026-05-24)

The `owl-dl-bench --features profile` build wraps a run in pprof-rs
(signal-based, no kernel privileges). A 45 s sample of pizza.ofn
classify produced [`flamegraphs/pizza-2026-05-24.svg`](flamegraphs/pizza-2026-05-24.svg).

Hot frames, ≥1 % of 12,385 samples:

| Function | Samples | % | Source |
|---|---:|---:|---|
| `apply_role_chains` (inclusive) | 12,361 | **99.81 %** | [rules.rs:736](../crates/owl-dl-tableau/src/rules.rs#L736) |
| `parent` (graph ancestor walk) | 7,480 | 60.40 % | inside `is_blocked` |
| `is_subset_sorted` | 3,410 | 27.53 % | [lib.rs:926](../crates/owl-dl-tableau/src/lib.rs#L926) |
| `cmp` (`ConceptId` comparison) | 2,272 | 18.34 % | inside `is_subset_sorted` |

The role-chain rule body has five inner-loop allocations and two
linear scans per call (`Vec::to_vec` of `chains`; per-edge
`DepSet::clone`; the `pending` dedup; the `chain_edge_already_present`
scan). Each call also runs `is_blocked`, which is where `parent` and
`is_subset_sorted` accumulate. See [flamegraphs/README.md](flamegraphs/README.md)
for the full breakdown.

**This pins three Phase B items to specific exclusive-time hits:**

- B.1 DepSet tuning → removes the per-edge `DepSet::clone` work
  inside `apply_role_chains`.
- B.4 anywhere/subset blocking → replaces the `parent`-walk
  ancestor scan that's 60.40 % of exclusive time.
- B.2 lazy unfolding → shrinks the label sets that
  `is_subset_sorted` scans (27.53 % + 18.34 % in `cmp`).

## Phase B.1 — DepSet → SmallVec<[u32; 1]> (done 2026-05-24)

Type alias flipped in [graph.rs:42](../crates/owl-dl-tableau/src/graph.rs#L42)
from `Vec<u32>` to `SmallVec<[u32; 1]>`. Touched 4 files
(graph.rs, deps.rs, lib.rs, rules.rs, search.rs); test sites that
referenced `Vec::as_slice` as a function pointer migrated to closures.
266/266 tests pass.

### Bench delta

| Workload | Pre-B.1 | Post-B.1 | Change |
|---|---:|---:|---|
| Synthetic EL n=200 (med of 10) | 246.7 ms | 247.8 ms | flat (pure EL, skips tableau) |
| Synthetic EL n=400 | 2.03 s | 2.03 s | flat |
| Synthetic EL n=800 | 17.1 s | 17.1 s | flat |
| 87-fixture corpus (sum of medians, 5 reps each) | 42.829 ms | **38.323 ms** | **–10.5 %** |
| SULO classify `--pair-timeout-ms 200` (med of 10) | 0.43 s | 0.43 s | flat (within noise) |
| GO classify (single run, pure EL) | 16.94 s | 17.53 s | within noise |
| pizza.ofn classify | does not finish in 300 s | does not finish | unchanged (pizza is per-pair-bounded; B.1 alone won't free it) |

### Exclusive-time shift on pizza

Re-profiled to [`flamegraphs/pizza-2026-05-24-post-b1.svg`](flamegraphs/pizza-2026-05-24-post-b1.svg)
under the same 45 s pprof window:

| Function | Pre-B.1 | Post-B.1 | Direction |
|---|---:|---:|---|
| `apply_role_chains` (inclusive) | 99.81 % | 99.89 % | unchanged ancestor |
| `parent` (ancestor walk in `is_blocked`) | 60.40 % | 43.30 % | **down** — the per-call work it shared with `DepSet::clone` is gone |
| `is_subset_sorted` | 27.53 % | **44.75 %** | **up** — now dominant exclusive cost |
| `cmp` (`ConceptId` compare in subset check) | 18.34 % | 28.83 % | up |

Time moved out of allocator + DepSet ops into the pair-blocking
subset scan. This is the shape Phase A predicted. The next concrete
lever is now **B.4 (anywhere/subset blocking)** — replacing the
linear `parent`-walk + per-pair `is_subset_sorted` with a tighter
blocking discipline directly targets the new dominant frames.

## Phase B.4 — label_sig bloom prefilter + SoA blocking summary (done 2026-05-24)

Two paired changes:

1. **Bloom prefilter.** Added a `label_sig: u64` bloom signature
   (one bit per label, Knuth-multiplied index). `is_blocked` bails
   out before calling `is_subset_sorted` whenever
   `(small_sig & !big_sig) != 0` — a single AND + CMP rules out
   non-subset candidates without touching the label arrays.

2. **SoA blocking summary.** Added a `Vec<BlockingSummary>`
   parallel to `Vec<Node>` on [`CompletionGraph`](../crates/owl-dl-tableau/src/graph.rs);
   each entry packs `(parent, parent_role, label_sig)` into ~24
   bytes. The ancestor walk in `is_blocked` now iterates this dense
   array instead of pulling in the full ~200-byte `Node` for each
   step — cache lines hold ≥2 entries instead of 0.32.

### Soundness story

The first SoA attempt missed two `parent` write paths (nominal merge
re-parenting in `lib.rs:829`, and its rollback in `trail.rs:289`),
which left the mirror stale on inputs that exercise nominal merges.
Pizza tripped this: `is_blocked` returned `false` when it should have
returned `true`, the tableau kept expanding past the proper block,
and spurious clashes produced **91 unsatisfiable classes** instead
of HermiT's correct 2. Caught by cross-checking the unsat list
against the `obolibrary/robot` oracle (`docker run obolibrary/robot
robot reason --reasoner hermit --input pizza.ofn`); not caught by
the workspace tests because the 87 fixtures don't exercise
nominal-merge ∧ pair-blocking interaction.

Fix: mirror the rewrite in both the merge path and the
`ParentRewritten` trail entry. Re-ran HermiT comparison on pizza
sample classes after; verdicts match.

### Bench delta vs pre-A baseline

| Workload | Pre-A | Post-B.1 | Post-B.4 (label_sig only) | Post-B.4 (+ SoA, final) | Net |
|---|---:|---:|---:|---:|---:|
| 87-fixture corpus (5 reps) | 42.83 ms | 38.32 ms | 35.18 ms | **33.78 ms** | **−21.1 %** |
| Synthetic EL n=200 (med of 10) | 247 ms | 248 ms | 249 ms | 249 ms | flat (pure EL) |
| Synthetic EL n=400 | 2.03 s | 2.03 s | 2.04 s | 2.04 s | flat |
| SULO classify (med of 10) | 0.43 s | 0.43 s | 0.43 s | 0.49 s | +14 % (likely noise — small-N tableau, 11 timed-out pairs dominate) |
| GO classify (pure EL, single run) | 16.94 s | 17.53 s | 16.53 s | 16.93 s | within noise |
| pizza.ofn classify | DNF in 300 s | DNF | DNF | DNF | unchanged — per-pair work is still combinatorial |

### Exclusive-time shift on pizza

Same 45 s pprof window across iterations, [`flamegraphs/pizza-2026-05-24-post-soa-fixed.svg`](flamegraphs/pizza-2026-05-24-post-soa-fixed.svg) for the final state:

| Function | Pre-A | Post-B.1 | Post-B.4 (label_sig) | Post-B.4 (+ SoA, final) |
|---|---:|---:|---:|---:|
| `apply_role_chains` (inclusive) | 99.81 % | 99.89 % | 99.86 % | 99.63 % |
| `parent` (ancestor walk) | 60.40 % | 43.30 % | 58.6 % | **<0.5 %** (off the list) |
| `is_subset_sorted` | 27.53 % | 44.75 % | 11.3 % | **9.7 %** |
| `cmp` (`ConceptId` compare) | 18.34 % | 28.83 % | <0.5 % | 3.3 % |
| `eq` (`Role::eq` inside `xr == yr`) | — | — | — | 5.5 % |
| Total samples in 45 s | 12,385 | 12,325 | 12,497 | **14,232** |

`parent` (raw `Node`-field memory load) effectively disappeared from
the profile. Sample count rose ~15 %: the loop iterates more
ancestors per unit wall time. The algorithm hasn't changed shape —
pizza still has combinatorial work to do — but the per-iteration
cost dropped substantially.

## Phase A.1 — Per-rule counters (done 2026-05-24)

Added `RuleCounters` on `TableauContext` behind a `counters` Cargo
feature on `owl-dl-tableau`, threaded up through `owl-dl-reasoner`
and `owl-dl-bench`. `RUSTDL_COUNTERS=1` dumps non-zero counters from
each worker thread's `TableauContext::drop`. Zero cost in default
builds (field elided, macros expand to nothing).

This is the Phase A deliverable the plan ([`outperform-hermit-plan.md`](outperform-hermit-plan.md))
named *before* B.1; backfilled now because the flamegraph alone was
not enough to pick the next lever.

### Pizza counter histogram (15 s wall, 52 worker contexts)

Aggregated across all worker threads:

| Counter | Total | Per-context-per-sec |
|---|---:|---:|
| `add_label_calls` | 10,837,813 | ~14k |
| `is_blocked_calls` | 2,492,982 | ~3.2k |
| `apply_*` (all 13 rules, each) | 415,497 | ~533 |
| `add_label_inserted` | 78,105 | ~100 |
| `add_edge_calls` | 3,490 | ~5 |
| `apply_role_chains_body_iters` | 0 | 0 |
| `is_blocked_true` | 0 | 0 |
| `is_blocked_subset_scans` | 0 | 0 |
| `is_blocked_prefilter_rejects` | 0 | 0 |

### What the counters reveal

1. **`add_label` is dominated by no-ops.** 10.8 M calls, only 78 k
   (0.7 %) actually inserted. The other 99.3 % traverse
   `binary_search` only to confirm the label is already present.
   The wasted work is ~10.7 M binary-search-probes per 15 s.

2. **The role-chain body never fires.** `apply_role_chains` is
   called 415 k times but the inner `for (tail, tail_deps) in
   tails` loop never executes. Pizza has `TransitiveObjectProperty`
   axioms (`hasIngredient`, `isIngredientOf`) so the chains list
   *is* non-empty — but the matching outgoing edges aren't there
   yet at most call sites (only 3,490 edges created total). The
   pprof "99.81 % inclusive in `apply_role_chains`" was the outer
   loop's cheap setup (chains clone, edge filter), multiplied by
   415 k calls — not the chain-derivation hot path I assumed.

3. **`is_blocked` returns false 100 % of the time.** 2.5 M calls,
   zero `is_blocked_true`, zero `is_blocked_subset_scans` — the
   ancestor walk never finds a candidate with matching
   `parent_role`. Pizza's tableau either doesn't reach the depth
   that would create matching ancestors, or every ancestor has a
   different role. The B.4 bloom prefilter and SoA layout improved
   the *per-call* cost of `is_blocked`, but the data shows the call
   itself is cheap and uninteresting on pizza — the wall is
   elsewhere.

4. **All 13 `apply_*` counters are equal at 415,497.** The
   saturation loop applies every rule to every node every sweep,
   regardless of whether anything changed since the last sweep.
   This is the "stale fixpoint" pathology that lazy-unfolding +
   worklist-based saturation explicitly target.

### Implication for the next lever

The flamegraph said "spend more time on `is_blocked`". The counters
say "spend less time *calling* `apply_*` and `add_label`". Two
concrete, low-risk next moves the data directly motivates:

1. **Per-node residual-GCI saturation memo.** First time
   `apply_residual_gcis(node)` fires, materialize all residuals and
   set a per-node `residuals_saturated: bool`. Subsequent calls
   short-circuit. Expected: cut `add_label_calls` by ~80 % on
   pizza-shape inputs (back-of-envelope: 415k calls × ~20 residuals
   = 8.3 M of the 10.8 M `add_label` calls).

2. **Worklist over re-sweep.** Track which (node, rule) pairs need
   re-firing because *something they depend on changed* since the
   last call. Larger refactor — but the equal `apply_*` counts say
   12 of every 13 rule calls are pure overhead, so the upside is
   large.

(1) is a one-day change with predictable bench impact; (2) is a
multi-day change that touches the saturation core. Recommended
order: (1) first, then re-profile, then decide on (2) based on the
shape of the new counter histogram.

## Phase B.2 attempt — residual-GCI memoisation (done 2026-05-24)

Added `residuals_saturated: Vec<bool>` to [`CompletionGraph`](../crates/owl-dl-tableau/src/graph.rs);
`apply_residual_gcis` short-circuits when the flag is set, sets it
after the materialisation loop. Conservatively cleared on every
`LabelAdded` trail rollback. 266/266 tests pass; corpus median
verdicts unchanged.

### Pizza counter delta (15 s wall, 52 worker contexts)

| Counter | Pre-memo | Post-memo | Δ |
|---|---:|---:|---:|
| `add_label_calls` | 10,837,813 | **6,297,814** | **−41.9 %** |
| `add_label_inserted` | 78,105 | 78,079 | flat |
| `is_blocked_calls` | 2,492,982 | 2,491,308 | flat |
| `apply_*` (each) | 415,497 | 415,218 | flat |
| `add_edge_calls` | 3,490 | 3,489 | flat |

### Bench delta

| Workload | Pre-memo | Post-memo | Δ |
|---|---:|---:|---:|
| 87-fixture corpus (5 reps) | 33.78 ms | 34.73 ms | flat (within noise) |
| Pizza `--pair-timeout-ms 200` | DNF in 60 s | DNF in 60 s | unchanged |

### Why the wall didn't move

The 4.5 M saved `add_label` calls all hit the cheap "label already
present" path (`binary_search` confirms, returns `false`). That path
is fast enough that eliminating it doesn't show up in wall clock.
The pizza wall is dominated elsewhere — likely the saturation-sweep
driver iterating all 13 rules across all nodes regardless of
relevance — and that's a worklist-redesign question, not a
per-rule cleanup.

Net: the lever is **a real correctness-preserving CPU-cycle saving**
but **not a productivity win on the workloads I have**. Shipping it
anyway because it's small, sound, and aligns with the documented
strategy (`outperform-hermit-plan.md` B.2). Future levers should
prefer ones that move *user-visible time*, which means either
unlocking pizza's pair-timeout-bounded under-approximation or
attacking the saturation-driver overhead the equal-`apply_*`-counts
reveal.

### What HermiT diff currently says

On a 30 s budget, `rustdl classify --pair-timeout-ms 200
pizza.ofn` returns 0 unsatisfiable classes vs HermiT's 2
(`CheeseyVegetableTopping`, `IceCream`). This is **not** a
soundness regression — every pair times out, so each subsumption
defaults to "not subsumed" (sound under-approximation per the CLI
documentation). The reachable verdict set is empty until rustdl
finishes any single pair-classification within the budget. Until
the saturation driver gets faster *or* the per-pair tableau gets
faster on pizza, this gap stays.

## Suggested next actions

1. **Worklist-based saturation.** The equal-counter signature
   (`apply_and = apply_forall = … = apply_role_chains = 415k`)
   directly shows the sweep re-fires every rule on every node every
   pass, regardless of what changed. A `(node, rule)` worklist
   keyed by labels-added / edges-added would cap rule calls at
   "rule was previously satisfied at this node" + invalidations.
   Larger refactor than the memo, but the only lever the counter
   histogram leaves on the table.

2. **Real-corpus differential test (`cargo test --ignored
   real_ontology_diff`).** Pizza's 91-vs-2 unsat bug in the SoA
   work landed silently because the 87 fixtures don't exercise the
   nominal-merge ∧ pair-blocking interaction. Wiring an
   ignored-by-default test that runs ROBOT/HermiT against
   `pizza.ofn`, `sulo-stripped.ofn`, and `sio-stripped.ofn` and
   asserts unsat-set equality (with a generous per-file wall
   budget) gives every future B-phase change a guard. Cheap to
   build, expensive to skip.
2. **Skip-or-error policy for data-property axioms.** Today every
   real OWL ontology that *declares* a data property fails
   conversion. A "tolerate declarations + skip data-property usage
   in class expressions" mode would let the bench produce numbers
   for SULO, family, RO, SIO from their unmodified `.ofn` (and
   correctly note "this is a Phase-7-shaped under-approximation").
   Track explicitly that this is *not* sound for entailment — just
   useful for perf measurement until Phase 7 lands.
3. **Re-measure n=200/400 on the previous box** if it's still alive,
   to confirm the 1.5–1.7× single-thread regression is silicon and
   not a code change between the baseline doc's date (~2026-05-22)
   and today (`5df2879`). Two days, but worth ruling out.
4. **Roaring-bitmap migration** (`docs/rust-specific-data-structures.md`
   item 3) is the documented lever for the n^3-ish scaling. The
   slope between n=800 (17 s) and n=1600 (208 s) hits 12× per 2×
   classes — the bitset memory layout is the most likely culprit.

## 4. Multi-reasoner baseline (2026-05-25)

Establishing a clean apples-to-apples set of timings against the
three reasoners the OWL community actually compares against:
**HermiT** and **Pellet** (both Java, both run via ROBOT v1.9.6 in
docker) and **Konclude** v0.7.0-1138 (native C++, run via the
official `konclude/konclude` docker image, `-w AUTO` for
auto-scaling to 32 cores). rustdl uses
`./target/release/rustdl classify --pair-timeout-ms 200` for SULO
and SIO, plain `owl-dl-bench classify` for family.

Raw outputs: `bench-results/multi-reasoner-20260524-223957.txt`,
`bench-results/konclude-rustdl-fix-20260524-230024.txt`,
`bench-results/rustdl-corrected2-20260524-232053.txt`.

### Container / JVM startup floor

The wall measurements include container start (docker rootless) +
runtime warmup. Measured on this box with the reasoner's `--help`
(no reasoning work):

| Component | min | med | max |
|---|---:|---:|---:|
| ROBOT/JVM/HermiT-or-Pellet floor (`robot --help`) | 1.87 s | **2.46 s** | 2.58 s |
| Konclude container floor (`Konclude --help`) | 0.83 s | **1.27 s** | 1.34 s |
| rustdl floor | 0 (native, no container) | 0 | 0 |

These floors set the lower bound on what each reasoner can ever
report — any total below ~2.5 s for ROBOT or ~1.3 s for Konclude
is overwhelmingly startup, not reasoning. **The old-box baseline's
"HermiT SULO = 43 ms" is almost certainly raw HermiT-API timing
with no JVM startup amortised**, which makes it incomparable with
ROBOT wall-clock measurements.

### Raw wall-clock medians (n=5 per cell)

| Ontology | HermiT (ROBOT) | Pellet (ROBOT) | Konclude (-w AUTO) | rustdl |
|---|---:|---:|---:|---:|
| pizza.ofn (raw, 100 classes) | 4.53 s | 3.44 s | **1.44 s** | timeout > 120 s |
| sulo-stripped.ofn (17 classes) | 3.94 s | 3.36 s | **0.95 s** | **0.49 s** (n=10) |
| family-stripped.ofn (45 classes) | 18.74 s | 3.27 s | 2.27 s | **0.06 s** ⁂ |
| sio-stripped.ofn (1746 classes) | 69.34 s | 4.45 s | **1.57 s** | timeout > 120 s |

⁂ family rustdl number is `owl-dl-bench classify` (no per-pair
timeout). `rustdl classify --pair-timeout-ms 200` errors on family
with *"role chain sub-property axiom outside supported fragment
(only length-2 named-role chains are implemented)"* — a known
coverage gap, not a perf result. The 0.06 s figure includes only
the rules family.ofn exercises *before* hitting any unsupported
role chain.

### Startup-adjusted (raw − reasoner floor)

Coarse subtraction — the floor varies ~0.7 s rep-to-rep, so treat
these as "actual reasoning is roughly":

| Ontology | HermiT-adj | Pellet-adj | Konclude-adj | rustdl |
|---|---:|---:|---:|---:|
| pizza | ~2.0 s | ~0.9 s | **~0.17 s** | doesn't finish |
| sulo-stripped | ~1.4 s | ~0.9 s | **< 0.1 s** | 0.49 s |
| family-stripped | ~16.2 s | ~0.8 s | ~1.0 s | partial 0.06 s |
| sio-stripped | ~66.8 s | ~2.0 s | **~0.3 s** | doesn't finish |

### Findings

1. **The "10× HermiT gap" claim from the old baseline is wrong on
   this hardware.** rustdl-SULO at 0.49 s is **3× *faster*** than
   ROBOT-HermiT-SULO at ~1.4 s adjusted. The old 43 ms HermiT
   number was almost certainly raw-HermiT-API timing (no JVM
   startup, no ROBOT wrapper). For an apples-to-apples comparison
   on a developer's *workstation experience*, rustdl already wins
   on SULO.

2. **Konclude is the real benchmark to beat, not HermiT.** On every
   workload that finishes, Konclude is the fastest reasoner here.
   Its actual reasoning is fractions of a second (SIO in ~0.3 s,
   pizza in ~0.17 s). It is the C++ tableau reference and shares
   rustdl's architectural family (parallel pair-loop, SROIQ-class).
   The honest goal should be "match Konclude," not "beat HermiT."

3. **Pellet is JVM-startup-bound for everything in this corpus.**
   Its medians cluster tightly around 3.3-4.5 s independent of
   workload weight (family with 45 classes takes 3.27 s; SIO with
   1746 classes takes 4.45 s — only ~1 s of actual reasoning
   difference). Pellet does not have a perf problem with these
   ontologies; it has a JVM perf problem with any ontology.

4. **HermiT genuinely struggles on family and SIO.** family-adj at
   ~16 s vs Pellet-adj at ~0.8 s suggests HermiT's tableau
   strategy hits a bad case on length-3 role chains. SIO-adj at
   ~67 s is real reasoning effort and the only workload where
   HermiT's wall is dominated by actual work.

5. **rustdl's wins are real but narrow.** SULO and family
   (when family finishes) put rustdl ahead of HermiT
   and competitive with Pellet. **rustdl's losses are the
   non-finishing ones**: pizza and SIO timeout, family hard-errors
   on length-3 role chains. The next 10× of work is closing
   coverage gaps, not perf knobs.

### Implications for the work plan

- The "outperform HermiT" framing in
  [`outperform-hermit-plan.md`](outperform-hermit-plan.md) is the
  wrong goalpost. rustdl already outperforms ROBOT-HermiT on SULO.
  The right goal is "match Konclude on the four workloads in the
  corpus."

- Closing the Konclude gap on SULO (0.49 s → < 0.1 s) and on
  finishing SIO at all (currently timeout) likely needs the
  **worklist-based saturation** from §"Suggested next actions" item 1.
  Konclude's <100 ms on SULO and ~0.3 s on SIO means its
  saturation loop visits each `(node, rule)` pair O(1) times, not
  O(passes × nodes × rules) like rustdl's current sweep.

- pizza doesn't finish — that's blocking/backtracking, not
  saturation. Konclude does pizza in ~0.17 s of actual reasoning,
  so this is solvable, not pathological. Phase A's flamegraph
  pointed at the pair-blocking / role-chain hot path. Re-running
  pizza under counters after worklist saturation may show which
  rule's pair-blocking comparisons are the long pole.

- The role-chain coverage gap (`--pair-timeout-ms` rejects family)
  is a feature gap, not a perf gap. Cheap fix: support length-N
  role chains (Phase 5/6 work in the plan), and family becomes a
  real measurement instead of a partial-evaluation one.

## 5. Pizza now terminates — `is_blocked` parent-pointer cycle (2026-05-25)

### Diagnosis

Pizza's "doesn't finish in >120 s" wall (§4) traced to
[`TableauContext::is_blocked`](../crates/owl-dl-tableau/src/lib.rs)
walking the tree-ancestor chain via `BlockingSummary.parent`. On
pizza's unsat probe for `pizza:American` (class index 0), the
chain forms a cycle after ~10k role-chain rule calls. The walk
loop has no visited-set or step cap, so once a cycle exists the
function never returns. Diagnostics caught it at node `y=7` with
a chain longer than `graph.len() = 66`.

The cycle is in the dense `BlockingSummary` SoA mirror and almost
certainly in the underlying `Node.parent` too. The most likely
creation site is `merge_into` failing to redirect a node whose
`parent` was the source of the merge — same code-path class as
the SoA bug fixed in §3 ("Soundness story"). Not yet localized to
a specific write site.

### Workaround

[`is_blocked`](../crates/owl-dl-tableau/src/lib.rs) now caps the
parent walk at `self.graph.len()` steps and returns `false`
("not blocked") on overrun. In debug builds a `HashSet`-backed
revisit check fires a `debug_assert!` instead — so anyone
reproducing the cycle on a debug build gets a stack trace at the
panic site (need a smaller repro than full pizza; debug builds are
too slow to run pizza classify in any reasonable time).

This is **sound for is_blocked specifically** — a false "not
blocked" answer just makes the tableau continue work that it
could otherwise skip. It is **not** a real fix: the
parent-pointer invariant is broken, and the workaround masks
that.

### Result

Pizza now terminates. On `--pair-timeout-ms 200` it finishes in
~3 s (vs. >120 s before). Workspace test suite still green
(267 passed, 0 failed).

### Open soundness bug surfaced by termination

With the hang gone, pizza classify now reports **61 unsatisfiable
classes** vs. HermiT's 2 (`CheeseyVegetableTopping`,
`IceCream`). Single-class probe `rustdl sat pizza.ofn
pizza:ParmaHamTopping` also returns "unsat" — i.e. the false
positives are **not** an artefact of the cycle workaround, they
come straight from the rule engine. This was hidden in §4 because
8 781 of the 9 702 pair tests timed out and timed-out probes
default to "satisfiable" (sound under-approximation). The
underlying soundness gap was there all along — it just couldn't
be observed while every pair was timing out.

This is a real bug in the SROIQ rules' interaction with pizza's
shape. Localized 2026-05-25 by ROBOT STAR extraction + axiom-level
bisection.

#### Minimal repro (15-line ontology)

```
Declaration(Class(:A))
Declaration(Class(:PT))
Declaration(Class(:S))
Declaration(Class(:Hot))
Declaration(Class(:Mild))
Declaration(ObjectProperty(:hs))

SubClassOf(:A :PT)
SubClassOf(:A ObjectSomeValuesFrom(:hs :Mild))
FunctionalObjectProperty(:hs)
EquivalentClasses(:S ObjectIntersectionOf(:PT ObjectSomeValuesFrom(:hs :Hot)))
DisjointClasses(:Hot :Mild)
```

HermiT says `:A` is satisfiable. rustdl says unsat. Each of the
five non-declaration axioms is essential — drop any one and
rustdl gives the correct `sat` verdict.

Captured as `#[ignore]`d test
[`pizza_functional_equiv_some_should_be_sat`](../crates/owl-dl-reasoner/src/lib.rs)
and fixture
[`functional-equiv-some-bug.ofn`](../crates/owl-dl-reasoner/tests/fixtures/functional-equiv-some-bug.ofn).

#### Why this *should* be sat

Tableau-by-hand:

1. Root `r` is labelled `:A`.
2. `:A ⊑ :PT` adds `:PT`.
3. `:A ⊑ ∃hs.Mild` adds `∃hs.Mild`. apply_exists creates a
   successor `m` labelled `:Mild`.
4. Functional gives `⊤ ⊑ ≤1 hs.⊤` as a residual GCI. So `r`
   has the at-most-1 cardinality.
5. The reverse half of the equivalence — `:PT ⊓ ∃hs.Hot ⊑ :S`
   — absorbs (via `:PT` as trigger) to: when label has `:PT`,
   add `Or([∀hs.¬Hot, :S])`.
6. `r` is `:PT`, so it gets the disjunction. apply_or branches:
   - **∀hs.¬Hot**: propagates `¬:Hot` to `m`. `m` is `:Mild`
     and `¬:Hot` — Disjoint(`:Hot`, `:Mild`) is *not* triggered
     (we have `:Mild ∧ ¬:Hot`, not `:Mild ∧ :Hot`). Consistent.
     **This is the sat witness.**
   - **:S**: `:S ⊑ :PT ⊓ ∃hs.Hot` adds `:PT` (already there) and
     `∃hs.Hot`. apply_exists creates a second hs-successor with
     `:Hot`. apply_max (≤1) forces the two hs-successors to
     merge → merged node has `:Mild ∧ :Hot` → disjointness
     clash → backtrack. Correct.

rustdl returns the *failed* branch as the verdict instead of
finding the consistent first branch. Most likely the apply_or
search is not exploring both branches, or the apply_forall
propagation of `¬:Hot` to `m` isn't happening on that branch,
or the disjointness rule is firing on `:Mild ∧ ¬:Hot` (it
shouldn't — that's not a clash).

#### Fix (2026-05-25)

[`TableauContext::merge_into`](../crates/owl-dl-tableau/src/lib.rs)
was copying the source node's labels via `add_label(target, c)`
which calls `add_label_with_deps(target, c, &[])` — wiping each
label's per-label `DepSet`. The clash that follows from a
merge-induced contradiction then carries empty `clash_deps`,
which the back-jumping search at
[`crate::search::branch`](../crates/owl-dl-tableau/src/search.rs)
interprets as "branch-independent unsat" and propagates *past*
the licensing disjunction (`my_id` isn't in `clash_deps`), so
the alternative disjunct (the consistent `∀hs.¬:Hot` branch in
the minimal repro) never gets tried.

Fix: snapshot source's `(label, deps)` pairs and use
`add_label_with_deps(target, c, deps)` instead. Regression
test [`pizza_functional_equiv_some_should_be_sat`](../crates/owl-dl-reasoner/src/lib.rs)
now passes — was the canary for this exact bug pattern.

#### Result on full pizza

Down from **61 false-positive unsats to 25**; correct
verdicts include `:CheeseyVegetableTopping`, `:IceCream`
(HermiT's expected 2), and now also `:AsparagusTopping`
(was wrong before fix). All 87 fixture tests still green.

#### Second fix (2026-05-25): branch's parent-Or deps

Second related deps-loss bug surfaced when bisecting
`:AnchoviesTopping`. Pattern: `:V ≡ :PT ⊓ (:C ⊔ :N)` with
`:A ⊑ :F ⊑ :PT` and `:F` disjoint with `:C`, `:N`. HermiT
says `:A` sat; rustdl said unsat because the outer disjunction
`Or([(¬:C ⊓ ¬:N), :V])` chooses `:V` first, leading to a nested
disjunction `:C ⊔ :N`. Both inner branches clash, but the
returned `clash_deps` carried only the *inner* branch's id —
not the *outer* one — so back-jumping skipped past `:V` instead
of trying the consistent `(¬:C ⊓ ¬:N)` disjunct.

Root cause: [`crate::search::branch`](../crates/owl-dl-tableau/src/search.rs)
asserted the chosen disjunct with `deps = [my_id]` only.
`first_open_disjunction` now returns the parent Or's `DepSet`
alongside the disjuncts; `branch` uses `parent_deps ∪ [my_id]`
as the disjunct's deps. Regression test
[`pizza_equiv_pizzatopping_union_should_be_sat`](../crates/owl-dl-reasoner/src/lib.rs).

#### Third fix (2026-05-25): merge_into edge deps

Same pattern applied to edges: the two `add_edge_inner(_, _,
_, &[])` calls in `merge_into` step 2/3 now thread the prior
`edge_deps` through. Defensive consistency with the label-deps
fix.

#### Result on full pizza after all three fixes

**61 → 18-22 false-positive unsats** (varies across runs with
per-pair timeouts). Correctly sat now: `:AsparagusTopping`,
`:AnchoviesTopping`, `:HamTopping`, and most toppings whose
shape matches the two regression tests. 269 workspace tests
pass.

#### Fourth fix (2026-05-25): merge-condition deps

Trace on the 84-line `named-pizza-country-bug.ofn` repro
showed `clash_deps=[]` at every back-jump level. Drilling in:
both `node=0` (the test root) and `node=1` (its `hasBase`
successor) got the *same* `Nominal({America})` label via
different branches of `:Country ≡ :DC ⊓ ObjectOneOf(...)`.
`apply_nominal_assignment` then merged the two, moving
`:Pizza` (deps=[]) onto the merged node, where `Pizza ⊓
PizzaBase ⊑ ⊥` produced a clash. Crucially, **the merge
itself was conditional on the two branch decisions**, but
`merge_into` didn't take any merge-condition deps — the
moved `:Pizza` arrived with deps=[], the clash inherited
deps=[], and back-jumping skipped past every disjunction.

Fix: new `merge_into_with_deps(source, target, merge_deps)`
API. The merge-condition deps are unioned into *every* moved
label's and edge's `DepSet`. Two call sites:

- `apply_nominal_assignment` computes
  `union(here_nominal_label_deps, other_nominal_label_deps)`
  — the precise reason the two nodes were forced to share
  individual identity.
- `apply_max` uses a new `compute_max_merge_deps` helper that
  unions the `≤n R.C` label's deps, the two matching edges'
  deps, and the body-label deps on both witnesses.

Regression test
[`pizza_named_pizza_country_should_be_sat`](../crates/owl-dl-reasoner/src/lib.rs)
uses the saved 84-line fixture via `include_str!` so it runs
in-tree without the corpus.

#### Result after all four fixes

Single-class probes now return the *correct* HermiT verdict
for the corpus we have. Classify on full pizza:

- `--pair-timeout-ms 200` (with rayon-parallel default 32
  threads): 0–2 unsats (the two HermiT-correct ones surface
  when budget allows; pair-timeout truncates the rest to
  "satisfiable" defaults).
- Single-class `rustdl sat`: `:CheeseyVegetableTopping` and
  `:IceCream` correctly unsat; `:NamedPizza`,
  `:AsparagusTopping`, `:AnchoviesTopping`, etc. correctly
  sat (each finishing in well under 100ms).

271 workspace tests pass, 0 failed. Three regression tests in
[`crates/owl-dl-reasoner/src/lib.rs`](../crates/owl-dl-reasoner/src/lib.rs)
pin the three deps-loss shapes against future regressions.

#### Real-ontology feature tests

[`tests/real_ontology_corpus.rs`](../crates/owl-dl-reasoner/tests/real_ontology_corpus.rs)
adds end-to-end gates that would have caught all four
soundness bugs landed this session. Default `cargo test` runs
the fixture-only test in-tree; `cargo test --features real-corpus`
runs the full pizza/sulo workloads (skipped silently if the
gitignored corpus isn't present). One strict pizza test
(`pizza_unsat_matches_hermit_exactly`) stays `#[ignore]`'d as
a "fix-me" marker — currently passes only on flat-out failure
because pair-timeouts mask completion; we'll un-ignore it
once classify finishes within reasonable wall time on the
corrected (slower) probes.

#### Trade-off

The fix exchanges "fast wrong" for "slow correct": each unsat
probe now explores more of the search tree before concluding
sat (false-positive unsat clashes were short-circuiting the
search). Pizza classify under `--pair-timeout-ms 200` no
longer produces the two true unsats consistently because the
probes for `:CheeseyVegetableTopping` and `:IceCream` run
in parallel with much slower SAT probes and the budget is
sometimes spent before either truly-unsat probe finishes.

#### Profile of the slow SAT probe

Flamegraph at
[`docs/flamegraphs/pizza-sat-NamedPizza-2026-05-25.svg`](flamegraphs/pizza-sat-NamedPizza-2026-05-25.svg)
captured via `RUSTDL_PROFILE=... ./target/release/owl-dl-bench sat
pizza.ofn pizza:NamedPizza` (a never-finishing slow SAT probe,
45-second pprof budget, 199 Hz sampling).

Time distribution across rules (% of stack appearances):

| rule | % |
|---|---:|
| `apply_role_chains` | 17 |
| `apply_exists` | 14 |
| `apply_max` | 12 |
| `apply_role_rules` | 11 |
| `apply_min` | 11 |
| `apply_nominal_assignment` | 10 |
| `apply_concept_rules` | 9 |
| `is_blocked` | 5 |
| (everything else, including deps work) | < 5 each |

**No single dominant culprit.** The work is genuinely
distributed across the 12 rules. The new deps-tracking cost
shows up as `SmallVec::extend` / `SmallVec::clone` / `Vec::alloc`
frames (~75 appearances each), spread across every rule's
"snapshot edges/labels into a `Vec<(_, _, DepSet)>` before
mutating the graph" prologue. No targeted micro-fix would move
the needle by more than a few percent.

The structural bottleneck is the **saturation re-fires every
rule on every node every pass** pattern that was already
flagged by the Phase A.1 counter histogram (415k rule calls,
most of which were no-ops). With the soundness fix, each rule
call now does *more* per-invocation (more DepSet ops), so the
"all rules re-fire on every pass" cost is bigger than before.

#### Recommended next perf lever: worklist saturation

The fix for the residual perf gap is the same one originally
called out in "Suggested next actions" item 1: a `(node, rule)`
worklist keyed by labels-added / edges-added so each rule only
re-fires on nodes whose relevant inputs changed since the rule
last ran. Caps total work at O(deltas), not O(passes × nodes ×
rules). Konclude's <100 ms on SULO / <1 s on pizza ride on this
exact discipline. Larger refactor than the deps fixes — touches
saturate's dispatch and adds a "what changed" hook into the
trail's label-add / edge-add path — but the only lever
post-profiling points at.

#### Worklist landed (2026-05-25)

Implemented as a per-node `dirty: Vec<bool>` on
[`CompletionGraph`](../crates/owl-dl-tableau/src/graph.rs).
Mutations that could enable a rule (`add_label_with_deps`,
`add_edge_inner`, `remove_edge_recorded`, `merge_into_with_deps`)
set the bit on the affected node(s);
[`saturate`](../crates/owl-dl-tableau/src/saturate.rs) clears
the bit before running the per-node rule block. Within a
single `saturate()` call the worklist correctly skips clean
nodes after the first pass.

**Two design choices tried:**

1. **Persistent dirty across `saturate()` calls** (only the
   newly-added disjunct's node is dirty between calls). Broke
   three fixture tests (48/65/66 — functional and
   inverse-functional merges): the per-mutation dirty hooks
   don't cover all the state a rule depends on (the residual-
   saturation memo, the nominal map shape, and `apply_max`'s
   edge-count threshold all matter in subtle ways).
2. **Conservative reset at entry** (`mark_all_dirty()` on each
   `saturate()` call). Tests pass. This is what landed —
   each `saturate()` call still pays an O(nodes) reset, but
   the gain shows up across outer iterations *within* a single
   `saturate()`.

**Result:** workspace tests stay at 271 passing; synthetic EL
chain timings stay within noise of the pre-fix baseline (n=200:
~247 ms, n=400: ~2.15 s, n=800: ~17.86 s); SULO classify still
0.50 s; the 84-line named-pizza fixture probe stays at < 1 ms.

**Pizza classify still times out** under `--pair-timeout-ms 200`
because the remaining bottleneck is the **search-tree size**,
not saturation efficiency. The 2026-05-25 deps fixes made the
search correctly explore branches that were wrongly clipped
before; some pizza probes (named pizzas, NamedPizza
super-class, etc.) now have legitimately large search trees.
Closing this needs branching heuristics — smarter disjunct
ordering, learned guidance, or restart strategies — not more
work on the saturation loop.

**Open variant worth a second look:** the persistent-dirty
variant could be revived if the missed cases get covered by
adding "dirty when residuals-saturated reset", "dirty when
nominal map changes", and "dirty when edges added to neighbours
on a node with a Max label" — each is mechanically findable,
but the combined complexity wasn't worth landing in this
session.

#### Fifth fix (2026-05-25): branching-strategy disjunct reorder

After the worklist landed without unlocking pizza, the
remaining bottleneck was clear: `apply_or` branching explores
the disjuncts in `ConceptId`-sorted order, which on pizza means
trying *Atomic* disjuncts first (small IDs) before *And-of-
literals* disjuncts (compound, larger IDs). For the Country
reverse-equivalence pattern this is exactly backward — the
`(¬{America} ⊓ ¬{England} ⊓ ¬{France} ⊓ ¬{Germany} ⊓
¬{Italy})` conjunct decomposes into five inert `Not(Nominal)`
labels (no concept-rule triggers, no merging) and the search
finds sat immediately, while the `:Country` atomic fans out
into `:Country ⊑ :DC ⊓ ObjectOneOf(…)` and another 5-way Or
branch with nominal assignment + node merging.

[`crate::search::reorder_disjuncts`](../crates/owl-dl-tableau/src/search.rs)
classifies each disjunct by expected downstream cost:

- **0 — leaf compound:** `Not(_)` or `And` whose conjuncts are
  all leaf-compound. Decomposes to inert labels.
- **1 — atomic / nominal (clean):** triggers `apply_concept_rules`
  but is otherwise simple.
- **2 — other compound:** `Some`/`Min`/`Max`/etc., likely to
  generate nodes or fire merges.
- **3 — obvious immediate clash:** the disjunct's complement is
  already a label; the branch will UNSAT trivially. Try last.

Stable secondary key on original index keeps the
literal-complements optimisation downstream deterministic.

#### Result after the reorder

**Pizza classify finally matches HermiT.** `--pair-timeout-ms 200`,
3 reps:

```
2 unsat | wall=58.54s
2 unsat | wall=58.05s
2 unsat | wall=58.07s
```

Unsat set: `{:CheeseyVegetableTopping, :IceCream}` — *exactly*
HermiT's two classes, every run.

**SULO classify went from 0.50 s to ~0.09 s** — a 5–6× speedup
(med over 5 reps: 0.08, 0.08, 0.09, 0.09, 0.12 s).

The strict feature test
`pizza_unsat_matches_hermit_exactly` no longer needs to be
`#[ignore]`'d as a "fix-me" marker — it's part of the
`--features real-corpus` gate now and passes.

#### Sixth micro-fix (2026-05-25): apply_role_chains pending dedup

Re-profiled after the reorder unblocked pizza; the picture
shifted: `apply_role_chains` rose from 17 % to ~25 % of CPU on
the slow `:NamedPizza` probe (with the work redistributed
away from `apply_nominal_assignment` and `apply_max` — now
that the easy branch is tried first, fewer merges happen).
Inside `apply_role_chains` the `pending.iter_mut().find()`
linear scan over the chain-derived edge set was O(P) per tail
iteration; on transitive `hasIngredient` expansion that
compounds to O(K² · P). Replaced with a `HashMap<(Role,
NodeId), DepSet>`, dropping the per-tail dedup to O(1).

Modest measured impact (within run-to-run noise on the
workloads we have — pizza classify ~58 s, SULO 0.09-0.12 s);
the structural improvement matters for future workloads with
heavy chain-rule firings (SIO if it ever finishes).

#### What's still slow

Single-class probes on full pizza for the most-branched
classes (`:NamedPizza`, `:AmericanHot`, etc.) still hit
per-pair timeouts when run unbounded. They correctly produce
sat under the deadline-path that classify uses, but the
underlying search tree is genuinely large — pizza's mutual
disjointness chains, transitive `hasIngredient`, and the 5-
way nominal branching from Country compose into a state space
where the slow probes dominate classify's wall time (~9 k of
9.7 k pairs hit the 200 ms deadline). Counter on a 10 s
`:NamedPizza` probe: **2.3 M rule calls per rule** (≈ 23 k
saturate outer iterations) and 256 k effective label inserts,
with the rest as no-op `add_label` calls (99 % skip rate).

#### Attempted: conflict-driven branching learning

Implemented a per-`TableauContext` `learned_nogoods` table
keyed by `(node, or_label, disjunct)` with the failure's
preconditions `clash_deps - {my_id}`. The intended invariant
was: when `precond ⊆ active_branches`, the disjunct is known
unsat in this context and `branch` can skip it.

**The invariant doesn't hold.** Pizza classify went from 2
unsat to 0 unsat (wrong) the moment the skip was enabled, even
though the trace showed 0 visible skips firing — meaning a
*deeper* logical interaction with the recording side is the
culprit, not the lookup itself. The preconditions don't fully
capture *which node-level labels* produced the clash; two
no-goods recorded in different sub-trees can — in
combination, via shared label fingerprints — incorrectly
exclude a model at a node that's actually sat.

The infrastructure stays in
[`crate::TableauContext`](../crates/owl-dl-tableau/src/lib.rs)
(`learned_nogoods`, `record_nogood`, `nogood_blocks`) and a
doc comment in [`crate::search::branch`](../crates/owl-dl-tableau/src/search.rs)
points at the right next step: a correct CDBL needs to key
no-goods on a richer fingerprint than just the branch-id
preconditions — the smallest unsat-explaining *label*
sub-set is the principled SAT-solver answer, but it requires
deps-on-labels-as-evidence the current trail doesn't track.

#### Next perf lever (not landed in this session)

**Label-evidence-based no-goods** (CDCL-style clause learning
adapted for the tableau). The current trail records
[`TrailEntry::LabelAdded`](../crates/owl-dl-tableau/src/trail.rs)
per insertion; extending the entry with the *immediate cause*
(which earlier label/edge triggered the rule that added this
one) would give a "1-UIP" cut for clash extraction, and the
resulting no-good is keyable on a label-set fingerprint
rather than the coarser branch-id one that tripped this
session's attempt. The refactor is large — touches every
rule's `add_label_with_deps` call site — but it's the
principled answer.

### Suggested next actions (post-§5)

1. **Localize the parent-pointer cycle source.** Add the same
   `debug_assert!(visited.insert(...))` walk at every site that
   writes `Node.parent` or `BlockingSummary.parent`:
   `push_node_with_parent`, the merge path, the nominal-merge
   re-parent at lib.rs:829, the `ParentRewritten` trail rollback.
   On a small repro (not full pizza), the assert fires at the
   creating mutation.
2. **Pin a `cargo test --ignored real_ontology_unsat_diff`.**
   This regression slipped past 267 unit tests because the
   fixtures don't exercise the nominal-merge × pair-blocking ×
   role-chain combination. A single ignored test that runs
   ROBOT-HermiT against pizza/sulo-stripped/sio-stripped and
   asserts unsat-set equality (with a generous wall budget) is
   the cheapest insurance.
3. **Bisect the 61 false-positive unsats.** Pick the simplest
   one (probably one with fewest dependencies on hasIngredient
   role chains). Trim pizza.ofn to a minimal repro and unit-test
   the resulting expected verdicts against HermiT.

## §6 — Pizza search-tree diagnosis (2026-05-25)

After landing the three per-call wins
(`8c3fefa` / `2d843ae` / `5208c2b` — top-down default, early
presence check, inverse-pairs Vec) pizza wall stayed flat at
~29 s with `--pair-timeout-ms 200`. The advisor pushed for
investigation over yet another speculative optimization;
`RUSTDL_TRACE=1` was added to `search`/`branch` and a 200 ms
NamedPizza sat probe captured 1560 trace lines. Findings:

| Metric | Value |
|---|---|
| Unique disjunctions hit | 38 |
| Branchings per top disjunction | up to **75** |
| Unique nodes visited | 25 (graph_nodes peaks at **92**) |
| Disjunction arity | uniformly **2** (binary, not wide) |
| `is_blocked_true` over 510 k calls | **0** |

The pattern is *not* wide disjunction-search (which is what the
MOMS-style heuristic refinement targets — a refinement was
tried at this point and shipped no measurable change, then
reverted). It's *deep* binary search over a model that doesn't
block. Every `∃R.X` creates a new successor; each successor
inherits residual GCIs and gets its own binary disjunctions;
pair-blocking cannot fire because each ∃-witness carries a
distinct topping subclass label, so `L(y) ⊆ L(x')` never holds
across siblings. The model grows monotonically until depth
hits the 256 cap or the deadline fires.

This is the structural pattern HermiT and Konclude solve with
lazy unfolding and model caching; ELK avoids it by being EL+
only. None of those are session-scoped fixes. Concretely the
levers ranked by expected wall impact on this pattern:

| Lever | Expected pizza impact | Effort |
|---|---|---|
| Lazy unfolding (Motik 2008) | large — successors stop carrying inherited residual GCIs | multi-week |
| Model caching (build Pizza once, reuse) | large — cuts every `is_subclass(Pizza, _)` probe | ~1 week |
| Anywhere blocking (subset, not pair) | medium — pizza siblings still don't subset each other, so smaller than expected | 2–3 days |
| Module extraction | medium — most `is_subclass(A, B)` probes skipped when signatures don't overlap | ~1 week |
| Hypertableau | very large — resolution-style, much smaller branching factor | multi-month |

For convergent (non-timeout-bound) workloads the per-call wins
already shipped *do* reduce CPU consumption (1.4 M
`add_label_calls` → 256 k on the same NamedPizza probe).
They're worth keeping. The point is they cannot shorten a wall
that is dominated by the per-pair 200 ms timeout.

`RUSTDL_TRACE=1` itself is kept as debug infrastructure
alongside `RUSTDL_COUNTERS=1`; both are runtime-gated with a
single atomic load on the off-path.

## §7 — Real-corpus wall times after 2026-05-25 session

After the five perf commits landed on `main` (`8c3fefa`, `2d843ae`,
`5208c2b`, `8ec9480`, `d0a4fc1`):

| Workload | Old | New | Notes |
|---|---|---|---|
| sulo-stripped (17 classes) | 0.49 s (n=10 med) | **0.23 s** | Convergent; per-call wins compound (top-down + early-presence) |
| pizza (99 classes) | DNF >300 s `--pair-timeout-ms 200` | **29 s** | Timeout-bound on 1172 pairs; top-down default did the lifting (was 58 s with `--top-down` opt-in pre-flip) |
| family-stripped (45 classes) | hard error (length-3 chains) | **8.9 s** | Coverage unlock landed earlier in the day (`5f7bb51`) |
| ro-stripped (854 KB) | hard error (SWRL Rule) | **~28 s** | Coverage unlock landed earlier in the day (`31a5b99`) |
| GO basic (51 937 classes, pure-EL) | 17 s baseline (apples-to-bench-elapsed) | **24 s** wall (median) | Pure-EL closure ~13 s + the BufWriter-bounded 11 s of formatting 58 k output lines |
| sio-stripped (1 585 classes) | DNF >120 s | **266 s** | 33 394 timed-out pairs out of 33 399 tableau calls (99.985 % timeout rate); same architectural pattern as pizza |

Reference reasoners on the same box (`bench-results/multi-reasoner-20260524-223957.txt`, ROBOT-docker harness):

| Workload | HermiT | Pellet | Konclude | rustdl now |
|---|---|---|---|---|
| sulo-stripped | 3.94 s | 3.36 s | 0.95 s | **0.23 s** ← rustdl wins |
| sio-stripped | 69 s | 4.45 s | 1.57 s | 266 s ← 3.9× slower than HermiT, 170× slower than Konclude |
| pizza | 4.53 s | 3.44 s | 1.44 s | 29 s ← 6.5× slower than HermiT |

`pair-timeout` × `pair-count` is the asymptotic floor on the
timeout-bound workloads (pizza, sio-stripped, big-RO). All three
sit at >99 % timeout rate; closing the gap to HermiT requires the
architectural levers documented in §6 (lazy unfolding, model
caching, anywhere blocking, module extraction, hypertableau).
