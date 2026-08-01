# Characterizing the 257 ontologies rustdl cannot classify

**Date:** 2026-08-01 · **Binary:** rustdl v0.4.6 · **Host:** 32-core / 251 GB, single-thread, 120 s cap
**Raw data:** `owl-reasoner-harness` repo, `baselines/2026-08-01-*` (measurement lives there; interpretation lives here)

> **STATUS: IN PROGRESS.** The Konclude leg is complete and the four subsystem code reviews
> have reported. HermiT and KM legs are still running, so the A/B partition below is a
> **bound, not a final count**. One validation is outstanding and is called out in
> § Threats — do not treat the partition as settled until it passes.

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

Because HermiT and KM can only move ontologies *from* B *to* A, this already bounds the
partition:

> **Set A ≥ 242. Set B ≤ 15. At most 6% of the tail is plausibly intrinsic.**

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
| 7 | inefficient | `tableau/src/saturate.rs:118` | `Instant::now()` before each of 13 rules per node per pass (in-code comment claims negligible) | vdso clock **11.28% self** on 10019 |
| 8 | inefficient | `data_axioms.rs:3143` | `emit_data_cardinality_violations_typed` rescans the whole `ind_dp_vals` map inside a per-(constraint × individual) loop with `String` compares | 7.3% of `ore_ont_16632`; ~1.3 s of 18.2 s |

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

## 6. Threats to validity

1. **The rustdl side of the partition is not yet validated uncontended.** The 257 come from a
   120 s re-run that ran **four ontologies concurrently**. Contention inflates wall, so a
   borderline ontology could be a *phantom* Set A member. This confound has already bitten
   this arc twice (55 of 312 "DNF" completed at a larger budget; a separate measurement was
   inflated 9× by `-P4`). `scripts/validate-dnf.sh` re-runs a seeded random sample strictly
   sequentially and **refuses to run while any measurement process is alive**. It must pass
   before the 242 is final.
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

- Finish HermiT and KM legs; finalise A/B/C; run the uncontended validation in §6.1.
- Cluster Set A by (last phase reached × structural signature) — **re-derived, not inherited**:
  the earlier A/B DNF taxonomy was falsified as an artifact of which budget each phase honours.
- Take defects 1, 3 and 6 through the Phase 4 gate (report-only → flag OFF → sabotaged canaries
  → FP=0 net → flag-OFF byte-identity → `harness compare` for `LOST_BY_ON`).
- Defects 4 and 5 are **correctness** and outrank the performance work.
- Re-examine whether rustdl's remaining weakness is RSS or wall. CLAUDE.md says RSS. The
  evidence so far is mixed: `11085` was pure RSS, but `10019` — the most extreme peer ratio in
  the corpus — uses 0.01 GB.
