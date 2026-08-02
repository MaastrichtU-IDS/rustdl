# Iterative deepening × `--pair-timeout-ms`: does the shallow phase steal the budget?

**Flag:** `RUSTDL_ITERATIVE_DEEPENING`, **default ON** since 2026-08-02 (`=0`
reverts). This document was written while the flag was still default OFF, so its
arms are labelled `=1` / `=0`; the measurements are unaffected.
**Date:** 2026-08-02 · **Base:** `main` @ `d0859ba`, rustdl 0.4.11.
**Binary:** `/tmp/rustdl-id`, sha256 prefix `fee336354f3dfeb2` — **one** binary for
both arms; the arms are env settings, not builds.

**Verdict: ON ≥ OFF at every budget on every ontology measured. The exposure does
not materialise, and it does not materialise for a reason that inverts the
prediction** — the shallow phase is not overhead that competes with the deep
search, it is a cheap decision procedure that *returns* budget the deep search
would otherwise have burned stalling. 32 (ontology, budget) cells: **0 pairs lost,
0 closures shrank, 3 cells where OFF produces no closure at all and ON produces
the full one.**

---

## 1. The exposure this measures

`docs/2026-08-02-iterative-deepening-results.md` §8(2) flags exactly one untested
interaction and calls it "the one real exposure":

> Under `--pair-timeout-ms` the shallow phase spends up to 1/4 of the budget, so a
> pair that OFF decided in the last quarter of its budget could be lost.

That matters because a per-pair budget is a *documented operating mode*, not an
exotic knob — `CLAUDE.md` tells users to classify `wine` with `--pair-timeout-ms 25`.

The arithmetic (`id_shallow_deadline`, `crates/owl-dl-reasoner/src/lib.rs:1619`) is
`shallow = min(RUSTDL_ID_SHALLOW_MS, remaining_caller_budget / 4)` with
`ID_SHALLOW_BUDGET_MS = 5` and `ID_SHALLOW_BUDGET_DIVISOR = 4`:

| `--pair-timeout-ms` | shallow slice | **% of the pair budget at risk** |
|---|---|---|
| 5    | 1.25 ms | **25 %** (divisor-clamped) |
| 25   | 5 ms    | **20 %** |
| 100  | 5 ms    | **5 %** |
| 1000 | 5 ms    | **0.5 %** |

The theft fraction is largest at the smallest budgets, so a regression must show up
there if it exists. This is **not** a soundness question (§1b of the results doc: a
depth cap can only *suppress* an `Unsat`, never manufacture one); a loss here would
be a **completeness/throughput** regression that gates the default.

## 2. Method

* One pinned binary, `/tmp/rustdl-id` (`fee336354f3dfeb2…`), both arms switched by
  `RUSTDL_ITERATIVE_DEEPENING=0` / `=1`.
* Every probe under `( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1 timeout N … )`,
  run serially, at most two concurrent probes. An unrelated full-corpus sweep owned
  by another session ran on this 32-core host throughout (load ≈6/32).
* **Arms per (ontology, budget), in order: OFF, OFF, ON.** The second OFF is the
  *noise control* — one binary, one configuration, so any OFF-vs-OFF difference is
  the host, not the flag. The ordering keeps all three runs tightly interleaved.
* Metrics: wall, `grep -c '^direct'`, and the `# timed-out pairs:` banner figure
  (absent from the banner ⇒ zero, shown as `–` below).
* **Closures compared, not just counts**: `grep -v '^#' | sort`, then `comm`. The
  results doc records `pass_through` and the wall breakdown as wall-clock-dependent,
  so a banner difference must never be read as an answer change.

### 2a. Selecting the ontologies — and a negative finding about the pool

The brief asked for ORE ontologies that "complete only under a budget", sourced from
ontologies that DNF unbounded but finish at `--pair-timeout-ms 100`.

**That population is essentially empty on v0.4.11.** All 13 `TIMEOUT` rows in the
last full-pool sweep (`/data/dumontier/ore-run/work/fragments.tsv`) were re-probed at
`--pair-timeout-ms 100`; **every one reports `sweeps=0`**. Six now complete on the
saturation/pure-EL fast path (`11395` 32.4 s, `15703` 34.4 s, `3377` 25.2 s,
`3524` 37.7 s, `6212` 18.1 s, `9498` 12.6 s) and seven still DNF for a reason that
never reaches `HyperCache::decide` (`1194`, `15695`, `16744`, `7127`, `7646`, `8737`,
`9663`). **A `sweeps=0` ontology cannot exercise this flag** — it changes exactly one
call site, the classify per-pair oracle. All 13 are inert and were discarded.

