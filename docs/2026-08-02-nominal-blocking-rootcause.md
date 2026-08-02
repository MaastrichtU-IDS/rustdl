# Root-cause: `ore_ont_2182` — the nominal-blocking hypothesis is REFUTED

**Task B of `docs/superpowers/plans/2026-08-02-next-block-v2.md`. Investigation only — no
production code was written.**

Binary: `rustdl 0.4.11`, built from `main` @ `03a77bb`,
`sha256 78b6309aaf46653647c67ffc7406e89f7f1754cc02412dd17c717ee22eabb86f`, pinned to
`…/scratchpad/bin/rustdl-v0411-main-03a77bb` and used for every measurement below.
Every probe run under `( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1 timeout N … )`,
serially.

Toolchain note: `rustup` is not on `PATH` on this host and a bare `cargo` fails with
*command not found* (not the `1.95.0`-missing-cargo failure CLAUDE.md documents). Build with
`PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
RUSTUP_TOOLCHAIN=stable cargo build --release`.

---

## 0. Two premises in the plan are wrong; correct them before reading further

**(a) `ore_ont_16481` is not a 0.14 s completer — it DNFs exactly like `ore_ont_2182`.**

| file | rustdl classify (no pair budget, 120 s cap) | Konclude |
|---|---|---|
| `ore_ont_2182` | **timeout, rc=124, 0 lines** | 0.14 s |
| `ore_ont_16481` | **timeout, rc=124, 0 lines** | 0.14 s |

The 0.14 s in the plan is *Konclude's* wall for `16481`, not rustdl's. So `16481` replicates
the failure (see §6) but is **not** the contrast case the plan intended it to be; a completing
contrast had to be found separately (§3).

**(b) CLAUDE.md's issue-#35 v4 entry mis-states the blocking exclusion.** It says
nominal-tainted nodes are excluded from "`is_blocked_anywhere`/`_ancestor`". Reading the source:

- `crates/owl-dl-tableau/src/lib.rs:1073` `is_blocked_anywhere` — **has** the exclusion, twice
  (`:1080` for `y`, `:1120` for the candidate `x'`).
- `crates/owl-dl-tableau/src/lib.rs:961` `is_blocked_ancestor` — **no nominal check anywhere**
  in the function body (`:961`–`:1032`).
