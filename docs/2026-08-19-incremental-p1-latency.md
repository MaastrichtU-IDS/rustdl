# P1 exit criterion: measured per-revision latency of `IncrementalSession`

**Date:** 2026-08-20
**Task:** 9 of `docs/superpowers/plans/2026-08-19-incremental-reasoning-p1.md`
**Criterion under test:** on galen, a single-axiom addition must complete in **≤ 2× the measured
5.8 ms lowering floor (≤ ~12 ms)** — see
[`2026-08-19-incremental-lowering-floor-findings.md`](2026-08-19-incremental-lowering-floor-findings.md) §4.

## Verdict

# FAIL.

**galen p50 per-revision latency is 886 ms against a 12 ms bar — 74× over.**
The measured speedup against a from-scratch `classify()` of the same ontology is **0.99×**:
the session is, to within noise, exactly as expensive as re-classifying from scratch. Against
the floor doc's ~13× galen ceiling that is **7.6 % of the ceiling**, and against
`classify_saturation_only` (the floor doc's honest denominator) the session is **11× SLOWER**,
not faster.

Per the task brief this is where the work stops and escalates. **Do not build P2 deletion on
this design until the cause below is addressed** — but read the cause first, because it is not
"retained state is too slow". It is the opposite.

## The numbers (galen, 100 revisions, median of 3 whole-run repetitions)

| quantity | run 1 | run 2 | run 3 | **median** |
|---|---|---|---|---|
| per-revision p50 (apply+classify) | 868.54 | 886.43 | 895.43 | **886.43 ms** |
| per-revision p95 | 968.59 | 936.68 | 957.88 | **957.88 ms** |
| per-revision max | 999.51 | 1033.23 | 1008.19 | **1008.19 ms** |
| per-revision min | 843.67 | 848.65 | 871.66 | 848.65 ms |
| **`apply` p50 alone** | 4.62 | 4.63 | 4.69 | **4.63 ms** |
| **`classify` p50 alone** | 863.78 | 881.80 | 890.26 | **881.80 ms** |
| baseline from-scratch `classify` | 856.60 | 873.71 | 927.00 | 873.71 ms |
| baseline from-scratch `classify_saturation_only` | 73.78 | 77.14 | 78.80 | 77.14 ms |

Session counters, identical in all three runs:

| counter | value |
|---|---|
| `revisions` | 100 |
| `rebuilds` | **1** (at revision 64) |
| `additions_reused` | **99** |
| `closure_answered` | **0** |

### Achieved vs ceiling

| denominator | measured | ceiling from the floor doc | achieved / ceiling |
|---|---|---|---|
| from-scratch `classify` (873.7 ms) | **0.99×** | — | — |
| from-scratch `classify_saturation_only` (77.1 ms) | **0.09×** | ~13× | **0.7 %** |

Read against the floor doc's own framing — "0.99× of a 13× ceiling" — the session captures
essentially none of the available headroom. For calibration, KM publishes 4.90× on its
addition-only EL++ microbench.

## Where the 886 ms goes, and why it is not the floor's fault

Split the revision. `apply` is **4.63 ms**; `classify` is **881.80 ms**. The floor doc measured
`convert_ontology(galen)` at 5.8 ms.

**The addition path already meets the criterion.** 4.63 ms is *below* the 5.8 ms full-lowering
floor (`apply` re-lowers only the delta plus the derivation overlay, not the whole file), and
99 of 100 commits were absorbed by `SaturationState::apply_additions` with no rebuild. Tasks 6
and 7 did their job. If the criterion were scoped to `apply`, galen would pass at 0.8× the floor.

**All of the failure is in `classify`, and it is a fragment-gate problem, not a performance
problem.** `IncrementalSession::recompute_classification`
(`crates/owl-dl-reasoner/src/incremental.rs:414`) gates the retained-closure answer on a single
predicate:

```rust
let full = if classify::is_pure_el(&self.internal) {
    self.stats.closure_answered += 1;
    classify::classify_from_closure(&self.internal, self.saturation.subsumers())
} else {
    classify::classify_top_down_internal(&self.internal, None, None)?
};
```

`is_pure_el` is false on galen — galen declares 150 `FunctionalObjectProperty` and 207
`InverseObjectProperties`, and `is_el_axiom` admits neither. So every revision takes the `else`
arm: the full hybrid top-down classifier, which **re-saturates the whole ontology from scratch**
(`let closure = saturate(internal);`, `classify.rs:1605`) and discards the closure the session
just spent 4.63 ms maintaining. `closure_answered == 0` is the direct evidence — that counter
exists precisely to distinguish "reused" from "silently re-derived everything".

The retained state is therefore not *slow*. It is **unused**.

### galen cannot reach the bar even with a wider gate

The obvious patch — widen the session's gate from `is_pure_el` to the classifier's own
`is_pure_el || saturator_complete_fragment || tbox_only_saturator_eligible` — **would not rescue
galen.** galen is off `saturator_complete_fragment` too. Measured A/B:

```
RUSTDL_HORN_SHORTCIRCUIT=1 (default)  owl-dl-bench classify galen  → 855.7 ms
RUSTDL_HORN_SHORTCIRCUIT=0            owl-dl-bench classify galen  → 871.9 ms
```

Flipping the Horn shortcircuit does not move galen's wall, so galen never took that fast path.
That reproduces the floor doc's own galen numbers (881.7 ms `classify` / 76.6 ms sat-only) and
contradicts the stale "galen classify ~0.59 s via the fast path" claim in `CLAUDE.md`.

This matters for the criterion itself. The ~13× galen ceiling in the floor doc is
`sat_only / convert` = 76.6 / 5.8, and it is only a meaningful ceiling **when the saturation
closure is the complete answer**. On galen it is not: galen's sound-and-complete classification
requires the hybrid path, and the EL closure is a sound under-approximation. So:

> **The ≤12 ms bar is unreachable on galen by construction, not merely unmet.** P1 retains a
> saturation closure; galen's answer is not computable from a saturation closure. The exit
> criterion was pointed at an ontology outside the fragment the feature serves.

The honest ceiling for a *sound* galen session is `full classify / apply` ≈ 873.7 / 4.63 ≈ 189×,
and P1's design offers no mechanism to approach it — nothing in P1 makes
`classify_top_down_internal` incremental.

## The reuse path is dead on the entire real corpus, not just galen

This is the finding that outranks the galen verdict. `closure_answered == 0` on **every** real
ontology measured, so no real ontology in the corpus ever benefits from the retained closure:

| ontology | classes | revisions | p50 | apply p50 | classify p50 | rebuilds | `closure_answered` | speedup vs from-scratch classify | ≤12 ms |
|---|---|---|---|---|---|---|---|---|---|
| sulo | 17 | 30 | 1.41 ms | 0.09 | 1.33 | 0 | **0** | 1.13× | PASS |
| mie | 84 | 30 | 4.63 ms | 0.40 | 4.25 | 0 | **0** | 0.95× | PASS |
| ro | 58 | 30 | 7.00 ms | 0.95 | 6.03 | 0 | **0** | 1.02× | PASS |
| sio | 1585 | 30 | 158.62 ms | 2.52 | 156.09 | 0 | **0** | 1.03× | FAIL |
| **galen** | **2748** | **100** | **886.43 ms** | **4.63** | **881.80** | **1** | **0** | **0.99×** | **FAIL** |
| synthetic pure-EL (400 cls) | 400 | 30 | 0.63 ms | 0.42 | 0.22 | 0 | **31** | **2.01×** | PASS |

Note what the sulo/mie/ro "PASS" rows actually mean: they pass the 12 ms bar only because those
ontologies are small enough that a **full from-scratch classify** already costs under 12 ms.
Their speedup is ~1.0×. A session buys them nothing; the bar is simply not discriminating at
that size. Only the synthetic pure-EL row exercises the mechanism the plan built, and there it
does work — `closure_answered = 31`, 2.01× against a from-scratch classify that is itself
saturation-only (1.28 ms vs 1.22 ms), i.e. close to that input's own convert-dominated ceiling.

**So: the machinery is correct and does pay off — on inputs inside `is_pure_el`. No ontology in
the local real corpus is inside `is_pure_el`.** The synthetic row is the only positive control,
and it was constructed for this measurement.

## `INITIAL_SLACK = 64` — the invented constant is real and visible

The brief flagged `INITIAL_SLACK = 64` (`incremental.rs:54`) as unmeasured. It is now measured.
A 70-revision per-revision trace on galen:

```
  rev    63: total=  889.55 ms  apply=   4.66  classify=  884.89
  rev    64: total=  949.10 ms  apply=  75.82  classify=  873.29  REBUILD
  rev    65: total=  870.29 ms  apply=   4.92  classify=  865.37
```

Slack exhausts after exactly 64 new named classes and forces a full `SaturationState::build`.
The rebuild costs **75.8 ms of `apply` against a 4.6 ms steady state — a 16.4× spike**, and it
equals the session's own cold build (75.4 ms) to within noise, as expected. Slack then doubles,
so the next rebuild would land at revision 192.

**Assessment: on galen this is not what fails the criterion** — it is one revision in 100, and
even a 75.8 ms `apply` is swamped by the 881 ms `classify`. But it is a genuine p-max
contributor that would dominate a *working* session: on the synthetic pure-EL input, where a
revision costs 0.63 ms, a 75 ms rebuild spike would be a **120× p-max outlier**. It should be
sized against the expected edit-burst length (P0's edit-locality measurement), not left at 64.

## Method

- **Host:** Apple M5 Max, 18 cores, 128 GB, macOS 26.5.2. Single machine, no load control
  beyond the note below. **These are ratios, not publishable absolute timings.**
- **Build:** `RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-bench`, rustc 1.96.0.
  Release for two reasons: the criterion is a timing one, and `classify(pizza.ofn)` trips a
  `debug_assert!` at `hyper.rs:3677` on debug builds (pizza was not measured regardless).
- **What else was running:** an interactive agent session and the editor. Load average during
  the galen runs was 8–20 on 18 cores, largely from the harness itself (rayon drives the
  classify path to ~1000 % CPU). Run-to-run p50 spread on galen was 868–895 ms (±1.5 %), which
  is small relative to a 74× miss, so contention does not change the verdict.
- **Repetitions:** each galen figure is the p50 over 100 revisions within a run, and the table
  reports the median across 3 whole-run repetitions. Baselines are medians of 3.
- **Deltas:** each revision adds one `SubClassOf(<urn:rustdl-bench-gen:i>, <anchor>)` where
  `anchor` is the first reported class. **Class axioms on purpose** — the session rebuilds
  unconditionally on any property-axiom addition and on any removal (P1 is addition-only), so a
  property delta would time the rebuild path and measure nothing about retention.
- **Reproduce:**
  ```sh
  RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-bench
  ./target/release/owl-dl-bench incremental-latency ontologies/external/galen.ofn --revisions 100
  ./target/release/owl-dl-bench incremental-latency ontologies/external/galen.ofn \
      --revisions 70 --baseline-repeats 0 --per-revision      # the slack-exhaustion trace
  ```
  galen is **not** committed (1.2 MB; `/ontologies/` is gitignored — corpus fetched by xtask,
  not vendored). The subcommand takes a path and hard-errors with a pointer to
  `scripts/fetch-real-ontologies.sh` when the file is absent.

## Things that could make you distrust these numbers

1. **Two open production defects sit under the measurement.**
   [`dkey-id-aliasing-classify-fp.md`](known-limitations/dkey-id-aliasing-classify-fp.md) — not
   triggered here; galen declares no data properties, so no `DKey` ids are minted.
   [`top-down-classify-misses-equivalences.md`](known-limitations/top-down-classify-misses-equivalences.md)
   — **is** on the measured path: galen's session answers via `classify_top_down_internal`, which
   is the incomplete walk. The ratio is unaffected (baseline and session take the same code
   path), but the absolute 881 ms is the cost of an *incomplete* classification. Fixing that
   incompleteness would make the per-revision number **worse**, not better.
2. **`classify_saturation_only` is not a valid answer for galen**, so the 0.09× figure against it
   is a scale reference, not a like-for-like comparison. The like-for-like number is 0.99×.
3. **Timer scope.** `apply` and `classify` are timed separately with `Instant`, and the reported
   total is their sum, so the split is exact but excludes the `AxiomDelta` construction (a
   handful of allocations, sub-microsecond).
4. **The anchor class is fixed** across all revisions, so every synthetic leaf attaches at the
   same point. A spread of anchors would touch more of the closure. This makes the measurement
   *optimistic* for the incremental path, which strengthens a FAIL.
5. **Cache warmth.** The initial `classify()` is excluded from the samples; revision 0 is
   included and shows no warmup artifact (833 ms vs an 865 ms p50 in the traced run).

## What to decide before P2

Stated plainly, because the task exists to produce this signal:

1. **P1's retained state works and meets the floor on the path it owns** (`apply` = 4.63 ms
   ≤ 5.8 ms, 99 % reuse). Nothing measured here argues for re-examining Tasks 5–7.
2. **P1's retained state is never consumed on real input.** The gate at
   `incremental.rs:414` is `is_pure_el`, which no local real ontology satisfies. Widening it to
   the classifier's own three-way gate is a cheap and obvious next step, but it is **unvalidated
   for galen** (galen fails that gate too) and needs a fragment survey before anyone claims it
   helps.
3. **The exit criterion needs re-pointing.** A ≤12 ms bar derived from a saturation-only ceiling
   should be measured on an ontology whose complete answer *is* the saturation closure. The floor
   doc already names the right target and says it is missing locally: **GO-basic, ~52k classes,
   the `classify_pure_el` zero-tableau path**. Until an in-fragment ontology at scale is measured,
   the feature has no honest headline.
4. **`INITIAL_SLACK` should be sized, not guessed** — see above.
