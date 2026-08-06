# R6 — adversarial review of VALUE and PRIORITY: `docs/superpowers/plans/2026-08-06-deadline-triggered-pair-budget.md`

**Reviewer scope:** value and priority only, not code correctness.
**Binary used for every measurement below:** `target/release/rustdl` at HEAD `ef03b44`, pinned
immediately to `…/scratchpad/pins/rustdl-review`, sha256 `7d80aeea63028d6d…`. Distinct from
`bin/rustdl-invpair-9f74f15` (`a37522841ce72493…`), the binary the 08-06 docs measured on.
**Corpus:** `/data/dumontier/ore-run/pool_sample/files` (the sweep corpus).
**Contention note:** the 39-ontology battery in §1 ran serially at `RAYON_NUM_THREADS=1` with at
most one other single-thread probe on a 32-core host. Walls are therefore mildly pessimistic;
the `ore_ont_14272` result in §1a was taken in isolation and is clean. Raw data committed
alongside this review as `R6-pt1-single-thread-39.tsv` (`ont`, `exit`, `wall_s`, `direct_rows`;
`exit 124` = 60 s timeout).

---

## Verdict

**DO IT AFTER measuring a lower flat `--pair-timeout-ms` DEFAULT on the two gates this plan
already specifies** — the plan's benefit is real but its *shape* (total-wall trigger + restart +
a 106th env flag) is unjustified until the one-line default change has been swept, because that
change buys the identical prize and the only thing the trigger adds over it is **80 missed pairs
across 13 of 392 ontologies**, a number already sitting in a committed artifact.

---

## Strongest argument against

**The plan's headline number does not reproduce under the conditions of its own gate, and the
evidence base commits the exact confound it warns about by name.**

The plan's Task 4 Step 3 gate is a 1,920-ontology two-arm sweep. That sweep is
`scripts/sweep-arm.sh:26` → `--cap-secs 60 --threads 1`, and the arm-off baseline header records
`"threads": 1, "cap_secs": 60` (`runs/full-2026-08-06-invpair-off.jsonl`). The evidence doc's
`dnf → ok` partition was **not** measured under those conditions.

### §1a The single decisive instance

`ore_ont_14272`, `--pair-timeout-ms 1`, same binary, same file
(`md5 5a905b306e183d51fe8f0ded9c511fef`):

| condition | outcome |
|---|---|
| threads free (32 cores) | **2.77 s**, 832 direct rows, `prepare=5 ms` |
| `RAYON_NUM_THREADS=1`, 70 s cap | **DNF** |
| `RAYON_NUM_THREADS=1`, 60 s cap (sweep condition) | **DNF** |

The 2.77 s figure reproduces `docs/2026-08-06-unbounded-per-pair-is-the-wrong-default.md`'s
"2,808 ms" and its `prepare=5` breakdown to three digits — so the doc's numbers were taken with
threads free, and the doc does not state its threading. `ore_ont_14272` is one of the 15 the doc
scores as **exact** (4,137 = 4,137).

### §1b The population, re-measured under the gate's conditions