- `crates/owl-dl-tableau/src/hyper.rs:1902` `HyperEngine::is_blocked` (the **wedge**, which is
  where classify's time actually goes) — **no nominal check at all**; `hyper.rs:4467` records
  the reason: *"nominal-aware blocking is moot because same-nominal nodes merge."*

Classify runs ancestor blocking in the main tableau and the wedge's own `is_blocked` in the
accelerator. **Neither excludes nominals.** The premise "a nominal-bearing ontology loses
termination-by-blocking entirely" is therefore false on the classify path. CLAUDE.md's cited
line numbers `1021/1062` both fall inside `is_blocked_anywhere` in the older revision — one
predicate, not two.

---

## 1. What the ontology says

`ore_ont_2182` is the **Wine ontology** (`http://swat.cse.lehigh.edu/onto/wine.owl#`, in the
Manchester DL-approximation family as `…_wl.co.hasoneof.owl.xml`), 74 classes, `SHOIN` per
Konclude. Its whole shape is:

1. **Every wine has exactly one of each descriptor:**
   `Wine ⊑ =1 hasBody ⊓ =1 hasColor ⊓ =1 hasFlavor ⊓ =1 hasSugar ⊓ =1 hasMaker ⊓ ≥1 madeFromGrape`.
2. **Each descriptor's range is a small nominal enumeration:** `WineBody ≡ {Medium,Full,Light}`,
   `WineColor ≡ {White,Rose,Red}`, `WineFlavor ≡ {Strong,Delicate,Moderate}`,
   `WineSugar ≡ {Sweet,OffDry,Dry}` (4 `EquivalentClasses`-level `ObjectOneOf`).
3. **27 `∀R.ObjectOneOf(…)` axioms narrow each wine subclass to a subset** —
   `Chianti ⊑ ∀hasBody.{Light,Medium}`, `WhiteLoire ⊑ ∀madeFromGrape.{PinotBlanc,CheninBlanc,SauvignonBlanc}`,
   and the worst one,
   `Meritage ≡ Wine ⊓ ∀madeFromGrape.{5 grapes} ⊓ ≥2 madeFromGrape`.

Plus 115 `ObjectHasValue` (`Chianti ⊑ hasColor value Red` — also nominals), 22 `≤n` / 6 `=n` /
2 `≥n`, transitive `locatedIn`, inverse `hasMaker`/`producesWine`, and an ABox of 182
`ClassAssertion` + 246 `ObjectPropertyAssertion`.

A model is *not* obviously small in the search sense: each `Wine` node forces six successors and
each successor under a `∀`-`OneOf` faces a 2–5-way choice, so refuting one subsumption means
solving a nominal-assignment constraint problem.

Construct counts are **identical** between `2182` and `16481`; a sorted diff of the two files is
82 lines, all of it a `Sauternes`→`Sauterne` rename, `ObjectOneOf` member reordering, and three
extra axioms in `16481` (`WineGrape ⊑ Grape`, `Wine ⊑ PotableLiquid`,
`madeFromGrape ⊑ madeFromFruit`). They are the same ontology.

---

## 2. Where the time goes: wall ≈ (unrefutable pairs) × (per-pair budget)

`classify --pair-timeout-ms t`, single-threaded:

| `t` | wall | `direct` | timed-out pairs (of 2265) | wedge fallthrough |
|---|---|---|---|---|
| 1 ms | 5.55 s | 120 | **2101** | ran=2101 rescued=0 noverdict=2101 |
| 10 ms | 32.39 s | 120 | **2064** | ran=2064 rescued=0 noverdict=2064, `from_diverged=2063` |
| 100 ms | **>200 s (timeout)** | — | — | — |

Wall is linear in the budget and the hierarchy does not improve with it (`direct=120` at both
1 ms and 10 ms). The `# wall breakdown ms:` banner (trustworthy at v0.4.11) attributes it to
`sweeps` + `tier_walk` — 4192+1140 ms of 5.55 s at `t=1`, 25075+6844 ms of 32.4 s at `t=10` —
i.e. to the per-pair subsumption loop, not to conversion, saturation, or label-cache build
(`saturate=2 precheck=1 prepare=6 label_cache_build=159`).

**93 % of candidate pairs are unrefutable within budget.** Not a hard-pair tail — the whole
matrix. `from_diverged=2063` says the adaptive-budget divergence detector cuts almost every one.

---

## 3. The hypothesis, tested directly — REFUTED

`rustdl hyper-classify-probe` already reports `blocks_fired` / `block_eligible` /
`is_blocked_calls` (`crates/owl-dl-tableau/src/hyper.rs:518`–`532`, printed at
`crates/owl-dl-cli/src/main.rs:2014`). No new instrumentation was needed, so no
compiled-away-probe hazard.

**Firing criterion, declared before the run:** the reading is valid only if
`is_blocked_calls > 0` **and** `block_eligible > 0`; the hypothesis is supported only if
`blocks_fired/block_eligible < 1 %` on `2182` **and** ≥10× higher on a completing
nominal-bearing contrast.

The contrast: **`ore_ont_7668`** — same corpus, same wine family, **115 `ObjectHasValue`, 22
`≤n`, 6 `=n`, 2 `≥n`, 246 `ObjectPropertyAssertion`** — i.e. the same nominal-and-cardinality
profile — and it classifies in **0.03 s**. It is the DL-approximation tool's `…nooneof…`
variant: `ObjectOneOf` 31→0 and `ObjectAllValuesFrom` 28→1. A second contrast,
`ore_ont_13404` (0.03 s), is included to show what a non-branching ontology looks like.

All four rows from the identical probe invocation (`--per-pair-timeout-ms 20`, depth 256):

| ontology | outcome | disj. clauses | stalled | max depth | `block_eligible` | `blocks_fired` | **fired/eligible** |
|---|---|---|---|---|---|---|---|
| `ore_ont_2182` | **DNF** | 48 | 2357 | **256** | 12,097,933 | 2,223,726 | **18.4 %** |
| `ore_ont_16481` | **DNF** | 48 | 2385 | **256** | 11,005,998 | 2,246,594 | **20.4 %** |
| `ore_ont_7668` | 0.03 s | 15 | 0 | **2** | 16,929 | 936 | 5.5 % |
| `ore_ont_13404` | 0.03 s | 0 | 0 | 0 | 127 | 0 | 0 % |

Instrument fired (`is_blocked_calls` = 13,362,778 on `2182`; `block_eligible` = 12.1 M ≫ 0).

**The hypothesis is refuted, and refuted in the direction opposite to its prediction.** The
DNF ontologies do not block less than the completing one — they block **more**, 18–20 % versus
5.5 %, and 2.2 **million** times in absolute terms. Blocking engages heavily and is doing its
job. What differs is `max_depth_reached`: **256 (the cap) versus 2**.

### `RUSTDL_MAX_NODES` is not hit — not even close

`classify --pair-timeout-ms 1` at three cap settings:

| `RUSTDL_MAX_NODES` | wall | timed-out pairs | `direct` | hierarchy content |
|---|---|---|---|---|
| default (50000) | 5.56 s | 2101 | 120 | — |
| `0` (disabled) | 5.56 s | 2101 | 120 | **identical** to default |
| `200` | 4.57 s | 2101 | 120 | **identical** to default |

A cap of **200 nodes** changes nothing. The completion graph never approaches even that. The
question is *not* "generating unboundedly" — the graph is bounded and small. **The search tree
is what explodes, not the model.**

---

## 4. What the cause actually is

**`∀R.ObjectOneOf(…)` is the mechanism, and it is a disjunctive-search cost, not a blocking or
graph-growth cost.**

Each `∀R.{a₁…aₙ}` clausifies to one non-Horn DL-clause `R(x,y) → {a₁}(y) ∨ … ∨ {aₙ}(y)`. The
correspondence is exact: stripping 27 of them takes the wedge's disjunctive clause count from
**48 to 21** — a delta of exactly 27.

Because `Wine ⊑ =1 hasBody ⊓ =1 hasColor ⊓ =1 hasFlavor ⊓ =1 hasSugar ⊓ =1 hasMaker ⊓
≥1 madeFromGrape`, every wine node forces six successors, each landing under one of those
covering disjunctions, and the NN-rule merges each choice onto a nominal node. Refuting a
subsumption means finding a consistent joint assignment. The wedge attacks this with
**chronological DFS to a fixed cap of `HYPER_WEDGE_DEPTH = 256`
(`crates/owl-dl-reasoner/src/lib.rs:1506`), with no restarts.** The measured top pairs are
uniform:

```
Stalled wall=5.13ms branches=1018 (disj=1018 merge=0) restores=1018 depth=256  Meritage <= WhiteLoire
Stalled wall=5.11ms branches=1018 (disj=1018 merge=0) restores=1018 depth=256  Meritage <= RedBurgundy
… 13 more identical rows
```

`disj=1018, merge=0` — **every** branch is a `⊔` decision, none is a `≤n` merge, so this is not
the cardinality mechanism. `restores == branches` — every branch fails. `depth=256` — the cap is
reached. A cap-hit returns `Stalled`, which carries no `DepSet`, so there is nothing to backjump
on and nothing to learn; the search then re-descends for the next sibling. The adaptive-budget
divergence detector cuts it at ~1018 branches (two `DIV_WINDOW=500` windows), which is why the
per-pair walls read ~5 ms against a 20 ms budget.

### The depth cap is the wrong size, and the direction is counter-intuitive

Same probe, same 20 ms budget, only `--depth` varied, on `ore_ont_2182`:

| depth cap | subsumptions found | stalled | probe wall |
|---|---|---|---|
| 8 | **264** | **4** | **1.01 s** |
| 16 | 264 | 4 | 1.45 s |
| 32 | 264 | 6 | 2.44 s |
| 64 | 264 | 78 | 4.02 s |
| 128 | 258 | 2097 | 6.97 s |
| **256 (shipped)** | 258 | 2357 | 13.31 s |
| 1024 | 258 | 2357 | 25.45 s |

Monotone. Depth 8 is **13× faster, 590× fewer stalls, and finds 6 MORE sound subsumptions**
(a probe `Unsat` is a sound subsumption at any depth, so more is strictly more complete). The
shipped cap is not merely useless here — it is actively harmful, because DFS spends the entire
per-pair budget descending one bad branch to 256 before ever trying the sibling that closes at
depth ≤7.

**But this does not generalise, and that is the important qualification.** Same experiment on
curated fixtures:

| fixture | depth 8 | depth 256 |
|---|---|---|
| `pizza.ofn` | 691 subs, 123 stalled, 2.70 s | **695 subs**, 0 stalled, 2.25 s |
| `wine.ofn` | 614 subs, 11421 stalled, 26.7 s | **624 subs**, 5371 stalled, 102.0 s |
| `bibtex.ofn` | 16 subs, 0 stalled, 12.7 ms | 16 subs, 0 stalled, 9.4 ms |

A globally lowered cap **loses** 4 subsumptions on pizza and 10 on curated `wine`. So the defect
is not the cap's *value*; it is that the cap is **fixed and the search has no restarts**. Some
proofs genuinely need depth; on `2182` none of them do, and the fixed deep cap converts that
into 2357 stalls.

---

## 5. Step 3 as specified: strip the nominals — reported as a BOUND, with cost profile

Semantics-changing; **not** a soundness argument. Three arms, each replacing the construct with
a fresh atomic class, on `ore_ont_2182`. Firing was verified against a pre-declared count before
any arm was measured.

| arm | what was replaced (verified) | classify wall | `direct` | timed-out pairs |
|---|---|---|---|---|
| baseline | — | **DNF @120 s** | — | 2101 @1 ms budget |
| `all` | `ObjectOneOf` **only as a `∀` filler**: 27 sites; `ObjectOneOf` 31→4, `ObjectHasValue` 115→115 | **0.06 s** | 118 | **0** |
| `oneof` | all `ObjectOneOf`: 31→0 | **0.06 s** | 122 | **0** |
| `hv` | all `ObjectOneOf` **and** all `ObjectHasValue`: 31→0, 115→0 | **0.05 s** | 120 | **0** |

Cost profile of the cut arm (`all`), from the same probe as §3 — required by the v1 rule that a
strip result is uninformative unless cost also improves:

| | baseline `2182` | arm `all` | contrast `7668` |
|---|---|---|---|
| disjunctive clauses | 48 | **21** | 15 |
| stalled pairs | 2357 | **0** | 0 |
| max depth reached | 256 | **7** | 2 |
| `block_eligible` | 12,097,933 | **220,490** | 16,929 |
| probe wall | 13.28 s | **0.76 s** | 0.40 s |
| **subsumptions (sound lower bound)** | 258 | **253** | 247 |

Cost improves on **every** axis while retaining 253 of 258 entailments. This is not the v1 trap
(cheap subsumptions becoming expensive refutations) — the cut arm refutes everything with zero
stalls. Read strictly as a bound: **an upper bound of "DNF → 0.06 s" is available to any change
that removes the `∀`-`OneOf` disjunctive-search cost.** It does not prove any particular fix
achieves it.

The minimal arm is the informative one. Arm `all` leaves all 115 `ObjectHasValue` nominals and
all 4 `EquivalentClasses`-level `ObjectOneOf` definitions in place and still rescues. **Nominals
per se are not the problem — `∀` over a nominal enumeration is.** The corpus corroborates this
independently: `ore_ont_7668` is the DL-approximation tool's own `nooneof` variant of the same
ontology, keeps all 115 `ObjectHasValue`, and completes in 0.03 s.

### Null result: the ABox is not the amplifier

Deleting all 182 `ClassAssertion` + 246 `ObjectPropertyAssertion` + `DifferentIndividuals` and
re-running `classify --pair-timeout-ms 1`:

| | wall | timed-out pairs | `direct` |
|---|---|---|---|
| `2182` | 5.56 s | 2101 | 120 |
| `2182` minus ABox | 6.72 s | **2520** | 118 |

No rescue; strictly worse. (Deleting axioms turned 419 more pairs into non-subsumptions that
must be refuted — the v1 lesson, reproduced.) Recording this as a null: the ABox is not
implicated, so `RUSTDL_CLASSIFY_TBOX_ONLY` being disabled by the presence of nominals is not
what costs here.

A first attempt at this arm used `hyper-classify-probe`, which returned byte-identical numbers
with and without the ABox. That reading was **void, not evidence**: `hyper_subsumption_probe`
builds its engine with `HyperEngine::new(&clauses, q)`, never `new_seeded`, so it never seeds
ABox nodes at all. Discarded and re-run through `classify`.

---

## 6. Konclude: what it did, not just how fast

`Konclude v0.7.0-1138 classification -v`, single processing unit, `ore_ont_2182`:

```
Ontology parsed in 6 ms.
Finished preprocessing in 4 ms.
Precomputing ontology, expressiveness 'SHOIN'.
Finished precomputing in 35 ms.
Classifying ontology, expressiveness 'SHOIN'.
Finished class classification in 67 ms.
Total processing time: 119 ms.
```

`16481` is the same to within noise (preprocess 6 ms, precompute 40 ms, classify 61 ms).

Konclude exposes no satisfiability-test or backtracking counters at any available verbosity
(`-v` and `-a` are the only diagnostics; neither prints them), so the *tests-versus-work-per-test*
split cannot be read off directly. What the phase split does say is architectural and is enough
to decide the question the plan posed: Konclude spends **35 ms once** in a global precomputation
(saturation + pseudo-model/completion caching) and then **67 ms for the entire 74-class
hierarchy** — ~0.9 ms per class, ~12 µs per ordered pair. rustdl performs **5402 independent
per-pair model constructions**, 2357 of which cannot finish. So the gap is **more tests**, by
construction, and secondarily far more work per test. rustdl's own probe shows the per-pair work
is not intrinsic: at depth 8 the same 5402 pairs cost 1.01 s total (187 µs/pair) and stall 4
times.

---

## 7. Replication on `ore_ont_16481`

Replicates on every axis: DNF at 120 s (§0); 48 disjunctive clauses, 2385 stalled,
`max_depth_reached=256`, `blocks_fired/block_eligible` = 2,246,594/11,005,998 = **20.4 %** (§3).
Same file, same mechanism. `ore_ont_6272`, `ore_ont_1958`, `ore_ont_13859`, `ore_ont_4903` carry
the same `146 nominal / 30 cardinality` signature and are likely the same family.

---

## 8. Verdict on the deferred issue-#35 v4 redesign

**The evidence does not support it.** Nominal-aware blocking / an NN-rule redesign would not
rescue `ore_ont_2182`:

1. Blocking already fires 2.2 M times, at a **higher** rate (18.4 %) than on a completing
   ontology of the same nominal-and-cardinality profile (5.5 %).
2. The engine that consumes the time — the wedge — has **no nominal exclusion in its blocking
   predicate at all**, so there is nothing there to make nominal-aware.
3. Classify's main-tableau path uses `is_blocked_ancestor`, which also has no nominal
   exclusion. The exclusion exists only in `is_blocked_anywhere`, which classify does not use by
   default.
4. The completion graph never exceeds 200 nodes (`RUSTDL_MAX_NODES=200` is verdict- and
   content-identical to the 50000 default). There is no unbounded generation to block.
5. The rescue is obtained by removing 27 `∀`-`OneOf` **disjunctions**, while leaving every
   `ObjectHasValue` nominal in place.

This closes a long-standing open item **for the classify path**: on this instance class,
nominals cost through the disjunctions they induce, not through lost blocking. It does **not**
retract issue-#35 v4 for `realize` on the deadline-free path, where `is_blocked_anywhere` is the
active predicate and the original reproducer was a nominal-anchored generating cycle — a
different code path with different evidence, untouched here.

### The supported alternative, with a falsifiable prediction

The defect is **fixed-depth chronological DFS with no restarts** in the wedge. The useful proof
depth on `2182` is ≤7; the shipped cap is 256; the cap-hit returns a dependency-free `Stalled`.
Iterative deepening (or restarts over a growing cap) within the existing per-pair budget would
buy `2182`'s depth-8 profile without giving up the depth that `pizza`/`wine` genuinely need.

**Prediction, stated before any fix is written.** Replace the fixed `HYPER_WEDGE_DEPTH = 256`
with iterative deepening over the per-pair budget (8 → 32 → 128 → 256, each restart reusing the
remaining budget), changing nothing else. Then, measured with the existing
`hyper-classify-probe --per-pair-timeout-ms 20`:

- `ore_ont_2182`: `stalled` falls from 2357 to **≤10** and probe wall from 13.3 s to **≤2.0 s**;
- `ore_ont_16481`: same, from 2385 and 12.9 s;
- `pizza.ofn` retains **695** subsumptions and `wine.ofn` retains **624** — the depth-256 values.

**If `pizza` or `wine` loses even one subsumption, the change is rejected.** Falsifiable with
the probe alone, no new instrumentation.

**Scope and risk.** The change is confined to the depth argument threaded from
`crates/owl-dl-reasoner/src/lib.rs:1506` into `HyperEngine::decide_with_deadline` and to the
`Stalled`/`DepthLimit` return path; it adds no rule and touches no clausification. It is
**FP-safe by construction**: a depth cap can only suppress an `Unsat` verdict, never manufacture
one, so any depth schedule can lose subsumptions but cannot create a false positive — the
soundness gate is unaffected and the completeness gate is exactly the prediction above plus the
11-fixture closure-diff. The material risk is not soundness but *breadth*: `HYPER_WEDGE_DEPTH`
is global to classify, consistency and realize, so the corpus-wide non-regression net is the
real cost of the change, not the edit.

**Addressable set — not yet established.** `grep -lE 'ObjectAllValuesFrom\([^)]*ObjectOneOf'`
matches **27 of 1920** pool files. That is a *syntactic upper bound on one inline form*, and this
project has been burned three times by treating a grep as a gate. The correct measurement is a
gate probe: the delta in `hyper-classify-probe`'s `disjunctive` clause count with and without
the `∀`-`OneOf` sites, per ontology. Do that before sizing the lever. The plan's "20 of 54
expressive ontologies are nominal+cardinality" is **not** the addressable set either —
`ore_ont_7668` and `ore_ont_13404` are both nominal+cardinality and both classify in 0.03 s.
