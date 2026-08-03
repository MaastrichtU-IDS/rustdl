# Iterative deepening of the MAIN TABLEAU depth cap — built, measured, **NO-GO**

**Flag:** `RUSTDL_TABLEAU_ITERATIVE_DEEPENING`, **DEFAULT OFF** (`=1` enables; the
house default-OFF idiom, `is_some_and(|v| v == "1")`).
**Date:** 2026-08-03 · **Base:** `feat/cb-alch-taming` @ `d567319`, rustdl 0.4.12.

> ## Headline
>
> **The transplant does not deliver, and the reason is structural rather than a
> tuning failure.** Iterative deepening at the main tableau is **wall-neutral and
> recovery-free on all five of the audit's measured cases** — 0 of 3 DNFs
> recovered, 0 of 2 speedups reproduced, every row count identical. The
> implementation is correct (14 canaries, 8 of 10 sabotages caught, and a positive
> control proving the driver executes), so this is a measured null, not a bug.
>
> **The cause, established by measurement and not by argument:** on every one of
> those five ontologies the probes whose cost a shallow cap saves are probes that
> reach **no verdict at either depth**. Iterative deepening is verdict-monotone by
> construction, so it *must* re-run each undecided probe at a final level
> `>= MAX_SEARCH_DEPTH` under the caller's own deadline. Doing so reproduces the
> flag-OFF cost exactly. The audit's wins come from **giving up sooner**, which is
> the one thing deepening is defined not to do.
>
> **The lever the audit's data actually supports is the fixed lower cap, and it is
> a completeness trade, not a free win.** Re-measured here on a pinned control
> binary: `ore_ont_10019` at a fixed cap of 8 gives closure **158, FP = 0, MISSED
> 4 of Konclude's 162** in 61.08 s, against **0 / DNF** for the shipped default.
> That is a large sound improvement — and it *loses* 4 pairs a completing
> depth-256 run would find. Pursuing it is a different project with a different
> gate (an ORE-wide MISSED net), and it must not be smuggled in under the
> "deepening cannot lose a pair" licence, because it can.

---

## 0. Binaries and host discipline

Every binary pinned to a uniquely named path **immediately after the build that
produced it**, and named after its configuration.

| path | sha256 | what |
|---|---|---|
| `…/scratchpad/bin/rustdl-BASE-d567319` | `4e00e437dd1539e6…b11046` | unmodified `d567319`, control |
| `…/scratchpad/bin/rustdl-DEPTHOVR` | `2f4265a9c21133ae…5ced854` | **TEMPORARY** `RUSTDL_AUDIT_MAX_SEARCH_DEPTH` override of the constant — reverted before any feature code was written; the audit-reproduction and fixed-cap control arms |
| `…/scratchpad/bin/rustdl-TID-v1` | (superseded) | the feature, pre-telemetry |
| `…/scratchpad/bin/rustdl-TID-v2` | `a74a602c963432ed…41fdb3` | **the gated binary** — the feature plus `RUSTDL_TABLEAU_ID_STATS` |
| `…/scratchpad/bin/rustdl-TID-FINAL` | `67112642307c1b38…a8416a` | **built from the committed source** (post-`fmt`/clippy); gate 2 re-run on it, see §5b |

**Both arms of every A/B come from ONE binary switched by an env var**, never from
two builds, so a stale-binary mix-up cannot produce (or hide) a delta. Built with
`PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
RUSTUP_TOOLCHAIN=stable cargo build --release` (a bare `cargo` is not on `PATH`
here).

Host: every probe under `( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1
timeout N … )`, run serially within each battery. Population scans ran three
`nice`d workers at `ulimit -v 12G` and a 10–15 s cap. A second agent was active
on the host, so **walls are comparable within a battery** (same host, interleaved
arms) and should not be compared to numbers in other documents.

---

## 1. The premise the brief rests on, verified before anything was built