Selection therefore used the **binding predicate for this experiment**: the ontology
must do real wedge work (`sweeps > 0`) so a per-pair budget can actually truncate it.
Candidates came from the 32 `sweeps > 0` ontologies the results doc §5 screened out
of the pool, re-probed at `--pair-timeout-ms 100`, plus the two root-caused instances
and curated `wine`. Final set — 8 ontologies × 4 budgets:

| ontology | why | `sweeps` at b=100 |
|---|---|---|
| `wine.ofn` | the documented budgeted case (`CLAUDE.md`: `--pair-timeout-ms 25`) | 87 005 ms |
| `ore_ont_10407` | root-caused instance (needs depth 319) | binds hard |
| `ore_ont_2182`  | root-caused instance (useful depth ≤7) | binds hard |
| `ore_ont_16800` | heaviest wedge in the §5 screen | 11 667 |
| `ore_ont_7203`  | wedge-heavy | 730 |
| `ore_ont_11554` | wedge-moderate | 342 |
| `ore_ont_3164`  | wedge-light (near-inert control) | 101 |
| `ore_ont_2622`  | wedge-light (near-inert control) | 71 |

## 3. Run-to-run noise — the OFF-vs-OFF control, per budget

Established **before** attributing anything to the flag. 31 cells have two OFF runs
(`wine` @1000 has one — see §4a).

| ontology | b=5 | b=25 | b=100 | b=1000 |
|---|---|---|---|---|
| `wine`    | 0.0 % | 0.0 % | 0.0 % | (one run) |
| `ore_ont_2182`  | 0.1 % | 0.0 % | 0.0 % | both DNF |
| `ore_ont_10407` | 0.1 % | 0.3 % | 0.0 % | both DNF |
| `ore_ont_3164`  | 0.4 % | 0.5 % | 2.5 % | 0.4 % |
| `ore_ont_2622`  | 1.4 % | 0.7 % | 0.0 % | 1.1 % |
| `ore_ont_7203`  | 2.1 % | 0.3 % | 0.1 % | 0.7 % |
| `ore_ont_11554` | 0.8 % | 3.5 % | 1.6 % | 0.5 % |
| **`ore_ont_16800`** | **80.4 %** | **16.2 %** | **60.1 %** | 5.1 % |

**Median control spread 0.6 %; max 80.4 %, and the whole tail is one ontology.**
`ore_ont_16800` is the same outlier the results doc §5 already isolated (it measured
a 16.65–22.74 s OFF band there and traced it to `sweeps` = 13 132–19 210 ms across
three OFF runs of one binary). Excluding it, **the largest control spread anywhere is
3.5 %**. Any ON-vs-OFF wall difference on `16800` below ~60 % is uninterpretable and
is treated as such below.

**On the metric that actually decides the question, the control is perfect:** in all
31 cells the two OFF runs agree on the `direct` count, on the `# timed-out pairs:`
figure, and on the banner-stripped closure **exactly**. So a nondeterministic
timed-out-pair count is *not* a confound here — at these budgets it is stable.

## 4. ON vs OFF, per budget per ontology

Wall in seconds; `sub` = `grep -c '^direct'`; `t/o` = `# timed-out pairs:`
(`–` = banner line absent = 0). `Δ` is ON against the **faster** of the two OFF runs
(the conservative direction).