All 39 sub-1 s Set-A tail members (reconstructed from
`baselines/2026-08-04-triage-table.jsonl`: `set == A`, `konclude == CLASSIFIED`,
`konclude_wall_s < 1.0` — n = 39, matching the doc's population exactly), at
`--pair-timeout-ms 1`, `RAYON_NUM_THREADS=1`, 60 s cap, cross-tabulated against the default arm
in `runs/full-2026-08-06-invpair-off.jsonl`:

| | doc (threads free) | **measured, single-thread** |
|---|---:|---:|
| already `ok` at default | 3 | 2 |
| **`dnf` → `ok` at a 1 ms budget** | **23** | **12** |
| `dnf` even at 1 ms | 13 | **25** |

**The prize is 12, not 23** — 48% of the claimed size. Ontologies the doc scores as **exact** that
DNF here include `ore_ont_14272` (4,137 = 4,137), `ore_ont_10807` (4,789 = 4,789), `ore_ont_5764`
(5,678 = 5,678), `ore_ont_8429` (5,049 = 5,049) and `ore_ont_9864` (4,443 = 4,443).

The 12 recoveries and their single-thread walls at 1 ms:

```
10019 0.55   6485 1.04   12723 5.69   15526 5.86   1707 7.47   4796 14.89
6333 17.28   10460 18.50   934 19.47   2901 19.67   10109 19.93   9890 20.37
```

**Nine of the 12 need 5.7–20.4 s**, against the doc's stated range of 0.2–7.8 s.

This matters twice over. First, the prize is less than half the claimed size at the measurement
condition the gate will use. Second, it collides with `T`: with `T` ≥ 30 s forced by the plan's own
kill rule (§1c), a recovery costs `T` + re-run = **35.7–50.4 s**, i.e. 10–24 s of margin inside a
60 s cap. A host 20% slower loses most of the 12. That is
`RUSTDL_CLASSIFY_INCONSISTENCY_MS`'s "~13% headroom" failure — which the plan cites as its own
cautionary precedent — reproduced in advance, from the plan's own numbers.

**This is confound #1 from the plan's own list** — "a number measured under one configuration
read as a property of the mechanism" — committed in the document the plan cites as its evidence
base, on the plan's single load-bearing number. The plan lists the confound and then inherits it.

### §1c Task 1 is a 30-second computation, and it lands on its own kill line

The plan makes Task 1 first because "this task can kill the plan", and pre-registers: *if more
than ~2% of completers sit above the best `T`, stop.* That distribution is already committed. From
`runs/full-2026-08-06-invpair-off.jsonl` (1,753 `ok`):

median 0.13 s · p90 4.31 s · p95 9.76 s · p99 35.97 s · max 59.13 s

| `T` | completers above | share |
|---:|---:|---:|
| 5 s | 152 | 8.67% |
| 10 s | 85 | 4.85% |
| 15 s | 58 | 3.31% |
| **30 s** | **33** | **1.88%** |
| 45 s | 8 | 0.46% |

Only `T` ≥ 30 s passes the ≤2% rule, and it passes by 0.12 pp — and the 60 s cap truncates the
distribution (max 59.13 s), so every count is a **lower bound**; `wine` at ~74 s unbounded is one
of the invisible ones and would be restarted. So the pre-registered rule is at best marginally
satisfied at a `T` that, per §1b, leaves the fallback almost no room.

Task 1 needed no build and no new run. It is a `python3 -c` over a file already on disk. Writing
a 179-line plan whose first task is a 30-second query, and reviewing it, cost more than the query.

### §1d The default is **1000 ms**, not unbounded — the title is wrong

`crates/owl-dl-cli/src/main.rs:155`: `#[arg(long, default_value_t = 1000)] pair_timeout_ms`, and
`README.md:183` — "cap each pairwise tableau probe (default 1000; `0` = unbounded)". The sweep
invokes `classify {}` with no flags (`sweep-arm.sh:25`), so the arm-off baseline ran at 1000 ms.

The finding is therefore not "unbounded per-pair is the wrong default" but "**1000 ms is too high
for a subset of the tail**". That is a default-*value* question with a one-line answer, not a
missing mechanism. Every downstream design decision in the plan — the trigger, the restart, the
new flag, the threshold — exists to avoid changing a number that is already a tunable default.

### §1e A structurally identical lever is already built, sound, and dormant

`RUSTDL_BOUND_DIVERGED_TAIL` (`crates/owl-dl-reasoner/src/lib.rs:2158–2172`, **default OFF**):
skip the main-tableau fallthrough on a divergence-`Stalled` pair. Its own doc comment records
"`ore_ont_10019` `tier_walk` 77.7 s → 43.4 s if ALL fallthroughs are skipped", and it is
explicitly "**Completeness, not soundness** — it only ever *removes* subsumptions (FP=0
trivially); a completable ontology does not trip `is_diverging`, so it is untouched."

