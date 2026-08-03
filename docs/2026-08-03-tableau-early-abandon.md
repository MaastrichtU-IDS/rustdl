# Adaptive early-abandon on the MAIN TABLEAU — a completeness/wall trade, measured

**Flag:** `RUSTDL_TABLEAU_EARLY_ABANDON`, **DEFAULT OFF** (`=1` enables; house idiom
`is_some_and(|v| v == "1")`). Limit override `RUSTDL_TABLEAU_EARLY_ABANDON_HITS`
(default 32, `0` = accounting live but never cut). Telemetry
`RUSTDL_TABLEAU_EA_STATS=1`.
**Date:** 2026-08-03 · **Base:** worktree @ `72a1103`, rustdl 0.4.13 — the same
commit the committed MISSED-net baseline was built from.

> ## Headline
>
> **ΔMISSED = 0 against the 5 198 baseline, FP = 0, and the recoveries are real.**
> Over the committed 400-ontology MISSED-net population the arm scores
> **MISSED = 5 198 on 60 of 393 ontologies — identical to the baseline, pair for
> pair** (0 lost, 0 gained, 0 newly unscored), with **all 400 closures
> byte-identical** and **0 outcome changes**, while the aggregate wall falls
> **−12.2%** and **22 ontologies get 1.3×–5.7× faster**.
>
> **Three DNFs recover, and all three are COMPLETE** against Konclude ∪ HermiT
> (`ore_ont_3250` 76 pairs, `8666` 68, `3281` 224 — **FP = 0 and MISSED = 0 vs
> BOTH peers on each**). One of them is **`ore_ont_3281`, the ontology a fixed
> lower cap makes two orders of magnitude WORSE** — which is the clearest evidence
> that the adaptive shape is the right one rather than a re-tuned constant.
>
> **`ore_ont_10019` is NOT recovered, exactly as predicted in writing before the
> measurement** (§3): its probes at the default budget mostly never reach the cap
> (median `depth0 = 0`), so there is no stall to detect.
>
> **By the pre-fixed decision rule — ΔMISSED = 0 with recoveries — this is an
> unambiguous win, and the recommendation is DEFAULT ON.** The flag ships **OFF**
> in this commit, because flipping a default was explicitly out of scope here.

---

## 1. What this lever is, and why it could not be evaluated before

`docs/2026-08-03-constant-audit.md` §4 measured `MAX_SEARCH_DEPTH = 256` (the
**main tableau's** deadline-bounded depth cap, `owl-dl-reasoner/src/lib.rs`) binding
on **27 of the 33** ORE ontologies that reach the main tableau at all, with **zero
headroom on every one**. A *fixed lower* cap of 8 recovers three DNFs
(`ore_ont_10019` 60.6 s, `3250` 7.8 s, `8666` 10.2 s) and makes two completers
~14× faster with byte-identical rows (`13545` 46.30 → 3.11 s / 2 482 rows; `2826`
7.29 → 0.54 s / 197 rows).

`docs/2026-08-03-tableau-iterative-deepening.md` then established from the classify
banner that **the whole 14× is time on probes that reach no verdict at either
depth** — `tableau=0` for subsumption at both depths, identical timed-out-pair
counts, identical rows; the difference is only *how long a doomed probe burns
before giving up*. Iterative deepening is verdict-monotone by construction, so it
must re-run every undecided probe at a final level `>= MAX_SEARCH_DEPTH`; it
reproduced the flag-OFF cost exactly (0 of 3 DNFs recovered, 0 of 2 speedups) and
is default OFF as a documented null. And a fixed lower cap is not the answer
either: `ore_ont_3281` is made two orders of magnitude worse by it (10.3 M `search`
entries, 7.9 M cap hits).

So the shape the evidence supports is an **adaptive early-abandon**: stop a probe
that is not going to conclude, sooner than depth 256 forces. That trades
completeness for wall, which is why it was unevaluable — until the corpus MISSED
net (`owl-reasoner-harness/docs/missed-net.md`) gave a **baseline MISSED = 5 198**
over 60 of 393 scored ontologies with **FP = 0 on all 393**, and a later arm for
~10 minutes.

## 2. The abandon criterion, and the shape that was refuted first

### 2a. What was tried first, and why it does not work

The obvious transplant is the wedge's `is_diverging`: over a window of branches,
~all failed at saturated depth ⇒ `Stalled`. Its main-tableau analogue is sharper
than the wedge's proxy, because this engine can tell a *decisive* rollback (`Unsat`
from a child) from an *inconclusive* one (`DepthLimit`): abandon after a **run** of
depth-cap bottom-outs with no definite child verdict in between, resetting the run
on any definite verdict. "Progress" and "no progress" are then read directly off
the verdicts rather than inferred from `restores ≈ branches`.

