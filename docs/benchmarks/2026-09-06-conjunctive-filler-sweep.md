# Corpus gates for #110 — conjunctive `ObjectPropertyDomain`/`Range` filler

Date: 2026-09-06. Task 4 of the #110 arc (Tasks 1–3: engine fix, fragment gates, oracle
adjudication). This document answers one question: **does the fix change answers or outcomes
across ORE?**

**Headline: no — and the pre-registered corpus expectation was wrong.** The change is **inert on
ORE by two independent instruments covering its two separable effects** (§2b): zero
fragment-routing movers across all 1,920 ontologies, and zero answer changes across the
27-ontology bearing frame plus 20 controls. Two frame members are UNMEASURED — one a symmetric
DNF, one truncating in both arms — so the zero-loss result covers **25 measured bearing
ontologies plus 20 controls**. **`ore_ont_4796` did NOT gain, and the reason is a confound in
the Task-3 measurement, not a broken fix** (§3).

---

## 0. Pins

| arm | commit | `rustdl` sha256 | `fragment_probe` sha256 |
|---|---|---|---|
| BEFORE | `0abc21b` (parent of Task 1), built in `/tmp/wt-110-before` | `41099bfe…` | `de5e7f80…` |
| AFTER | `0dc4b61` (worktree HEAD) | `e9f11f43…` | `339cb292…` |

All four hashes differ. The AFTER `rustdl` hash `e9f11f43…` reproduces Task 3's recorded AFTER
pin exactly. The BEFORE hash differs from Task 3's `ac6477fc…` because release builds here embed
debuginfo paths and Task 3 built at a different throwaway path — which is why a hash is not the
pin verification. **The pin verification is behavioural**, and both binaries were checked against
a discriminating input before any sweep ran:

| input | BEFORE | AFTER |
|---|---|---|
| `dom_conj.ofn` `classify --json` rows | **0** | **2** |
| `dom_conj.ofn` fragment | **Horn** | **PureEl** |
| `rng_conj.ofn` fragment | **Horn** | **PureEl** |

**The probe binary carries its own pin check, and it needs one.** A mis-pinned `fragment_probe`
reports "0 movers", which is indistinguishable from a clean sweep — the same failure shape as a
guard test that cannot fail. It was re-verified *after* the corpus sweep returned zero movers,
and still discriminated.

Toolchain: `stable-x86_64-unknown-linux-gnu` on `PATH` for every build. A bare `cargo` here is
1.75, fails on edition2024, and **a failed build silently reuses a stale binary** — both builds
were confirmed `rc=0` before being copied to their pinned paths.

---

## 1. Frame — 27, confirmed by two agreeing instruments

The frame is the superset of ontologies authoring a conjunctive `ObjectPropertyDomain`/`Range`
filler. Two instruments were run and **agree exactly at 27**, matching the list carried into this
task:

* same-line `grep -lE 'ObjectProperty(Domain|Range)\([^)]*ObjectIntersectionOf'` → 27
* a whitespace-insensitive Python scan (`\s*` between tokens, `re.S`), which guards against a
  multi-line axiom the same-line grep would miss → the identical 27

This corrects the ≤14 still recorded in `CLAUDE.md`, whose scan `break`s after the first axiom
body matching a broad construct regex and so drops any ontology whose *first* Domain/Range axiom
is non-conjunctive.

### 1a. Shape census — where the fix *can* fire

The fix admits a **fully decomposable** filler. A conjunct that is a complement, union,
cardinality or `∀` is not decomposable, and the pass correctly declines. Binning the frame's
conjunctive Domain/Range axioms:

| bin | count | members |
|---|---:|---|
| at least one plain (EL-decomposable) conjunction | 13 | `10517 10807 11064 11296 14272 16371 16814 4827 5764 6923 7828 8429 9864` |
| every filler carries a complement/union | 14 | `10080 10109 10908 11305 11647 12107 12342 12451 15993 16372 4796 5964 714 8094` |

Spot-checked: `11064` is `∃r.C ⊓ D`, `16814` is `A ⊓ B` — genuinely decomposable.
**`ore_ont_4796` is in the second bin** — all four of its conjunctive Range fillers contain
`ObjectComplementOf` — so the fix is *structurally incapable* of moving it. That is a
mechanism-level fact, established independently of the null measurement in §3.

