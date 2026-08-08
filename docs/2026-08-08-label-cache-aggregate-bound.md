# Aggregate bound on the label-cache build phase (`RUSTDL_LABEL_CACHE_TOTAL_MS`)

2026-08-08. Opt-in, default unbounded. Follows the v0.4.16 per-class deadline fix
(`RUSTDL_HYPER_MATCH_DEADLINE`) and the profiling in
`docs/known-limitations/label-cache-build-unbounded.md`.

## Why a second bound exists

`RUSTDL_LABEL_CACHE_TIMEOUT_MS` (and its adaptive default) bounds **one class**.
The phase costs `n × per-class`, and `n` reaches 8,025 on the affected
ontologies, so no per-class value bounds the phase. Profiling the 11
`label_cache_build`-dominated DNF-tail members found the **median** per-class
overshoot is **0 ms** — most classes are instant — with a tail of 400–560 ms
classes. Even a *perfect* 10 ms per-class bound leaves 1,682–8,025 classes ×
10 ms = **17–80 s**.

## The direction of risk is unusual, and it bounds what this can cost

The label cache is a **prune**: the orchestrator skips `subsumes_via_tableau`
when `D ∉ labels(C)`, justified by a counterexample model. An unbuilt label is
`NoVerdict`, which **disables** the prune. So cutting the phase short cannot
lose a subsumption by removing an inference — it removes an *optimisation*, and
the tier walk then does **more** tableau probes, not fewer.

The cost is therefore indirect: those extra probes consume the per-pair budget,
so pairs can time out that previously were never probed. That makes this a
genuine trade, not a free win — which is why the value was settled by
measurement.

## Measured: it binds, and on `ore_ont_16056` it wins

`ore_ont_16056`, `--pair-timeout-ms 50`, quiet host, arms differing **only** in
the flag:

| `RUSTDL_LABEL_CACHE_TOTAL_MS` | `label_cache_build` | wall | rows |
|---|---|---|---|
| unset | 30,909 ms | 111.9 s | 487 |
| **5000** | **5,018 ms** | **89.9 s** | 487 |
| 15000 | 15,018 ms | 96.5 s | 487 |

Three things this establishes:

1. **The instrument fires**, by a criterion declared in advance: `label_cache_build`
   tracks the requested budget to within 18 ms. A bound that did nothing would
   show 30,909 in every row.
2. **Answers are identical** — 487 rows in all three arms.
3. **The indirect cost did not materialise here**: 25.9 s of skipped label-cache
   work converts to 22 s of saved wall (−20%). The extra probing the disabled
   prune implies cost ~4 s, not the whole saving.

## Two corrections this work forced

**1. I RAISED A CONTRADICTION THAT DID NOT EXIST — and this is the more useful
lesson.** I re-measured `ore_ont_16056` at *default* settings, saw it DNF at
200 s, and wrote up the v0.4.16 record (`dnf/150s → ok/485r/17s`) as
non-reproducing. It reproduces fine. The recovery arm's conditions are stated
plainly in `label-cache-build-unbounded.md` — `LABEL_CACHE_TIMEOUT_MS=50`,
`--pair-timeout-ms 1`, threads=1, pinned sha — and that same doc *already* says
"At *default* budgets `16056` still DNFs, because the default per-class budget is
`clamp(n × per_pair, …)` = 30 s × 309 classes." **I declared a contradiction
without reading the conditions attached to the number I was contradicting.**
The correction is withdrawn.

What survives is narrower and still worth having: at **default** `--pair-timeout-ms
1000`, `16056`'s wall is dominated by the pair loop, not the label cache
(`label_cache_build=30,909` of a >200 s run). So "label-cache-bound" is a
statement about a *configuration*, not about an ontology — which is exactly the
distinction the cluster analysis below turns on.

**2. A whole sweep was void'd for an invalid flag.** The first recovery sweep
passed `--threads 1`, which `rustdl classify` does not accept. All 33 cells were
instant argument errors that the script printed as `dnf/150s`, and the resulting
"zero recoveries" table read exactly like a real negative result. The fix is in
the harness, not in vigilance: `/tmp/lcagg2.sh` onward carry a guard that runs a
known-good invocation first and **aborts the sweep** unless it produces a banner,
plus an explicit `ARGERR` cell that can never be confused with a DNF. That guard
fired on its first use (mis-calibrated, since the banner prints twice), which is
the behaviour wanted from it. Single-threading here is `RAYON_NUM_THREADS=1`.