| ontology | b | OFF#1 | OFF#2 | **ON** | OFF sub | **ON sub** | OFF t/o | ON t/o | Δ wall |
|---|---|---|---|---|---|---|---|---|---|
| `wine` | 5    | 37.26 | 37.26 | **2.84** | 197 | **197** | 3340 | 48 | **−92.4 %** |
| `wine` | 25   | 108.83 | 108.79 | **4.60** | 197 | **197** | 3335 | 39 | **−95.8 %** |
| `wine` | 100  | 361.06 | 361.13 | **9.95** | 197 | **197** | 3332 | 39 | **−97.2 %** |
| `wine` | 1000 | **DNF @600 s** | — | **73.86** | **0** | **197** | – | 39 | **ON only** |
| `2182` | 5    | 22.00 | 21.98 | **1.02** | 120 | **120** | 2094 | 13 | **−95.4 %** |
| `2182` | 25   | 63.81 | 63.83 | **1.96** | 120 | **120** | 2064 | 13 | **−96.9 %** |
| `2182` | 100  | 220.85 | 220.91 | **5.07** | 120 | **120** | 2064 | 13 | **−97.7 %** |
| `2182` | 1000 | **DNF @300 s** | **DNF @300 s** | **41.99** | **0** | **120** | – | 13 | **ON only** |
| `10407`| 5    | 7.99 | 7.98 | **7.47** | 510 | **510** | 616 | 616 | −6.4 % |
| `10407`| 25   | 24.08 | 24.17 | **10.58** | 510 | **510** | 386 | 2 | **−56.1 %** |
| `10407`| 100  | 81.97 | 81.95 | **10.46** | 510 | **510** | 386 | – | **−87.2 %** |
| `10407`| 1000 | **DNF @300 s** | **DNF @300 s** | **10.53** | **0** | **510** | – | – | **ON only** |
| `16800`| 5    | 31.97 | 17.72 | 16.96 | 6689 | 6689 | – | 1 | −4.3 % † |
| `16800`| 25   | 13.45 | 15.63 | 16.31 | 6689 | 6689 | – | – | +21.2 % † |
| `16800`| 100  | 21.88 | 13.67 | 13.90 | 6689 | 6689 | – | – | +1.7 % † |
| `16800`| 1000 | 15.27 | 16.04 | 18.59 | 6689 | 6689 | – | – | +21.8 % † |
| `7203` | 5    | 9.08 | 9.27 | 9.23 | 69021 | 69021 | 36 | 36 | +1.7 % |
| `7203` | 25   | 9.69 | 9.71 | 9.95 | 69021 | 69021 | 2 | **13** | +2.7 % |
| `7203` | 100  | 9.50 | 9.50 | 9.88 | 69021 | 69021 | – | – | +4.1 % |
| `7203` | 1000 | 9.61 | 9.67 | 9.88 | 69021 | 69021 | – | – | +2.8 % |
| `11554`| 5    | 0.92 | 0.93 | 0.92 | 100 | 100 | 27 | 27 | −0.5 % |
| `11554`| 25   | 1.09 | 1.05 | 1.32 | 100 | 100 | 1 | **4** | +25.6 % |
| `11554`| 100  | 1.05 | 1.03 | 1.21 | 100 | 100 | – | – | +17.1 % |
| `11554`| 1000 | 1.03 | 1.04 | 1.19 | 100 | 100 | – | – | +15.7 % |
| `3164` | 5/25/100/1000 | 2.51–2.58 | 2.52–2.54 | 2.52–2.58 | 90 | 90 | – | – | −0.4 … +2.8 % |
| `2622` | 5/25/100/1000 | 2.83–2.88 | 2.85–2.90 | 2.85–2.86 | 93 | 93 | – | – | −0.6 … +0.6 % |

† inside `ore_ont_16800`'s own 60–80 % OFF-vs-OFF control band (§3) — not interpretable.

### 4a. `wine` @1000: why one OFF run and a 600 s cap

`wine`'s OFF wall is `#timed-out-pairs × budget` and scales linearly with the budget
(37.26 s @5 → 108.83 @25 → 361.06 @100), which projects **≈3300 s @1000** — running
that twice was not affordable. It was run once under a 600 s cap. That still answers
the question the row is for: **which arm produces a closure at all.** OFF produced
none in 600 s; ON produced the full one in 73.86 s. Dropping the second OFF run here
is safe because the control at 5/25/100 is 0.0 % on wall and exact on every count.

Note the ON figure: **73.86 s at `--pair-timeout-ms 1000` reproduces the 73.82 s the
results doc measured for ON with no budget at all** — at 1000 ms the budget has
stopped binding for the ON arm.

## 5. Did any closure genuinely shrink? No.

Banner-stripped (`grep -v '^#' | sort`) comparison, all 32 cells:

* **OFF#1 vs OFF#2: IDENTICAL in all 31 two-OFF cells.**
* **`OFF \ ON` (pairs lost by turning the flag on) = 0 in all 32 cells.**
* `ON \ OFF` (pairs added) = 0 everywhere except the three cells where OFF DNF'd and
  produced an empty closure: `wine`@1000 (+201 rows), `10407`@1000 (+512),
  `2182`@1000 (+123).

The two cells where **ON reports more timed-out pairs than OFF** are the only places
the feared mechanism could have bitten, and they cost nothing:

* `ore_ont_7203` @25: OFF 2 → ON 13 timed-out pairs, closures **byte-identical**
  (69 034 rows).
* `ore_ont_11554` @25: OFF 1 → ON 4, closures **identical** (100 rows).