This census **sizes a population; it does not predict an effect.** The gate does that (§2).

---

## 2. Fragment routing — zero movers across all 1,920 (gate, not grep)

Admitting these axioms is supposed to move ontologies onto the pure-EL fast path, changing which
engine answers. That is the behaviour change the fix ships, so it is measured **by gate**.

`crates/owl-dl-reasoner/examples/fragment_probe.rs` prints `<path>\t<fragment>` from
parse + convert + `analyze_fragment` only — **no reasoning**. This matters: the `# fragment:`
banner is written only *after* classification completes, so diffing it costs a full two-arm
classify and, worse, a DNF yields an empty string, making a both-arm DNF read as "not a mover"
and a one-sided DNF read as a false mover. The probe terminates on ontologies that stall in the
engine.

All 1,920 ORE ontologies, both arms, **arm order alternated by index**, 60 s cap:

| transition | count |
|---|---:|
| `OutOfFragment → OutOfFragment` | 690 |
| `Horn → Horn` | 662 |
| `PureEl → PureEl` | 564 |
| `UNMEASURED → UNMEASURED` | 4 |
| **any move** | **0** |

**Zero movers. Zero `OutOfFragment → Horn` anomalies** (the pre-registered forbidden outcome).

**What the 1,920 does and does not bound — read this before quoting it.** `analyze_fragment`
(`classify.rs:367`) observes `is_pure_el` + `clausify_with_stats` only; it **never observes
`is_saturator_axiom`**, the Horn-shortcircuit gate that Task 2 (`2fa8d0f`) moved in lockstep.
So this sweep bounds the `is_el_axiom` gate over 1,920 and the Horn-shortcircuit gate only over
the 47 ontologies answer-compared in §4.

That is sufficient, and the reason is that **the change is admission-only and requires an `And`
filler**. The pre-fix predicate was `Atomic | Bot | Top`; the post-fix one is
`decompose_role_filler(c, …) || Bot`, and `decompose_role_filler` returns `true` for `Atomic`
and `Top` (`owl-dl-saturation/src/lib.rs:3464,3469`) with every new admission arriving through
its `And` arm. So no ontology *without* a conjunctive Domain/Range filler can be routed
differently by **either** gate — which makes the 27-grep a provable superset for both, and all
27 were answer-compared.

**The 4 UNMEASURED do not weaken this, and the argument is by construction rather than by
assumption.** They are `ore_ont_10860` (the known `horned-owl` SWRL `BuiltInAtom` grammar gap)
and `2504 / 4572 / 8445` (the documented conversion-bound DKey set). **None is in the 27-frame**,
and an ontology with no conjunctive Domain/Range filler cannot be a mover, so they are excluded
as movers without needing a verdict.

### 2a. Calibration — the gate-only probe agrees with the surface it stands in for

A probe validated only on a synthetic could still misread real pool files. Checked against the
`# fragment:` banner that `classify` emits, on real ORE ontologies, **3 of 3 agree**:

| ontology | `fragment_probe` | `classify` banner |
|---|---|---|
| `ore_ont_714` | `Horn` | `Horn (trust_sat sound by construction; hyper Horn fixpoint is complete)` |
| `ore_ont_16814` | `OutOfFragment` | `out-of-EL` |
| `ore_ont_11296` | `OutOfFragment` | `out-of-EL` |

A wholesale harness failure was already excluded by construction (it would have produced 1,920
`UNMEASURED`, not real fragment values); this closes the narrower gap that the probe might
*disagree* with the banner on real input.

### 2b. The fix has TWO separable effects, and only one of them is a gate move

`dom_partial.ofn` (`Domain(r, P ⊓ ∃s.S)` — a filler whose conjuncts are decomposable but which
does not make the ontology pure-EL) separates them:

| probe | BEFORE fragment | AFTER fragment | BEFORE rows | AFTER rows |
|---|---|---|---|---|
| `dom_conj.ofn` | `Horn` | **`PureEl`** | 0 | **2** |
| `dom_partial.ofn` | `Horn` | `Horn` (**no move**) | 0 | **2** |

So the change is:

