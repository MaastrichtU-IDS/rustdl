# `build_told_tables` was O(n²) in memset traffic (2026-08-21)

**A per-class BFS allocated a fresh `vec![false; n]` on every iteration.** n allocations of n
bytes, so the closure build cost O(n²) in *zeroing alone*, independent of how sparse the
subsumption graph is.

```rust
for c in 0..n_u32 {
    let mut visited = vec![false; n];   // n bytes, allocated AND ZEROED, n times
```

On `ore_ont_9674` (**981,148 classes**) that is ~963 GB of zeroing.

## How it was found, and how the quadratic was confirmed

Not by reading the code — by following a bucket down. The chain was: the 20 s phase census's
`no-banner` bucket → `locality-stats` as a **conversion-bound detector** (parse+convert, no
reasoning) → `tbox-stats` to split parse from `convert_ms` → `perf`.

`perf record` on `tbox-stats` (parse + convert, **no reasoning**) attributed **69.74%** of
self-time to `__memset_avx2_unaligned_erms`, with `owl_dl_core::told::build_told_tables` the top
rustdl frame at 3.14%.

**The quadratic was then confirmed by FIT, not by inspection.** `convert_ms / n²` is constant
across six large ontologies spanning a 1.34× range of n:

| ont | classes | convert_ms | ms/n² |
|---|---:|---:|---:|
| `9674` | 981,148 | 53,965 | 56,059 |
| `868` | 981,151 | 53,954 | 56,047 |
| `10689` | 981,148 | 53,917 | 56,009 |
| `8486` | 903,617 | 45,286 | 55,462 |
| `14459` | 847,760 | 40,075 | 55,761 |
| `16008` | 733,100 | 30,913 | 57,519 |

**3.6% spread.** The cost is quadratic in the **CLASS COUNT**, not the file size — which is why the
corpus's *largest* file (`10926`, 557 MB) converted **fastest** (13.2 s): it has 176k classes, so
its quadratic term is small and other costs dominate. That inversion is what made the bucket look
inexplicable before the fit.

## The fix is exact, not approximate

`ups` receives a node **exactly** when that node is marked visited — every `visited[i] = true` is
immediately followed by `ups.push(i)`. So the visited set and `ups` are the same set, and clearing
the `ups` entries restores `visited` to all-false in **O(|ups|)** instead of O(n). The BFS is
otherwise untouched, so the tables are byte-identical by construction. `queue` is hoisted too.

## Measured

| | conversion | classify wall |
|---|---|---|
| `9674` | 53,965 → 8,675 ms (**6.2×**) | 61.9 → 32.6 s |
| `8486` | 45,286 → 7,578 ms (6.0×) | 52.7 → 28.4 s |
| `16008` | 30,913 → 6,148 ms (5.0×) | 55.7 → 24.5 s |
| `14459` | — | 68.0 → 27.6 s (**2.46×**) |

**Full A/B with answer identity, all six, single-thread, 300 s cap**
(`data-2026-08-21-told-hoist-ab-identity.tsv`):

| ont | pre | post | speedup | rows pre = post |
|---|---:|---:|---:|---|
| `10689` | 62.3 s | 31.1 s | **2.00×** | 981,144 = 981,144 |
| `868` | 63.4 s | 32.6 s | 1.94× | 981,144 = 981,144 |
| `9674` | 61.9 s | 32.6 s | 1.90× | 981,144 = 981,144 |
| `8486` | 52.7 s | 28.4 s | 1.86× | 903,613 = 903,613 |
| `14459` | 49.5 s | 27.1 s | 1.83× | 847,755 = 847,755 |
| `16008` | 40.4 s | 25.1 s | 1.61× | 733,098 = 733,098 |

Aggregate **330.2 → 176.5 s (1.87×)**, **6/6 byte-identical** output.

An earlier arm of this same measurement reported the walls as 458 → 250 s. Both are real; they
were taken under different background load (a 43-ontology convergence run was in flight for the
first). The **ratio** is stable at 1.83-1.87x across both, which is why the ratio is quoted rather
than the absolute walls. The first arm also reported `rows=-1` on *both* sides — an inline-python
extractor breaking on large JSON, not a reasoning failure; the identity numbers above come from
the file-based re-run.

## Why this bucket was worth attacking when `tier_walk` was not

**Conversion is not deadline-bound.** No budget bounds it (`--global-timeout-ms` charges from after
parse), so a conversion saving converts **1:1 into wall**. Contrast the same day's retracted
`tier_walk` finding, where `wall = #pairs × per-pair-deadline` and a 2× faster engine merely
performs 2× the branches inside the same 5 ms. **Before costing out any hot frame, ask whether a
deadline will absorb the saving.** Deadline-bound phases (`tier_walk` 13, `label_cache_build` 78 —
91 of the ~140 tail) are immune to engine speed; conversion-bound ones are not.

## Gates

* **FP=0 net: PASS** — every fixture with a present oracle VERIFIED, closures exact, **MISSED=0**.
  This is the load-bearing correctness gate here: the told tables feed the tier walk and the
  tableau, so a stale `visited` entry would shrink the closure and surface as MISSED>0.