That is this plan's argument, verbatim, for a lever that was built, gated on curated MISSED=0,
and never flipped. It is the single best predictor available of where this plan lands.

---

## Strongest argument for

**The cost side is measured near-free rather than assumed, and on some completers a small
per-pair budget is strictly better — lossless *and* several times faster.**

I tried to find the restart tax and largely failed to. Default vs `--pair-timeout-ms 1`,
`RAYON_NUM_THREADS=1`, normalised transitive closures via `harness/scripts/normalise.py` on both
sides:

| ontology | sweep wall (arm-off) | closure @default | closure @1 ms | Δ |
|---|---:|---:|---:|---:|
| `ore_ont_13859` | 48.25 s | 6,250 | 6,250 | **0** |
| `ore_ont_16481` | 44.99 s | 354 | 354 | **0** |
| `ore_ont_6132` | 33.62 s | 710 | 710 | **0** |
| `ore_ont_3250` | 32.14 s | 115 | 113 | −2 |

Four of the 33 completers a `T` = 30 s trigger would restart; **total tax 2 pairs**. And on the
two cases the constant audit flagged as depth-cap speedups
(`docs/2026-08-03-constant-audit.md:507`), a 1 ms budget is a free win:

| ontology | default | @1 ms | closure |
|---|---:|---:|---|
| `ore_ont_13545` | **46.30 s** | **10.55 s** (4.4×) | 22,501 both — **identical** |
| `ore_ont_2826` | **7.06 s** | **0.21 s** (33×) | 656 both — **identical** |

So the mechanism's direction is right and better-evidenced than the plan claims: a small per-pair
budget is not only a tail-recovery lever, it is a **latent multi-× speedup at zero completeness
cost** on part of the working corpus. It also dominates the deferred fixed-depth-cap project:
`docs/2026-08-03-tableau-iterative-deepening.md` records `ore_ont_10019` at a fixed cap of 8
reaching closure **158 in 61.08 s**; I measure `--pair-timeout-ms 1` reaching **160 in 0.55 s** on
the same ontology. The already-shipped flag beats the unbuilt project on both axes.