1. a **routing** effect — the fragment gate admits the axiom, which moves an ontology onto the
   pure-EL fast path **only when that axiom was its sole blocker**; and
2. a **derivation** effect — the saturator decomposes the filler and derives subsumptions
   **whether or not the gate moves**.

**This is why §2's zero-movers result does not on its own establish inertness, and why §4 is not
redundant with it.** The gate sweep bounds effect (1); only the two-arm classify sweep can
observe effect (2). Both are zero on ORE, measured separately.

### Why zero *routing*, mechanistically

26 of the 27 frame members are `OutOfFragment` in **both** arms — they carry other non-EL
constructs, so admitting one more axiom shape does not lift them. The single `Horn` member,
`ore_ont_714`, has only complement-bearing fillers, so the pass declines on it. The routing win
is real (`dom_conj.ofn` moves `Horn → PureEl`) but **no ORE ontology is bottlenecked on this
axiom shape alone**.

---

## 3. `ore_ont_4796` — the pre-registered expectation is CONTRADICTED, and the fix is not at fault

The pre-registered expectation was that `4796` (DOLCE-Lite) must gain
`agent ⊑ endurant` and `agent ⊑ particular` (closure 1,224 → 1,226) under the fix, and that a
flat result means the fix is not working. **It is flat, and the fix is working.**

The 2×2 — both arms × default and `RUSTDL_CLASSIFY_SAME_TIER=1`, closures compared:

| | BEFORE | AFTER | BEFORE vs AFTER |
|---|---:|---:|---|
| default | 1,224 | 1,224 | gained 0, lost 0, **triple identical** |
| `SAME_TIER=1` | **1,226** | **1,226** | gained 0, lost 0, **triple identical** |

The within-binary column is what settles it:

| comparison | gained | lost |
|---|---:|---:|
| BEFORE default → BEFORE `SAME_TIER=1` | **2** | 0 |
| AFTER default → AFTER `SAME_TIER=1` | **2** | 0 |

The two gained pairs are exactly `DOLCE-Lite#agent ⊑ endurant` and `⊑ particular` — **in both
arms**. So the 1,224 → 1,226 gain is caused **entirely by `RUSTDL_CLASSIFY_SAME_TIER=1`, which
is default OFF, and is present in the pre-fix binary**. The Task-3 measurement compared
default-vs-`SAME_TIER` within one binary and attributed the difference to the fix; the
discriminator is BEFORE vs AFTER **under the same flag**, and under either flag it is zero.

Three independent facts agree that this is attribution, not breakage:

1. All four of `4796`'s conjunctive Range fillers contain `ObjectComplementOf`, so the pass
   provably cannot fire on it (§1a).
2. `4796` is `OutOfFragment` in **both** arms (§2), so it is not routed differently either.
3. The pins demonstrably discriminate on `dom_conj.ofn` (§0), so the fix is live in the AFTER
   binary.

The entailments themselves are real and KM-confirmed; they are recovered by the documented
same-tier limitation being lifted, which is a different open issue from #110.

**This is the corpus-reward claim of #110 being corrected downward to zero, not the fix failing.**
The value #110 ships is the removal of a silent drop (the D10 shape — BEFORE returns ∅ with
`incomplete: false` on `dom_conj`) plus the routing win where the shape is the only blocker,
which ORE happens never to exhibit.

---

## 4. Two-arm classify sweep — 27 bearing + 20 controls

Sequential, **arm order alternated by index**, 240 s cap, **default per-pair budget**. The frame
is deliberately *not* run at `--pair-timeout-ms 1000`, which is what makes `10517` and `12451`
explode. Comparison is the **TRIPLE** (closure pairs, unsatisfiable set, equivalence partition),
never `direct_subsumptions`, which is the Hasse relation and is restructured rather than extended
by progress.

### 4a. Bearing frame (27)

**25 IDENTICAL / 0 DIFFER / 2 UNMEASURED / 0 regressed / 0 lost entailments.** The two
UNMEASURED are `ore_ont_10080` (symmetric DNF) and `ore_ont_10517` (§4b), so **the zero-lost
result covers 25 measured bearing ontologies, not 26.**

