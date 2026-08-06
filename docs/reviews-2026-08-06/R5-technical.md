# R5 — adversarial technical review: `2026-08-06-deadline-triggered-pair-budget.md`

**Reviewer:** R5 (technical) · **Date:** 2026-08-06 · **Binary:** `target/release/rustdl` 0.4.14 (built 2026-08-06 10:39)
**Method:** the plan, its evidence doc, the calibration NO-GO, the source, plus ~25 minutes of probes —
the committed `runs/full-2026-08-06-invpair-off.jsonl` sweep, the committed
`baselines/2026-08-03-missed-net-*` artifacts, and 20 direct `classify` runs on ORE members.

---

## Verdict

**NO-GO.** The plan's central premise is factually wrong (the CLI's per-pair default is **1000 ms, not
unbounded** — `crates/owl-dl-cli/src/main.rs:155-156`), its restart-cost claim is off by ~107×, its
headline recovery walls are 32-core numbers that do not survive the single-thread configuration its own
gate uses (5 of 15 sampled recoveries DNF at threads=1), its two Task-5 decision rules are mutually
unsatisfiable against committed data, and the benefit it proposes to build is **already shipped** as
`--pair-timeout-ms` / `--global-timeout-ms`, which in 8 of 8 measured cases matches or beats the
proposed mechanism with zero engineering.

---

## Blockers

### B1. The premise in the title is false: the per-pair default is **not** unbounded

`crates/owl-dl-cli/src/main.rs:155-156`:

```rust
#[arg(long, default_value_t = 1000)]
pair_timeout_ms: u64,
```

The CLI has shipped a **1000 ms** per-pair budget as its default since long before this plan, with a
doc comment (`:143-154`) already arguing the empirical-knee case for it. The evidence doc's title —
"unbounded per-pair is the wrong default" — and the plan's architecture line ("run unbounded; if total
wall exceeds `T`, re-run with per-pair budget `B`") describe a default that the user-facing surface does
not have. Every "current default returns nothing at all" row in the evidence table
(`docs/2026-08-06-unbounded-per-pair-is-the-wrong-default.md:29-43`) was measured at
**pair_timeout = 1000 ms** — confirmed: the arm-off sweep header records `args_template: "classify {}"`,
i.e. bare defaults, and all 15 table members are `dnf` there.

Unbounded is only the **library** default (`classify()`, `crates/owl-dl-reasoner/src/classify.rs:763-766`
passes `None, None`). The plan does not distinguish the two surfaces anywhere.

What is actually being proposed, therefore, is **"restart at B=1 ms when the shipped B=1000 ms is heading
for a DNF"** — a *constant re-tune wrapped in a restart*. That is the category the plan's own **Confound
#4** ("a time constant chosen by intuition", `:74-76`) warns about, and it is the category
`docs/2026-08-03-constant-audit.md` already worked through. The plan should be re-argued as "1000 is the
wrong constant for this population", which is a much weaker and much cheaper claim.

**Required change:** rewrite the premise against the real default, and justify why a restart beats simply
lowering (or adaptively selecting) the shipped constant.

### B2. "Preparation is ~5 ms, so the re-run is nearly free" — off by ~107×

`prepare_wall_ms` covers **only** `PreparedOntology::from_internal_with_deadline` plus the lazy
`abox_verdict()` (`classify.rs:2218-2248`). It does **not** cover:

| phase a re-run must redo | where | on `ore_ont_14272` (the plan's own example) |
|---|---|---:|
| `convert_ontology` | `classify.rs:842`, *before* `classify_start` at `:2056` — so it is **outside the printed breakdown entirely** | ~5 ms |
| `saturate` | `classify.rs:2112-2120` | 3 ms |
| `prepare` | `classify.rs:2218-2248` | 5 ms |
| **`label_cache_build`** | `classify.rs:2270-2317` | **523 ms** |

Reproduced on this host (`RUSTDL_TIMING=1`, default threads):
`TIMING parse_ms=6.0 classify_ms=2850.5`,
`# wall breakdown ms: saturate=3 precheck=0 prepare=5 label_cache_build=523 unsat_probe=8 tier_walk=1632 sweeps=671 matrix=0 unattributed=3`.

**True re-run floor ≈ 536 ms of a 2,850 ms run — 19%, not 0.2%.** The plan reads `prepare=5` out of a
breakdown that lists `label_cache_build=525` **on the same line** and concludes the restart is free.
That is the same error pattern the retracted `2026-08-04-definitorial-absorption.md` was killed for:
walking past an adverse number the author had already printed.

It gets worse where it matters. On `ore_ont_15010` the *entire* run is the label cache
(`label_cache_build=5567` of a 5,600 ms wall, threads=1) — i.e. on such an ontology the wasted `T` is spent
almost **wholly in the phase the restart throws away and must redo**.

**Required change:** replace "prepare is ~5 ms" with the measured convert + saturate + label-cache floor,
per ontology, and re-derive whether the restart is affordable.

### B3. `B = 1 ms` is not a safe fallback — it can make a run **18× slower**, and the plan's own cited arm records this

`ore_ont_15010`, threads=1, this binary:

| arm | wall | rows | breakdown |
|---|---:|---:|---|
| default (`pair_timeout=1000`) | **5.6 s** | 171 | `label_cache_build=5567 tier_walk=0 sweeps=52` |
| `--pair-timeout-ms 1` | **104.0 s** | 171 | `label_cache_build=5510 tier_walk=38912 sweeps=59550` |

**The hierarchies are identical** (`comm` on sorted non-comment rows: 0 lines either side). 18.6× the wall
for a byte-identical answer. And this is exactly why it appears in the committed `TIGHT1MS` arm as
`{"ont": "ore_ont_15010", "rustdl_outcome": "dnf", "rustdl_wall_s": 60.0, … "status": "arm_no_closure"}`
— a completer with a perfect closure (284 = Konclude = HermiT) turned into **no output at all**. The plan
quotes `delta_MISSED_total: 80` from that arm's summary JSON (`:34-36`) and steps past
`newly_unscored: [["ore_ont_15010", "scored", "arm_no_closure"]]` in the same object.

The mechanism is in the source, not mysterious: `per_pair_timeout` is not a pure "cut per-pair search"
knob. It feeds two other budgets.

- `adaptive_label_cache_ms` (`crates/owl-dl-reasoner/src/lib.rs:2721-2735`): `per_pair = None` ⇒ base =
  `LABEL_CACHE_CEILING_MS = 30_000` (`:2713`); `per_pair = 1 ms` ⇒ `n × 1` clamped to ≥50. For n=178 that
  is **178 ms instead of 30,000 ms per class**, so the label heuristic — which prunes 96–100% of pairs —
  loses verdicts (`misses` 0 → 339) and every affected class falls through to the tier walk and the
  defined-sup sweeps.
- `sweep_budget = per_pair_timeout.unwrap_or(200ms)` (`classify.rs:2639`) ⇒ 1 ms sweeps, ×N probes = the
  59.5 s `sweeps` above.

Three consequences, each independently blocking:

1. **The fallback run is unbounded in wall.** The plan specifies `B` and `T` and no bound on the second
   run. Worst case is `T` + 104 s and still nothing.
2. **`ok → dnf` is not a hypothetical near the cap** (Task 5 bullet 3); it is measured, on a 5.6 s
   completer, at any `T < 5.6 s`.
3. **The "raising the budget buys almost nothing" curve is confounded** by this same coupling: raising `B`
   also raises the label-cache and sweep budgets, so 1 ms → 50 ms is *not* a clean per-pair-search sweep.
   Task 2 as written will re-measure the confound.

**Required change:** bound the fallback run (`--global-timeout-ms`-style), and re-measure Task 2 with
`RUSTDL_LABEL_CACHE_TIMEOUT_MS` pinned so `B` varies one thing.

### B4. The headline recovery walls are **32-core** numbers; the gate sweep is **threads=1**

The evidence doc's "0.2–7.8 s" and "`prepare=5 ms` of a 2,808 ms run" are full-parallelism figures. My
default-threads re-run of `ore_ont_14272` at pt=1: **2.70 s** — matching 2,808 ms almost exactly. At
`RAYON_NUM_THREADS=1` (what `sweep-arm.sh` uses; the arm-off header records `"threads":1`) the same run is
**73.5 s**.

Sampling 14 of the doc's 15 table members at pt=1, threads=1, 4-way concurrency (the harness's own
configuration), 65 s cap:

| ontology | pt=1 wall | | ontology | pt=1 wall |
|---|---:|---|---|---:|
| `ore_ont_10019` | 0.6 s | | `ore_ont_6333` | 17.3 s |
| `ore_ont_6485` | 1.1 s | | `ore_ont_10460` | 18.5 s |
| `ore_ont_12723` | 5.7 s | | `ore_ont_934` | 19.5 s |
| `ore_ont_15526` | 5.8 s | | `ore_ont_2901` | 19.7 s |
| `ore_ont_1707` | 7.5 s | | `ore_ont_10109` | 19.9 s |
| **`ore_ont_8429`** | **DNF @65 s** | | **`ore_ont_9864`** | **DNF @65 s** |
| **`ore_ont_10807`** | **DNF @65 s** | | **`ore_ont_5764`** | **DNF @65 s** |

Solo, threads=1, 200 s cap, to rule out contention:

| ontology | pt=1 parallel | pt=1 threads=1 | ratio | rows |
|---|---:|---:|---:|---:|
| `ore_ont_5764` | 5.1 s | **95.4 s** | 18.7× | 1160 both |
| `ore_ont_9864` | 3.2 s | **79.4 s** | 24.8× | 904 both |

So **5 of 15** sampled "recoveries" (`5764`, `9864`, `8429`, `10807`, `14272`) do not complete inside 60 s
at the configuration the Task-4 gate runs. The "23 of 36" partition is therefore a 32-core number and the
gate will not reproduce it. **This is the plan's own Confound #1, self-inflicted.**

**Required change:** re-run the partition at `threads=1` and restate the addressable set. Every wall in
the plan and its evidence doc must carry its thread count.

### B5. Task 1 is already answered from committed data, and it **fails the plan's own pre-registered rule** at every `T` below 30 s

