# Deadline-triggered per-pair budget fallback — Implementation Plan

> # ⛔ RETRACTED 2026-08-06 — NEVER EXECUTED. The correct answer is to build NOTHING.
>
> Two independent adversarial reviews returned **NO-GO**
> (`docs/reviews-2026-08-06/R5-technical.md`, `R6-value.md`). Every load-bearing claim below
> is wrong, and I verified the three decisive ones myself:
>
> 1. **`--pair-timeout-ms` does NOT default to unbounded — the CLI default is 1000 ms**
>    (`crates/owl-dl-cli/src/main.rs:155`, `README.md:183`). Unbounded is only the *library*
>    default. So this was never "unbounded is the wrong policy"; it was "1000 ms is losing and
>    1 ms wins" — a **constant re-tune**, which is this plan's own Confound #4.
>    **Worse: the flag's own doc comment at `main.rs:150-155` already says so** — *"a low
>    budget like `--pair-timeout-ms 25` is much faster with no completeness loss (wine: 7.5×
>    faster, identical hierarchy, verified MISSED=0 vs HermiT across the corpus; only
>    pizza-class ontologies actually need the larger default)."* I measured a property the
>    codebase had already documented in the flag I was measuring.
> 2. **"A restart is nearly free" is wrong by ~107×.** `prepare_wall_ms` covers only
>    `from_internal` + `abox_verdict` (`classify.rs:2218-2248`); a re-run must also redo
>    convert, `saturate` and `label_cache_build` — **≈536 ms of 2,850 ms (19%)** on my own
>    example. I read `prepare=5` off a banner line that printed `label_cache_build=525`
>    immediately next to it. **This is precisely the retracted definitorial-absorption
>    failure mode.**
> 3. **The headline walls are 32-core; the gate is `--threads 1`.** Verified:
>    `ore_ont_14272` at pt=1 is **2.69 s on all cores and 73.65 s single-threaded** (27×).
>    Re-run under gate conditions the addressable set is **12, not 23**, five ontologies I
>    scored "exact" DNF, and the survivors need 5.7–20.4 s rather than 0.2–7.8 s.
>
> **`--global-timeout-ms` already ships** (`main.rs:167`) and is the mid-run bound this plan
> dismissed as infeasible; measured, it matches or beats pt=1 (5,952 vs 5,954 rows over 8
> targets, with a hard wall bound and no wasted `T`).
>
> The decision rules were also mutually unsatisfiable: completers above `T` are 1.88% at 30 s
> (so the ≤2% rule passes by 0.12 pp), ΔMISSED ≤ 5 requires `T` ≥ 45 s, and at `T` = 45 s only
> ~8 of 23 recover — below the plan's own "<10 ⇒ stop" line.
>
> **What to do instead: document `--pair-timeout-ms 1` (optionally with
> `--global-timeout-ms`) as the recommended flags for the DNF tail. Zero engineering, zero
> risk, and it is most of the value.**
>
> **The one genuinely new finding is a defect this surfaced, not this plan:** a *tighter*
> per-pair budget silently shrinks the **label-cache** budget (`lib.rs:2721-2735`:
> 30,000 ms → 178 ms), and on `ore_ont_15010` that turns **5.6 s into 104 s for a
> byte-identical hierarchy**. Bounded, cheap, and worth its own plan.
>
> Everything after this banner is the retracted draft.