Related, and the reason the second sweep's walls are also not quotable: it ran
while `cargo test`/`clippy` were running in the same session — 15-minute load
average **16.49**. Wall-clock arms need an idle host; the `16056` table above
was re-measured alone.

## Recovery: NO-GO, and the freed time provably does not pay

Zero of the 11 recovered — at default settings **and** at `--pair-timeout-ms 50`,
the budget at which such ontologies can finish at all (validated harness: binary
marker checked, known-good-invocation guard, `ARGERR` distinguished from DNF).

The `--global-timeout-ms` diagnostic is what settles *why*, and it is the first
direct look inside these runs — a DNF prints no banner, so the phase breakdown
had never been observable. Forcing a 100 s global budget makes them report:

**These five ARE genuinely label-cache-dominated** (unlike `16056`):

| ontology | classes | `label_cache_build` | `tier_walk` |
|---|---|---|---|
| 6134 | 1,682 | 101,593 ms | 6 ms |
| 12432 | 2,748 | 80,966 | 16,924 |
| 10080 | 3,533 | 99,886 | 10 |
| 13122 | 7,120 | 125,301 | 13 |
| 6910 | 6,131 | 100,721 | 20 |

Note `101,593 ms` under a **100,000 ms** global budget: the phase overruns the
global deadline outright.

**The bound works exactly as designed** — and it still buys nothing. Same 100 s
global budget, arms differing only in the flag:

| ontology | baseline rows | `TOTAL=5000` rows | Δ | `tier_walk` |
|---|---|---|---|---|
| 6134 | 2,349 | 2,355 | **+6** | 5 → 93,829 ms |
| 12432 | 3,311 | 3,311 | 0 | 17,758 → 88,753 |
| 10080 | 3,858 | 3,858 | 0 | 7 → 88,790 |
| 13122 | 6,442 | 6,442 | 0 | *did not bind* |
| 6910 | 10,072 | 10,072 | 0 | 21 → 68,974 |

**~95 seconds of wall is redirected from the label cache into the tier walk, and
it yields 6 rows out of 2,349 on one ontology and zero on the other four.** This
is the indirect cost predicted in the canary docstring, confirmed at scale: an
unbuilt label disables the prune, the tier walk then probes far more, and that
probing does not pay. The lever is therefore **not** a recovery lever for the
DNF tail, and the aggregate-bound conclusion drawn from profiling — that these
ontologies are aggregate-bound and an aggregate bound is what they need — was
**half right**. They *are* aggregate-bound; bounding the aggregate does not
rescue them, because the phase it starves is the one paying for the phase that
follows.

**`ore_ont_13122` did not bind at all** (`label_cache_build=113,989` against a
5,000 ms budget). The bound is checked at the top of each class's iteration, so a
**single** class whose `classify_labels` call runs ~100 s cannot be interrupted.
That is the same residual unguarded region inside `solve` that the v0.4.16
per-class fix reduced but did not close (~85 ms mean, 560 ms max overshoot on
`ore_ont_6134`). An aggregate bound cannot be tighter than one class.

## The pre-registered rule was right, and my reasoning overturned it wrongly

`label-cache-build-unbounded.md` pre-registered a decision rule before the
v0.4.16 cluster run: *"≥6 ⇒ the aggregate-bound work is justified and urgent;
2–5 ⇒ keep the flag OFF; ≤1 ⇒ the fix is a correctness repair only and the
cluster needs a different mechanism."* The run recovered **1 of 12**, and the
doc concluded, correctly: **"the aggregate-bound follow-up does NOT inherit a
justification from it."**

I then overturned that on a mechanism argument — profiling showed median
per-class overshoot of 0 ms and a tail of 400–560 ms classes, from which "these
are aggregate-bound, so an aggregate bound is what they need" follows very
naturally. It is also wrong, and the measurement above says so: bounding the
aggregate frees ~95 s and buys 6 rows out of 2,349 on one ontology.

