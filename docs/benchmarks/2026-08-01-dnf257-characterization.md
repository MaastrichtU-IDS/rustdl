# Characterizing the 257 ontologies rustdl cannot classify

**Date:** 2026-08-01 · **Binary:** rustdl v0.4.6 · **Host:** 32-core / 251 GB, single-thread, 120 s cap
**Raw data:** `owl-reasoner-harness` repo, `baselines/2026-08-01-*` (measurement lives there; interpretation lives here)

> **STATUS: triage COMPLETE (all three peers), fixes in progress.** The A/B/C partition below
> is final. One validation of the *rustdl* half is still running and is called out in
> § Threats.

> # ⚠️ SUPERSEDED 2026-08-04 — the population is now 151, not 257, and the partition has been re-measured.
>
> This document's numbers are correct **for v0.4.6** and remain the reference for that binary. Eight
> releases later, on **v0.4.14**, the same 199-survivor list re-measured at the same 120 s cap gives
> **48 newly complete** (median 17.6 s) and a tail of **151** — a strict **subset** of these 257,
> with **zero new entrants**, so it is the identical population re-measured rather than an
> approximate comparison.
>
> | | 08-01 (v0.4.6) | 08-04 (v0.4.14) |
> |---|---:|---:|
> | tail | 257 | **151** |
> | Set A (≥1 peer classifies) | 242 (**94.2%**) | **138 (91.4%)** |
> | Set B (no peer) | 15 (5.8%) | **13 (8.6%)** |
>
> **The peer-solvable fraction went DOWN and Set B's share rose — rustdl has been harvesting the
> ontologies that were easier for peers too.** Of the 106 recovered, **104 (98.1%) were Set A**, and
> on the 08-01 data Konclude's median was 3.08 s on the recovered versus 5.06 s on the survivors.
> The enrichment is nonetheless **mild**: ~91% of the residual is still demonstrably tractable, so
> this does **not** revive the "intrinsic SROIQ hardness" framing — the gap stays overwhelmingly
> algorithmic while the per-ontology price rises (median target 3.47 s → 4.42 s).
>
> **New structural fact this document could not have seen:** HermiT's 63 and KM's 38 are strict
> **subsets** of Konclude's 138; their union **is** Konclude's set exactly, with **zero peer-only
> solves**. The three reasoners are *nested* on this population, not complementary — one peer
> determines the whole partition. Konclude: median 5.11 s, 94 of 138 under 10 s, **39 under 1 s**,
> **41 at ≥120×**. Verdicts are not run-to-run noise (Konclude 151/151 and KM 151/151 identical
> on re-run; HermiT 148/151, its 3 flips at the cap boundary and partition-irrelevant).
>
> Two self-contained clusters were also isolated: **three tail members are simply INCONSISTENT**
> (`ore_ont_16372`/`4141`/`8445` — Konclude finds `owl:Thing` unsat in 0.14–2.55 s while rustdl DNFs
> at 120 s, matching the documented `RUSTDL_CLASSIFY_INCONSISTENCY` under-approximation residual)
> and **two are ~140 k-class ontologies with a genuinely flat hierarchy** (`16744`/`8737`: 0 of
> 142,884 and 0 of 136,612 `SubClassOf` axioms have a non-`Thing` superclass) — scale, not reasoning.
>
> > **AMENDED 2026-08-05 — the statement above is CONFIRMED, including the counts; and `ore_ont_16372`
> > is now FIXED.** All three are genuinely inconsistent, each backed by **two** independent peers, and
> > the unsat counts reproduce exactly (745/746, 107/108, 338/339 classes collapsed, versus 0/121 on a
> > consistent control). **A claim that "KM is wrong on all three" appeared in an intermediate version
> > of this note and is WITHDRAWN — it was a misreading of KM's `CONSISTENT 0` output, which is a
> > boolean meaning *inconsistent*.** KM in fact agrees on `4141`/`8445` on every release tested. `ore_ont_16372` **left the tail**: the domain-absorption flip turned a
> > wrong `consistent` verdict into the correct `inconsistent` and classify DNF → **2.92 s**. The other
> > two remain **open rustdl misses** (TIMEOUT at 200 s). Reaching HermiT's verdict on them required
> > percent-encoding a malformed `xsd:anyURI` literal that made it throw before reasoning; the repair is
> > verdict-neutral for Konclude. An intermediate version of this amendment wrongly called those two
> > "contested" and withdrew the counts — both reversed. A fourth tail member, `ore_ont_20`, also
> > surfaced as a single-peer inconsistency candidate. Full record:
> > `docs/2026-08-05-inconsistent-tail-members.md`.
>
> The § "Next" item *"Phase 2 clustering of the survivors — tooling built, blocked on the
> re-measure"* is now **unblocked**. Current data:
> `docs/2026-08-04-tail151-peer-triage.md`; tail list
> `owl-reasoner-harness/baselines/2026-08-04-tail-v0414-list.txt`; ranked targets
> `…/2026-08-04-setA-138-ranked.txt`.

