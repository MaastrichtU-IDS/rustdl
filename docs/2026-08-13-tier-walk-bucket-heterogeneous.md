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