---

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`.
> **DRAFT — awaiting adversarial review. Do not execute until § Adversarial review is filled in.**

**Goal:** Recover ontologies that currently return **no classification at all** by applying a
small per-pair budget **only when a run is already heading for a DNF**, so ontologies that
complete today are untouched.

**Architecture:** No engine change. A policy wrapper around the existing, shipped
`--pair-timeout-ms` mechanism: run unbounded; if total wall exceeds a threshold `T`, re-run
with per-pair budget `B`. Preparation is ~5 ms, so the re-run is nearly free.

**Tech Stack:** Rust (`RUSTUP_TOOLCHAIN=stable`), `owl-dl-reasoner` classify entry points,
`owl-dl-cli`; the MISSED net and `sweep-arm.sh` as gates.

---

## Evidence this is worth building

From `docs/2026-08-06-unbounded-per-pair-is-the-wrong-default.md`, over the 39 Set-A tail
members Konclude classifies in under 1 s:

- **23 of the 36 still-failing go `dnf` → `ok` once per-pair search is capped.** Per-pair
  search is their binding cost.
- At `--pair-timeout-ms 1` their normalised closures are **99.9% of Konclude's** in aggregate
  (59,911 / 59,949 over a 12-ontology batch), **9 of 12 exact**, worst case 93.4% — while the
  **current default returns nothing at all**.
- **Raising the budget buys almost nothing:** 1 ms → 50 ms adds 0–2 pairs, and wall grows
  with the budget until they DNF again at 500 ms. A pair that does not resolve in ~1 ms
  essentially never resolves.
- **`prepare` is ~5 ms** of a 2,808 ms run (`ore_ont_14272`), so a restart is affordable.

**Why not simply default `--pair-timeout-ms 1` globally:** measured cost already in the
record — a pre-registered MISSED-net arm at 1 ms shows **ΔMISSED +80** corpus-wide. That
taxes ~1,750 ontologies that answer completely today, to help 23. The trigger is what makes
the trade acceptable.

## Global Constraints

- **FP=0 is absolute.** `--pair-timeout-ms` is an established **sound under-approximation**:
  a cut pair defaults to *not subsumed*, so the failure mode is a MISS, never a false
  positive. This plan adds no new inference — it only chooses when to cut.
- **Zero cost on ontologies that complete before `T` — by construction, and it must be
  verified, not assumed.** Any completer whose wall exceeds `T` *will* be restarted and
  *will* lose pairs, so ΔMISSED is **0 only for the sub-`T` population**. Task 1 measures how
  many completers sit above candidate thresholds; if that number is not small, the design is
  wrong, not the threshold.
- **Two gates, neither substituting for the other:** the **MISSED net** (baseline
  MISSED = 5,198, FP = 0) *and* a **1,920-ontology two-arm sweep**. The net's frame is drawn
  from completers so it cannot see the `dnf → ok` win; the sweep cannot see lost pairs on
  completers. This lever's benefit and cost land in different instruments — quote both.
- **Report incompleteness.** rustdl already emits `incomplete: true` and
  `# timed-out pairs: N` when pairs are cut. A fallback run **must** carry that signal, and
  must say that a fallback happened.
- Build with `RUSTUP_TOOLCHAIN=stable`; a bare `cargo` fails and a skipped build silently
  reuses a stale binary. **Pin every measured binary immediately after the build that
  produced it and verify the pin against a discriminating input.**
- `cargo fmt --all`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- Sabotage every canary; report counts **as run, including survivors**.

## Confounds that have already produced wrong conclusions here — check for these by name

1. **A number measured under one configuration read as a property of the mechanism.** Twice
   in one day: a `rescued=0` fallthrough rate that was an artifact of a 1 ms budget cutting
   the tableau probes too, and a "catastrophic incompleteness" reading that was a **Hasse
   output compared against a transitive closure**. **Normalise both sides with
   `normalise.py`, and re-measure any striking ratio under a second configuration before
   believing it.**
2. **A single instance standing in for a population.** Three builds were prevented this week
   by a census that a single ontology had made look worthwhile. **Census before build.**
3. **An env-gated flag that does not reach the reasoner** reads as "no regression". Prove
   propagation on a discriminating input before any sweep.
4. **A time constant chosen by intuition.** `RUSTDL_CLASSIFY_INCONSISTENCY_MS`'s "obviously
   ample" few-hundred-ms default silently lost `family.ofn` (needs ~2.6 s), and its flat
   3,000 ms replacement had ~13% headroom. **Both `T` and `B` need a measured basis.**

---

## Task 1: Measure `T`'s basis — the completer wall distribution — BEFORE choosing it

**This task can kill the plan** and is deliberately first. If many ontologies complete only
just below a viable `T`, the restart taxes them and the design fails.

**Files:** `docs/2026-08-06-threshold-basis.md` (new). No source changes.

- [ ] **Step 1.** From the existing arm-off sweep (`runs/2026-08-06-invpair-off`, 1,753 `ok`,
      a 60 s cap), compute the wall distribution of completers: median, p90, p95, p99, max,
      and the **count above each candidate `T` ∈ {5, 10, 15, 30, 45 s}**.
- [ ] **Step 2.** Note the cap ceiling honestly: that sweep used a **60 s cap**, so it cannot
      show completers between 60 s and, say, 300 s. Those exist (`wine` is ~74 s unbounded).
      Re-measure the >30 s subset at a 300 s cap to get the true tail, or state the bound.
- [ ] **Step 3.** Cross-tabulate against the 23: they complete at `B` = 1 ms in **0.2–7.8 s**
      (one outlier at 26.6 s). A viable `T` must sit **above** most completers and **below**
      the point where a DNF is already certain.
- [ ] **Step 4.** State the chosen `T` with its numeric justification, and the number of
      completers it will restart. **Pre-register the decision rule now:** if more than ~2% of
      completers sit above the best `T`, stop — the restart tax is too broad and the design
      needs rethinking (e.g. mid-run switch instead of restart).

## Task 2: Measure `B`'s basis — the completeness/wall curve

**Files:** append to `docs/2026-08-06-threshold-basis.md`.