---

## 1. What was measured, and against what

Of 1,920 ORE ontologies, rustdl classifies 1,607 within 30 s. 312 exceed that cap; a full
120 s re-run of all 312 found **55 complete (18%) and 257 still unfinished**. Those 257 are
this document's subject.

The governing question is not "how slow is rustdl" but **"is this ontology hard, or is
rustdl missing something?"** — because those have opposite consequences. So each of the 257
was given to three peer reasoners at the *same* 120 s cap, single-thread:

| set | definition | consequence |
|---|---|---|
| **A — gap** | ≥1 peer classifies it | Algorithmic gap in rustdl. The peer's wall is the target. **The work set.** |
| **B — intrinsic** | no peer classifies it | Intrinsic hardness for this generation. Record and stop. |
| **C — disagreement** | classifying peers disagree | A soundness signal, orthogonal to A/B |

---

## 2. Headline: the tail is overwhelmingly tractable

**Konclude classifies 242 of the 257 (94%).**

| | count |
|---|---|
| Konclude CLASSIFIED | **242** |
| Konclude DNF at 120 s | 14 |
| Konclude front-end failure (EMPTY) | 1 |

Konclude's walls on those 242: **median 3.57 s**, p90 18.79 s, max 88.93 s, and **186 of 242
finish in under 10 seconds**. Their closures are substantial — median 162,152 pairs, max
2,432,194 — so this is real classification work, not empty output.

**All three peers, final:**

| peer | CLASSIFIED | DNF | NO_OUTPUT | EMPTY |
|---|---|---|---|---|
| Konclude (native) | **242** | 14 | 0 | 1 |
| HermiT (1.4.3, ~0.56 s docker+JVM floor) | 146 | 99 | 8 | 4 |
| KM (`c6ced84`, 20 GB cap) | 105 | 148 | 4 | 0 |

> **Set A = 242 (94%). Set B = 15 (6%). Set C = 25 (orthogonal, lower bound).**
> Median fastest-peer wall on Set A: **3.47 s**.

**HermiT and KM each rescue ZERO of Konclude's 15.** Three independent reasoners failing
exactly the same 15 is the strongest evidence available at this scale that Set B is genuinely
hard — and that the other 242 are not.

### Set C is a peer-soundness signal, and it indicts KM

Of the 25 size-disagreements, **KM is the sole outlier in nearly all**, deviating in *both*
directions:

| ontology | Konclude | HermiT | KM |
|---|---|---|---|
| `ore_ont_10407` | 8 | 8 | **510** |
| `ore_ont_15703` | 1,604,386 | 1,604,386 | **71,410** |
| `ore_ont_10006` | 204,418 | 204,418 | 201,129 |

That independently replicates KM's documented concrete-domain unsoundness, and is the
concrete reason FP adjudication is against **Konclude ∪ HermiT**, never one oracle.

**The only Konclude-vs-HermiT disagreement in all 257 is `ore_ont_9540` (66 vs 71)**, with
Konclude under-reporting — the same direction as the previously recorded `10407` case.
Conversely, the two agreeing *exactly* on **121 of 122** shared closures, across two unrelated
output formats, is a far stronger validation of the normaliser than its 11-fixture gate.

### This contradicts the standing account

CLAUDE.md and the project memory describe this tail as intrinsic SROIQ hardness — "all
levers measured out", "no cheap entry", "the remaining frontier is a multi-month
clash-driven rewrite". Measured against peers, that is not what the corpus says. A reasoner
of the same generation, on the same hardware, at the same budget, does 94% of it with a
median wall of 3.6 seconds.