Closures spot-checked against known values: `ore_ont_10908` = 6,001, matching the curated
oracle. Two members classify every class unsatisfiable in both arms (`11305` 3,660 unsat,
`16372` 744) — identical across arms.

* **`ore_ont_10080` — UNMEASURED, symmetric.** `rc=124` in both arms at 240 s, matching the
  pre-registered expectation of a symmetric DNF. Not a regression: no arm completed.
* **`ore_ont_12451` — measured, no cap escalation needed.** It completed in **both** arms
  (AFTER 157 s, BEFORE 234 s) and is triple-identical at 27,230 closure pairs. The
  "needs ≥900 s / one-sided timeout is a candidate not a loss" warning did not bind at the
  default budget.
* **`ore_ont_10517` — UNMEASURED (§4b).** The raw comparison shows a difference, but both arms
  truncate, so the run does not measure the arms; it is booked UNMEASURED, not DIFFER.

### 4b. `ore_ont_10517` — UNMEASURED, demonstrated rather than asserted

Both arms complete (~98 s) but **both report `incomplete: true`**, which is exactly the
pre-registered candidate condition. The raw cross-arm comparison reads 45 gained / 70 lost.

The decisive control is not the arms but the **binary against itself** — three interleaved
repeats per arm on an idle host:

| run | BEFORE closure | AFTER closure |
|---|---:|---:|
| r1 | 8,836 | 8,829 |
| r2 | 8,867 | 8,853 |
| r3 | **8,504** | 8,850 |

`BEFORE.r1` vs `BEFORE.r3` — **one unchanged binary, same input** — differs by **364 lost / 32
gained**, five times the 70/45 seen between arms. The cross-arm difference is budget truncation,
not a fix effect. Recorded **UNMEASURED, symmetrically**.

**Stated precisely:** this bounds *attribution*, not existence — it shows the arm difference
cannot be attributed to the fix, not that no difference exists. An earlier draft said "its
apparent losses are not real", which overshoots the evidence.

**Three runs were necessary, not two.** r1 vs r2 shows only 29 lost — had the control stopped
there it would have read as "self-consistent" and the arm difference would have looked real.
That is the recorded two-runs-cannot-establish-stability trap, reproduced.

### 4c. Controls (20)

Twenty ontologies with **zero** conjunctive Domain/Range (verified: the count is 0 for all 20),
none in the frame, and — following review — all selected to be `Horn` or `OutOfFragment` in the
**BEFORE** arm. Selection is a fixed deterministic stride (`awk 'NR%53==7'`, first 20) over that
population in name order, so it is reproducible and was not chosen after seeing results. That selection is load-bearing: an ontology already `PureEl` in BEFORE cannot
exhibit a spurious move, so it would be an unobservable control.

**20 IDENTICAL / 0 DIFFER / 0 UNMEASURED**, including large closures (`ore_ont_10073` 305,078
pairs, `5253` 65,805, `13233` 25,684).

### 4d. Wall

**No wall claim is made for this change.** The sweep is one run per arm and was sized to answer
an answer-identity question, not a timing one, so it cannot support a performance figure in
either direction. For completeness: frame completed runs excluding `12451` read BEFORE 253 s /
AFTER 258 s and controls BEFORE 36 s / AFTER 40 s, while `12451` alone reads 234 s vs 157 s —
a spread that at one run per arm is run-to-run variance on a heavy ontology. Quote none of
these as a result.

---

## 5. FP=0 net — inertness, not correctness

`./scripts/run-soundness-diff.sh`, rc=0: **11 VERIFIED, every closure exact, FP=0 / MISSED=0**
— galen 27,997, notgalen 32,739, sio 8,904, wine 653, ore-10908 6,001, pizza 499, alehif 247,
ro 158, ore-15672 142, sulo 51, bibtex 16.

Two notes:

* **galen reads 27,997 here, not the 28,007 the brief expected.** The gate is *exact match
  against the committed oracle*, and that oracle reads 27,997 in this checkout — so the gate is
  green either way. The 28,007 in `CLAUDE.md` is a **different oracle instance**; the committed
  `ontologies/external/galen-classified.owx` here is dated 2026-06-15. **Vintage unresolved** —
  an earlier draft of this document asserted the CLAUDE.md oracle was six weeks *newer*, which
  is the reverse of what CLAUDE.md itself says, and nothing in this task establishes either
  direction. Not a reasoner difference, and it changes no #110 verdict.
