# Iterative deepening of the classify per-pair wedge depth cap — results

**Flag:** `RUSTDL_ITERATIVE_DEEPENING`, **default OFF** (`=1` opts in).
**Date:** 2026-08-02 · **Base:** `main` @ `68597ba`, rustdl 0.4.11.

Binaries, pinned immediately after the build that produced each:

| path | sha256 | what |
|---|---|---|
| `…/scratchpad/bin/rustdl-BASE-68597ba` | `c10d59138add491a5…8208ee` | pre-change `main` |
| `…/scratchpad/bin/rustdl-ID-v2` | `7bdbb76fbb3f3763c…a3e5ec` | this change (flag OFF *and* ON arms) |

Both arms of every A/B below come from the **same** `rustdl-ID-v2` binary, switched
by the env flag, so a stale-binary mix-up cannot produce the delta. `BASE` is kept
only to confirm the flag-OFF path did not move.

Host discipline: every probe under
`( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1 timeout N … )`, run serially on
an otherwise idle 32-core host. Build:
`PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
RUSTUP_TOOLCHAIN=stable cargo build --release` (a bare `cargo` is not on `PATH` here).

---

## 1. The defect, and what "iterative deepening" has to mean to fix it

`const HYPER_WEDGE_DEPTH: usize = 256` is wrong in both directions —
`docs/2026-08-02-cardinality-rootcause.md` (`ore_ont_10407` needs **319**) and
`docs/2026-08-02-nominal-blocking-rootcause.md` (`ore_ont_2182`'s useful proof depth
is **≤7**). The change replaces the single call at
`crates/owl-dl-reasoner/src/lib.rs:2895` — `HyperCache::decide`, the classify
per-pair subsumption oracle — with a loop over a depth schedule.

### 1a. Which of the seven call sites changed, and why only one

| site | what it is | changed? |
|---|---|---|
| `:2895` `HyperCache::decide` | **classify per-pair subsumption oracle** | **YES** |
| `:3319` `HyperCache::classify_labels` | per-class label cache | no |
| `:3658` `ConsistencyCache::decide` | ABox consistency | no |
| `:3680` `base_model_types` | pseudo-model realize witness | no |
| `:3912` `get_or_build_snapshot` | `RUSTDL_SNAPSHOT_CAPTURE`, default OFF / FP-unsound | no |
| `:6924` | a diagnostic timing probe | no |

Both investigations attribute the cost to `sweeps` — the per-pair loop — and both
report `label_cache_build` at **113 ms** (`10407`) and **159 ms** (`2182`), three
orders of magnitude below the `sweeps` figure. The consistency and realize paths are
not implicated at all. Changing one site keeps the blast radius to the surface the
evidence names.

### 1b. FP-safety, and why the correctness check is not byte-identity

A depth cap can only **suppress** an `Unsat`: on `depth == 0` `HyperEngine::solve`
returns `Stalled`, and a parent frame with any stalled child returns `Stalled`
instead of `Unsat`. It cannot **manufacture** one. So no depth schedule can invent a
subsumption. Deepening may only add entailments — hence superset + oracle, not
byte-identity.

### 1c. Why it does not LOSE entailments: depth is verdict-monotone

Raising the cap from `k` to `k' > k`:

* `Unsat` at `k` requires *every* branch decisively unsat (no child stalled), so the
  identical DFS at `k'` re-derives it;
* `Sat` at `k` means a completed model was found; at `k'` the DFS prefix is identical
  except that frames which returned `Stalled` may now return `Sat` (immediate `Sat`)
  or `Unsat` (the parent continues to the next disjunct — exactly what it did after
  `Stalled`), so the outcome is still `Sat`;
* only `Stalled` can change, and only into a definite verdict.

The adaptive-budget divergence cut does not break this: `is_diverging` requires depth
saturation, so a larger `init_depth` makes it fire *less*.