**A pre-registered rule outperformed a plausible post-hoc mechanism argument.**
The argument was not sloppy — it was quantitative, correctly derived from real
profiling, and it predicted the wrong thing anyway, because it reasoned about
which phase *consumes* the wall without asking what that phase *buys* the phase
after it. When a pre-registered rule and a fresh mechanism story disagree, the
cheap move is to measure the story's own prediction, not to re-derive the rule.

## Status: opt-in, NOT a default candidate

Default **unbounded** (opt-in). The measured case for it is **one ontology**
(`ore_ont_16056`, −20% wall, identical answers) against **zero** recoveries and
**zero** meaningful row gain across five tail members. That is a narrow
throughput knob, not a default flip, and it should not be quoted as more than
one instance. `0`, unset, and unparseable all mean unbounded;
an unparseable value must not silently become a bound. Canaries:
`crates/owl-dl-reasoner/tests/label_cache_total_budget.rs` (6).

**Sabotage: 4 of 4 caught** — dead flag, always-bounded, `0`-means-zero,
garbage-means-zero. **Honest limit:** the dead-flag sabotage was caught by *one*
test, the parse canary. The two behavioural canaries assert `bounded == unbounded`
on a small fixture, which holds trivially when the flag does nothing — they are
**non-regression only** and do not guard the bound's *effect*. The effect is
guarded by the `16056` measurement above, not by a unit test.

The fixture is deliberately **out-of-EL** (`ObjectAllValuesFrom`): on the pure-EL
fast path the label cache is never built, so an EL fixture would make both
behavioural canaries vacuous. Both assert `!pure_el_mode` as a precondition.

## Follow-on: the per-pair budget overshoots too, and the stride is NOT why

The `--global-timeout-ms` diagnostic that exposed the label-cache phase also exposes
the pair loop. On `ore_ont_6134` at `--pair-timeout-ms 50` (global 240 s), the
wedge-cost histogram puts **19,906 pairs in the 100–999 ms bucket** and 26 at
≥1000 ms — a **2–20× overshoot** of the budget. The pattern is budget-relative: at
`pair=1000`, 4,314 land in the ≥1000 ms bucket and only 97 in 100–999. So pairs
consistently consume their full budget and spill into the next bucket.

**Consequence worth flagging beyond this ontology:** a per-pair budget below the
overshoot scale is silently not honoured. That includes the documented
`--pair-timeout-ms 25` wine guidance and the 50 ms adaptive label-cache floor —
neither delivers the budget it names.

**Hypothesis tested and REFUTED: it is not `MATCH_DEADLINE_STRIDE` granularity.**
Made the stride env-tunable and swept it (prediction declared first: the 100–999 ms
bucket shrinks, `ran` rises, rows ≥ baseline):

| stride | rows | fallthrough `ran` | 100–999 ms | ≥1000 ms |
|---|---|---|---|---|
| 4096 | 2,360 | 22,688 | 19,525 | 82 |
| 256 | 2,360 | 22,237 | 19,180 | 46 |
| 64 | 2,360 | 22,194 | 19,640 | **17** |

The probe demonstrably **fires** — the ≥1000 ms tail falls 82 → 17 (4.8×) — which is
what makes the flat 100–999 ms column a finding rather than a non-firing instrument.
Rows are identical at 2,360 across all three arms. **So the bulk overshoot is not in
`enumerate_matches`.** The experimental patch was reverted; the residual unguarded
region is now localised by exclusion, and the candidates are the regions `solve`
enters after its entry-time deadline check (notably the `horn_fixpoint` drain and
disjunctive branching).

## Follow-on: `rescued=0` was a budget artifact for the SECOND time

The wedge-stall→tableau fallthrough reported `ran=9325 rescued=0` at a 100 s global
budget, which reads like a pure-waste path worth deleting. It is not. Raising the
global budget to 240 s gives `ran=21596 rescued=12`. Rescues then *fall* as the
per-pair budget grows (12 → 4 → 0 → 0 at 50/200/1000/5000 ms) because `ran` collapses
21,596 → 416 under a fixed global budget: **rescues track throughput, not per-pair
generosity.** A prior session killed a lever on exactly this artifact. Any
`rescued=0` observation here must be re-checked at a budget large enough for the
fallthrough to run at volume before it is treated as evidence of a dead path.