The brief and the audit both describe `MAX_SEARCH_DEPTH` as binding in the
"default operating mode (no per-pair budget)". That phrasing is loose in a way
that mattered, so it was pinned down first.

`MAX_SEARCH_DEPTH` is used at **exactly one** call site (`lib.rs:6831`), inside
`if deadline.is_some()`. With **no** deadline, `decide` instead takes the
`DEEP_SEARCH_DEPTH = 1_000_000` branch on a 1 GiB stack. So the cap is reachable
only when a deadline exists — and `rustdl classify` supplies one by default:
`--pair-timeout-ms` has `default_value_t = 1000` (`owl-dl-cli/src/main.rs:156`).
Passing `--pair-timeout-ms 0` makes the cap **unreachable**.

So "the default operating mode" means *the CLI default of 1000 ms per pair*, not
"unbounded". Everything below runs in that mode. Had the audit's language been
taken literally (`--pair-timeout-ms 0`), the target would not have existed at all.

## 2. The audit reproduces exactly, on a pinned control

`rustdl-DEPTHOVR`, one binary, two arms via
`RUSTDL_AUDIT_MAX_SEARCH_DEPTH`; 90 s cap, single thread.

| ontology | fixed depth 8 | fixed depth 256 (shipped) | audit reported |
|---|---|---|---|
| `ore_ont_13545` | **3.15 s**, 2 482 rows | 46.30 s, 2 482 rows | 3.11 / 46.30, 2 482 both ✓ |
| `ore_ont_2826` | **0.54 s**, 197 rows | 7.29 s, 197 rows | 0.54 / 7.29, 197 both ✓ |
| `ore_ont_10019` | **61.08 s** | dnf @90 s | 60.62 s / dnf ✓ |
| `ore_ont_3250` | **7.79 s** | dnf @90 s | 7.80 s / dnf ✓ |
| `ore_ont_8666` | **10.06 s** | dnf @90 s | 10.15 s / dnf ✓ |

Five for five, to two decimal places on the completers. The audit is sound and
its instrumentation was correct.

## 3. Why deepening cannot capture any of it — read off the banners, before building

The `classify` banner decomposes the two speedup cases, same binary, both arms:

| | `13545` @ 8 | `13545` @ 256 | `2826` @ 8 | `2826` @ 256 |
|---|---|---|---|---|
| `subsumption: … tableau=` | **0** | **0** | **0** | **0** |
| `satisfiability probes: … tableau=` | 30 | 30 | 1 | 1 |
| `timed-out pairs` | **14** | **14** | **6** | **6** |
| `fallthrough … noverdict=` | **14** | **14** | **6** | **6** |
| `unsat_probe` ms | **562** | **30 014** | **35** | **1 000** |
| `tier_walk` / `sweeps` ms | 760 / 1 380 | 2 719 / 13 120 | 111 / 293 | 2 041 / 4 150 |
| `direct` rows | 2 482 | 2 482 | 197 | 197 |

**Every count of *decided* work is identical between the two depths**;
`tableau_subsumption_calls` is 0 at both, the timed-out-pair set is the same size
at both, and the row counts match. The entire 14.7× is `unsat_probe` — 30 probes
each burning the full 1 000 ms per-pair budget at depth 256 versus ~19 ms at
depth 8 — and all 30 return **no verdict either way** (`unwrap_or(true)` ⇒ the
class is reported satisfiable in both arms).

So the depth cap's only measured effect on this population is *how long a doomed
probe burns before giving up*. Iterative deepening must hand every probe the
shallow level did not decide to a final level at cap `>= MAX_SEARCH_DEPTH` with
the caller's own deadline — that is what makes it verdict-monotone. On a doomed
probe that is precisely the flag-OFF search. **The two requirements are
incompatible: no verdict-monotone schedule can reproduce a saving whose whole
source is early abandonment.**

## 4. What was built anyway, and what it measured

The prediction in §3 was not accepted as a result. The feature was implemented
and measured.

### 4a. Design