The schedule's last level is therefore required (compile-time asserted) to be
`>= HYPER_WEDGE_DEPTH`. With an unbounded deadline that makes the final level
dominate today's fixed cap, so ON ⊇ OFF by construction.

---

## 2. The schedule, and the measurement that forced its shape

**Plain geometric iterative deepening — the obvious implementation — is REFUTED on
`ore_ont_10407`.** Baseline `hyper-classify-probe --per-pair-timeout-ms 0`, single
thread, idle host, pinned `rustdl-BASE-68597ba`:

| depth cap | `10407` wall / stalled / subs | `2182` wall / stalled / subs |
|---|---|---|
| 8 | **68.18 s** / 1726 / 926 | **1.06 s** / 4 / 264 |
| 32 | 169.30 s / 1726 / 926 | 2.50 s / 6 / 264 |
| 64 | 162.13 s / 1726 / 926 | 4.09 s / 78 / 264 |
| 128 | 82.47 s / 787 / 926 | 7.12 s / 2097 / 258 |
| 256 (shipped) | 44.50 s / 357 / 926 | 13.41 s / 2357 / 258 |
| 512 | **10.45 s** / 0 / 926 (max depth 319) | 13.36 s / 2357 / 258 |

On `10407` the shallow levels find **nothing** the final level does not (926
subsumptions at every depth) and cost 68 + 169 + 82 s of pure re-work before the
10.45 s level that actually decides. Unbounded deepening would make `10407` far
*worse* than the DNF it already is.

The per-pair profile says why, and says what to do:

| | `10407` @ depth 8 | `2182` @ depth 8 |
|---|---|---|
| stalled-pair cost | **~50 ms** each | — |
| branches | 502 (uniform — the adaptive divergence cut) | 110–216 |
| cost per branch | ~100 µs | ~8 µs |
| slowest pair | 50.45 ms | **1.67 ms** |

The adaptive-budget cut already bounds the *branch count*; it does not bound the
*wall*, and a `10407` branch is 12× more expensive than a `2182` one. A few
milliseconds cleanly separates the population a shallow level rescues from the one it
merely taxes.

**Schedule shipped: `[8, 32, 128, 512]`, with the whole shallow phase (every level
but the last) sharing one wall budget of `min(5 ms, remaining_caller_budget / 4)`.**

* `8` — `2182`/`16481` need ≤7; measured 13× cheaper and *more* complete there.
* `512` — `10407`/`9941` need 319, and `512 >= HYPER_WEDGE_DEPTH` is the
  monotonicity requirement of §1c.
* `32`, `128` — geometric intermediates; they cost nothing extra, because the shallow
  budget is shared across the whole phase rather than per level, and the loop jumps
  straight to the final level once that budget is spent.
* **`5 ms`** — 3× above `2182`'s slowest depth-8 pair (1.67 ms), 10× below
  `10407`'s (50 ms). Overridable via `RUSTDL_ID_SHALLOW_MS`; `0` disables the bound
  (that is the refuted unbounded variant, kept as an A/B arm).
* **`/4`** — without it, a small `--pair-timeout-ms` would be spent entirely on
  shallow probes and the final level, the only one that can decide a deep pair, would
  never run. At least 3/4 of a caller budget always reaches it.

**Bounding the shallow phase cannot change a verdict** when the final level runs: a
cut shallow level returns `Stalled`, and the unbounded final level then reproduces
whatever it would have concluded (§1c). The shallow levels are pure accelerators;
their budget is a wall knob, not a semantic one.

### Deadline

The caller's deadline bounds the **whole loop** — every level is passed the same
`Instant` and the loop breaks before starting a level once it has passed — so
deepening never multiplies the per-pair budget. The converse is the one real
completeness exposure and is recorded as such: under a *bounded* deadline the shallow
phase spends up to 1/4 of a budget the final level might have needed. Unbounded runs
cannot lose (§1c).

---

## 3. The four target ontologies — gate 2