And the addressable set, even at the measured 12 rather than the claimed 23, is an order of
magnitude above everything else adjudicated this week: the classify global pre-check NO-GO was
**2** ontologies, its `abox_check` pigeonhole alternative **1** (census over all 1,920). Twelve
recoveries with `ok → dnf` = 0 would also clear the plan's own Task 5 threshold only if that
threshold is relaxed from `≥ 15` — as measured, **the plan fails its own go criterion**
(`dnf → ok < 10 ⇒ stop` is not triggered, but `≥ 15 ⇒ recommend default ON` is not met either, so
12 lands in the plan's unspecified middle).

**But this argument supports the mechanism, not the plan's shape.** Everything above is obtainable
from a flag that shipped in v0.3.x.

---

## The questions, answered

### 1. What is the actual prize, in user-visible terms?

**12 measured under the gate's conditions (§1b), not 23** — 7.9% of the 151-tail and **0.6%
of the corpus**. At `--pair-timeout-ms 1` those return a 93.4–100% normalised closure (my
`ore_ont_10019`: 160 of Konclude's 162, 98.8%, in 0.55 s) with `incomplete: true`, a
`# timed-out pairs: N` banner, and a prominent stderr `INCOMPLETE` warning naming the remedy —
verified by running it.

*Against:* an incomplete closure presented to a downstream materialisation pipeline is a silent
data-quality hazard; a DNF is loud and unignorable, whereas `incomplete: true` must be honoured by
a consumer who may not. And the flag currently under-reports its own severity: the warning says
"may be missing real ones", not "157 of your 208 hard pairs were never attempted".

*For:* 9 of 12 in the doc's batch match the oracle **exactly**, so most users of the recovered set
get not "an approximation" but *the right answer*, faster. Reasoners are used interactively — to
inspect a hierarchy, to find where a class landed — far more than as blind pipeline stages, and
for that use a 98.8% closure is transformatively better than a hang. The honest-DNF argument also
proves too much: rustdl's default `--pair-timeout-ms 1000` **already** returns flagged-incomplete
answers routinely, so this is not a new epistemic category.

**Pick: materially better, yes.** But the prize is 12, and it is available today (Q2).

### 2. Is `--pair-timeout-ms 1` already the answer?

**Substantially yes, and the plan's own framing conceals it** (§1d: the default is 1000 ms, so
this is a *value*, not a *mechanism*). Measured on `ore_ont_10019`, one binary, single-thread:

| invocation | wall | normalised closure (Konclude 162) |
|---|---:|---:|
| `classify` (default 1000 ms) | **DNF at 60 s** | — |
| `classify --pair-timeout-ms 1` | 0.55 s | **160 (98.8%)** |
| `classify --global-timeout-ms 5000` | 5.01 s | 153 (94.4%) |
| `classify --global-timeout-ms 30000` | 30.01 s | 160 (98.8%) |

**Two** shipped flags already convert this DNF into a flagged-incomplete near-complete answer.
`--global-timeout-ms` is the closer analogue of what the plan wants — a total-wall bound, in one
process, with no restart and no threshold to guess — and it is documented in `README.md:184-189`
as exactly that ("a hard 'give me whatever you have in N ms' bound on large or hard ontologies").

**Is documenting the flag most of the value? Bluntly: no — something even cheaper is.** The
zero-engineering move that captures more than documentation is to **change the default value**,
gated by the two instruments the plan already names. Documentation only helps a user who has
already hit a DNF, diagnosed it as search-bound, and gone looking; a default helps everyone,
including the ORE sweep, and it costs one line.

And this project does not need a 106th flag. `grep -o 'RUSTDL_[A-Z_0-9]*'` over `crates/*/src`
gives **105 distinct** flag names; at least **18** carry an explicit "default OFF" in their own
doc comment (`RUSTDL_ANYWHERE_BLOCKING`, `AT_MOST_EXHAUST_PROBE`, `BOUND_DIVERGED_TAIL`,
`CLASSIFY_SAME_TIER`, `CLASSIFY_DEFINED_SWEEP`, `INVERSE_PAIR_FUNC`, `LAZY_ABOX_SATURATION`,
`MRV_ORDERING`, `PREP_DEADLINE`, `PROOF`, `SAT_ENQUEUE_DEDUP`, `SAT_LOOKAHEAD`,
`SEMANTIC_BRANCHING`, `SHADOW_DEP_PROBE`, `SNAPSHOT_CAPTURE`, `TABLEAU_ITERATIVE_DEEPENING`,
`TAUTOLOGY_SKIP`, `WIDE_BODY_VARS`).

### 3. Opportunity cost — ranked

| # | item | prize | cost | evidence | verdict vs this plan |
|---|---|---|---|---|---|
| **1** | **Lower the flat `--pair-timeout-ms` DEFAULT (1 / 10 / 50 ms), gated by the MISSED net + a 1,920-ont sweep** | the *same* 12 recoveries **plus** measured multi-× speedups on completers (`13545` 4.4×, `2826` 33×, both closure-identical) | **one line** + the same two gates the plan already budgets | the 1 ms MISSED arm is fully recorded: `baselines/2026-08-03-missed-net-TIGHT1MS.summary.json` — ΔMISSED **+80**, `onts_lost_pairs` **13** of 392, `onts_gained_pairs` **0**, FP **0**; §for above | **strictly cheaper, same prize.** DO THIS FIRST |
| **2** | **The 13 tail members where per-pair search is NOT the cost** | 13 ontologies, mechanism *unknown* — a genuinely new frontier | 1 attribution run; they are not even enumerated anywhere yet | `docs/2026-08-06-unbounded-per-pair-is-the-wrong-default.md:17`; the 13 are defined only as a set complement, no doc names or characterises them | **higher information value per hour.** The plan's own stopping rule forbids touching them, which is right — but they should be *someone's* next task |
| **3** | **`ore_ont_16372` flag-ON classify regression** | unblocks `RUSTDL_INVERSE_PAIR_FUNC`, a sound oracle-validated flag worth +17 pairs at exact Konclude∩HermiT parity on `ore_ont_13859` | narrowed already: not DKey, not consistency, confined to classify | `docs/2026-08-06-invpair-sweep-results.md:108–115`; and it is now measurably *cheap to probe* — `16372` classifies in 3.42–3.50 s flag-OFF | **comparable value, much smaller blast radius.** Also: `--pair-timeout-ms 1` is the instrument that doc names as next, so item 1 supplies it |
| 4 | `ore_ont_8445` ddmin (and `4141`) | 2 ontologies; but a *wrong-verdict* class, not a slow one | ddmin already started, unfinished | `docs/2026-08-05-inconsistent-tail-members.md`; `docs/known-limitations/inverse-pair-functionality-not-derived.md` reduced `4141` to a **7-axiom core** | below this plan on breadth, above it on severity (a missed inconsistency is a wrong answer) |
| 5 | The 2-ontology classify-vs-`consistent` residual | **2** of 1,920, both characterised | every candidate fix already measured out | `docs/2026-08-06-classify-global-precheck-NOGO.md` | **closed. Do not reopen** |
| 6 | Flip `RUSTDL_INVERSE_PAIR_FUNC` | 17 pairs on 1 ontology | blocked on #3 *and* a classify/`consistent` contradiction | same doc, § "Decision" | blocked; not a candidate now |

### 4. Is the risk priced?

**Priced in prose, not in the design.** The plan states the blast radius honestly ("Zero cost on
ontologies that complete before `T` — by construction, and it must be verified, not assumed") and
pre-registers `any ok → dnf ⇒ keep OFF`. That is good practice and better than the two flips that
regressed this month. But the *ratio* is not priced: a total-wall timer plus a restart path on
**every** classify call, against a prize I measure at **12 ontologies (0.6% of the corpus)**.

The two precedents are exact:
- `RUSTDL_CLASSIFY_INCONSISTENCY` was flipped on a **12-ontology** cost benchmark reading −1.5%;
  a 1,920-ontology sweep then found **4 `ok → dnf`** (`ore_ont_10838` 4.86 s → DNF, `15846`
  21.73 s, `16315` 4.42 s, `3087` 4.80 s).
- `RUSTDL_INVERSE_PAIR_FUNC`'s sweep found **4 `ok → dnf`**, all of them 1–5 s ontologies going
  non-terminating.

**Probability it ships default-OFF: ~65%.** Grounded, not intuited:
1. `T` ≥ 30 s is forced by the plan's own ≤2% rule (§1c) while 9 of the 12 recoveries need
   5.7–20.4 s (§1b) — total 35.7–50.4 s against a 60 s cap, before any code is written.
2. The plan's Task 4 Step 2 rule is **internally inconsistent with §1c**: it predicts ΔMISSED ≈ 0
   and reads any non-zero ΔMISSED as "`T` is too low". But at `T` = 30 s, **33 completers are
   restarted by design**, so a non-zero ΔMISSED is the expected outcome, not a trigger fault. The
   rule as written will fire a false stop.
3. Base rate: ≥18 of 105 flags are documented default-OFF, and `RUSTDL_BOUND_DIVERGED_TAIL`
   (§1e) is a built, sound, subtractive tail-bounding lever in this exact design space that was
   never flipped.
4. Recent hit rate on 08-05/08-06 docs: 1 default change shipped, 2 NO-GO, 1 do-not-flip, 1
   retraction.

### 5. Does the steelman win?

**It wins the direction and loses the shape.** Each of its four claims survives scrutiny — the
addressable set really is in the dozens rather than 1–2; the cost on completers really is near-zero
(0/0/0/−2 pairs on 4 of the 33 above-`T`, and *negative* cost on `13545`/`2826`); it really adds no
inference; and "no answer → near-complete answer" really is the most user-visible improvement on
the table.

But every one of those claims is a claim about **a small per-pair budget**, which shipped years
ago and is a *default value*. None of them is a claim about a total-wall trigger, a restart, or a
new flag. The plan's only genuine differentiator over changing the default is that it spares the
completers — and that differential is now a measured quantity: **80 missed pairs across 13 of 392
ontologies, 1.5% of a 5,198 baseline, with FP = 0 on both arms and 0 ontologies gaining pairs**
(`baselines/2026-08-03-missed-net-TIGHT1MS.summary.json`). Spending a build, a new flag, a
threshold constant and a restart path to buy back 80 pairs — in a codebase whose own record calls
`RUSTDL_CLASSIFY_INCONSISTENCY_MS`'s intuited time constant a cautionary precedent — is the wrong
order of operations, not the wrong idea.

One further item that arm records and the plan does not: `newly_unscored:
["ore_ont_15010", "scored", "arm_no_closure"]`. At a global 1 ms budget, **one ontology stopped
producing a closure at all.** That is an `ok → no-closure` regression the MISSED net caught by
accident, and it is precisely the class of harm the sweep exists to find. It belongs in whichever
plan proceeds.

---

## Required changes (conditional on the DO-IT-AFTER path completing and pointing back here)

1. **Restate the prize as 12, not 23** (§1b — measured at `RAYON_NUM_THREADS=1`, 60 s cap, the
   gate's own conditions; raw data
   `…/scratchpad/pt1-st.tsv`). Then revisit Task 5, whose `dnf → ok ≥ 15` go criterion the
   measured value does not meet. Record the threading of every arm in every doc — its absence is
   what let this through.
2. **Correct the evidence doc's title and framing.** The default is 1000 ms
   (`main.rs:155`, `README.md:183`). "Unbounded per-pair is the wrong default" is false as
   written; the true claim is "1000 ms is too high for part of the tail".
3. **Delete Task 1 and paste in its answer.** It is a 30-second query over
   `runs/full-2026-08-06-invpair-off.jsonl`, already done in §1c: `T` ≥ 30 s is the only value
   satisfying the ≤2% rule, at 1.88%, and every count is a lower bound because of the 60 s cap.
4. **Fix the Task 4 Step 2 decision rule.** It will misread a *designed* cost as a trigger fault
   (§4.2). State the expected ΔMISSED as a function of the 33 restarted completers and gate on
   *that*, not on zero.
5. **Add `--global-timeout-ms` as an explicit baseline arm.** It is the shipped mechanism closest
   to what the plan builds; if `--global-timeout-ms T` matches the fallback on the recovered set,
   the fallback is redundant. My one-ontology probe says it is *worse* than `--pair-timeout-ms 1`
   (153 vs 160 pairs, 5 s vs 0.55 s) — which is a point for the plan and should be measured
   across the population rather than left to a footnote.
6. **Carry `ore_ont_15010` forward** as a known `ok → no-closure` risk under a small budget.
7. **Strengthen the incompleteness surface, and do it whichever path wins.** The current stderr
   warning says "may be missing real ones" while the banner separately reports
   `timed-out pairs: 208`. Put the count in the warning, and state the fraction of pairs cut. This
   is the cheapest genuinely user-visible improvement in the whole area and needs no threshold.

---

## One sentence the plan's author will not want to hear

Your own Task 1 was a thirty-second query against a file already on disk, and running it first —
as the plan says it will — shows the only viable threshold sits 0.12 pp inside your pre-registered
kill line while your recovery walls are two-to-four times what you measured, so the plan argues
itself out of existence in its first paragraph of real data and you wrote the other 170 lines
anyway.