One loop at the single `MAX_SEARCH_DEPTH` call site
(`search_iterative_deepening`), mirroring `HyperCache::decide_iterative_deepening`
rather than inventing a second mechanism:

* **Schedule `[8, 32, 256]`**, compile-time asserted strictly increasing with the
  final level `>= MAX_SEARCH_DEPTH`. The final level is `256` **exactly**, and
  that is a deliberate divergence from the wedge's `512`: the audit measured that
  *raising* this cap recovers nothing (`ore_ont_10019` reads `search_depth0 = 0`
  at 512 and 2048 — the cap stops binding, its true requirement being 459–460 —
  and still DNFs). `== MAX_SEARCH_DEPTH` additionally makes the ON path's final
  level the OFF path's only level, verbatim. Overridable via
  `RUSTDL_TABLEAU_ID_SCHEDULE`; a malformed override (unparsable, empty,
  non-increasing, or final `< MAX_SEARCH_DEPTH`) is **rejected wholesale**.
* **One context reused across levels**, not rebuilt. `decide`'s per-probe setup
  clones the whole `ConceptPool` (documented there as dominating on large
  ontologies), so rebuilding per level would itself be a per-probe tax. This is
  sound because `search::branch` rolls back to its checkpoint on every `Unsat` /
  `DepthLimit` / `NodeCap`: what survives a `DepthLimit` is exactly the top-level
  `saturate` fixpoint — the deterministic closure of the root, which a fresh
  run's own first `saturate` would recompute. Extra deterministic labels can only
  make clashes *more* likely, so a reused-context `Sat` is still a model of the
  root and a reused-context `Unsat` still rests only on entailed labels.
* **`TableauContext::clear_deadline_hit`** (new, tableau crate). `deadline_hit` is
  *sticky*, so a shallow level's elapsed sub-deadline would otherwise persist and
  make the final level's genuine depth-cap `DepthLimit` read as a deadline cut.
  Those are **not** interchangeable at the mapping site — a deadline `DepthLimit`
  becomes `Ok(None)` while a depth-cap one becomes `Err(NoVerdict)`, and
  `classify_internal_with_timeout` propagates that `Err` with `?`. The flag is a
  reporting channel only and can never turn an inconclusive verdict into a
  definite one.
* **The caller's deadline bounds the whole loop.** The final level gets it
  verbatim; non-final levels get `min(shallow budget, deadline)`; the loop breaks
  before starting a level once it has passed. Deepening never multiplies the
  per-probe budget.

### 4b. How the per-pair-tax failure mode is avoided

The wedge's regression is the explicit precedent: `ID_SHALLOW_BUDGET_MS = 5` is a
**per-pair** constant, so its total cost scaled with the pair count — quadratic in
classes — and took `ore_ont_13991` (3 119 classes, 56 760 pairs) from a 32.79 s
completion to a DNF at 180 s. The main tableau is exposed to the *same*
arithmetic, and worse per unit, because its shallow budget is 4× the wedge's.
Two layers, both mirroring the shipped fix:

1. **Per-probe bound.** The whole shallow phase of one probe shares
   `min(TABLEAU_ID_SHALLOW_BUDGET_MS, remaining_caller_budget / 4)` — the wedge's
   `id_shallow_deadline` arithmetic reused verbatim, so at least 3/4 of a caller
   budget always reaches the final level.
2. **Cumulative waste shutoff.** `RUSTDL_TABLEAU_ID_SHALLOW_WASTE_MS` (default
   1000, `0` disables) budgets **wall spent in shallow phases that did NOT decide
   their probe**, per classify. A decide is never charged — it is the thing being
   paid for. Once the budget is reached the shallow phase stops running, so it can
   no longer add to the total: self-latching, no second flag.

   This metering choice is not free-form. **A consecutive-miss counter is already
   refuted** and is not reintroduced: `13991`'s shallow phase decides 84 pairs
   while missing 200 and those interleave, so any decide resets the run and a
   latch tolerating even a short streak never trips. "Consecutive" measures the
   wrong statistic — the harm is accumulated wall, which is what the budget
   denominates.