**It was built, armed in telemetry-only mode (`…_HITS=0`), and refuted by its own
calibration data** on the two ontologies the audit names as the 14× cases
(one binary, `RAYON_NUM_THREADS=1`, `ulimit -v 24 GB`, CLI-default 1 000 ms
per-pair budget):

| ontology | armed probes | trials | **definite** | depth-cap hits | **longest run** |
|---|---|---|---|---|---|
| `ore_ont_2826` | 7 | ~1 072 | **229** | 86 | **2** |
| `ore_ont_13545` | 44 | ~2 575 | **242** | ~1 012 | **32** |

These probes are **locally productive throughout** — 9–21% of trials return a
definite verdict, and they interleave — and they still reach no verdict at all
(`fallthrough … noverdict=6` / `noverdict=14`, `subsumption: … tableau=0` on both).
A run-based cut would need a limit of 2 on `2826` to fire, which would cut
essentially every search. **"Consecutive" measures the wrong statistic here** — the
same conclusion the wedge write-up reached about a consecutive-miss counter,
re-established independently on this engine. The `stall_run` / `max_stall_run`
telemetry is retained in `EarlyAbandon` so this refutation stays checkable rather
than becoming folklore.

### 2b. The criterion that ships

**Abandon a probe once it has bottomed out at the depth cap `K` times in total
(`depth0 >= K`), `K = 32`.** Latched at the source of the harm
(`TableauContext::note_depth_cap_hit`); from the latch on, every `search` entry
returns `DepthLimit`, so the DFS unwinds and no sibling is tried.

Why a cap bottom-out is the right currency, and how it separates *"will not
conclude"* from *"needs more depth"*:

1. **A bottom-out is direct evidence of the incomplete regime.** In
   `search::branch`, a `DepthLimit` from any child sets `depth_limited = true` and
   the frame returns `DepthLimit` **instead of** `Unsat(combined)`. Once a subtree
   has been cut off by the cap, the frame above it can only still conclude via a
   *sibling*'s `Sat` or a *back-jumping* `Unsat` (one whose deps exclude that
   frame's own `branch_id`). `K` such poisoned subtrees is evidence the probe is
   grinding through a region this cap cannot close.
2. **"More depth" has been refuted as the missing ingredient on this population,
   by measurement rather than argument.** The audit read `ore_ont_10019`'s genuine
   depth requirement off two independent arms — 459 at cap 512, 460 at cap 2048,
   with `search_depth0 = 0` at both, i.e. the cap no longer binding — and it
   **still DNFs**. And iterative deepening, the instrument that hands a probe more
   depth, measured a null on all five cases. A probe that keeps bottoming out is
   not one that a larger budget of the same kind rescues.
3. **A converging search is cheap under this criterion in the only sense that
   matters.** The criterion does not fire on a probe that never reaches the cap —
   and that is not hypothetical: `ore_ont_10019`'s probes at the default budget
   have **median `depth0 = 0`** with 2 077 definite trials of 2 297 (see §3), so
   the lever is inert on them by construction.

### 2c. Calibration of `K = 32`