`classify` with **no** pair budget, `timeout 120`, single thread,
`ulimit -v $((24*1024*1024))`, both arms from `rustdl-ID-v2`:

| ontology | OFF | ON | ON `direct` rows |
|---|---|---|---|
| `ore_ont_10407` | **DNF @120 s** | **10.46 s** | 510 |
| `ore_ont_9941`  | **DNF @120 s** | **10.58 s** | 510 |
| `ore_ont_2182`  | **DNF @120 s** | **41.99 s** | 120 |
| `ore_ont_16481` | **DNF @120 s** | **45.03 s** | 122 |

All four DNF → complete. `10407`/`9941` land essentially on the depth-512 probe
wall (10.45 s), i.e. the bounded shallow phase costs them almost nothing.
`2182`/`16481` are slower than their 1.01 s depth-8 probe because the pairs the
shallow level cannot decide now each get a full **unbounded depth-512** search —
that is the price of not lowering the global cap, and it is what keeps `pizza` and
`wine` whole (§4).

**A fifth, unasked-for result:** curated `wine.ofn`, unbounded,
**DNF @900 s → 73.82 s**, closure **653 = Konclude 653 = HermiT 653, FP 0 /
MISSED 0**. That is the committed FP=0 reference count, reached with no per-pair
budget for the first time.

## 4. Superset + oracle — gate 3

`ON \ OFF` = added, `OFF \ ON` = lost (hard stop), added pairs adjudicated against
**Konclude ∪ HermiT** (`/data/dumontier/reasoners/run-{konclude,hermit}.sh`,
normalised and compared with
`owl-reasoner-harness/scripts/normalise.py`). Closure counts are transitive-closure
counts under the normaliser's symmetric unsat / thing-equivalent exclusion.

| ontology | OFF | ON | added | **lost** | oracle ∪ | **FP** | MISSED vs oracle |
|---|---|---|---|---|---|---|---|
| `ore_ont_10407` | 0 (DNF) | 890 | 890 | **0** | 8 = 8 = 8 | **0** | 0 |
| `ore_ont_9941`  | 0 (DNF) | 890 | 890 | **0** | 8 = 8 = 8 | **0** | 0 |
| `ore_ont_2182`  | 0 (DNF) | 287 | 287 | **0** | 287 = 287 = 287 | **0** | 0 |
| `ore_ont_16481` | 0 (DNF) | 351 | 351 | **0** | 351 = 351 = 351 | **0** | 0 |
| `pizza`   | 499 | 499 | 0 | **0** | 499 = 499 = 499 | **0** | 0 |
| `wine`    | 0 (DNF @900 s) | 653 | 653 | **0** | 653 = 653 = 653 | **0** | 0 |
| `sio`     | 8904 | 8904 | 0 | **0** | Konclude 8904 | **0** | 0 |
| `go-basic`| 357043 | 357043 | 0 | **0** | Konclude 357043 | **0** | 0 |

**No pair lost anywhere; no added pair the oracle lacks.** The two most sensitive
completeness fixtures behave exactly as the monotonicity argument predicts:
`pizza` is unchanged at 499 (a globally *lower* fixed cap loses 4 there) and `wine`
gains its full 653 rather than losing 10. `sio` and `go-basic` are unchanged
pair-for-pair.

Reading note for `10407`/`9941`: the on/off column counts 890 under the ON-vs-OFF
exclusion set, and 8 under the oracle exclusion set. The difference is not a
disagreement — 18 of `10407`'s 50 classes are equivalent to `⊤` (38 of its
"cardinality axioms" are `ObjectMinCardinality(0 R)`, i.e. literally `⊤`), and the
normaliser drops thing-equivalent classes symmetrically once Konclude/HermiT declare
them. rustdl, Konclude and HermiT agree on all 8 surviving pairs, which reproduces
the `FP 0 / MISSED 0, closures 8 = 8` recorded in
`docs/2026-08-02-cardinality-rootcause.md`.

