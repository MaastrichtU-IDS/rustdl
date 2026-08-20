# P1 exit criterion: measured per-revision latency of `IncrementalSession`


> **Provenance caveat (added 2026-08-20).** rustdl is developed on two machines. Every number in
> this document was measured on **Apple M5 Max / 128 GB** against `ontologies/external/galen.ofn`
> at **sha256 `4b3f900883a9b59c…`** (1,241,952 bytes; 2,748 classes; 207 `InverseObjectProperties`).
> That file declares **no ontology IRI and no versionIRI**, and it is **not** fetched by
> `scripts/fetch-real-ontologies.sh` — so it cannot be identified across machines by anything but
> its hash. Do not compare these figures to measurements from another host or another galen copy
> without first confirming the hash matches. See
> `docs/known-limitations/galen-off-the-fast-path.md`.


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

**Two things are true and the record needs both.** (1) The bar was pointed at the wrong ontology:
galen's complete classification is provably not computable from the EL closure P1 retains, so
≤12 ms was unreachable there by construction. (2) Running it anyway caught a real P1 scope
finding: **`classify` is whole-ontology on every path except `is_pure_el`, and no ontology in the
local corpus satisfies `is_pure_el` — so P1's end-to-end value is 1.0× on 100 % of the real
corpus.** Re-pointing the criterion at a friendlier ontology without recording (2) would lose the
substantive result of this task.

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
galen.** Two one-command checks settle it.