Telemetry-only arm (`…_HITS=0`), same host discipline, per-probe cap-hit counts:

| ontology | probes | cap hits per probe | note |
|---|---|---|---|
| `ore_ont_13545` | 44 | min **1 003**, max 1 040 | every probe far above 32 |
| `ore_ont_8666` | 89 | min **864**, median 1 216 | every probe far above 32 |
| `ore_ont_3250` | 86 | median **648**, max 1 652 | most probes far above 32 |
| `ore_ont_2826` | 7 | **86** | 32 ≈ 37% of the probe's budget |
| `ore_ont_10019` | 63 | **median 0**, max 965 | **mostly not addressable** |

32 fires on all four addressable targets while leaving 32 poisoned subtrees of
slack. It is not tuned against the MISSED net (that would launder the gate into the
design); it is read off the calibration arm and then measured once.

### 2d. Soundness — re-derived for this engine, not inherited

* `search` returns `DepthLimit` at `max_depth == 0` **before** any saturation or
  clash detection;
* in `search::branch` a `DepthLimit` child yields `DepthLimit` from the frame, never
  `Unsat`;
* `decide` maps the abandon to **`Ok(None)`** — a new arm placed *before* the
  deadline arm, so an abandon reports the same sound "don't know" a deadline cut and
  a `NodeCap` trip already report, and never `Err(NoVerdict)`.

So the cut can only **suppress** an `Unsat`, never manufacture one: **FP = 0 is
untouched in both directions**, and the entire exposure is completeness. The
reported-answer consequence is a MISS (`subsumes_via_tableau` treats a non-verdict
as "not subsumed"; a class satisfiability probe's `unwrap_or(true)` reports
"satisfiable"), plus a *larger* `timed-out pairs` count — i.e. rustdl reports **more**
incompleteness than before, which is the conservative direction.

### 2e. Proof that the instrument fires — criterion declared in advance

Declared before running: *with the flag ON at the default `K = 32`, `ore_ont_13545`
must report `abandoned=1` on ≥90% of its armed probes, with `depth0` in [32, 64]*.

Measured: **44 armed probes, 44 `abandoned=1`, `depth0 = 32` on all 44.**

## 3. Prediction — committed BEFORE the net was run

Committed in this file, in the commit whose message begins
`spec(tableau): early-abandon criterion + committed prediction`, before any arm was
swept and before any wall other than the calibration arm above was recorded.

**ΔMISSED vs the 5 198 baseline:** **> 0 but small — predicted range +0 to +150**,
**concentrated entirely in the out-of-EL/search stratum**, and **exactly 0 in the
140 pure-EL/no-search and Horn/no-search rows** (a saturation-fast-path ontology
issues no main-tableau probe, so the flag is dead code for it). **FP = 0**,
structurally. **`newly_unscored` = 0** — the cut can only make a probe cheaper, so
no ontology should turn from an answer into a DNF.

**Recoveries and walls on the five named ontologies** (60–90 s cap, single thread,
one binary switched by env):

| ontology | flag OFF (expected) | **flag ON — PREDICTED** |
|---|---|---|
| `ore_ont_13545` | ~46 s, 2 485 rows | **~5–9 s**, rows within a few of 2 485 |
| `ore_ont_2826` | ~7.3 s, 201 rows | **~3.5–5 s** (only ~1.5–2×: 32 is 37% of its probe budget) |
| `ore_ont_3250` | dnf | **RECOVERED, ~8–25 s** |
| `ore_ont_8666` | dnf | **RECOVERED, ~10–30 s** |
| `ore_ont_10019` | dnf | **NOT recovered — still dnf.** Its probes mostly never reach the cap at this budget (median `depth0 = 0`), so there is no stall to detect; the audit's depth-8 recovery there comes from *shrinking the tree*, not from abandoning at the cap. Predicting this in advance is the point. |

**25-ontology fast-population control:** **0 outcome changes, 0 row-count
differences, no aggregate wall regression.** The flag is unreachable for the ~98%
of completers that never enter the deadline-bounded main tableau (the
iterative-deepening census measured 1 638 of 1 664).