* `tbox-stats` counters **identical 8/8** — with `convert_ms` **stripped**, since that timing line
  is in the output and a raw hash reports a spurious DIFFER.
* Curated `classify --json` **identical 6/6**; slow-family `classify --json` **identical 4/4** with
  exact row counts (981,144 / 903,613).
* Suite **1655/0/78** over the six affected crates. (`owl-dl-py`'s lib test fails to LINK with
  `undefined symbol: PyObject_GetAttr` — pre-existing pyo3, unrelated to a change in
  `owl-dl-core`.)
* Full two-arm 1,920-ontology sweep: **in flight** at time of commit.

## Corpus note: the ORE population contains near-duplicates

`868` / `9674` / `10689` are identical in size and rule count, as are `15059` / `16744` and
`10926` / `12898`. **Bucket counts over-count distinct ontologies** — the "12 non-DKey
conversion-bound" are ~7 distinct. Worth deduplicating before quoting any bucket size as a
population.

## Sibling instance in `role_hierarchy.rs`, and a commit-hygiene correction

Sweeping the tree for the antipattern (`vec![false; n]` allocated inside a loop over n) found
`RoleHierarchyBuilder::build` with the **identical** defect — same per-iteration allocation, same
BFS, same `ups` invariant — and the same exact fix applies.

**It carries NO measured corpus win, and is not claimed to.** `n` there is the ROLE count, whose
ORE maximum is 11,312 (median 24, mean 98) — ~128 MB of zeroing, about 13 ms. It was sized *before*
being applied, specifically so it would not be mis-sold as a sibling of the 6.2× told win. Applied
because it is the same defect class with an output-identical transformation, and it would bite an
ontology with ~100k roles.

The same sweep turned up something more useful: **`crates/owl-dl-reasoner/src/classify.rs` already
carries the canonical in-tree fix for this exact pattern**, using a generation counter, and its
comment names the hazard outright — *"the O(n²) `vec![false; n]`-per-class allocation (fatal on
55k-class onts)"*. So the pattern was known and fixed in **one** place while two others kept it.
**When touching any closure builder here, grep for `vec![false; n]` inside a loop.**

> **COMMIT HYGIENE — the `role_hierarchy.rs` change is in the WRONG COMMIT.** A `git add -A` for
> the convergence measurement swept it into **`6905aae`**, whose subject is
> `measure: 60% of the non-label-cache tail is genuinely STALLED, not slow`. Twenty lines of code
> therefore live inside a measurement-only commit, where nobody bisecting a performance change
> would look. Not rewritten, because `main` is shared and force-pushing it to fix attribution is
> the more expensive error. Recorded here instead — and the general fix is to stage explicit paths
> rather than `git add -A` when a measurement and a code change are in flight together.

## Sabotage: the clears ARE guarded (run, not assumed)

Per [[sabotage-your-own-guard-tests]], the correctness argument ("`ups` *is* the visited set") was
not taken on faith. Deleting each clear loop and re-running:

| sabotage | result |
|---|---|
| drop the clear in `role_hierarchy.rs` | **FAILS** `role_hierarchy::tests::linear_chain`, `::equivalence_cycle` |
| drop the clear in `told.rs` | **FAILS** `told::tests::sub_class_of_chain_closes_transitively`, `::equivalent_classes_creates_bidirectional_subsumption`, `::disjoint_union_contributes_both_subsumption_and_disjointness`, `::equivalence_with_complex_member_only_uses_atomic_pairs` |

So the buffer reuse is covered by pre-existing tests in both files — a stale `visited` entry
shrinks the closure and is caught immediately. Ordering was also verified by reading the patched
regions: `visited` is read only inside the BFS, and after the clear only `ups` is touched (sort,
`sub_closure` push, move into `super_closure`). Nothing reads `visited` between the clear and the
next iteration.

## Why no MISSED net was run, stated explicitly

This record is emphatic that the FP=0 net is **FP-shaped** and a sweep is **outcome-shaped**, and
that neither can see a lost entailment — which is why the corpus MISSED net exists for exactly this
kind of change.

It is not needed here, and the reason is specific: **byte-identity subsumes ΔMISSED for an
output-identical transformation.** Identical `classify --json` output cannot have lost a row. The
identity evidence is 298/298 (and rising) in the two-arm sweep, 6/6 on the slow family with exact
row counts, 6/6 curated, and `MISSED=0` in the FP=0 net. Had the change altered *any* output, the
MISSED net would be mandatory.

## Recovery count, with its cap attached

The two-arm sweep runs at a **60 s cap**, and the recovery count only means anything with that
stated. `ore_ont_10689` goes DNF → 44.7 s. The other slow-family members were *already* under 60 s
before the fix (`8486` 52.7 s, `14459` 49.5 s, `16008` 40.4 s), so they appear as both-ok identical
and contribute **zero** recoveries despite being 1.6–1.9× faster.

**So the honest headline is "≈3 recoveries at a 60 s cap, plus 1.87× on six large ontologies" — NOT
a reduction in the tail count.** The 140-member tail was censused at a different budget; moving that
number would require re-running the census on the post binary, which has not been done. Quote the
cap with the count, as the v0.4.21 record does.