The honest form of the older claim is narrower and still stands: *the specific mechanisms
previously investigated* were measured out. That is not the same as the tail being intrinsic.

---

## 3. Where rustdl actually spends itself

Four subsystem reviews profiled the engines. **Three standing beliefs did not survive.**

### 3.1 The hard tail is not the wedge

The residual DNF cost has long been attributed to disjunctive branching in the hypertableau
wedge. Profiling both named dense-SROIQ cases:

- **`ore_ont_10019`** — **84.6% main tableau** (`search::branch` via `replay_with_neg_sup`) vs **15.3%** `hyper::solve`.
- **`ore_ont_1508`** — 22,454 of 22,456 `match_body` samples under the **Horn** `fire_clause` path; 2 under `find_open_disjunction`; `save` (once per branch) absent entirely.

Independent phase attribution agrees: `ore_ont_10019` has **47 classes**, 182 concept rules
and **0.01 GB** peak RSS; conversion and saturation each finish in 0.01 s; the stall is
entirely after the label cache; disabling the data channel changes nothing. Konclude does it
in **0.06 s**.

A 47-class ontology that will not classify in 120 s is search pathology — not scale, not
memory, and apparently not the wedge. Falsifier on record: `RUSTDL_HYPERTABLEAU=0` should
move neither wall.

### 3.2 Two recorded levers are dead

- The **"~35% allocator churn in `match_body`/`enumerate_matches`"** lever is *already
  harvested* by the existing `SmallVec<[HNode;8]>` — only 1.4% of `match_body` inclusive has
  a libc leaf. Predicted value of building it: zero.
- **`find_open_at_most`** (`hyper.rs:3704`) genuinely lacks the `is_blocked` guard its three
  siblings have — the exact shape of the ⊔ termination bug that once took `ore_ont_15672`
  from 138 s to 0.05 s. Instrumented (marker verified present in the binary, 96,001 events):
  **`on_blocked_node = 0`** across 10019/pizza/wine/13723. Not a second instance.

### 3.3 A prior measurement was too narrow

The per-class `clauses.clone()` (`lib.rs:2895`) was recorded at 0.55–6.3% of CPU and
therefore "not a DNF lever". Re-measured on `ore_ont_1508`: **20.28% inclusive**, ~24 s of a
120 s wall. The earlier figure was taken on different inputs and should not have been
generalised.

---

## 4. Confirmed defects, with predicted effect

Nothing below is implemented on `main`. Categories are *inefficient* / *incorrect* /
*missing*.

| # | cat | site | mechanism | measured / predicted |
|---|---|---|---|---|
| 1 | missing | `reasoner/src/lib.rs:2958` | `classify_labels` rebuilds `ClauseIndexes` **per class**: SP2.1/SP3 seed clauses are absent from `base_indexes` while `RUSTDL_SAT_SEED` defaults ON, so v0.3.39's per-*pair* amortization never reaches the label-cache build. The amortizer exists (`hyper.rs:1349/1167`), unwired. | 31.0% incl on 1508. Isolated: **209.6→119.9 s** (1508), **109.9→52.8 s** (12698), closures byte-identical |
| 2 | inefficient | `owl-dl-saturation/src/lib.rs:373/958/1146` | Subsumer worklist records into `subsumers` at **pop**, so a deep backlog has no in-queue membership test and transitivity re-pushes each pair per intermediate. ≥414 M of 927 M pushes provably duplicates. | **`11085`: OOM-abort 16.96 GB → completes at 491 MB (35×)**; see §5 |
| 3 | inefficient | `convert.rs:3080` (`seed_bucket`) | Unguarded, unindexed k²−k subset scan | **96.0% of `ore_ont_9347`'s 10.71 s wall while emitting ZERO axioms**; predict 10.71→~0.5 s |
| 4 | **incorrect (D10)** | `convert.rs:2431-2484` | The five numeric-`DataOneOf` DKey buckets are never collected into `seed_dkey_subsumptions` — no told edges, no disjointness — while `is_pure_el` still certifies completeness | Konclude-confirmed both directions: 3 missed subsumptions **and** a missed `∀p.DataOneOf(1,2) ⊓ ∃p.{3}` clash |
| 5 | **incorrect (D10)** | `convert.rs:2210` | `dkey_components` runs **pre-NNF**, so a `∀p.DKey` that only exists post-NNF (`¬∃q.¬DKey`, legal OWL 2 DL) marks neither `merge_inducing` nor collapse/broadcast | Konclude: `Negated ≡ Nothing`; rustdl: satisfiable under every flag. A **completeness regression** from the 07-20/07-30 gates |
| 6 | inefficient | `reasoner/src/lib.rs:2895` | Per-class `self.clauses.clone()` purely to concatenate, though `new_with_prebuilt_extras` already takes base+extras as slices | 20.28% incl on 1508 |
| 7 | ~~inefficient~~ **REFUTED 2026-08-05** | `tableau/src/saturate.rs:118` | `Instant::now()` before each of 13 rules per node per pass (in-code comment claims negligible) | vdso clock **11.28% self** on 10019 — **but see below: batching this site buys NOTHING, and the in-code comment was right** |
| 8 | inefficient | `data_axioms.rs:3143` | `emit_data_cardinality_violations_typed` rescans the whole `ind_dp_vals` map inside a per-(constraint × individual) loop with `String` compares | 7.3% of `ore_ont_16632`; ~1.3 s of 18.2 s |