**`TABLEAU_ID_SHALLOW_BUDGET_MS = 20`, not the wedge's 5.** The constant must
follow the engine it bounds. A *whole* depth-8 main-tableau probe on the audit's
own targets costs ~19 ms (`13545`: `unsat_probe` 562 ms over 30 probes), so a 5 ms
budget would cut the shallow level short on the very population it exists to serve
while still charging every probe. 20 ms clears a depth-8 probe there and is ~50×
below the 1 000 ms per-pair budget it is carved out of.

### 4c. Separate accumulator from the wedge's — the justification

Asked explicitly by the brief. **Separate**, for three reasons in order of force:

1. **Volume.** The wedge runs on *every* classify pair; the main tableau runs only
   on the wedge's fallthrough subset. On `ore_ont_2826` that is 342 pairs reaching
   the wedge against 6 reaching the tableau — a ~57× ratio. A shared accumulator
   would be spent almost entirely by the higher-volume engine, latching the
   other's shallow phase off before it had a chance to pay for itself: the meter
   would measure one engine and charge both.
2. **Units.** A wedge shallow phase is bounded at 5 ms, a tableau one at 20 ms.
   One budget cannot be correctly sized for both, and the entire point of the
   waste metric is that it is denominated in the harm's own units.
3. **Revertibility.** The wedge feature is default ON and this one default OFF, so
   a shared mutable counter would make the OFF path observably depend on the ON
   path's behaviour — the flag would no longer be a clean A/B.

### 4d. Soundness, re-derived for THIS engine rather than inherited

The brief asks for this explicitly, and the two engines do differ. From
`owl-dl-tableau/src/search.rs`:

* `search` returns `SearchVerdict::DepthLimit` at `max_depth == 0` **before** any
  saturation or clash detection runs;
* in `search::branch`, a `DepthLimit` from *any* child sets `depth_limited = true`
  and the frame returns `DepthLimit` **instead of** `Unsat(combined)` — the
  `Unsat` arm is reached only when every option clashed decisively;
* `decide` maps `DepthLimit` to `Ok(None)` / `Err(NoVerdict)`, which every caller
  treats as "satisfiable / not subsumed".

So a depth cap can only **suppress** an `Unsat`, never manufacture one: **FP=0 is
untouched in both directions**, and deepening can only ADD entailments.
Monotonicity (`Unsat` at `k` re-derives at `k' > k` because it required no stalled
child; `Sat` at `k` has an identical DFS prefix at `k'`; only `DepthLimit` can
change, and only into a definite verdict) makes flag-ON's verdict a **superset** of
flag-OFF's — so a lost pair means the implementation is wrong, not the idea.

## 5. Gate 2 — the five measured cases, OFF vs ON

`rustdl-TID-v2`, one binary, two env arms; `classify` at the CLI default budget,
90 s cap, single thread.

| ontology | OFF | OFF rows | **ON** | **ON rows** | verdict |
|---|---|---|---|---|---|
| `ore_ont_13545` | 46.27 s | 2 482 | **46.30 s** | **2 482** | **no change** (+0.06%) |
| `ore_ont_2826`  | 7.30 s | 197 | **7.30 s** | **197** | **no change** |
| `ore_ont_10019` | dnf @90 s | 0 | **dnf @90 s** | **0** | **not recovered** |
| `ore_ont_3250`  | dnf @90 s | 0 | **dnf @90 s** | **0** | **not recovered** |
| `ore_ont_8666`  | dnf @90 s | 0 | **dnf @90 s** | **0** | **not recovered** |

**0 of 3 DNFs recovered, 0 of 2 speedups reproduced. Rows match on both
speedups** (2 482 and 197), which is the gate's own correctness requirement and is
met — trivially, because nothing changed.

### 5a. The positive control that makes this a null and not a dead code path