## 5. No regression on currently-fast ontologies — gate 4

**Sample selection matters more than sample size here.** The flag touches only
`HyperCache::decide`, the classify per-pair oracle. An ontology that takes the
pure-EL / saturation fast path never calls it, so a random ORE sample would be
mostly *inert* and would measure nothing. 160 pool ontologies (deterministic
`NR%11==3` stride) were screened OFF at a 25 s cap and kept only if they
**completed** AND showed **`sweeps > 0`** in the wall-breakdown banner — evidence
that the per-pair wedge loop actually ran. 145 completed; **32 qualified**, and all
32 were used. The screen was for selection only; the A/B below is a fresh,
strictly interleaved OFF/ON pass on an idle host (90 s cap).

**Outcome changes: 0 of 32.** Every `direct`-row count is identical OFF vs ON and
every ontology stays `ok`.

Wall, sorted by delta (single run each):

| ontology | OFF | ON | Δ | | ontology | OFF | ON | Δ |
|---|---|---|---|---|---|---|---|---|
| `16800` | 15.91 | 23.58 | **+48.2%** | | `12474` | 0.27 | 0.28 | +3.7% |
| `8460` | 0.09 | 0.11 | +22.2% | | `8322` | 0.27 | 0.28 | +3.7% |
| `15682` | 0.16 | 0.19 | +18.8% | | `2232` | 22.02 | 22.50 | +2.2% |
| `11554` | 1.03 | 1.20 | +16.5% | | `16286` | 5.73 | 5.77 | +0.7% |
| `5298` | 0.22 | 0.25 | +13.6% | | 9 ontologies | — | — | 0.0% |
| `7537` | 0.23 | 0.25 | +8.7% | | `4609` | 2.88 | 2.79 | −3.1% |
| `2622` | 2.88 | 3.12 | +8.3% | | `14331` | 0.27 | 0.26 | −3.7% |
| `16642` | 0.25 | 0.27 | +8.0% | | `3164` | 2.65 | 2.55 | −3.8% |
| `4162` | 0.25 | 0.27 | +8.0% | | `1062` | 0.25 | 0.24 | −4.0% |
| `12325` | 0.13 | 0.14 | +7.7% | | `7014` | 0.25 | 0.23 | −8.0% |
| `7203` | 9.50 | 9.90 | +4.2% | | `8387` | 0.04 | 0.03 | −25.0% |
| `1091` | 0.24 | 0.25 | +4.2% | | `10285` | 0.02 | 0.01 | −50.0% |

Only one row crosses the "material" line (>25% **and** >2 s): `ore_ont_16800`,
+7.7 s. **It does not reproduce.** Min-of-3, same binary, same host:

| | run 1 | run 2 | run 3 | min | mean |
|---|---|---|---|---|---|
| OFF | 22.44 | 22.74 | 16.65 | **16.65** | 20.61 |
| ON | 17.72 | 15.88 | 18.89 | **15.88** | 17.50 |

`ore_ont_16800`'s own run-to-run spread OFF is 16.65–22.74 s (a 37% band, visible
in `sweeps` = 13132–19210 ms across the three OFF runs); ON is *faster* on both min
and mean. The single-run +48.2% was noise. `direct` rows are 6689 in all six runs.

**So: 0 material regressions and 0 outcome changes over 32 ontologies.** Aggregate
wall over the 32, substituting the min-of-3 for `16800`: OFF 72.33 s → ON 72.82 s
(**+0.7%**). Curated fixtures from §4 for completeness: `pizza` 0.14 → 0.26 s
(+0.12 s absolute), `sio` 0.66 → 0.71 s, `go-basic` 5.89 → 5.84 s.

The bounded shallow phase is why this is flat. Each pair pays at most ~5 ms extra,
and only pairs the shallow level cannot decide pay it at all.

## 6. Canaries and sabotage — gate 5