### Defect 7 is REFUTED, and the 11.28% belongs to a DIFFERENT loop that was already fixed

Built and measured on 2026-08-05, then **reverted**. Stride-sampling the 11 intra-node
`check_deadline` probes in `saturate.rs`'s `step!` macro, min-of-2 on `ore_ont_10019`
(the named target), serially, pinned binary:

| `RUSTDL_DEADLINE_STRIDE` | wall |
|---|---|
| 1 (exact, = shipped) | **93.41 s** |
| 8 | 93.70 s |
| 32 | 96.14 s |

**Flat — no win, and stride 32 is if anything worse.** So the in-code comment this row
mocks ("a cheap Instant comparison, dwarfed by rule bodies") is **empirically correct at
this site**, and the row's premise was wrong.

**Where the 11.28% actually lives, and why it looked open.** `owl-dl-saturation/src/lib.rs:183`
carries `DEADLINE_CHECK_STRIDE = 4096` with the comment: *"a per-pop clock read is a measured
cost in this codebase (11.28% self-time on one ontology), while a 4096-pop stride overshoots a
deadline by microseconds."* That is **the same number**, attached to the **EL saturator's
worklist loop** — a different loop, in a different crate, and **already fixed**. This row
mis-attributed a shipped fix's motivating measurement to an unrelated call site.

Two lessons, both cheap to apply next time:
- **Before optimising a profile attribution, grep the codebase for the number itself.** One
  `grep 11.28` would have found the shipped constant and its comment in seconds.
- **A profiler frame naming a cheap primitive (`Instant::now`, `memcpy`, a lock) does not
  localise the cost to every call site of that primitive.** Confirm which loop by an A/B at
  the specific site, which is what finally settled this.

### A landmine in a fix already recorded as ready

The `seed_bucket` **singleton-skip** was previously recorded as "sound by the code's own
invariant" and left unbuilt as a small win. That invariant — *"distinct keys ⟹ strict
subset, since equal ranges share one ClassId"* — is **false** in the `f:`/`db:` buckets:
`f64::to_bits` distinguishes `-0.0` from `+0.0` while `FloatRange::subset` does not, so
`DKey(f:-0.0)` and `DKey(f:0.0)` are mutual-subset mutual-singletons. Implementing the skip
without normalising signed zero at key-mint would silently drop an equivalence rustdl and
Konclude currently agree on.

### An unexplained result that undercuts a shipped gate

On a `∀`-after-NNF fixture, the default finds both classes unsat while
`RUSTDL_DKEY_MERGING_GATE=0` finds **neither** (deterministic). Adding sound DKey
disjointness *loses* entailments there — so the "flag-OFF byte-identity" argument that two
shipped DKey specs rest on does **not** establish behavioural equivalence for that channel.
Not yet root-caused.

---

## 5. First fix through the gate: saturator enqueue-dedup

Implemented behind `RUSTDL_SAT_ENQUEUE_DEDUP` (**default OFF**), isolated worktree, not merged.