Computed directly from `runs/full-2026-08-06-invpair-off.jsonl` (1,753 `ok`, 166 `dnf`, 1 `err_reject` —
matching the plan's stated baseline; 60 s cap, threads=1):

median **0.13 s**, p90 **4.31 s**, p95 **9.76 s**, p99 **35.97 s**, max **59.13 s**.

| `T` | completers above `T` | share | plan's ≤2% rule |
|---:|---:|---:|---|
| 5 s | 152 | **8.67%** | **FAIL** |
| 10 s | 85 | **4.85%** | **FAIL** |
| 15 s | 58 | **3.31%** | **FAIL** |
| 20 s | 45 | **2.57%** | **FAIL** |
| 30 s | 33 | 1.88% | pass (marginal) |
| 45 s | 8 | 0.46% | pass |

And 1.88% at 30 s is a **lower bound**: the sweep's 60 s cap hides completers above 60 s — the plan's own
Step 2 says so, and B3 supplies an instance (`15010` needs 104 s at pt=1). CLAUDE.md's peer triage puts
the >60 s population at ~157 ontologies, most of which Konclude classifies, so an unknown number of the
166 `dnf` rows are >60 s completers that the tax would hit.

Task 1 cost me one Python script. **It was worth running before writing the plan**, and it says the only
thresholds that survive the plan's own rule are ≥30 s — which is what B6/B7 then break.

### B6. Task 4 Step 2's written prediction ("ΔMISSED ~0, because the net's population is completers") is **false**, and measurable in advance

The MISSED-net population is 400 ontologies deliberately **over-weighted toward the search stratum**
(`baselines/2026-08-03-missed-net-population.meta.json`: `out-of-EL/search` 188 of 400 sample against 188
of 1,746 frame). Cross-tabbing all 400 against the arm-off sweep:

| `T` | net members with wall > `T` |
|---:|---:|
| 5 s | 59 |
| 10 s | 30 |
| 20 s | 24 |
| 30 s | **18** |
| 45 s | 5 |

So the net's population **does** trigger, and the plan's inference chain ("non-zero ΔMISSED ⇒ `T` is too
low") is not a trigger-correctness test — it is a guarantee of a non-zero reading.

Worse, ΔMISSED is **predictable now** from the committed `TIGHT1MS` arm (which ran the whole population at
exactly `B = 1 ms`): the fallback's ΔMISSED = the TIGHT arm's per-ontology losses, restricted to members
above `T`. The arm's 13 losers, with their default walls:

| ontology | Δ pairs lost at 1 ms | default wall |
|---|---:|---:|
| `ore_ont_12191` | 16 | 3.17 s |
| `ore_ont_11378` | 15 | 3.34 s |
| `ore_ont_3077` | 12 | 2.86 s |
| `ore_ont_699` | 7 | 2.72 s |
| `ore_ont_3917` | 6 | 2.13 s |
| `ore_ont_7893` | 6 | 0.17 s |
| **`ore_ont_9151`** | **6** | **35.62 s** |
| `ore_ont_12698` | 3 | 5.04 s |
| **`ore_ont_1509`** | **2** | **28.34 s** |
| `ore_ont_7532` | 2 | 2.82 s |
| **`ore_ont_8911`** | **2** | **32.99 s** |
| `ore_ont_9662` | 2 | 2.33 s |
| `ore_ont_3806` | 1 | 0.03 s |

| `T` | predicted ΔMISSED | vs gate ≤5 |
|---:|---:|---|
| 5 s | 13 | FAIL |
| 10–20 s | 10 | FAIL |
| 30 s | **8** | **FAIL** |
| 45 s | 0 | pass |

**Required change:** state this prediction in the plan (it is free) rather than predicting ~0.

### B7. The Task-5 decision rules are **mutually unsatisfiable**

Chain the above:

- ΔMISSED ≤ 5 (Task 5 bullet 1) ⇒ **`T` ≥ 45 s** (B6).
- At `T` = 45 s under the gate's 60 s cap, a recovery must finish its `B`-run in **< 15 s**. From my
  threads=1 sample that is `10019` (0.6), `6485` (1.1), `12723` (5.7), `15526` (5.8), `1707` (7.5) — **5 of
  14**, with 4 outright DNF and 5 more in the 17–20 s band. Extrapolated to 23, that is ~8 —
  **below Task 5's "`dnf → ok` < 10 ⇒ record as a measured negative and stop"**.
- Task 5 bullet 2's escape ("`T` is too low; raise it") therefore walks straight into bullet 4.

Separately, answer the value question the review brief asks: at `T` = 45 s the recovered ontology returns
at **45–53 s** (and `9151`-class completers get taxed 45 s for a worse answer). Konclude classifies these
same 39 in **under 1 second** (`baselines/2026-08-04-triage-konclude-c120.jsonl`, the population's own
selection criterion). "An answer in 45–53 s where the peer takes 0.5 s and where the user's budget is
plausibly 30 s" is not a useful recovery — and at a 30 s budget the lever is **inert by construction**,
which is what the project's own earlier 30 s-cap sweeps used
(`baselines/2026-07-31-ore-rustdl-v046-t1-c30.jsonl`).

### B8. The mechanism is **already shipped**, and the plan dismissed it as the infeasible option

The plan rejects a mid-run switch on the grounds that "the pair loop is rayon-parallel". It exists:

- CLI `--global-timeout-ms` (`crates/owl-dl-cli/src/main.rs:166-167`) → `classify_with_budget`
  (`main.rs:1096`, `classify.rs:836-845`) → `classify_top_down_internal(internal, per_pair, global_deadline)`
  (`classify.rs:2049-2053`).
- `effective_deadline` (`classify.rs:2033-2046`) cuts every probe at `min(global, per_pair)`, so
  **before `T` nothing is cut** — the exact "zero cost on completers below `T`" property the plan wants —
  and at `T` every undecided pair becomes "not subsumed" and the run returns immediately, with **no wasted
  `T` and no restart**.
- The label-cache build already honours it (`classify.rs:2301-2309`).
- `RUSTDL_AGGREGATE_DEADLINE_MS` (`classify.rs:2012-2016`, `:2093-2099`) is the env form for the
  per-pair-only path.
- The incompleteness signal already fires on this path. Measured on `ore_ont_6333`:
  `classify --global-timeout-ms 5000 --json` ⇒ `{'consistent': True, 'incomplete': True}` and
  `⚠ INCOMPLETE: 81 class pair(s) hit the 1000 ms per-pair / 5000 ms global timeout`. So Task 3 Step 5 is
  already satisfied.

**Measured head-to-head, threads=1** (rustdl Hasse rows, both arms — a like-for-like rustdl-vs-rustdl
comparison; I did **not** normalise to closure, so do not read these against Konclude's numbers):

| ontology | `--pair-timeout-ms 1` rows / wall | `--global-timeout-ms 30000` rows / wall |
|---|---:|---:|
| `ore_ont_14272` | 835 / 73.5 s | 835 / 20.0 s (at `g=20 s`) |
| `ore_ont_5764` | 1160 / 95.4 s | 1160 / 30.0 s |
| `ore_ont_9864` | 904 / 79.4 s | 904 / 30.0 s |
| `ore_ont_12723` | 1434 / 5.7 s | 1434 / 30.1 s |
| `ore_ont_10109` | 180 / 19.9 s | 180 / 30.0 s |
| `ore_ont_1707` | 1297 / 7.5 s | 1294 / 30.1 s |
| `ore_ont_10019` | 58 / 0.6 s | **60** / 30.0 s |
| `ore_ont_6333` | 86 / 17.3 s | 85 / 30.0 s |
| **total** | **5,954** | **5,952** |

A completeness wash (−0.03%, better on `10019`, worse on `1707`/`6333`), with a **hard wall guarantee** and
**zero engineering**. And the two compose: `--pair-timeout-ms 1 --global-timeout-ms 30000` bounds the B3
pathology too (`ore_ont_15010`: 171 rows in 30.2 s instead of 104 s; `5764`: 1160 rows in 30.0 s).

Note also `classify.rs:2075-2084`, which records that **synthesizing a default aggregate deadline was
already considered and deliberately rejected**, with reasons (it would cap the CLI default, `owl-dl-bench`,
and the closure-diff fixtures, turning legitimately-slow completers into new MISSED). The plan's design is
a variant of that rejected move and never engages the comment.

**Answer to review question D(iii): yes, (iii) is the correct answer.** Document
`--pair-timeout-ms 1 --global-timeout-ms <budget>` as the recommended recipe for the DNF tail — a README /
CLAUDE.md paragraph, no code, no flag, no gate, no restart tax. That captures the entire measured benefit.

### B9. Task 3 Step 3's stated requirement is **not implementable** as written

"An explicit user-supplied `--pair-timeout-ms` must win over the fallback" is undecidable in the CLI
today: `pair_timeout_ms` is `#[arg(long, default_value_t = 1000)]` (`main.rs:155-156`) and
`grep -rn "value_source\|ValueSource" crates/owl-dl-cli/src/` returns **nothing**. So the binary cannot
distinguish "user passed 1000" from "clap defaulted to 1000". A naive implementation either makes the
fallback **dead code** (pair_timeout is always "supplied") or **silently overrides an explicit
`--pair-timeout-ms 1000`** — which is the exact failure the requirement forbids. Fixing it means
`Option<u64>` or `ArgMatches::value_source`, a CLI-surface change the plan does not scope.

### B10. Wrong file and wrong function in Task 3

- Task 3 names `crates/owl-dl-reasoner/src/lib.rs` for "classify entry points". They are in
  **`crates/owl-dl-reasoner/src/classify.rs:763-872`**.
- Task 3 says to "check whether `classify_internal_with_timeout` already provides the total-wall hook —
  **reuse it if so**". `classify_internal_with_timeout` (`classify.rs:939-942`) is the **naive n² path**,
  reached only from `classify_n2_with_timeout` (`:866-872`) and `disjointness.rs:60`, takes **no**
  global argument, and is **not what the CLI runs**. The default path is `classify_top_down_internal`
  (`classify.rs:2049-2053`), which already has the hook. A worker following the instruction literally
  would instrument a path the default classify never enters — and would then read "no regression" from a
  lever that never fired, the plan's own **Confound #3**.

---

## Concerns

1. **What 93.4% means, concretely.** `ore_ont_6333`: 328 of 351 oracle pairs, i.e. **23 entailments
   silently absent**, surfaced only as an aggregate `incomplete: true` + a pair count — the user cannot
   learn *which*. And the two policies disagree on *which* (86 rows at pt=1 vs 85 at global-30), so
   "which 23" is a function of the flag. For a reasoner whose selling point is soundness with a truthful
   incompleteness signal, "here is 93.4% of your hierarchy, and the missing 6.6% is unlocatable" needs an
   explicit product decision, not a footnote.
2. **Konclude alone is off-method for this project.** The evidence table is Konclude-only. The MISSED
   net's own rule is Konclude ∪ HermiT with peer disagreements **excluded**
   (`docs/2026-08-03-*`, CLAUDE.md), and Konclude is documented to under-report on three recorded
   instances (`ore_ont_10407`, `9540`, `15682`). The net's summary shows HermiT is **not** a free addition
   here — of 400, HermiT is `DNF` 31 / `EMPTY` 15 / `NO_OUTPUT` 14 — so the honest form is
   "Konclude-only, HermiT unavailable on N of the 23", stated, not omitted.
3. **A(iii), the cap question:** the 60 s cap cannot *manufacture* a completion (a capped run records
   `dnf`), so no member of the 23 completes "because the cap truncated something". But the cap does hide
   how close to it they are — and at threads=1, 5 of 15 are outside it (B4).
4. **Hasse vs closure (scope of my own numbers).** All my row counts are rustdl Hasse output, compared
   rustdl-to-rustdl. That is valid for the ON/OFF design question and invalid against Konclude figures —
   the same trap the evidence doc self-corrected at `:127-130`. Do not lift my totals into an
   oracle comparison.
5. **Task 3 Step 1's fixture choice will be slow and flaky.** "A fixture that DNFs unbounded within a
   small `T` and completes under `B`, using one of the 23 real ontologies" means a test whose runtime is
   `T` + a `B`-run, i.e. tens of seconds in a release build and far more in the test profile — and B4 shows
   its outcome is thread-count dependent. Expect the same `#[ignore]`d/release-only compromise the
   adaptive-inconsistency-budget canaries needed.
6. **`incomplete: true` is not sufficient for a *restart*.** If a fallback ships, the JSON needs the
   distinct "a fallback ran, here is `T` and `B`" fact — the plan says this (Step 5) and is right, but note
   the existing global path already carries everything except that fact, so the marker is the only genuinely
   new output surface.

---

## Confirmations (checked, and correct)

1. **FP=0 reasoning is right.** A cut pair defaults to "not subsumed"; `classify.rs:829-831` and the CLI
   warning both state the sound-under-approximation contract. Nothing in this plan can manufacture a
   subsumption. The Global Constraints' soundness paragraph is accurate.
2. **`prepare = 5 ms` of ~2,808 ms on `ore_ont_14272` reproduces exactly** (5 ms of 2,850 ms at default
   threads). The measurement is right; only the inference drawn from it (B2) is wrong.
3. **The addressable set is genuinely failing today.** All 15 sampled members are `dnf` at 60 s in the
   committed arm-off sweep, at bare CLI defaults. This is not a phantom target.
4. **Task 1 first, and kill-capable, is the correct instinct**, and the pre-registered ≤2% rule is the
   right *kind* of rule. It simply answers "no" (B5) — which is the plan working as designed, one step
   before it was executed.
5. **The Global Constraints correctly name the restart tax** ("Any completer whose wall exceeds `T` *will*
   be restarted and *will* lose pairs … if that number is not small, the design is wrong, not the
   threshold"). That paragraph is the most honest thing in the plan; B5/B6 just supply the numbers that
   trigger its own conclusion.
6. **The two-gates framing is right** (the net cannot see `dnf → ok`; the sweep cannot see lost pairs on
   completers). Both gates are real and neither substitutes.
7. **`RUSTDL_HYPER_*` / soundness invariants are untouched** — this is a policy wrapper, and the claim
   "adds no new inference" is accurate.

---

## The single highest-value finding

**The entire measured benefit is already available from a flag that ships today, and the plan's evidence
doc never mentions it.** `--global-timeout-ms` (`crates/owl-dl-cli/src/main.rs:166-167` →
`classify.rs:836-845` → `classify.rs:2049`, with `effective_deadline` at `:2033-2046`) *is* the mid-run
switch the plan dismissed as infeasible: nothing is cut before `T`, everything undecided is cut at `T`, the
run returns at `T`, `incomplete: true` and the timed-out-pair count are already surfaced, and there is no
restart and no wasted `T`. Measured on 8 of the plan's own targets at threads=1, `--global-timeout-ms 30000`
returns **5,952 Hasse rows against `--pair-timeout-ms 1`'s 5,954** — a 0.03% wash, better on `10019`,
with a hard wall bound the proposed design does not have.

The sharpest single falsifier of the proposed design sits alongside it: **`ore_ont_15010` completes in
5.6 s at the shipped default and takes 104 s at `B = 1 ms` for a byte-identical hierarchy** (threads=1),
because `per_pair_timeout` silently shrinks the label-cache budget (`lib.rs:2721-2735`, ceiling 30,000 ms →
178 ms) and the sweep budget (`classify.rs:2639`). It is recorded as `arm_no_closure` in the very
`TIGHT1MS` summary the plan quotes its `+80` from.

**Recommendation, in order:**

1. **Do (iii): document, don't build.** Add `--pair-timeout-ms 1 --global-timeout-ms <budget>` to the
   README / CLAUDE.md as the recommended recipe for the DNF tail, with the threads=1 walls from B4/B8.
   Zero engineering, and it captures the whole measured win.
2. **Then investigate the B3 coupling as a defect in its own right.** "A tighter per-pair budget makes a
   completer 18× slower for an identical answer" is a genuine, bounded, cheap-to-attack bug — decouple the
   label-cache budget from `per_pair_timeout`, or floor it. That is a real lever this review surfaced and
   the plan did not, and it may well move several of the 23 without any policy wrapper at all.
3. **Only if 1 and 2 are exhausted**, re-argue the constant: is the CLI's 1000 ms default right for this
   population? That is the honest version of this plan, and it needs no `T`, no restart, and no new flag —
   just the MISSED net plus the two-arm sweep it already specifies.