- [ ] **Step 1.** For **all 23** search-bound ontologies (not the 15 already sampled), measure
      normalised closure versus Konclude at `B` ∈ {1, 5, 20, 50} ms, plus wall. Use
      `normalise.py` on **both** sides — the Hasse-vs-closure error above came from not doing
      this.
- [ ] **Step 2.** Report the curve. The prior sample says completeness saturates almost
      immediately (1 ms → 50 ms adds 0–2 pairs) while wall grows roughly linearly in `B`; if
      that does **not** reproduce over all 23, say so — it changes `B`.
- [ ] **Step 3.** Choose `B` on the measured knee, and record the completeness cost in pairs,
      per ontology and in total, against the oracle. That number is the honest price of the
      recovery.

## Task 3: Implement the fallback

**Files:** `crates/owl-dl-reasoner/src/lib.rs` (classify entry points; check whether
`classify_internal_with_timeout` already provides the total-wall hook — **reuse it if so
rather than adding a second timing path**), `crates/owl-dl-cli/src/main.rs`,
new `crates/owl-dl-reasoner/tests/deadline_fallback.rs`.

- [ ] **Step 1: Write the failing canary first.** A fixture that DNFs unbounded within a
      small `T` and completes under `B`. Use one of the 23 real ontologies rather than a
      synthetic — a synthetic that merely *looks* hard may not exercise the wedge-stall path
      at all (`pizza`/`sio`/`ro` emit **no fallthrough banner**, i.e. they never reach it).
- [ ] **Step 2.** Run it; confirm it fails, and that it fails by exceeding `T`, not by a
      parse or setup error.
- [ ] **Step 3: Implement** behind `RUSTDL_PAIR_BUDGET_FALLBACK` (**default OFF**), with
      `RUSTDL_PAIR_BUDGET_FALLBACK_MS` for `T` and the reused `--pair-timeout-ms` value for
      `B`. On exceeding `T`: abandon, re-run with `B`, and mark the result. **An explicit
      user-supplied `--pair-timeout-ms` must win over the fallback** — never silently
      override a caller's budget.
- [ ] **Step 4.** Canary passes.
- [ ] **Step 5: Surface it.** `classify --json` must show that a fallback occurred and carry
      `incomplete: true` plus the timed-out pair count. A recovery that looks like a complete
      classification is worse than a DNF, because a DNF is honest.
- [ ] **Step 6: Sabotage.** At minimum: never trigger; trigger always (must break the
      no-cost-on-completers property); drop the `incomplete` flag; let the fallback override
      an explicit user budget. Report counts as run.
- [ ] **Step 7.** fmt; clippy; `cargo test --workspace --exclude owl-dl-py --no-fail-fast`.

## Task 4: Gates

- [ ] **Step 1. FP=0 net, flag ON** — expect 13 VERIFIED, no `FP>0`/`MISSED>0`. The curated
      fixtures complete in well under any `T`, so this is an **inertness** check; say so
      rather than presenting it as evidence of correctness under load.
- [ ] **Step 2. MISSED net, flag ON.** Predict ΔMISSED in writing first. **The prediction is
      ~0**, because the net's population is completers and they should not trigger — so a
      **non-zero ΔMISSED means the trigger is firing on completers**, i.e. `T` is too low.
      That makes this net a *trigger-correctness* test, not just a completeness test.
- [ ] **Step 3. 1,920-ontology two-arm sweep.** Per-arm wrapper scripts; propagation proven
      on a discriminating input first; arms sequential; `--digest-strip-comments`. Report
      `dnf → ok` (the win), `ok → dnf` (the harm), and answer changes. Validate arm-off
      against the known `ok=1753, dnf=166, err_reject=1` baseline before trusting anything.

## Task 5: Decide, by a rule fixed before Task 4's numbers are seen

- **`dnf → ok` ≥ 15, `ok → dnf` = 0, ΔMISSED ≤ 5** ⇒ recommend default ON.
- **`dnf → ok` ≥ 15 but ΔMISSED > 5** ⇒ `T` is too low; raise it using Task 1's distribution
  and re-run. Do not trade completer completeness for tail recoveries silently.
- **any `ok → dnf`** ⇒ keep OFF and diagnose. A fallback that turns a completer into a DNF is
  strictly worse than doing nothing, and the restart cost (`T` + re-run) is the obvious
  suspect near the cap.
- **`dnf → ok` < 10** ⇒ the 23 did not generalise; record as a measured negative and stop.

## Stopping rules

- **Task 1 can end this plan** and is first for that reason.
- **A recovery that hides its incompleteness does not count.** The `incomplete` signal is a
  correctness requirement here, not cosmetics.
- **Do not widen scope to the 13 ontologies where search is not the cost.** They are a
  different problem and this lever cannot help them.

## Adversarial review

*(To be filled in from independent review before execution. Do not start Task 1 until this
section records the findings and their resolutions.)*
