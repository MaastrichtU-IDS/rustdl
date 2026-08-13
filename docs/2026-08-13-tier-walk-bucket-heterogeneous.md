# The `tier_walk` bucket is heterogeneous too — and gdb cannot finish the job

2026-08-13. Follows the census correction that moved four `unsat_probe` members into
`tier_walk`, making it the best-supported target in the DNF tail.

## The shape generalises; the mechanism does not

All five members carry the shape diagnosed on `ore_ont_10019` — `EquivalentClasses` with an
`ObjectIntersectionOf` right-hand side, which `clause.rs::absorb_hard_antecedent`
clausifies as a disjunction over a shared conjunct:

| ontology | classes | EquivCls | of which `∩`-RHS | DataProperty |
|---|---|---|---|---|
| `ore_ont_10019` | 47 | 29 | **26** | **0** |
| `ore_ont_7828` | 831 | 23 | 19 | 98 |
| `ore_ont_10517` | 904 | 23 | 19 | 165 |
| `ore_ont_8273` | 316 | 30 | 9 | 31 |
| `ore_ont_934` | 108 | 16 | 6 | 143 |

But shape presence is not cause — the design record's own rule is that *a shape census
sizes a population and does not predict a rescue*. Sampling self-time during `tier_walk`:

| | `ore_ont_7828` | `ore_ont_934` |
|---|---|---|
| `??` (unresolved) | **41** | 6 |
| `{closure#1}` (unresolved) | **40** | **21** |
| `concrete_domain_clash` | 13 | 6 |
| `_int_free` / `libc_free` | 29 | — |
| `rotate_left<u64>`, `index<ConceptExpr>` | 28 | — |

**`concrete_domain_clash` — the datatype reasoner — appears on both, and it cannot be at
work on `ore_ont_10019` at all**, which has **zero** data properties. The function
early-outs on `self.dkey_ranges.is_empty()`, so it is free there and live on the other
four (31–165 data properties each).

So the bucket splits at least two ways:

* `ore_ont_10019` — no data content; the diagnosed clausification over-branching is the
  only candidate.
* `7828` / `10517` / `8273` / `934` — data-property-heavy, with the concrete-domain clash
  check live in the hot loop.

**Consequence: surrogate-atom absorption cannot be assumed to rescue the four new
members.** It is still the right fix for `10019`'s mechanism, but the bucket that looked
like a 35-member justification for it is not one mechanism. This is the *third* bucket in
this arc to fragment on inspection (`label_cache_build` and `prepare` were the others).

## Why this stops here: gdb cannot resolve the dominant frames

On `ore_ont_7828` the two largest self-time frames are `??` (41 samples) and
`{closure#1}` (40) — **81 of ~200 working samples are unattributable**. `concrete_domain_clash`
at 13 is real but not dominant, so naming the actual cost is not possible with the
available instrument.

This is the same limit flagged in the bucket-profile doc for `ore_ont_10140` and
`ore_ont_11460`, and it is now the **binding constraint** on further `tier_walk` work
rather than an annotation.

**`perf` is installed on `fsesrv-node000003` but not on `fsesrv-g1`, where this session
runs** — the two share `/data/dumontier` over NFS, which is why the repo looks identical
while `/usr/lib/linux-tools` does not exist here. Resolving these frames needs either
`linux-tools` on `fsesrv-g1` or the profiling re-run on `node000003`.

## What can be said without it

* `tier_walk` spends **98–113 s on ontologies of 108–904 classes**, which remains
  pathological on its face and unexplained.
* The `concrete_domain_clash` presence looked worth a targeted check, since it is
  refute-only and additive and `ore_ont_10517` has 165 data properties with **zero**
  `DataSomeValuesFrom` or `DatatypeRestriction` — i.e. possibly paying per-node for a check
  that can never fire.

## That hypothesis is REFUTED

`RUSTDL_DATA_PROPERTIES=0` empties `dkey_ranges`, so `concrete_domain_clash` early-outs
entirely. `tier_walk` does not improve:

| ontology | default | `DATA_PROPERTIES=0` | |
|---|---|---|---|
| `ore_ont_10517` | 98,175 ms | 107,308 ms | **worse** |
| `ore_ont_7828` | 98,606 ms | 102,192 ms | **worse** |
| `ore_ont_934` | 114,229 ms | 112,595 ms | flat |
| `ore_ont_8273` | 104,782 ms | 104,680 ms | flat |

So the 13-of-200 samples were real but marginal, and removing the check entirely buys
nothing. Cost 15 minutes and needed no profiler, which is why it was the right thing to try
before asking for one.

## Position: blocked on symbol resolution

Every cheap hypothesis for the `tier_walk` cost is now eliminated:

| hypothesis | outcome |
|---|---|
| clausification over-branching (the `10019` diagnosis) | **cannot apply** to the four data-heavy members' profile |
| `concrete_domain_clash` | **refuted** by ablation above |
| `env_read_lock` / flag reads | already fixed; 0 frames |
| divergence early-cut / `DIV_WINDOW` | measured null earlier in this arc |

What remains is the **81 of ~200 unattributable samples** (`??` 41, `{closure#1}` 40 on
`ore_ont_7828`). gdb cannot resolve them. Progress on `tier_walk` — 27–35 ontologies, the
second-largest bucket, spending 98–113 s on ontologies of 108–904 classes — requires
`perf` on `fsesrv-g1`, or the profiling re-run on `fsesrv-node000003` where it is already
installed.

**Do not propose another `tier_walk` lever before that.** Four have now been eliminated on
this bucket, and the pattern in this arc is that levers aimed at unattributed cost fail.


## perf, at last — and it overturns my own refutation

`linux-tools` installed on `fsesrv-g1`. The `/usr/bin/perf` wrapper refuses (running kernel
5.15.0-97 vs tools 5.15.0-187) but `/usr/lib/linux-tools/5.15.0-187-generic/perf` works and
resolves rustdl symbols with `--call-graph dwarf`; version skew is irrelevant for
user-space sampling.

`ore_ont_7828`, attached at t=25 s (inside `tier_walk`), 25 s, **34K samples**:

| % | symbol |
|---|---|
| 14.18% | `rules::apply_role_rules` + its closure (8.96 + 5.22) |
| 11.53% | `hyper::HyperEngine::match_body` |
| 11.77% | allocation — `malloc` 2.82, `_int_free` 3.43, `cfree` 2.32, `SmallVec::clone` 3.20 |
| **10.79%** | **`TableauContext::concrete_domain_clash`** |
| 9.76% | `hyper::HyperEngine::solve` |
| 5.98% | `[vdso] 0x6e8` (unresolved offset) |
| 4.46% | `core::hash::BuildHasher::hash_one` |
| 4.02% | `rules::apply_deferred_concept_or_rules` |
| 2.41% | `clash_deps_at` |

### MY REFUTATION OF `concrete_domain_clash` WAS INVALID

Above, I recorded it as "REFUTED by ablation" because `RUSTDL_DATA_PROPERTIES=0` did not
improve `tier_walk`. perf says it is **10.79%** — the second-largest single symbol.

The ablation was **not controlled**: `RUSTDL_DATA_PROPERTIES=0` removes the check *and*
deletes the data axioms, changing the reasoning problem. The tell was in the data and I
missed it — `ore_ont_10517` returned **more** rows under the ablation (1237 → 1253), i.e.
the search did *different* work, not *less*. This repeats a trap already recorded in the
design record: *"a controlled deletion is only controlled if the intervention changed ONE
thing."*

`concrete_domain_clash` is therefore a live ~11% target, and the honest way to test it is a
flag that skips the check while leaving the axioms in place — not `DATA_PROPERTIES=0`.

### And a lever killed before being built

The main tableau's `check_deadline` reads the clock on **every** call with no stride
(`lib.rs:711`, called from `search.rs:118` and three sites in `rules.rs`). Given the
unstrided design and a 5.98% unresolved `[vdso]` entry, striding it looked like an obvious
cheap win.

**It is not.** Clock reads by *named* symbol total **~1.2%** (`Timespec::now` 0.75% +
`clock_gettime` 0.44% + `__vdso_clock_gettime` 0.03%), and disabling both wedge deadline
flags changed `tier_walk` not at all (97,423 → 97,072 ms). The 5.98% `[vdso] 0x6e8` is a
different call — most plausibly `__vdso_getcpu` from glibc's malloc arenas, consistent with
11.77% in allocation.

### The real conclusion: there is no hot spot

The largest single area is 14%, and the profile is spread across role-rule application,
match enumeration, allocation, the concrete-domain check, and wedge search. **No single
lever produces a large win on this ontology.** That is why five successive levers failed
here: they were each aimed at one of these ~10% slices while the other 90% stayed.

Ranked by what perf actually supports:

1. **`concrete_domain_clash` ~11%** — largest *single* addressable symbol, and cheap to
   test properly (a skip-flag that keeps the axioms). Note `ore_ont_10517` has 165 data
   properties and **zero** `DataSomeValuesFrom`/`DatatypeRestriction`, so on that ontology
   the check may be structurally incapable of firing.
2. **Allocation ~12%** — diffuse; `SmallVec::clone` at 3.2% is the only named handle.
3. **`apply_role_rules` 14%** — largest area, but Phase 3e already attempted edge-keyed
   indexing here and was reverted at a +2.34% GALEN regression, so it is known-hard.