* The 3 `NOT VERIFIED` rows (`ro-stripped`, `sulo-stripped`, `sio-stripped`) are the
  long-documented absent fixtures, unrelated to #110.

**Record this as INERTNESS.** Corpus reach for this shape is now *measured* zero (§2, §4), so a
green net here demonstrates non-regression only. The correctness evidence is Task 3's four-way
oracle adjudication (rustdl / Konclude / HermiT / KM equal on 6 of 6) plus its sabotage battery.

---

## 6. DKey discriminators unmoved

| ontology | BEFORE `concept_rules` | AFTER `concept_rules` |
|---|---:|---:|
| `ore_ont_9347` | 113 | 113 |
| `ore_ont_5368` | 18,620,251 | 18,620,251 |

**Both** were run, not just `9347`. `9347` alone cannot discriminate DKey work — it reads 113
under the real gate *and* under a build emitting no DKey disjointness at all — so `5368` is the
one that carries the check.

---

## 7. Gates

| gate | result |
|---|---|
| pin verification (behavioural, both binaries + both probes) | PASS |
| FP=0 net | rc=0, 11 VERIFIED, closures exact |
| fragment routing (effect 1), 1,920 × 2 arms | 0 movers, 0 anomalies |
| classify answers (effect 2), 27 bearing + 20 controls | 45 IDENTICAL / 0 DIFFER / 2 UNMEASURED / 0 regressed |
| gate-only probe calibrated vs `classify` banner | 3/3 agree on real pool files |
| lost entailments on the transitive closure | **0**, over 25 measured bearing + 20 controls |
| `ok → dnf` | **0** |
| DKey discriminators | unmoved (both) |
| `cargo clippy --workspace --all-targets --all-features -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

---

## 8. Method notes worth keeping

1. **The closure comparator is a per-node BFS with equivalence groups expanded, and it was
   self-tested before use.** Equivalence groups are cycles; a memoised `anc(x)` under a
   cycle-detection stack returns wrong answers on a cycle, which has already produced one
   fabricated "3 lost entailments" finding in this project. Two self-tests were run against
   hand-computed values: a 3-cycle `A→B→C→A` plus `C→D` (closure 9, +4 on adding `D→E`), and an
   equivalence group `{P,Q}` expanding to a 2-cycle (closure 4 → 1, lost 3).
2. **The comparator's first version printed nothing** — `main()` was never called. It was caught
   only because the self-test was run before any real data. A silent instrument reads exactly
   like a clean result.
3. **A harness bug nearly swallowed the only DIFFER.** The summary loop parsed the comparator's
   stdout as a single JSON object; when gained/lost rows were appended, `json.load` raised
   `Extra data` — so the *one* interesting row in 27 surfaced as a traceback. A parse failure in
   a summary loop must be treated as a finding, not skipped.
4. **Compare BEFORE vs AFTER under the same flag.** §3 is an entire pre-registered expectation
   overturned by a comparison that crossed a flag boundary.
5. **A shape census sizes a population; only the gate predicts an effect.** §1a bins 13
   ontologies where the pass *can* fire; the gate then shows it changes nothing on any of them.
6. **Do not wrap runs in `/usr/bin/time`** — it has been observed on this host to hang for hours
   at 0% CPU after `timeout` kills its child. Walls here are `date +%s` deltas.
7. **Threat to validity, stated:** the frame classify sweep and the corpus fragment sweep
   overlapped in time on this 32-core host. The fragment probe is single-threaded, so contention
   was ~1 core, far from the 6-way parallelism that has manufactured spurious `ok → dnf` here
   before — and every flagged result (`10517`) was re-adjudicated sequentially on an idle host.
   The FP=0 net, the DKey discriminators and the `10517` control were all run with nothing else
   in flight.
8. **Separate a change's effects before choosing an instrument.** A gate sweep and an answer
   sweep look redundant here and are not: `dom_partial.ofn` shows the derivation effect firing
   with the gate *unmoved* (§2b). Had only the gate sweep been run, "0 movers" would have read as
   "inert" while leaving the larger half of the change unmeasured.
