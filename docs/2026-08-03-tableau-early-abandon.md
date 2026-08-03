# Adaptive early-abandon on the MAIN TABLEAU — a completeness/wall trade, measured

**Flag:** `RUSTDL_TABLEAU_EARLY_ABANDON`, **DEFAULT OFF** (`=1` enables; house idiom
`is_some_and(|v| v == "1")`). Limit override `RUSTDL_TABLEAU_EARLY_ABANDON_HITS`
(default 32, `0` = accounting live but never cut). Telemetry
`RUSTDL_TABLEAU_EA_STATS=1`.
**Date:** 2026-08-03 · **Base:** worktree @ `72a1103`, rustdl 0.4.13 — the same
commit the committed MISSED-net baseline was built from.

> ## Headline
>
> *(filled in after the net — see §4. The prediction in §3 was committed BEFORE
> any outcome was measured; the commit hash is recorded there.)*

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

*(§4 onwards written after the arm ran.)*