Record the derived pair at **enqueue** rather than at **pop** (ELK's order); all 8 push
sites routed through one chokepoint. Verdict-preserving because marking earlier is
monotone: rules test `contains` for *derivability*, never for "already processed", and every
enqueued pair is still popped and still runs its full rule scan. Every `subsumers.contains`
in the file was audited against that claim.

| gate | result |
|---|---|
| Flag-OFF vs pre-change build | **identical 8/8** fixtures |
| Flag-ON vs OFF | **identical 8/8**, 60,603 rows 3-way identical |
| Canary sabotage | **3 sabotages, all caught** |
| fmt / clippy `-D warnings` / `cargo test -p owl-dl-saturation` | 0 / 0 / 80 pass |
| **`ore_ont_11085`** | OFF **OOM-abort 51.4 s / 16.96 GB / 0 rows** → ON **687 s / 491 MB / 8,054,852 rows, exit 0** |

**The prediction was partially refuted, and that is reported rather than smoothed over.** The
stated prediction was that ON would complete within 180 s; it does not — it needs 900 s. The
fix removes the **memory wall** (35× RSS, OOM → completes), not the compute cost.
`ore_ont_11085` therefore remains a DNF at any production budget, and this fix alone does not
move it out of the 257.

Two by-products worth keeping:
- A sabotage that un-routed one seed site left the **closure-identity test green** — an
  unrecorded pair self-heals via backward transitivity through the recorded reflexive pair.
  Only the queue-peak assertion caught it. Both canaries are load-bearing.
- Scope correction: `1833`/`15655`/`3080`/`3914`/`9347` have **zero** `⊤⊑C` axioms, and
  `1833`'s saturation completes at ~130 MB. Its 7.94 GB is **not** this mechanism and must
  not be attributed to it.

---

## 5b. Fixes landed so far (all flagged default OFF, none merged)

| fix | flag | effect | gate |
|---|---|---|---|
| **Bare-declaration fragment gate** — `is_el_axiom`/`is_saturator_axiom` fell through to `_ => false` on bare `SymmetricObjectProperty` / `InverseObjectProperties` **declarations**, refusing the fast path to ontologies that merely *name* such a property | `RUSTDL_FRAGMENT_BARE_DECL` | **44 of 257 now classify** (`# mode: pure EL`, walls 0.52–38.92 s, median 1.87 s). `8470` 132.76 s → 0.53 s | `8470` fast path vs **unbounded** hybrid: 19,578 rows, **0 diffs**; 3 bounded ORE identities 0 diffs; curated 8/8 byte-identical; **6 sabotages, each individually load-bearing** |
| **`direct_subsumers` O(k²) per class** — in the **output** loop | `RUSTDL_FAST_DIRECT_SUBSUMERS` | **`ore_ont_10125`: DNF@900 s → 14.70 s COMPLETE (>61×)** | curated 10/10 + ORE 2/2 byte-identical incl. ordering; `9498` (305 k classes) 12.81→12.85 s, no off-path regression; **4 sabotages caught, incl. an ordering sabotage** |
| **Saturator enqueue-dedup** — worklist recorded at pop, so a deep backlog had no in-queue membership test | `RUSTDL_SAT_ENQUEUE_DEDUP` | `11085`: **OOM-abort 16.96 GB → 491 MB (35×)**, but at **687 s** — still DNF at any production budget | ON vs OFF byte-identical 8/8, 60,603 rows; 3 sabotages caught |
| **Lazy ABox saturation** — the `lib.rs:4662` saturation is provably dead on ABox-free input | `RUSTDL_LAZY_ABOX_SATURATION` | **PREMISE REFUTED as a perf lever: 0.02–0.14 s, 0.2–0.3%, RSS flat** | 15/15 byte-identical incl. `consistent` and `realize --json`; 2 sabotages caught |

**`ore_ont_10125` is the most instructive result here.** It finishes *classifying* in ~15 s
and then spends **≥385 s emitting output** — it was never failing to reason. A quadratic
transitive reduction in the print loop was presenting as a reasoning DNF.

**Two prior estimates were refuted by these measurements, and both were mine to pass on:**
- The lazy-saturation lever was projected at "~2.30 s/saturation, ~62% of prep removable".
  Measured: **noise**. Selection effect — every *large* ABox-free ontology takes the pure-EL
  fast path, which since 2026-07-30 uses `build_abox_check_inputs` and never reaches that
  call site. **Do not quote the 62% figure.**
- I briefed the bare-declaration fix as affecting 71 ontologies. The implementer
  **re-measured the denominator**: the truly blocked set is **76**, of which a *provably
  sound* predicate admits **44 (58%)**. The other 32 have a genuinely **read** role —
  admitting them would have been a D10 bug. Partial-and-sound beat full-and-risky.

---

## 6. Threats to validity

1. ~~The rustdl side of the partition is not validated uncontended.~~ **CLOSED — it passed.**
   A seeded random sample of 20 of the 257, re-run strictly sequentially on an idle host with
   the **same binary** (sha256 `fd8ad6573505`, verified identical to the sweep's):
   **completed = 0, dnf = 19, err = 1.** The single error is `ore_ont_10621` aborting at
   94.9 s under a 24 GB address-space cap that this check imposes and the original sweep did
   not — i.e. the check is *harsher* than the sweep, and that ontology was DNF there too. So
   20 of 20 confirm, and **Set A = 242 is a measured result rather than a bound.** This
   mattered: the same confound produced two retractions earlier in this arc (55 of 312 "DNF"
   completed at a larger budget; another measurement was inflated 9× by `-P4`).
2. **Peer walls are mildly contention-inflated** — a review agent profiled concurrently. This
   biases toward "peer failed", so it *understates* Set A; 242 is a floor for this reason too.
   Set A walls will be re-measured in isolation before being quoted as targets.
3. **Set C is a lower bound.** Disagreement is currently flagged by closure *size*, and size is
   invariant under relabelling — two normaliser bugs in this repo each corrupted hundreds of
   pairs while leaving counts untouched. True Set C needs `normalise.py compare` on the
   retained hierarchies.
4. **A single oracle is not an oracle.** Konclude is documented to under-report on at least one
   ontology where rustdl matched HermiT, and KM is documented unsound on ~10. FP adjudication
   is against Konclude ∪ HermiT.
5. **The 1,920-file pool is what is provisioned locally**, not ORE entire.

---

## 7. What follows

### Status as of v0.4.8

| item | state |
|---|---|
| Phase 0 harness + normaliser gate (11/11 exact) | **done** |
| Phase 1 peer triage, A/B/C = 242/15/25 | **done**, contention-validated |
| Two performance levers promoted default ON | **shipped v0.4.7** |
| Three correctness fixes promoted default ON | **shipped v0.4.8** |
| Re-measure of the 257 against v0.4.8 | **running** |
| Phase 2 clustering of the survivors | tooling built, blocked on the re-measure |
| `try_emit` ordering defect | in progress |
| `DataOneOf` bucket seeding (D10 #6) | in progress |
| R3-vs-R4 disagreement on `lib.rs:2958` | **unresolved** — see below |
| Unbudgeted prep + `tier_walk` mis-attribution | not started |
| `seed_bucket` indexing | not started, gated on signed-zero normalisation |

### The one disagreement that needs settling

On `reasoner/src/lib.rs:2958` (per-**class** `ClauseIndexes` rebuild) **R3 reports CONFIRMED**
— 31.0% inclusive on `ore_ont_1508`, isolated effect **209.6 → 119.9 s** with byte-identical
closures — while **R4 reports SUSPECTED** and flags its own measurement as confounded by
seed-clause removal. One is wrong. It is the largest unclaimed wall lever, and the amortizer
already exists at `hyper.rs:1349/1167`, merely unwired for this path. Adjudicate before
building.

### Original plan items, still standing

- Finish HermiT and KM legs; finalise A/B/C; run the uncontended validation in §6.1.
- Cluster Set A by (last phase reached × structural signature) — **re-derived, not inherited**:
  the earlier A/B DNF taxonomy was falsified as an artifact of which budget each phase honours.
- Take defects 1, 3 and 6 through the Phase 4 gate (report-only → flag OFF → sabotaged canaries
  → FP=0 net → flag-OFF byte-identity → `harness compare` for `LOST_BY_ON`).
- Defects 4 and 5 are **correctness** and outrank the performance work.
- Re-examine whether rustdl's remaining weakness is RSS or wall. CLAUDE.md says RSS. The
  evidence so far is mixed: `11085` was pure RSS, but `10019` — the most extreme peer ratio in
  the corpus — uses 0.01 GB.