**(a) galen is outside `saturator_complete_fragment`, by inspection.** `is_saturator_axiom`
(`classify.rs:1340-1428`) is a strict allowlist with **no `InverseObjectProperties` arm**; its
terminal comment names the exclusion outright (`classify.rs:1423-1427`: "EXCLUDED ⟹ fall back to
the hybrid path. All ABox assertions; InverseObjectProperties decls; …"). galen's axiom histogram:

```console
$ grep -oE '^[A-Za-z]+\(' ontologies/external/galen.ofn | sort | uniq -c | sort -rn
3237 SubClassOf(   3161 Declaration(   699 EquivalentClasses(   416 SubObjectPropertyOf(
 207 InverseObjectProperties(          150 FunctionalObjectProperty(   26 TransitiveObjectProperty(
```

207 `InverseObjectProperties` (and 150 `FunctionalObjectProperty`, which is what already fails
`is_pure_el`). One is enough to fail the `all(...)`. Confirmed at runtime: `rustdl classify galen`
prints `# mode: hybrid (saturation + tableau)`.

**(b) The EL closure is not galen's answer — it misses four real subsumptions.** This is the
decisive point, because it makes the gap a *fragment* fact rather than a tuning knob:

```console
$ rustdl classify --json ontologies/external/galen.ofn            > full.json   # 3291 direct
$ rustdl classify --json --saturation-only ontologies/external/galen.ofn > sat.json  # 3290 direct
```

Present in the hybrid answer, absent from the saturation closure:

```
Femur            ⊑ BodySpace
Tibia            ⊑ TibialPlateau
TibialTuberosity ⊑ TibialInterCondylarEminence     (the documented back-fold pair)
TricuspidValve   ⊑ ForamenOvale
```

(Both runs agree on all 19 equivalence groups. The saturation-only run shows three subsumptions
the hybrid run does not — `TibialTuberosity ⊑ Eminence`, `TibialTuberosity ⊑
MirrorImagedBodyStructure`, `TricuspidValve ⊑ HeartValve` — which are not extra entailments but
*direct*-parent restatements surfacing because the four above are missing.)

So **galen's complete classification is not computable from the EL closure P1 retains, even in
principle.** No widening of the fragment gate can put galen under the 12 ms bar without changing
the answer — that is exactly the D10 "unsound completeness" bug class the allowlist exists to
prevent.

**A null result, recorded so nobody repeats it.** An `RUSTDL_HORN_SHORTCIRCUIT` A/B looks like it
should settle (a) and does not: `=1` → 855.7 ms, `=0` → 871.9 ms. That ~2 % delta is *inside* this
harness's own ±1.5 % run-to-run noise, so it proves nothing on its own. Evidence (a) and (b) above
are what settle it.

**A third production defect falls out of this**, filed separately as
[`known-limitations/galen-off-the-fast-path.md`](known-limitations/galen-off-the-fast-path.md):
`classify.rs:1250-1252`, `classify.rs:1274-1276` and `CLAUDE.md:814-815` all assert galen keeps the
saturation fast path at ~0.5–0.59 s, and `classify.rs:1252` cites a test
`galen_notgalen_in_saturator_fragment` that **does not exist anywhere in `crates/`** (the comment
is its only occurrence in the repo). galen measures 874 ms hybrid against 77 ms for the fast path
it is documented to take. `CLAUDE.md` and `classify.rs` are deliberately **not** edited here —
whether the regression or the documentation is the thing to fix is the owner's call.

### What this means for the criterion

The ~13× galen ceiling in the floor doc is `sat_only / convert` = 76.6 / 5.8, and it is only a
meaningful ceiling **when the saturation closure is the complete answer**. Per (b), on galen it is
not. So:

> **The ≤12 ms bar is unreachable on galen by construction, not merely unmet.** P1 retains a
> saturation closure; galen's answer is not computable from one. The criterion was pointed at an
> ontology outside the fragment the feature serves.

The honest ceiling for a *sound* galen session is `full classify / apply` ≈ 873.7 / 4.63 ≈ 189×,
and P1's design offers no mechanism to approach it — nothing in P1 makes
`classify_top_down_internal` incremental.

**But the criterion was not merely mis-specified, and retiring it on that basis would lose its
real catch.** Pointing the bar at galen was wrong; *running* it surfaced a genuine P1 scope
finding the criterion was entitled to catch, and which no better-chosen ontology would have
exposed as sharply:

> **`classify` is whole-ontology on every path except `is_pure_el`.** Since no real ontology in the
> corpus satisfies `is_pure_el`, P1's end-to-end value is **1.0× on 100 % of the real corpus** —
> regardless of how good the retained state is. That is a scope defect in P1, not a measurement
> artifact, and it would have gone unrecorded if the criterion had been quietly re-pointed at a
> friendlier ontology.

Both things are true: the bar was aimed at the wrong ontology, **and** it caught a real scope
problem. Keep both in the record.

## The reuse path is dead on the entire local corpus, not just galen

This is the finding that outranks the galen verdict.

**All eight local ontologies classify `hybrid`** — the session's `is_pure_el` reuse arm is
unreachable on every one of them:

```console
$ for f in sulo mie ro sio paper5 pizza family galen; do rustdl classify --pair-timeout-ms 25 $f.ofn | grep '^# mode:'; done
sulo    # mode: hybrid (saturation + tableau)      # fragment: out-of-EL
mie     # mode: hybrid (saturation + tableau)      # fragment: out-of-EL
ro      # mode: hybrid (saturation + tableau)      # fragment: out-of-EL
sio     # mode: hybrid (saturation + tableau)      # fragment: out-of-EL
paper5  # mode: hybrid (saturation + tableau)      # fragment: out-of-EL
pizza   # mode: hybrid (saturation + tableau)      # fragment: out-of-EL
family  # mode: hybrid (saturation + tableau)      # fragment: out-of-EL
galen   # mode: hybrid (saturation + tableau)      # fragment: Horn
```

(galen's `# fragment: Horn` is the **pre-D10** clausal-Horn banner, which D10 itself documents as
an unsound gate — see the known-limitations doc. The dispatcher no longer honours it.)

And `closure_answered == 0` on every one measured with a session, so no real ontology in the
corpus ever benefits from the retained closure:

| ontology | classes | revisions | p50 | apply p50 | classify p50 | rebuilds | `closure_answered` | speedup vs from-scratch classify | ≤12 ms |
|---|---|---|---|---|---|---|---|---|---|
| sulo | 17 | 30 | 1.41 ms | 0.09 | 1.33 | 0 | **0** | 1.13× | PASS |
| mie | 84 | 30 | 4.63 ms | 0.40 | 4.25 | 0 | **0** | 0.95× | PASS |
| ro | 58 | 30 | 7.00 ms | 0.95 | 6.03 | 0 | **0** | 1.02× | PASS |
| sio | 1585 | 30 | 158.62 ms | 2.52 | 156.09 | 0 | **0** | 1.03× | FAIL |
| **galen** | **2748** | **100** | **886.43 ms** | **4.63** | **881.80** | **1** | **0** | **0.99×** | **FAIL** |
| **synthetic pure-EL (400 cls)** | 400 | 100 | **0.65 ms** | 0.44 | 0.22 | 0 | **101** | **1.69×** | PASS |

Note what the sulo/mie/ro "PASS" rows actually mean: they pass the 12 ms bar only because those
ontologies are small enough that a **full from-scratch classify** already costs under 12 ms.
Their speedup is ~1.0×. A session buys them nothing; the bar is simply not discriminating at
that size.

### The positive control

Only the synthetic pure-EL row exercises the mechanism the plan built, and there it does work:
`closure_answered = 101` (initial + 100 revisions) and **1.69×** against a from-scratch classify
that is itself saturation-dominated (1.08 ms `classify` vs 1.06 ms `classify_saturation_only`) —
i.e. close to that input's own convert-bound ceiling, since at 400 classes lowering is most of the
cost. Median of 3 whole runs; per-run speedup ranged 1.65×–1.97×, which is the honest spread for a
sub-millisecond measurement. Task 9's reviewer independently built their own 400-class partonomy
and got `closure_answered = 31` at 2.61× — different shape, same direction.

**Reproduce it from the repo** — this input is generated, not vendored:

```sh
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-bench
./target/release/owl-dl-bench emit-synthetic-el /tmp/synth-pure-el.ofn --classes 400
rustdl classify --pair-timeout-ms 25 /tmp/synth-pure-el.ofn | head -2
#   → # mode: pure EL (saturation-only)
#     # fragment: pure-EL (trust_sat sound by construction; saturator alone is complete)
./target/release/owl-dl-bench incremental-latency /tmp/synth-pure-el.ofn \
    --revisions 100 --baseline-repeats 9
```

`emit-synthetic-el` writes a balanced binary tree — `SubClassOf(:Ci :C{i/2})` for every class plus
an `ObjectSomeValuesFrom(:partOf :C{i/2})` on every 7th — which is inside `is_pure_el` by
construction. It is a tree rather than the existing `synthetic-el` chain because the chain's
closure is quadratic in depth: a 2000-class chain did not finish a 100-revision run in ten minutes.

**So: the machinery is correct and does pay off — on inputs inside `is_pure_el`. No ontology in
the local corpus is inside `is_pure_el`.** The synthetic row is the only positive control, and it
had to be constructed for this measurement.

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

**Assessment: Minor, tail-latency only — not what fails the criterion, and not a throughput
problem either.** On galen it is one revision in 100, and even a 75.8 ms `apply` is swamped by the
881 ms `classify`.

An earlier draft of this doc called it "a 120× p-max outlier on a working session" and that
**overstates it**, because slack *doubles* on each exhaustion. Rebuilds land at 64, 192, 448, 960,
… — additions between rebuilds double each time, so over `N` additions the number of rebuilds is
`O(log N)` and their total cost is a geometric series. **Amortized per-revision rebuild cost tends
to zero as the session runs.** Only the *first* rebuild, at revision 64, is meaningfully
unamortized, and it costs one extra cold engine build (75.8 ms).

What remains is a genuine **tail-latency** artifact: on a session whose steady-state revision costs
0.65 ms (the synthetic pure-EL control), a single 75 ms revision is a visible hitch for an
interactive editor even though it never shows up in throughput. That argues for sizing
`INITIAL_SLACK` against the expected edit-burst length (P0's edit-locality measurement) rather than
leaving it guessed at 64 — but as a p99 smoothing question, not a performance defect.

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
- **Percentiles:** nearest-rank, no interpolation — every reported figure is a duration some
  revision actually took. `p50` on an even-length sample is therefore the lower median. Pinned by
  five unit tests in `crates/owl-dl-bench/src/main.rs` (`percentile_*`), because every headline
  number in this doc passes through that one function.
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

  # the positive control, generated rather than vendored:
  ./target/release/owl-dl-bench emit-synthetic-el /tmp/synth-pure-el.ofn --classes 400
  ./target/release/owl-dl-bench incremental-latency /tmp/synth-pure-el.ofn --revisions 100
  ```
  galen is **not** committed (1.2 MB; `/ontologies/` is gitignored — corpus fetched by xtask,
  not vendored). The subcommand takes a path and hard-errors with a pointer to
  `scripts/fetch-real-ontologies.sh` when the file is absent.

## Things that could make you distrust these numbers

1. **Three open production defects sit under the measurement** (the third is filed by this task).
   [`dkey-id-aliasing-classify-fp.md`](known-limitations/dkey-id-aliasing-classify-fp.md) — not
   triggered here; galen declares no data properties, so no `DKey` ids are minted.
   [`galen-off-the-fast-path.md`](known-limitations/galen-off-the-fast-path.md) — **is** the reason
   the galen baseline is 874 ms rather than the ~0.5–0.59 s the repo documents. It does not
   invalidate the ratio (both sides of the 0.99× take the same path), but it means the *denominator*
   is a regressed number, and a fixed galen fast path would make the session's relative showing
   worse, not better.
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
5. **The population grows during the run, so the samples are not drawn from one distribution.**
   Each revision adds a class, so the ontology goes 2748 → 2848 classes over 100 revisions and
   per-revision cost drifts *upward*: in the traced run, rev 0 ≈ 833 ms and rev 66 ≈ 946 ms. The
   p50 is therefore a mid-run figure and the max is partly a late-run figure, not a pure tail.
   Two consequences: the p95/max understate nothing but *overstate* the variance attributable to
   noise, and a longer run would report a worse p50 purely from growth. This does not affect the
   verdict (a 74× miss survives a ~14 % drift) but it is the reason the p95 is only 8 % above p50.
6. **Cache warmth is the wrong end to worry about.** The initial `classify()` is excluded from the
   samples, and revision 0 shows no warmup artifact — it is the *cheapest* revision, not the
   dearest. Growth (above), not warmup, is what moves this measurement.

## What to decide before P2

Stated plainly, because the task exists to produce this signal:

1. **P1's retained state works and meets the floor on the path it owns** (`apply` = 4.63 ms
   ≤ 5.8 ms, 99 % reuse). Nothing measured here argues for re-examining Tasks 5–7.
2. **P1's retained state is never consumed on real input.** The gate at
   `incremental.rs:414` is `is_pure_el`, which no local real ontology satisfies. Widening it to
   the classifier's own three-way gate is a cheap and obvious next step, but it is **unvalidated
   for galen** (galen fails that gate too) and needs a fragment survey before anyone claims it
   helps.
3. **The exit criterion needs re-pointing — but not retiring.** A ≤12 ms bar derived from a
   saturation-only ceiling should be measured on an ontology whose complete answer *is* the
   saturation closure. The floor doc already names the right target and says it is missing
   locally: **GO-basic, ~52k classes, the `classify_pure_el` zero-tableau path**. Until an
   in-fragment ontology at scale is measured, the feature has no honest headline. **Re-point it,
   do not drop it** — the same run that failed on the wrong ontology produced finding 2, which is
   the substantive result of this task.
4. **`INITIAL_SLACK` is Minor, tail-latency only.** Rebuild cost amortizes geometrically
   (`O(log N)` rebuilds over `N` additions); only the first, at revision 64, is unamortized. Worth
   sizing against P0's edit-locality data as a p99 smoothing question, not a performance defect.
5. **A third production defect was filed on the way**, unrelated to the incremental work but
   found by it: [`galen-off-the-fast-path.md`](known-limitations/galen-off-the-fast-path.md).
   Three places in the repo claim galen classifies on the saturation fast path at ~0.5–0.59 s, one
   citing a test that does not exist; galen measures 874 ms hybrid. `CLAUDE.md` and `classify.rs`
   were deliberately left unedited — that correction needs the owner's call.