**23 canaries**, negatives first: three controls establish that the fixture
discriminates (`deep_chain_stalls_at_the_first_level`,
`deep_chain_is_unsat_at_the_second_level`, `shallow_chain_is_unsat_at_the_first_level`)
before any deepening claim is asserted.

> **The negative control paid for itself immediately.** The first fixture —
> `A_{i-1} ⊑ A_i ⊔ B_i` with `B_i ⊑ A_i` — was *not* deep at all: the two disjuncts
> share a common told subsumer, the minimal-common-subsumer pass rewrites the chain
> to Horn implications, and `sat_seed` hands the wedge `Y` at the root
> (`pairs_branched: 0`, decided at depth 0). Without that control, four "deepening
> works" assertions would have been vacuous. The shipped fixture gives `Q_i` no told
> superclass at all (a conjunctive-`⊥` GCI registers as told-DISJOINT, not a
> subsumer), so the case split survives to the engine.

**8 sabotages applied strictly serially — 8 caught, 0 survivors.**

| # | sabotage | canaries failed |
|---|---|---|
| 1 | the loop never deepens | 3 |
| 2 | deepen past the final cap (geometric, schedule ignored) | 1 (`never_leaves_the_schedule`) |
| 3 | a `Stalled` is treated as definite (terminal) | 3 |
| 4 | drop the `shallow_spent` term from `id_cap_was_not_binding` | 1 |
| 5 | drop the caller-budget divisor clamp | 1 |
| 6 | accept a malformed `RUSTDL_ID_SCHEDULE` | 1 |
| 7 | drop the caller-deadline guard from the loop | 1 |
| 8 | `RUSTDL_ID_SHALLOW_MS` garbage parses as `0` (bound disabled) | 1 |

Sabotage 4 is the one that mattered during development: an earlier revision *did*
have the bug it models — the "cap was not binding, stop deepening" exit fired on a
level the shallow budget had cut short, so the final level never ran. The predicate
was extracted into a pure function (`id_cap_was_not_binding`) precisely so it could
be pinned without a timing-dependent test.

## 7. Gates 6 and 7

* `cargo fmt --all -- --check` — clean.
* `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean
  (two findings fixed: `format_push_string` in the fixture builder,
  `unchecked_time_subtraction` in two canaries).
* `cargo test --workspace --exclude owl-dl-py --release` — **128 `ok` result groups,
  0 failed, 0 errors.**
* **FP=0 net with the flag ON** (`RUSTDL_ITERATIVE_DEEPENING=1
  ./scripts/run-soundness-diff.sh`) — **11 VERIFIED, every closure EXACT** at the
  committed reference: galen 27997, notgalen 32739, sio 8904, ore-10908 6001,
  wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16.
  Nothing grew and nothing shrank, so no adjudication was owed. The three
  `NOT VERIFIED (fixture absent)` entries (`ro-stripped`, `sulo-stripped`,
  `sio-stripped`) are the pre-existing documented gaps, unchanged.

### Flag-OFF equivalence

`rustdl-BASE-68597ba` vs `rustdl-ID-v2` with `RUSTDL_ITERATIVE_DEEPENING=0`:
`sio`, `go-basic`, `bibtex`, `ore-15672-shoin` byte-identical (1626 / 57810 / 22 /
84 lines). `pizza` differs only in the `# label heuristic: … pass_through=` and
`# wall breakdown ms:` telemetry — **an OFF-vs-OFF control on ONE binary reproduces
the same variation** (`pass_through` alternates 205/206 over 8 runs of `BASE`
alone), and `direct` is 309 in all 16 runs. Answers are identical; the counters are
wall-clock-dependent, as CLAUDE.md already records for the wedge histogram.

## 8. Recommendation, and what is NOT established

**Recommend: keep it default OFF for now; flip to default ON only after an ORE-wide
sweep.** The evidence for the change is strong and one-directional — four DNFs and
curated `wine` recovered, zero pairs lost, zero FPs, FP=0 net exact, zero material
regressions over 32 wedge-exercising ontologies, 8/8 sabotages caught — but:

1. **The addressable set is not measured.** Four ORE ontologies plus `wine` are
   confirmed. `docs/2026-08-02-nominal-blocking-rootcause.md` warns explicitly that
   a grep is not a gate; the right measurement is a full-pool ON-vs-OFF sweep
   counting DNF→ok transitions and outcome changes, which this work did not run.
   The 32-ontology sample bounds the *regression* risk, not the *reward*.
2. **The one real exposure is deadline-bounded runs.** Under `--pair-timeout-ms`
   the shallow phase spends up to 1/4 of the budget, so a pair that OFF decided in
   the last quarter of its budget could be lost. Every measurement here used either
   no pair budget or a budget large enough that this did not bite; the sweep in (1)
   should include a `--pair-timeout-ms` arm.
3. **The `5 ms` shallow budget is calibrated on two ontologies.** It is 3× above
   `2182`'s slowest depth-8 pair and 10× below `10407`'s. That separation is real
   but the population between them is uncharacterised.

None of these is a soundness risk (§1b), and none can lose a pair on an unbounded
run (§1c). They are reasons the *default* should follow a population measurement
rather than a five-instance one.

---

## Default-ON decision: the two gates, in flight (2026-08-02)

The flag ships **default OFF**. Two things gate the flip, both now running, and both chosen
because they are the gaps the implementation itself named rather than ones invented afterwards.

**Gate 1 — ORE-wide two-arm sweep.** `scripts/sweep-arm.sh` over all 1,920 ontologies, OFF arm
then ON arm, **sequential** (contention would inflate whichever arm shared the host), 60 s cap,
single-thread, `--digest-output`, one output file per chunk. Binary pinned `/tmp/rustdl-id`
sha `fee336354f3dfeb2`; the arms are **env settings on one binary**, not two builds.

This exists because the addressable set is **unmeasured** — 5 confirmed instances, and a grep is
not a gate. It also answers the question that actually matters for a default: does anything
*regress*. The precedent is direct and recent: `RUSTDL_CLASSIFY_INCONSISTENCY` was flipped ON in
v0.4.8 on a **12-ontology** benchmark reading −1.5%, and a full-corpus sweep later found **4
ontologies going from ~5 s to DNF**. Twelve ontologies is not a population.

Decision rule, fixed now:
- **any `ok → dnf`** ⇒ do not flip; root-cause first;
- **any closure that shrinks** ⇒ hard stop (deepening is monotone, so a loss means the
  implementation is wrong, not the idea);
- closures that **grow** are expected and permitted, but each added pair must be adjudicated
  against **Konclude ∪ HermiT** before acceptance;
- otherwise flip ON.

**Gate 2 — per-pair budget interaction.** The shallow phase shares
`min(5 ms, caller_budget/4)`. With no budget that is negligible, but `--pair-timeout-ms` is a
documented operating mode — CLAUDE.md tells users to run `wine` at `--pair-timeout-ms 25`, where
the shallow phase can take **~24% of every pair's budget**. Measured at budgets 5/25/100/1000
against an **OFF-vs-OFF control at each budget first**, because timed-out-pair counts vary
run-to-run on a single binary and that variance has already been mistaken for an arm effect once
in this arc.

Not a soundness risk — fewer answers, never wrong ones — but a completeness/throughput loss at
small budgets would gate the default, or require the schedule to skip the shallow phase below
some N.

**Known limits of what has been established.** The five recoveries are real and oracle-checked,
but they are five instances; the corpus-wide effect is exactly what Gate 1 measures. And the
`ore_ont_10407` root cause showed a target chosen from a *structural profile* can be
semantically misleading — 41 of its "50 cardinality axioms" were `MinCardinality(0 R)`
tautologies — so no claim about *which* ontologies benefit should rest on construct counts.