A measured "ON == OFF" is worthless without evidence the code ran.
`RUSTDL_TABLEAU_ID_STATS=1` dumps the accumulator on drop:

| ontology | arm | `shallow_decided` | `shallow_missed` | `shallow_waste_ms` |
|---|---|---|---|---|
| `ore_ont_2826` | OFF | 0 | 0 | 0 |
| `ore_ont_2826` | **ON** | **0** | **7** | **140** |
| `ore_ont_13545` | OFF | 0 | 0 | 0 |
| `ore_ont_13545` | **ON** | **0** | **44** | **880** |

The driver runs (44 probes on `13545` = its 30 unsat probes + 14 pairs, exactly),
and it **decides nothing** — the §3 prediction, confirmed by the feature's own
telemetry rather than by inference.

**Why the wall is unchanged rather than 880 ms worse.** The shallow phase does not
*add* wall; it spends part of the same per-pair budget. Each probe runs depth 8
for its 20 ms slice, then intermediate levels return immediately on their first
`check_deadline`, then the final level gets the remaining ~980 ms — total still
~1 000 ms, matching flag-OFF's 1 000 ms. So the ON path is wall-neutral and
carries a *small completeness exposure* (20 ms of a 1 000 ms budget that the final
level might have needed) in exchange for nothing measured. That is the honest
accounting, and it is why the recommendation below is not merely "leave it off".

### 5b. Re-run on a binary built from the COMMITTED source

`rustdl-TID-v2` was built *before* the `cargo fmt` + clippy fixes, so it is not
byte-identical to what is committed. Gate 2 was therefore re-run on
`rustdl-TID-FINAL`, built from the committed tree:

| ontology | OFF | ON | rows OFF / ON |
|---|---|---|---|
| `ore_ont_13545` | 46.29 s | 46.35 s | 2 482 / 2 482 |
| `ore_ont_2826` | 7.30 s | 7.32 s | 197 / 197 |
| `ore_ont_10019` | dnf @90 s | dnf @90 s | 0 / 0 |
| `ore_ont_3250` | dnf @90 s | dnf @90 s | 0 / 0 |
| `ore_ont_8666` | dnf @90 s | dnf @90 s | 0 / 0 |

Same null, on the source that ships. (The clippy fixes were confined to
`#[cfg(test)]` canary code, so this is a confirmation rather than a new
measurement — but it is the confirmation that makes the headline rest on the
committed tree rather than on a superseded build.)

## 6. Gate 3 — `ore_ont_10019` against the oracle

Konclude v0.7.0 native, normalised and compared with
`owl-reasoner-harness/scripts/normalise.py`. Konclude's closure is **162**.