Those extra stalls are on pairs that are not subsumptions either way, so the extra
`Stalled` verdicts default to the same "not subsumed" the OFF arm reached by
exhausting the budget.

For `wine`, every ON and OFF closure at every budget is identical to every other —
201 rows in all 11 runs.

## 6. Why the prediction inverted: the shallow phase *returns* budget

The exposure assumed the shallow phase is pure re-work charged against a budget the
final level needs. The `wine` banners at `--pair-timeout-ms 5` show what actually
happens (`wedge-cost-histogram ms`, buckets `0|1|2-4|5-9|…`):

```
OFF:  163 | 0 | 1 | 3340 | 0 | 0 | 0 | 0 | 0     timed-out 3340   sweeps 29253 ms
ON:  3454 | 2 | 0 |   48 | 0 | 0 | 0 | 0 | 0     timed-out   48   sweeps  1347 ms
```

OFF puts **3340 pairs in the 5–9 ms bucket**: at the fixed depth-256 cap each one
thrashes, burns its entire 5 ms budget, stalls, falls through to the tableau
(`fallthrough … ran=3340 … noverdict=3340`) and still gets nothing. ON puts **3454
pairs in the 0 ms bucket**: the depth-8 level finds a completed model immediately, a
*definite* `Sat` verdict, and the pair terminates at ~0 ms having spent none of its
budget.

A shallow `Sat` is a definite verdict, and by the depth-monotonicity argument
(results doc §1c) it is the verdict the deeper search would have produced. So the
population a small budget most endangers — pairs that thrash at depth 256 — is
exactly the population the shallow level rescues. The `1/4` clamp is what bounds the
worst case; on this population it never had to.

The mirror image is `ore_ont_11554` and `ore_ont_7203`: wedge ontologies where the
budget barely binds, so the shallow probes decide little and their cost shows as a
small constant — **+0.15 to +0.27 s on `11554`, +0.2 to +0.4 s on `7203`** (+2 to
+26 % of a 1–10 s wall). That is consistent with the results doc §5 aggregate
(+0.7 % over 32 wedge-exercising ontologies) and is the price paid for the
92–98 % reductions above.

## 7. Verdict

**Outcome 1 of the three: ON ≥ OFF at every budget. This exposure is closed, and the
default decision rests solely on the corpus-wide sweep the results doc §8(1) already
identifies as the missing measurement.**

Concretely, across 8 ontologies × 4 budgets (5, 25, 100, 1000):

* **0 subsumptions lost**, at any budget, on any ontology.
* **0 closures shrank**; every OFF-vs-OFF and ON-vs-OFF closure comparison is exact
  except where OFF produced nothing.
* **3 cells go from "OFF produces no closure" to "ON produces the full closure"** —
  `wine`, `ore_ont_10407` and `ore_ont_2182`, all at `--pair-timeout-ms 1000`.
* At the documented `wine --pair-timeout-ms 25` setting: **108.8 s → 4.6 s, same 197
  subsumptions, timed-out pairs 3335 → 39.**
* The only ON-slower rows are sub-second-to-sub-half-second constants on ontologies
  where the budget does not bind, or fall inside `ore_ont_16800`'s own 60–80 %
  run-to-run band.

### What this does NOT establish

1. **Population size.** Eight ontologies, chosen because they exercise the changed
   call site. The theoretical worst case the exposure describes — a pair OFF decides
   using more than 3/4 of its budget — was never exhibited, but eight ontologies
   cannot exclude it. It remains bounded by construction at 25 % of one pair's budget.
2. **The "completes only under a budget" population could not be sampled as asked**,
   because it is empty on v0.4.11 (§2a). The substitute predicate (`sweeps > 0`) is
   the right one for *this* question but is a different, broader set.
3. **`wine` @1000 OFF is a capped DNF, not a completed run** (§4a). Its true wall
   projects to ≈3300 s; only the "no closure in 600 s" claim is measured.
4. **No new oracle adjudication was run.** None is owed: no closure grew except from
   empty, and those three grew to exactly the closure the same ontology produces at a
   smaller budget under both arms (`wine` 201, `10407` 512, `2182` 123 rows).

### Recommendation

No change to the schedule is warranted by this measurement. The `min(5 ms, budget/4)`
clamp does not need to scale further with the caller's budget — at the budgets where
its share is largest (5 ms and 25 ms, 25 % and 20 %) the flag delivers its *largest*
wins, not its losses. The default should still follow the ORE-wide sweep, but this
particular blocker is retired.