## 4. Results

### 4a. Binaries and host discipline

Every binary pinned to a uniquely named path **immediately after the build that
produced it**, and named after its configuration. **Both arms of every A/B come
from ONE binary switched by `RUSTDL_TABLEAU_EARLY_ABANDON`**, never from two
builds, so a stale-binary mix-up cannot produce (or hide) a delta.

| path | sha256 | what |
|---|---|---|
| `…/missed-net/bin/rustdl-v0413-main-72a1103` | `44d7d80e…` | the committed MISSED-net baseline, control |
| `…/rustdl-scratch/ea-bin/rustdl-EA-72a1103` | `84b49e37…` | first build (refuted run-reset criterion + telemetry) |
| `…/rustdl-scratch/ea-bin/rustdl-EA-CAPHITS-72a1103` | `a9cf7210…` | **the measured feature binary** (§2c, §4b–4e) |
| `…/rustdl-scratch/ea-bin/rustdl-EA-FINAL-committed` | `da7f553a…` | **built from the COMMITTED tree** (post-`fmt`/clippy); §4f |

Built with `PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
RUSTUP_TOOLCHAIN=stable cargo build --release` (a bare `cargo` is not on `PATH`
here). Every probe under `( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1
timeout N … )`. The named-ontology and 25-ontology batteries ran **serially**; the
400-ontology sweep ran at `JOBS=2`, so its walls are comparable *within* the
sweep (both arms interleaved on the same host) and its one apparent regression was
re-checked serially (§4d).

**Flag-OFF is byte-identical to the pinned baseline binary**, checked before
anything else: `classify` closures match on `ore_ont_2826` (201 rows) and
`ore_ont_13545` (2 485 rows).

### 4b. The MISSED net — the gate

`scripts/missed-net.sh sweep EA32 <pinned bin>` over the committed
400-ontology population (seed 20260803), then
`net EA32 --baseline baselines/2026-08-03-missed-net-v0413.jsonl`.

| | baseline v0413 | **EA32 (flag ON)** |
|---|---|---|
| scored ontologies | 393 | **393** |
| **MISSED total** | **5 198** | **5 198** |
| ontologies with MISSED | 60 | **60** |
| **FP total** | **0** | **0** |
| ontologies with FP | 0 | **0** |
| `peer_disagreement` | 1 | 1 |
| oracle closure | 14.0 M | 14.0 M |

**`ΔMISSED = 0`. `onts_lost_pairs = 0`. `onts_gained_pairs = 0`.
`newly_unscored = 0`. FP = 0.** And stronger than the net requires: **all 400
closure hashes are identical between the two arms**, and there are **0 outcome
changes**.

### 4c. Proof the lever was not merely inert on that population

A net that reports 0 because the flag never fired is indistinguishable from a
broken net, so this is checked rather than assumed — and the check is the *reason*
the ΔMISSED = 0 is meaningful:

* **22 of 400 ontologies move by >20% and >1 s**, 21 of them **faster**, aggregate
  **1 537.4 s → 1 349.8 s (−12.2%)**.
* The attribution is exactly the mechanism. `ore_ont_13545`'s banner:
  `unsat_probe` **30 016 → 6 519 ms**, `sweeps` 13 142 → 3 775 ms, `tier_walk`
  2 713 → 1 163 ms — and the non-banner closure diff is **0 lines**. Same shape on
  `7011` (30 008 → 5 962), `1958` (30 010 → 6 605), `7007` (1 001 → 161),
  `2826` (1 000 → 642).
* Per-probe telemetry (§2e): **44 of 44** armed probes on `13545` abandon, at
  `depth0 = 32` exactly.