| arm | wall | closure | **FP** | MISSED of 162 |
|---|---|---|---|---|
| **ID ON (this feature)** | dnf @150 s | **0** | **0** | 162 |
| ID OFF (shipped default) | dnf @150 s | 0 | **0** | 162 |
| **fixed depth 8 (the audit's arm, control binary)** | **61.08 s** | **158** | **0** | **4** |

**ID gives 0, exactly as OFF does — no pair asserted, so nothing to adjudicate;
FP=0 holds trivially.** The audit's depth-8 result reproduces to the pair: 158,
FP=0, and the same 4 MISSED it named, `KetoneGroup ⊑ CarbonylGroup` among them
(the other three are the `SulfoxideGroup`/`SulfinicAcidGeneralGroup`/
`OrganicSulfurGroup` cluster). So the audit's soundness claim for the fixed lower
cap is confirmed independently here — it is just not a claim about *this* change.

## 7. Gate 4 — superset property where the tableau is actually exercised

**Selection is the whole gate here, so it was made by census rather than by
sample.** A random ORE draw would be almost entirely *inert* — the flag touches
only `MAX_SEARCH_DEPTH`, which is reached only when the main tableau runs on the
deadline-bounded path — and the check would be vacuous. So all **1,920** ORE
ontologies were classified flag-ON with `RUSTDL_TABLEAU_ID_STATS=1` (10–15 s cap,
three `nice`d workers) and kept iff the accumulator recorded at least one shallow
observation, i.e. the deepening driver provably executed.

| | |
|---|---|
| ontologies scanned | 1,920 |
| completed within the 10–15 s cap | 1,664 |
| **`ID_ACTIVE` — the driver ran at all** | **26** |
| of those, `shallow_decided > 0` | **3** (`ore_ont_1524` 107/0, `ore_ont_16274` 97/3, `ore_ont_10894` 4/50) |

`ID_ACTIVE = 26` is a **lower bound**: a run killed by `timeout` takes SIGTERM
with no unwinding, so `Drop` never dumps and slow/DNF ontologies read `x` rather
than a count (`ore_ont_13545`, known active at 46 s, is one of them). Read it as
*26 of the 1,664 completers within the cap*.

**All 26 were used — no sampling.** OFF vs ON from the one `rustdl-TID-v2`
binary, 60 s cap, single thread, closures compared line-by-line with the banner
stripped:

| | |
|---|---|
| ontologies | **26** |
| outcome changes (`ok` ↔ `dnf`) | **0** |
| **pairs LOST (`OFF \ ON`)** | **0** |
| pairs gained (`ON \ OFF`) | **0** |
| **closures byte-identical** | **26 / 26** |

**No pair lost anywhere**, so the superset property holds and the hard stop is
not triggered. Nothing gained either — which, with `shallow_decided = 0` on 23 of
the 26, is what §3 predicts. Row counts are identical on all 26 and walls agree
to within ±0.06 s except `ore_ont_10894` (2.45 → 3.44 s, +0.99 s), the single
ontology in the whole corpus where the shallow phase both decides *and* misses a
lot (4 decided / 50 missed) — i.e. the one place the tax is visible, and it is a
loss.

## 8. Gate 5 — no regression on the healthy population

25 currently-fast completers, deterministic stride (`NR%60==7`) over the census's
`ok` rows with `wall < 15 s` and `rows > 0`, **excluding** the 26 `ID_ACTIVE`
ontologies so this measures the inert population rather than re-measuring gate 4.
OFF vs ON, one binary, 60 s cap, single thread.

| | |
|---|---|
| ontologies | 25 |
| outcome changes | **0** |
| row-count differences | **0** |
| materially slower (>25% **and** >2 s) | **0** |
| aggregate wall | OFF 27.62 s → ON **27.58 s** (**−0.1%**) |

Row counts span 4 to 78,974, so this is not a set of trivial ontologies. The
flatness is structural, not luck: 1,638 of the 1,664 completers never reach the
deadline-bounded main tableau at all, so the flag is dead code for them.

## 9. Gates 6 and 7 — hygiene, and sabotage

* `cargo fmt --all -- --check` — clean.
* `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean
  (three findings fixed in the canaries: `redundant_closure`,
  `duration_suboptimal_units`, `unchecked_time_subtraction`).
* `cargo test --workspace --exclude owl-dl-py --release --no-fail-fast` —
  **128 result groups, 1,524 passed, 0 failed, 0 non-`ok` groups.** (`--no-fail-fast`
  matters: a fail-fast run of the same suite reports ~60 groups, and taking that
  as the total is a known way to under-report this gate.)
* **FP=0 net with the flag ON** (`RUSTDL_TABLEAU_ITERATIVE_DEEPENING=1
  ./scripts/run-soundness-diff.sh`) — **11 VERIFIED, every closure EXACT** at the
  committed reference: galen 27997, notgalen 32739, sio 8904, ore-10908 6001,
  wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16.
  Nothing grew and nothing shrank, so no adjudication was owed. The three
  `NOT VERIFIED (fixture absent)` entries (`ro-stripped`, `sulo-stripped`,
  `sio-stripped`) are the pre-existing documented gaps, unchanged.

### 9a. Canaries — 14, negatives first

Three controls run before any deepening or shutoff claim is asserted, and
**the first one paid for itself immediately.**

> The wedge's own 12-deep `⊔`-chain fixture — the one whose docstring explains at
> length why it needs branch depth 12 — is decided by the **MAIN TABLEAU at depth
> 8**. `absorb` plus dependency-directed back-jumping compress it to roughly one
> branch decision per five links. Measured: depth 8 decides n = 4, 8, 12 and 24,
> and first MISSES at n = 48. Without that control, `deep_chain_is_not_decided_by
> _the_shallow_level`, `deepens_past_a_depth_limited_shallow_level`,
> `a_non_deciding_probe_charges_waste` and `waste_budget_zero_disables_the_shutoff`
> would all have been vacuous — four green assertions about a fixture the shallow
> level was already solving. It is also the concrete reason the FP-safety and
> monotonicity arguments in §4d were re-derived from `search.rs` rather than
> transplanted: one fixture generator, two engines, two different depth
> requirements.

The other two controls pin that `shallow_decided` is a reachable state
(`shallow_chain_is_decided_by_the_shallow_level`, n = 12) and that the flag obeys
the default-OFF idiom including the empty-value case
(`flag_defaults_off_and_only_1_enables`).

### 9b. Sabotage — 10 applied strictly serially, **8 caught, 2 SURVIVORS**

Counts reported **as run, including survivors**. Each: apply one mutation to
`lib.rs`, run `cargo test -p owl-dl-reasoner --lib tableau_iterative_deepening`,
revert, next.

| # | sabotage | result | first canary to catch it |
|---|---|---|---|
| 1 | the loop never deepens (break after level 0) | **caught** (7 of 14 failed) | `deepens_past_a_depth_limited_shallow_level` |
| 2 | a `DepthLimit` is treated as DEFINITE (never deepened past) | **caught** (7 failed) | `deepens_past_a_depth_limited_shallow_level` |
| 3 | the shutoff NEVER triggers | **caught** (1 failed) | `a_latched_accumulator_skips_the_shallow_phase` |
| 4 | the shutoff triggers ALWAYS | **caught** (5 failed) | `waste_budget_zero_disables_the_shutoff` |
| 5 | charge waste on EVERY probe (a decide is not free) | **caught** (2 failed) | `a_deciding_probe_charges_no_waste` |
| 6 | never charge waste (the accumulator cannot grow) | **caught** (1 failed) | `a_non_deciding_probe_charges_waste` |
| 7 | a SKIPPED (latched) probe still observes — the latch is not self-sustaining | **caught** (1 failed) | `a_latched_accumulator_skips_the_shallow_phase` |
| 8 | a malformed `RUSTDL_TABLEAU_ID_SCHEDULE` is accepted wholesale | **caught** (7 failed) | `malformed_schedule_override_is_rejected_wholesale` |
| **9** | **drop `clear_deadline_hit` before the final level** | **SURVIVED — 14/14 green** | — |
| **10** | **a latched probe jumps to `last - 1`, not the final level** | **SURVIVED — 14/14 green** | — |

**The two survivors, honestly.**

*#9* is a real gap in coverage. `clearing_the_sticky_deadline_flag_works` pins the
`TableauContext` API but **not the call**, so the canaries do not detect the loop
failing to clear the flag. The consequence of that bug is confined to *which*
inconclusive result a probe reports — `Ok(None)` instead of `Err(NoVerdict)` — and
both are treated as a timed-out pair by `subsumes_via_tableau`, so no verdict
moves. It is observable only through `classify_internal_with_timeout`, which
propagates the `Err` with `?`; catching it needs a canary on the `n²` classify
driver, which these do not exercise. **Recorded as uncaught, not argued away.**

*#10* is the same limitation the wedge write-up records for its own sabotage #8,
and for the same reason: with `start = last - 1` the loop still deepens to the
final level and still gets the right answer, so it is not verdict-losing at all —
it only wastes one level's worth of re-descent. So the soundness canary's power is
demonstrated by #1–#8, which do change what runs, and **not** by #10. No sabotage
I could construct made this driver lose a verdict, which is consistent with the
monotonicity argument in §4d but is weaker evidence than a caught mutant.

## 10. Recommendation

**Leave `RUSTDL_TABLEAU_ITERATIVE_DEEPENING` default OFF, and do not queue an
ORE-wide two-arm sweep for it.** The brief's rule is "do not flip; an ORE-wide
sweep gates that decision" — but a sweep is what you run when instance evidence
looks good and you need to bound the population risk. Here the *instance* evidence
is a null and the *census* explains why:

* 0 of 3 DNFs recovered, 0 of 2 speedups reproduced, on the audit's own targets
  reproduced to two decimals on the control binary (§2, §5);
* the driver provably ran and **decided nothing** on either speedup case (§5a);
* over the whole 1,920-ontology corpus the driver is reachable on **26**
  ontologies and its shallow level decides on **3** (§7);
* on the 26, closures are byte-identical 26/26 and the only visible wall movement
  is a **loss** (`ore_ont_10894` +0.99 s).

A sweep would cost hours to confirm a mechanism already measured inert on its own
addressable set. **It is kept as opt-in scaffolding, not deleted**: the
implementation is correct, its monotonicity is canaried, and the marginal cost of
retaining it is one default-OFF branch on a path 1,638 of 1,664 ontologies never
reach. If a future workload appears where the main tableau's shallow level
genuinely *decides* pairs, it can be switched on rather than rebuilt.

### What the audit's data does support, and what it would cost

The measured lever is **a lower fixed cap**, and this document confirms its two
halves on a pinned control binary: three DNFs recovered (`10019` 61.08 s, `3250`
7.79 s, `8666` 10.06 s), two completers ~14× faster with byte-identical rows, and
`ore_ont_10019` at **closure 158, FP = 0, MISSED 4 of 162** against **0 / DNF**
today (§6).

It is **not** a free win and must not be pursued under this project's licence:

1. **It loses pairs.** Four on `10019` alone, named in §6. Deepening's "cannot
   lose a pair" guarantee does not transfer; the correctness gate for a lower cap
   is an **ORE-wide MISSED net against a Konclude ∪ HermiT oracle**, not a
   superset check.
2. **The audit already found the counter-example.** `ore_ont_3281` is made *two
   orders of magnitude worse* at depth 8 (10.3 M `search` entries, 7.9 M cap
   hits — the re-descent pathology, this time caused by a cap that is too
   *small*). So no single fixed value is right either, which is the original
   disease.
3. **The shape that fits the evidence is an adaptive early-abandon** — a
   main-tableau analogue of the wedge's `is_diverging` cut, abandoning a probe
   that is depth-saturated and making no progress instead of burning its whole
   budget. That is a *completeness-for-wall trade*, the opposite direction from
   deepening, and it needs its own spec and its own gate.

**Concretely: the next step is not a sweep of this flag. It is a spec for an
adaptive early-abandon on the main tableau, gated on an ORE-wide MISSED net.** The
26-ontology census in §7 is the addressable set that spec should be sized
against, and it is small — which is itself worth knowing before the work starts.

### What is NOT established

* **No ORE-wide two-arm sweep was run for this flag.** The 26-ontology gate-4 set
  is a census of where the driver *can* act, which is stronger than a sample for
  bounding the reward but does not bound wall risk on the 256 ontologies that DNF
  at a 10–15 s cap and whose counters were therefore unreadable.
* **The census cap is 10–15 s**, so `ID_ACTIVE = 26` is a lower bound (§7).
* **The `TABLEAU_ID_SHALLOW_BUDGET_MS = 20` constant is calibrated on two
  ontologies** (`13545`, `2826`). Since the feature is inert, it has not been
  stressed; anyone enabling the flag should re-derive it.
* **Sabotage #9 is uncaught** (§9b), so the canaries do not protect the
  sticky-deadline-flag clear.
