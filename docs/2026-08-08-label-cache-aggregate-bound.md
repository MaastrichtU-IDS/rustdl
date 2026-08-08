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

**1. `ore_ont_16056` does NOT recover at default settings.** The v0.4.16 record
has it going `dnf/150s → ok/485r/17s`. On a clean v0.4.16 HEAD binary, on a
quiet host, it **DNFs at 200 s** at the default `--pair-timeout-ms 1000`. It
completes only under a small per-pair budget (`--pair-timeout-ms 50`: 112 s,
487 rows). The `label_cache_build=30909` in that run shows why the earlier
framing misled: the label cache costs 31 s and then the **pair loop** consumes
the rest. **`16056`'s default-setting stall is the pair loop, not the label
cache.** Any claim that it is "label-cache-bound" needs restating with its
budget attached.

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