| ontology | OFF | **ON** | speedup |
|---|---|---|---|
| `ore_ont_7007` | 31.73 s | **5.59 s** | 5.7× |
| `ore_ont_7011` | 46.46 s | **11.02 s** | 4.2× |
| `ore_ont_13545` | 46.30 s | **11.91 s** | 3.9× |
| `ore_ont_1958` | 46.51 s | **12.17 s** | 3.8× |
| `ore_ont_9689` | 2.05 s | **0.55 s** | 3.7× |
| `ore_ont_10874` | 1.18 s | **0.32 s** | 3.7× |
| `ore_ont_14379` | 4.03 s | **1.51 s** | 2.7× |
| 13 more (`16847`, `5834`, `8042`, `850`, `2826`, `3156`, `3010`, `10702`, `13954`, `11477`, `16800`, `9053`, `5964`, `2182`) | | | 1.3×–1.6× |
| **`ore_ont_12698`** | 5.04 s | **6.40 s** | **0.8× — the only regression** |

### 4d. The five named ontologies (plus two controls), serial

One binary, two env arms, 90 s cap, single thread, `RAYON_NUM_THREADS=1`.

| ontology | OFF | **ON** | rows OFF / ON | closure |
|---|---|---|---|---|
| `ore_ont_13545` | 46.31 s | **11.88 s (3.9×)** | 2 485 / 2 485 | **IDENTICAL** |
| `ore_ont_2826` | 7.31 s | **4.85 s (1.5×)** | 201 / 201 | **IDENTICAL** |
| `ore_ont_3250` | **dnf @90 s** | **31.95 s — RECOVERED** | 0 / **76** | new |
| `ore_ont_8666` | **dnf @90 s** | **67.84 s — RECOVERED** | 0 / **68** | new |
| `ore_ont_10019` | dnf @90 s | **dnf @90 s — NOT recovered** | 0 / 0 | — |
| **`ore_ont_3281`** (audit's counter-example) | **dnf @90 s** | **19.91 s — RECOVERED** | 0 / **224** | new |
| `ore_ont_12698` (the sweep's regression) | 5.40 s | **5.46 s** | 21 316 / 21 316 | **IDENTICAL** |

`ore_ont_12698` is **noise, not a regression**: serially it is 5.40 → 5.46 s
(+1.1%) with an identical closure, against 5.04 → 6.40 s under the `JOBS=2`
sweep. It is the only ontology in 400 that moved the wrong way, and it does not
reproduce.

**`ore_ont_3281` is the finding that distinguishes this lever from a re-tuned
constant.** The audit measured it *harmed* two orders of magnitude by a fixed cap
of 8 (10.3 M `search` entries, 7.9 M cap hits — re-descent caused by a cap that is
too *small*), which is why "no single fixed value is right" was the audit's own
conclusion. The adaptive cut leaves the cap at 256 and **recovers it in 19.91 s**.

### 4e. FP adjudication of the three new answers — mandatory, and clean

The recovered ontologies are **not** in the MISSED-net population (they were DNF
when the frame was built, so they had no closure to diff), so their soundness is
not covered by §4b. Each was adjudicated separately against **both** peers at a
150 s cap, normalised and compared with `owl-reasoner-harness/scripts/normalise.py`
(`compare`), peer output accepted only when `triage.declared_real_class` confirms a
real hierarchy:

| ontology | rustdl ON rows | vs Konclude | vs HermiT |
|---|---|---|---|
| `ore_ont_3250` | 76 | **FP=0 MISSED=0** | **FP=0 MISSED=0** |
| `ore_ont_8666` | 68 | **FP=0 MISSED=0** | **FP=0 MISSED=0** |
| `ore_ont_3281` | 224 | **FP=0 MISSED=0** | **FP=0 MISSED=0** |

So these are not merely *fast* new answers, they are **complete** ones: three
ontologies go from no answer at all to full Konclude ∪ HermiT parity.

### 4f. Re-measured on a binary built from the COMMITTED source

`rustdl-EA-CAPHITS` predates the `cargo fmt` + clippy fixes, so the named-ontology
battery was re-run on `rustdl-EA-FINAL-committed` (`da7f553a…`), built from the
committed tree. The clippy fixes were confined to `#[cfg(test)]` canary code, so
this is a confirmation rather than a new measurement — but it is the confirmation
that makes the headline rest on what ships:

| ontology | OFF | ON | ON closure vs pre-`fmt` binary |
|---|---|---|---|
| `ore_ont_13545` | 46.35 s | 11.90 s | **SAME** |
| `ore_ont_2826` | 7.31 s | 4.82 s | **SAME** |
| `ore_ont_3250` | dnf | 31.98 s (76 rows) | **SAME** |
| `ore_ont_8666` | dnf | 67.91 s (68 rows) | **SAME** |
| `ore_ont_10019` | dnf | dnf | **SAME** |
| `ore_ont_3281` | dnf | 19.91 s (224 rows) | **SAME** |

### 4g. The 25-ontology fast-population control

Deterministic stride over the population's `ok` rows with `wall < 15 s` and
`rows > 0`, **excluding** the 22 wall movers, so this measures the *inert*
population rather than re-measuring §4c. Serial, one binary, two env arms.

| | |
|---|---|
| ontologies | 25 |
| outcome changes | **0** |
| row-count differences | **0** |
| closures identical | **25 / 25** |
| materially slower (>25% **and** >2 s) | **0** |
| aggregate wall | OFF 15.45 s → ON **15.33 s** (**−0.8%**) |

Row counts span 12 to 4 799. Two sub-second ontologies read slower in absolute
terms (`10409` 0.28 → 0.54 s, `11291` 0.02 → 0.12 s) — first-run page-cache
artefacts on the ON arm, well inside the >2 s materiality floor and not present in
the 400-ontology sweep, where both read flat.

The **400-ontology sweep in §4b is itself the stronger regression control**: 0
outcome changes and 400/400 identical closures over a population that includes
every fragment stratum.

## 5. Gates

* `cargo fmt --all -- --check` — clean.
* `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean
  (three findings fixed, all in canary code: `type_complexity`,
  `unchecked_duration_subtraction`, `assertions_on_constants`).
* `cargo test --workspace --exclude owl-dl-py --release --no-fail-fast` —
  **131 result groups, 1 566 passed, 0 failed, 0 non-`ok` groups.**
* **FP=0 net with the flag ON** (`RUSTDL_TABLEAU_EARLY_ABANDON=1
  ./scripts/run-soundness-diff.sh`, exit 0) — **11 VERIFIED, every closure
  EXACT** at the committed reference: galen 27997, notgalen 32739, sio 8904,
  ore-10908 6001, wine 653, pizza 499, alehif 247, ro 158, ore-15672 142,
  sulo 51, bibtex 16. Nothing grew and nothing shrank. The three
  `NOT VERIFIED (fixture absent)` entries (`ro-stripped`, `sulo-stripped`,
  `sio-stripped`) are the pre-existing documented gaps, unchanged. Note this net
  is **FP-shaped and curated**; it does not substitute for §4b, and §4b is what
  gates this change.

### 5a. Canaries — 13, negatives first

**9 in `owl_dl_tableau::search::early_abandon_tests`** (the mechanism, through the
real `search` driver) **+ 4 in `owl_dl_reasoner::tableau_early_abandon_tests`**
(the flag's default-OFF idiom, the limit override's fallback, the calibrated
constant, and the `decide` verdict mapping).

**Two controls paid for themselves immediately, and both were failures of the
obvious fixture rather than of the feature:**

> **(i) The naive `⊔`-chain has no depth at all.** Labelling `¬a, ¬b, ¬c_k` at the
> node and using bare atomics as the second disjunct means every such disjunct is
> pruned by the `⊔`-rule's literal-complement check, leaving one live disjunct per
> level: the whole chain unit-propagated to a clash with **zero** branch decisions
> and **zero** cap hits. Measured `trials = 14, definite = 14, depth0 = 0` at every
> cap from 2 to 40. Three controls passed and five assertions were vacuous.
> Fixed by making the second disjunct `c_k ⊓ ¬c_k` — a clash that needs
> *expansion* and therefore survives the prune.
>
> **(ii) `ConceptPool::or` FLATTENS a nested `Or`.** `or([level_{k-1}, c_k])`
> collapsed the 13-level chain into ONE flat 14-ary disjunction resolved in a
> single frame. The nested level had to be wrapped in `⊓(level, filler)`. The same
> bug bit the satisfiable fixture from the other side: flattening exposed bare
> atomics, `reorder_disjuncts` scored them 1 against the conjunctions' 2, so the
> **satisfiable** option was tried first, the chain was never entered and the cut
> never fired.

Both are recorded in the fixtures' own doc comments, because each is a trap the
next person writing a depth canary for this engine will otherwise re-enter.

### 5b. Sabotage — 10 applied strictly serially, **8 caught, 2 SURVIVORS**

Counts reported **as run, including survivors**. Each: apply one mutation, run
`cargo test -p owl-dl-tableau --lib early_abandon`, revert, next.

| # | sabotage | result | first canary to catch it |
|---|---|---|---|
| 1 | the cut NEVER fires | **caught** (4 of 9 failed) | `the_cut_fires_at_the_limit` |
| 2 | the cut ALWAYS fires (`0` no longer disables) | **caught** (4 failed) | `limit_zero_keeps_accounting_but_never_cuts` |
| 3 | `early_abandoned()` hard-wired `false` (search-entry latch removed) | **SURVIVED** | — |
| 4 | `note_branch_trial` never reports the cut | **caught** (1 failed) | `the_cut_does_strictly_less_work` |
| **5** | **criterion switched back to the refuted `stall_run`** | **caught** (1 failed) | `a_definite_verdict_does_not_reset_the_criterion` |
| 6 | `note_depth_cap_hit` call site deleted | **caught** (5 failed) | `limit_zero_keeps_accounting_but_never_cuts` |
| **7** | **a DEADLINE cut miscounted as a depth-cap hit** | **caught** (1 failed) | `a_deadline_cut_is_not_counted_as_a_depth_cap_hit` |
| 8 | `early_return.is_none()` guard dropped | **SURVIVED** | — |
| 9 | abandon reported as `Unsat` (the FP direction) | **caught** (2 failed) | `the_cut_never_manufactures_an_unsat` |
| 10 | arming alone aborts the search | **caught** (7 failed) | `unarmed_deep_cap_refutes_the_chain` |

**#5 and #7 survived the FIRST battery and were closed by new canaries, which is
the point of running it.** #5 is the design's central decision, and the original
`a_definite_verdict_does_not_reset_the_criterion` set its limit to `max_stall_run`
itself — a value the refuted variant also reaches, so all 9 stayed green. The
canary now sets the limit to `max_stall_run + 1`, reachable only cumulatively, and
asserts it is still `<= depth0`. #7 needed a fixture with an already-elapsed
deadline; without one, the mutation's added hook sits on an unreachable path.

**The two remaining survivors, honestly.** Both mutate code that is **provably
redundant**, and I could not construct a fixture that distinguishes either:

* **#3** — the `search`-entry latch check is defence in depth. The latch already
  propagates through `note_branch_trial`'s early-return, so after the cut fires no
  frame tries another sibling either way; removing the entry check is
  trial-for-trial identical at unit scale (`the_cut_does_strictly_less_work` reads
  the same counts). It earns its place only against a *future* edit that removes
  the `branch` early-return — which no canary here would then catch.
* **#8** — the `early_return.is_none()` guard cannot fire: a latched probe can no
  longer produce a `Sat` or a back-jumped `Unsat`, because every `search` entry
  returns `DepthLimit` from the latch onward. So the guard is unreachable by
  construction, exactly as its comment claims, and no fixture can show its
  absence. Recorded as uncaught rather than argued away.

## 6. Recommendation

**Against the decision rule fixed before any number was seen — "ΔMISSED = 0 with
recoveries ⇒ unambiguous win; recommend default ON" — this qualifies, and the
recommendation is DEFAULT ON.** The flag ships **OFF** in this commit because
flipping a default was explicitly out of scope for this work.

What carries it:

* **ΔMISSED = 0** on the committed 400-ontology net, with **0 ontologies losing a
  single pair** and **400/400 closures byte-identical** — not "small loss", *no*
  loss;
* **FP = 0** on the net, on the curated 11-fixture soundness diff with the flag
  ON, and on all three new answers against both peers;
* **3 DNFs recovered, all three at full Konclude ∪ HermiT parity**, including the
  audit's own fixed-cap counter-example `ore_ont_3281`;
* **−12.2% aggregate wall** over 400 ontologies with 22 ontologies 1.3×–5.7×
  faster, and **0 outcome changes**;
* **1 apparent regression in 400, which does not reproduce serially.**

**What would change my mind, and what a default flip still owes.** The population
here is the MISSED net's stratified 400, deliberately over-sampling
search-exercised rows; it is **not a corpus share**, and it cannot see wall risk on
the ~157 ontologies that DNF at 60 s and therefore have no closure to diff. The
precedent in this repo is explicit — *a flag flipped on a 12-ontology benchmark
took 4 others from ~5 s to DNF* — so **before flipping the default, run a full
1,920-ontology two-arm sweep** and look for `ok → dnf`. Any `ok → dnf`, any FP
anywhere, or any ontology losing a pair on a re-run of the net would reverse this
recommendation. Because the mechanism is a **cut**, an `ok → dnf` transition is
mechanistically hard to explain — but "hard to explain" is not "measured", and
this arc has retracted ten premises for less.

## 7. What is NOT established

* **No 1,920-ontology sweep was run.** §4b covers 400; the DNF tail is outside its
  frame by construction. That sweep is the one thing a default flip owes.
* **`ore_ont_10019` is not addressed, and this lever is the wrong instrument for
  it.** Its probes mostly never reach the cap at the default per-pair budget
  (median `depth0 = 0`, 2 077 definite trials of 2 297); its cost is a decisively
  exploring search that is simply too large. That is the `--pair-timeout-ms`
  frontier, not the depth frontier.
* **`K = 32` is calibrated on four ontologies** (§2c) and then measured once.
  It was deliberately **not** tuned against the MISSED net — doing so would
  launder the gate into the design. A different `K` has not been measured, so
  nothing here says 32 is optimal, only that it is safe *and* effective.
* **The lever is unreachable for the great majority of ontologies.** It touches
  only `MAX_SEARCH_DEPTH`, i.e. the deadline-bounded main tableau; the
  iterative-deepening census put that at ~26 of 1 664 completers. Read the −12.2%
  as concentrated, not broad.
* **Sabotages #3 and #8 are uncaught** (§5b), so the canaries do not protect the
  search-entry latch check or the `early_return` guard — both argued redundant,
  neither demonstrated so by a failing mutant.
* **Peer-leg repair, disclosed.** Running the peer legs for the three recovered
  ontologies re-used `missed-net.sh peer`, whose `sweep_chunks` rebuilds
  `<peer>.jsonl` from a chunk glob; the 3-ontology run overwrote chunk `c00` of
  the shared Konclude and HermiT legs, losing ~90 case records each (the raw
  hierarchies were untouched). **The EA32 net had already been computed on the
  intact legs.** The legs were repaired by re-running exactly the missing
  ontologies into fresh chunk files and rebuilding, and the repair was validated by
  re-running the net for **both** arms on it: baseline **5 198 / 60 / FP 0 /
  393 scored** and EA32 **5 198 / 60 / FP 0 / 393 scored, ΔMISSED 0** — i.e. the
  repaired oracle reproduces the committed baseline exactly. Anyone re-running a
  peer leg on a sub-list should write it to a distinct tag.
