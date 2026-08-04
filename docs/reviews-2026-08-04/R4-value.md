# R4 — VALUE / PRIORITY review of `docs/superpowers/plans/2026-08-04-guarded-absorption.md`

**Reviewer scope:** value and priority only. Nothing here is a claim about code correctness (that is R1/R3).
**Baseline being updated:** `docs/reviews-2026-08-04/R2-value.md`, the value half of the review that
retracted `2026-08-04-definitorial-absorption.md`.
**Everything below is grounded in a committed file, a number recomputed for this review, or a command
I ran. Two probes were executed for this review; both are reported, including their caveats.**

---

## Verdict

**DO IT AFTER a one-directional deletion pre-check over a *re-selected* target list — because I ran
that pre-check on the plan's single most favourable target and it came back null:** on
`ore_ont_9944` (rank 33 of 138, Konclude 0.81 s, **100% guard coverage**, 2,110/2,110 disjunctive
rules manufacturable, the smallest file in the gated set), deleting **all 2,110 sufficient
directions** — strictly stronger than guarding them — left rustdl **DNF at the cap with 0 rows,
identically to the intact arm**, and the same ontology still DNFs at 200 s even when **every
individual pair is capped at 50 ms**, so its cost is not a small set of pathological pairs.

---

## Strongest argument against

### 1. I ran the plan's missing pre-check on its best target and the answer was "no rescue"

The plan's Task 3 concedes the gap in its own `0 recover` branch — *"the census counts absorbed-TBox
shapes, not whether removing the disjunct changes the search"* — and then schedules that diagnosis
**after** Tasks 0–2. The project's standing instruction
(`docs/2026-08-01-domain-absorption-results.md`, § *What this implies for what to build*) says the
opposite, in bold: *"Do not build it yet. … The next step is the same one-directional falsification
that worked before: on a high-`rule_or` ontology, **delete** those rules (strictly stronger than
absorbing them) and see whether it completes."* R2 named this as required change #4. It is still not
in the plan.

It costs one `sed`. I ran it.

**Target:** `ore_ont_9944` — chosen because it is the most favourable member of the plan's own gated
set: 100% coverage (`or = 2110`, `mfg_A = 2110`), a shared trigger carrying **149** disjunctive rules,
`Konclude 0.81 s` (rank **33** of 138), and at **3.1 MB** the smallest file among the 18 so the probe
is cheapest there.

**Cut:** `sed 's/^EquivalentClasses(/SubClassOf(/'` — every one of the 2,110 one-line, two-argument
`EquivalentClasses` axioms becomes a `SubClassOf`, deleting the ⇐ direction outright. Verified:
`SubClassOf` count 14,395 → 16,505 = 14,395 + 2,110.

**Command:** `RAYON_NUM_THREADS=1 timeout 130 ./target/release/rustdl classify <file>`,
binary `target/release/rustdl` (Aug 4 20:00), same binary both arms.

| arm | outcome | wall | rows emitted | peak RSS |
|---|---|---|---|---|
| intact | **DNF** (rc 124) | 130 s cap | **0** | — |
| **⇐ direction deleted (all 2,110)** | **DNF** (rc 124) | 130 s cap | **0** | 82.3 MB |

Konclude does this ontology in **0.81 s**.

This is the *upper bound* on the lever: guarded absorption keeps the disjunction and only narrows the
node set on which it opens; deletion removes it entirely. **If deletion does not rescue, guarding
cannot.**

The `:336-349` correction says a non-rescue is uninformative unless the cut arm's cost profile
improves. So I ran both arms again under a **truncating** `--pair-timeout-ms 50`, which bounds every
individual pair:

| arm | rc | wall | rows | peak RSS |
|---|---|---|---|---|
| intact | 124 | **200.00 s (cap)** | **0** | 96.4 MB |
| **⇐ deleted** | 124 | **200.00 s (cap)** | **0** | 82.2 MB |

Two readings, and I am keeping them separate. **(a)** The cut arm's cost profile does **not** improve
at either budget, so this is a genuine null on the pre-registered probe rather than the ambiguous case
the correction warns about. **(b)** Because no single pair can exceed 50 ms here, `ore_ont_9944` is
**not** bottlenecked on a small set of pathological subsumption tests — which is the failure mode a
per-pair guard most directly attacks, and the one `ore_ont_10019` exhibits (373,919 branches /
5,001 ms on `KetoneGroup` alone, mechanism doc EXP-4). **I did not localise the stall further** and do
not claim to: `classify` emits only on completion, so `rows = 0` says *did not finish*, not *did not
start*. Honest limits in § *Probe 2*.

**What this does and does not establish.** It is **one** ontology of 18, on a **contended** host
(load average 4.88; a concurrent session was running `rustdl classify` on `13846`/`4412`/`11037` — the
plan's own targets — during my run). Contention biases **both** arms toward DNF equally, and the
comparison here is *cut vs intact on the same binary under the same load*, so the "no differential
rescue" reading survives it; what does not survive it is any absolute wall claim, and I make none.
It is a **falsification of one instance**, not of the mechanism. But it is the instance the plan would
have reached first, and per this project's own method note — *"A single instance beats a population
statistic on this tail"* (`CLAUDE.md`) — that is the evidence class this project has repeatedly found
decisive.

### 2. The 17-target gate selects *against* the peer evidence it claims to rest on

Full detail in the table below. The compressed statement: the plan gates on **full coverage**, and
full coverage over thousands of rules is a property of *large, structurally uniform* ontologies —
which are also the ones peers take longest on. Recomputed from
`docs/benchmarks/2026-08-04-guard-manufacturable-census.tsv` against
`baselines/2026-08-04-setA-138-ranked.txt`:

- **Of the 20 fastest-for-peers Set A members, not one is in the gated 26.** All 18 of them with
  `or > 0` are partially covered (`ore_ont_6485` 49/52, `6333` 60/64, `10460` 31/37, `16372` 89/111,
  `10019` 26/29) and every one falls short of full coverage.
- **Of the 41 Set A members a peer does in under 1 s** — the triage's own "most diagnostic evidence
  in this document" — exactly **3** are in the gated 26 (`ore_ont_6608`, `9944`, `12432`).
- The 17 Set-A targets' peer walls run **0.81 s → 15.97 s, median 6.73 s**, against a Set A median of
  **5.11 s** (Konclude) / **4.42 s** (fastest peer). The plan's flagship pair `3575`/`5218` sit at
  ranks **105** and **104** (12.05 s / 11.96 s); `4412` at **116** (15.97 s).

So the census and the triage are pointing at different populations, and the plan is gated on the
census's.

### 3. Guard-manufacturability is nearly ubiquitous on the tail, so it cannot select targets

Recomputed: of the 151 tail ontologies, **116 (77%)** have something manufacturable. Split by peer
solvability:

| | manufacturable | not | rate |
|---|---:|---:|---:|
| Set A (peer solves) | **107** | 31 | 77.5% |
| Set B (no peer solves) | **9** | 4 | 69.2% |

**Nine of the thirteen Set B members are guard-manufacturable.** The predicate is present at
essentially the same rate in the ontologies three reasoners twice failed as in the ones Konclude does
in a second — so it carries almost no information about whether a given ontology will recover. Worse
for target selection: the 31 Set A members with **nothing** manufacturable have a **median Konclude
wall of 1.68 s** against **6.14 s** for the 107 that do. The lever's population is enriched in the
*slower* half of Set A.

And `ore_ont_4669` — named explicitly in the plan's Task 2 Step 6 target list — **is Set B**. No peer
classifies it. It should not be a target of anything.

### 4. The target list is smaller than 17 distinct problems, and two of the flagships are one ontology

The census says "17 of these 26 need tier A only"; the actual count of `C_only == 0` rows in its own
§3.5 table is **18** (`11495 4669 7127 7581 7956 3575 5218 11745 11037 8194 2111 6608 9944 11311 239
9739 4412 13846`) — so the list was never enumerated. More materially, it contains duplicates.
Comparing sorted TBox axiom sets (`SubClassOf`/`EquivalentClasses`/`DisjointClasses`/
`SubObjectPropertyOf`/`Transitive`/domain/range, `sort -u`, `diff`):

| pair | TBox axioms | differing lines | verdict |
|---|---:|---:|---|
| `ore_ont_3575` vs `ore_ont_5218` | 115,148 | **4** | the same ontology (`00483.owl` vs `…GO_XP_ALL.owl`); identical `EquivalentClasses` md5, identical 98,942 `SubClassOf` / 145 `DisjointClasses` / 264,461 `ClassAssertion` counts |
| `ore_ont_239` vs `ore_ont_9739` | 16,027 | **8** | the same ontology (`fly_anatomy_xp` vs `00463.owl`); identical counts on every axiom type |
| `ore_ont_6608` vs `ore_ont_9944` | 23,318 / 16,618 | 8,368 | genuinely different (same defined-class set, different `SubClassOf` content) |
| `ore_ont_4669` vs `ore_ont_7581` | 112,707 / 179,599 | 139,888 | genuinely different |

So the plan's headline "15,928 rules on `3575` **and** `5218`" is one ontology counted twice, and
`239`/`9739` likewise. Effective distinct, peer-solvable, tier-A-only problems: **15**, not 17.
`≥6 recover` can therefore be satisfied by as few as **4** distinct ontologies.

### 5. Blast radius: the guard check lands in a path that has been tuned three times

`ConceptRule` is `{ trigger: ClassId, conclusion: ConceptId }` (`absorb.rs:144-148`) — `Copy`, 8 bytes.
Adding `guards: SmallVec<[ConceptId; 2]>` makes it non-`Copy` and inflates it. The consuming path is
not cold: `apply_concept_rules` (`rules.rs:155-200`) already carries three shipped optimisations
(Phase 3's 64-bit `label_sig` bloom prefilter on `needs_deferred_or`, Phase 3c's `bot_id` `OnceLock`,
Phase 3d's hoisted linear-scan gate) and its own in-code comment records that *"on pizza this rule's
clone chain is the top exclusive-time frame"*. Phase 3e is the cautionary precedent: a change in this
neighbourhood won 7.49 pp on a SIO flamegraph and was **reverted at +2.34% GALEN wall**
(`docs/phase3e-results.md`, dead-end ledger §16). The plan's Task 1 Step 3 checks *verdict*
inertness (byte-identical closures) but declares **no wall gate at all** for the flag-OFF path.

One thing the plan does not mention and should: `Or(_)` conclusions are **already deferred** out of
this loop (`rules.rs` comment: *"`Or(_)` conclusions are **deferred**: skipped here and materialised
at saturate stable-state by `apply_deferred_concept_or_rules` … only when no disjunct is already
present. This is the Lever-A extension"*). So rustdl already has one demand-driven mitigation on
exactly this pattern, and the guard's marginal value is on top of it — which the "10 disjunctions on
a bare `CarbonAtom`" framing does not price.

---

## Strongest argument for

Stated as strongly as the evidence permits, because most of it is genuinely strong and better
evidenced than anything else in the arc.

1. **This is the only mechanism in the record measured from the reference implementation's own
   output rather than inferred.** `docs/2026-08-04-konclude-cardinality-mechanism.md` dumps
   Konclude's absorbed TBox from a static binary via 4 config keys and reads the answer directly:
   **all 47 `CCIMPL` rules have exactly 2 guards; 0 fire on a bare node**, against rustdl's 1 guard
   and 10 firing on every bare `CarbonAtom`. That is a structural fact about the reference
   implementation, not a wall-clock inference — and it *corrected two prior conclusions in this
   project's own record* (§5(a): the surrogate-Horn account of Konclude is wrong; §5(b): the
   "absorption on 10019 is fully blocked" reading measured the right number and drew the wrong
   inference).
2. **The mechanism is isolated as *sufficient* by peer ablation, which is the strongest experimental
   design in this arc.** EXP-3: no single Konclude flag makes it slow. EXP-7: 14 optimisations off
   ⇒ **DNF at 180 s** (≥3,600×); re-enabling **binary absorption alone** ⇒ **0.055 s / 162 pairs,
   complete**, while **12 of the other 13** stay DNF at 30 s. When one-at-a-time ablation returns
   null and the joint arm returns everything, inverting the ablation is the right instrument, and it
   named one winner.
3. **EXP-8 is the cleanest discrimination on record and it validates the deletion probe as an
   instrument.** Converting `EquivalentClasses` → `SubClassOf` for all but the first *k* definitions:
   Konclude is flat (0.025 → 0.076 s over k = 0 → 29) while **rustdl's `sat` crosses into DNF between
   k = 3 and k = 5 and classify from k = 12**, and at k = 0 rustdl classifies in **0.014 s**. On the
   diagnostic instance the deletion probe rescues completely — which is exactly why running it on the
   *population* is cheap and decisive, and why my negative result on `9944` is meaningful rather than
   instrument noise.
4. **The prior review's entire hazard list is dissolved, and by a real design property rather than by
   assurance.** R1's cluster — vocabulary id-space suffix invariants, `reportable_class_iris` leakage,
   `told.rs` pollution, fragment-gate flips, surrogate bytes corrupting digest comparisons — all
   presupposed a *minted class*. Here the marker is the interned `∃r.F` itself: `RoleRule.target_label`
   is already a `ConceptId` and `Role::Inverse(r)` already fires on incoming `r`-edges
   (`absorb.rs:157-164`, which I read). No new signature, no new IRI. That is a genuine architectural
   simplification, and the plan makes it a Global Constraint with an explicit "if the design drifts
   toward minting, re-read R1 in full" tripwire.
5. **The plan is soundly *directed*: a missing guard is a MISS, never an FP.** Adding a conjunct to a
   rule body can only make it fire less. Combined with the tautology `F ⊑ ∀r⁻.(∃r.F)`, the FP surface
   is genuinely small — a rare property on this tail.
6. **Task 0 is a real, cheap, pre-registered kill switch on the highest technical risk**, with the
   right failure named (the `RUSTDL_OR_CARD_SATISFIED` precedent: branches halved, no class decided,
   nodes 135→257) and the right criterion declared in advance. Task 3 has the positive branch R2 said
   the retracted plan lacked, and `ore_ont_10019` is explicitly demoted to "gates nothing".
7. **The census's *partial*-coverage numbers, which the gate throws away, are the real prize.**
   116 of 120 tail ontologies with a disjunctive rule are partially covered, **93.1% of their 648,373
   disjunctive rules**, per-ontology median **98.3%**. If the mechanism works at all, its reach is far
   wider than 18 — the ≥6-of-17 gate is a floor, not a ceiling.

**Does the steelman win? No — and the reason is narrow and fixable.** Every item above argues that
the *mechanism* is real, correctly attributed and cheaply gated. Not one of them argues for **this
target list** or for **building before the deletion pre-check** — and those are the only two things
the plan actually commits to. (a)–(d) of the steelman as posed are all true; (c), "sized on the
actual DNF tail rather than the pool", is true of the *census* and false of the *gate*, which selects
the tail's slow-for-peers half and excludes 38 of the 41 sub-second targets.

---

## The 17-target cross-reference (question 3)

Method: tier-A-only = `mfg_B_only == mfg_C_only == mfg_synth_only == 0` and
`mfg_A == concept_rule_or` and `shared_ge5 > 0`, over rows with `status == OK` in
`docs/benchmarks/2026-08-04-guard-manufacturable-census.tsv` restricted to
`baselines/2026-08-04-tail-v0414-list.txt`. Set/rank/wall from
`baselines/2026-08-04-setA-138-ranked.txt` and `…-setB-13-list.txt`. **The census says 17; the
predicate yields 18.**

| # | ontology | `or` rules | max/trigger | set | fastest peer | wall | rank /138 | note |
|---|---|---:|---:|---|---|---:|---:|---|
| 1 | `ore_ont_4669` | 7,515 | 6,102 | **B** | **none** | — | — | **no peer classifies it; named in Task 2 Step 6** |
| 2 | `ore_ont_9944` | 2,110 | 149 | A | konclude | 0.81 s | 33 | **deletion probe run for this review: NO rescue** |
| 3 | `ore_ont_6608` | 2,110 | 149 | A | konclude | 0.88 s | 37 | same defined-class set as `9944` |
| 4 | `ore_ont_239` | 1,877 | 148 | A | konclude | 1.04 s | 42 | **≡ `9739` (TBox differs by 8 of 16,027 lines)** |
| 5 | `ore_ont_9739` | 1,877 | 148 | A | konclude | 1.09 s | 45 | **duplicate of `239`** |
| 6 | `ore_ont_11311` | 1,877 | 148 | A | konclude | 1.48 s | 50 | larger variant of the `239` family |
| 7 | `ore_ont_11495` | 7,682 | 6,102 | A | konclude | 3.06 s | 61 | |
| 8 | `ore_ont_7956` | 7,515 | 6,102 | A | konclude | 3.07 s | 62 | |
| 9 | `ore_ont_7127` | 11,598 | 6,102 | A | konclude | 5.40 s | 74 | |
| 10 | `ore_ont_8194` | 315 | 259 | A | konclude | 6.73 s | 78 | median of the 17 |
| 11 | `ore_ont_11037` | 3,046 | 850 | A | konclude | 7.50 s | 83 | |
| 12 | `ore_ont_11745` | 14,846 | 4,619 | A | konclude | 8.59 s | 86 | |
| 13 | `ore_ont_2111` | 1,173 | 234 | A | konclude | 9.09 s | 89 | |
| 14 | `ore_ont_13846` | 281 | 16 | A | konclude | 10.40 s | 97 | |
| 15 | `ore_ont_5218` | 15,928 | 4,704 | A | konclude | 11.96 s | 104 | **≡ `3575` (TBox differs by 4 of 115,148 lines)** |
| 16 | `ore_ont_3575` | 15,928 | 4,704 | A | konclude | 12.05 s | 105 | **duplicate of `5218`; the plan's flagship** |
| 17 | `ore_ont_7581` | 7,515 | 6,102 | A | konclude | 13.01 s | 108 | |
| 18 | `ore_ont_4412` | 642 | 78 | A | konclude | 15.97 s | 116 | |

**Result: 17 of 18 are Set A, so the populations do overlap — but the gate is drawn from the wrong
end of the ranking.** Median peer wall **6.73 s** vs Set A's **5.11 s** (Konclude) / **4.42 s**
(fastest). None of the 18 is in the triage's top-20. Two pairs are duplicates and one member is
Set B ⇒ **15 distinct peer-solvable problems.** Only 5 of the 18 have a sub-1.5 s peer wall, and the
two cheapest of those are the same defined-class set (`9944`/`6608`), one of which I have now probed
negative.

For contrast, the same census columns on the triage's **top 20**, none of which the gate admits:

| ontology | peer wall | rank | `or` | manufacturable | coverage |
|---|---:|---:|---:|---:|---:|
| `ore_ont_6485` | 0.08 s | 3 | 52 | 49 | **94%** |
| `ore_ont_16372` | 0.14 s | 11 | 111 | 89 | **80%** |
| `ore_ont_6333` | 0.18 s | 13 | 64 | 60 | **94%** |
| `ore_ont_10460` | 0.19 s | 14 | 37 | 31 | **84%** |
| `ore_ont_10019` | 0.05 s | 1 | 29 | 26 | 90% |
| `ore_ont_1707` | 0.31 s | 18 | 145 | 79 | 54% |

Those four 80–94%-coverage, sub-0.2 s targets are a strictly better gate than `3575` (12.05 s) and
`4412` (15.97 s), and `16372` is *triple*-indicated: it is also one of `RUSTDL_DOMAIN_ABSORPTION`'s
recoveries **and** one of the three inconsistent-ontology cluster members.

---

## The realistic prize (question 4)

`≥6 of the 151` is **4.0%** of the tail — and per §4 above, 6 ontologies can be as few as 4 distinct
TBoxes, so the honest floor is **~2.6–4.0%**. Against the base rate for a *single* mechanism, counted
from `CLAUDE.md`'s own default-flip records:

| mechanism | recoveries | cost |
|---|---:|---|
| `RUSTDL_FRAGMENT_BARE_DECL` | **44** | a fragment-gate arm |
| `RUSTDL_ITERATIVE_DEEPENING` | **16** | a search-depth schedule |
| v0.4.14 tableau early-abandon | **6** | shipped at 6, with −5.5% wall |
| `RUSTDL_DKEY_MERGING_GATE` | 2 recovered (325 reduced) | a component gate |
| `RUSTDL_FAST_DIRECT_SUBSUMERS` | 1 named (`10125` DNF@900 s → 14.6 s) | an output-loop fix |
| `RUSTDL_DOMAIN_ABSORPTION` | 3–4, **unshipped** | already built |

So **6 is not disqualifying** — early-abandon shipped at exactly 6. But early-abandon was a *cut* with
no soundness surface and a **wall win across the population**; this is a structural change to
`absorb.rs` plus a rule-shape change in a thrice-tuned tableau loop, traversed by all 1,914 convertible
ontologies to help ~15. **A good outcome for one mechanism here is 10–16** — the iterative-deepening
band — and the census's own partial-coverage numbers (116 of 120, 93.1% of rules) say that band is
reachable *if the mechanism works at all*. That is the right ambition, and it is an argument for
gating on the fast-peer cohort rather than on the 18: a mechanism that recovers 6 slow-for-peers
ontologies and none of the 41 sub-second ones has probably not found the operative cost.

## Dormant-flag risk (question 5)

**The route to ON is real, and it is better than the retracted plan's** — Task 3 has a positive branch
with a pre-declared threshold, which R2 identified as precisely what the retracted plan lacked (its
rule could only record a refutation). That is a genuine improvement and I credit it.

**But the gate that would block it is the 1,920-ontology two-arm sweep, and the risk is concrete, not
generic.** The flag adds, per qualifying disjunct, one `RoleRule` and one guard — on **489 of 1,914**
pool ontologies, with per-ontology marker counts up to **30,481** (census §3.2). It helps ~15. That
asymmetry is the shape of the v0.4.8 `RUSTDL_CLASSIFY_INCONSISTENCY` regression: a flip validated on a
12-ontology cost benchmark took **four ontologies from ~5 s to DNF**, and only a full sweep found
them. Two secondary blockers: (i) no flag-OFF wall gate exists in Task 1 (required change #4), and
Phase 3e in this same neighbourhood was reverted at **+2.34% GALEN wall**; (ii) `ΔMISSED > 0` via
tier-partition perturbation (`classify.rs:2409-2454` groups by raw subsumer count), which Task 3
correctly identifies as a *real trade* needing explicit acceptance rather than a bug — and an explicit
trade is exactly the kind of decision that leaves a flag OFF.

For calibration, **20 names** match the opt-in idiom in source
(`grep -rn 'var_os("RUSTDL_…")\s*\.is_some_and' crates/`); stripping the pure diagnostics
(`*_STATS`, `*_TRACE*`, `SHADOW_DEP_PROBE`, `LABEL_AMORTIZE_MARK`) leaves ~11 **behaviour** levers
built and never enabled: `RUSTDL_DOMAIN_ABSORPTION`, `RUSTDL_CLASSIFY_SAME_TIER`,
`RUSTDL_SEMANTIC_BRANCHING`, `RUSTDL_TABLEAU_ITERATIVE_DEEPENING`, `RUSTDL_SAT_ENQUEUE_DEDUP`,
`RUSTDL_LAZY_ABOX_SATURATION`, `RUSTDL_CLASSIFY_DEFINED_SWEEP`, `RUSTDL_BOUND_DIVERGED_TAIL`,
`RUSTDL_PREP_DEADLINE`, `RUSTDL_ANYWHERE_BLOCKING`, `RUSTDL_SAT_LOOKAHEAD` — out of **113** distinct
`RUSTDL_*` names in the tree. **R2 put "ships OFF and never turned on" above 80%. I put it at
~55–60%** — lower, because the decision rule now admits a win, and higher than it should be, because
the sweep asymmetry above is real and the target list as written is unlikely to clear the bar. Fixing
the target list is the single change that most improves the route to ON.

---

## Updated ranked alternatives

**How the ranking changed (question 1).** R2's item (1) *re-triage the tail* is **DONE**
(`docs/2026-08-04-tail151-peer-triage.md`: 151 / Set A 138 / Set B 13) and drops off — it was the right
call and it is what makes this review's cross-reference possible at all. R2's (2)
`RUSTDL_DOMAIN_ABSORPTION` **shrank as R2 predicted but stays first**: `ore_ont_3281` has left the tail,
leaving **3** recoveries — but all 3 are Set A at 0.14/0.97/1.04 s and it still costs **zero new code**,
which no other item can say. R2's (3) `Instant::now()` batching is **still unbuilt** (verified) and
drops from 3rd to 4th, displaced by a cluster R2 could not see because the triage had not run: the
**three inconsistent tail members**. R2's (4) cardinality surrogates is **dissolved** — the mechanism
doc's EXP-4/§2b establishes that Konclude never reverse-derives a `≤n` surrogate, so the sub-problem
R2 ranked 4th does not exist as posed, and this is the single largest genuine advance since R2. R2's
(5) *binary absorption re-justified on a population* is **this plan**, and it is now justified in the
sense R2 asked for — but on a population selected by the wrong predicate, and still without the
deletion falsification R2 made required change #4. **Net: the mechanism moved up, the plan did not.**

### 1. Settle `RUSTDL_DOMAIN_ABSORPTION`'s default — still first, still zero new code

R2 flagged that the prize may have shrunk. Checked against
`baselines/2026-08-04-tail-v0414-list.txt`: **`ore_ont_3281` has left the tail** (it now completes,
consistent with v0.4.14 early-abandon), and the other three are still there, all Set A with fast peer
walls:

| ontology | in 151 tail? | fastest peer | rank /138 | domain-absorption result (`2026-08-01-domain-absorption-results.md:203-213`) |
|---|---|---:|---:|---|
| `ore_ont_3281` | **no — recovered** | — | — | 11.49 s / 224 subs |
| `ore_ont_16372` | yes | konclude **0.14 s** | 11 | 6.66 s / 2,237 subs |
| `ore_ont_6132` | yes | konclude **0.97 s** | 40 | 33.34 s / 394 subs |
| `ore_ont_9899` | yes | konclude **1.04 s** | 43 | 33.16 s / 487 subs |

**3 tail recoveries for zero new code**, on a technique that is sound *and completeness-preserving by
logical identity* with `ObjectPropertyDomain`, already `fmt`/`clippy` clean, FP=0 net flag-ON with 11
VERIFIED closures exact, 6/6 sabotages caught, OFF for exactly one missing wall measurement already
descoped to ~1/6 wall as Task E of `2026-08-02-next-block-v2.md:159-163`. Three of the plan's ≥6
threshold, for a day's measurement rather than a week's build. **Also note `16372` is in all three of
this session's candidate clusters at once** — domain absorption, high guard coverage (89/111), and
the inconsistent-ontology cluster — which makes it the single best-indicated ontology on the tail.

### 2. The deletion pre-check over a peer-wall-selected target list — *the gate this plan needs*

- **Prize:** it can kill or confirm the whole line for hours, and it is the project's own standing
  instruction. `sed 's/^EquivalentClasses(/SubClassOf(/'`, two `classify` runs per ontology.
- **Cost:** I did one (§1 above). Ten more is an afternoon, serial, on an idle host.
- **Do it on the right list:** `6485` (0.08 s, 94% cov), `6333` (0.18 s, 94%), `10460` (0.19 s, 84%),
  `16372` (0.14 s, 80%), plus the census's own `6608`/`9944`/`12432` as the sub-1 s full-coverage
  members. Report **decided pairs and timed-out pairs under a non-truncating `--pair-timeout-ms`**,
  not just outcome, per the `:336-349` correction.
- **Deletion is *not* a clean weakening and the plan must say so:** `CLAUDE.md`'s method note —
  *"Deletion is NOT computationally stronger than absorption. It turns cheap subsumptions into
  non-subsumptions that must be refuted, so a cut arm can DNF for work the intact arm never does."*
  That is why the cost profile, not the outcome, is the read. A cut arm that DNFs **with a flat cost
  profile** (my `9944`: 0 rows both sides) is the *uninformative-for-rescue, informative-for-priority*
  case: it says this ontology's stall is not bottlenecked on the ⇐ disjunctions at all.

### 3. The three inconsistent tail members — smallest self-contained cluster on the tail

`ore_ont_16372` (Konclude 0.14 s, rank 11, 745 unsat), `ore_ont_4141` (KM 0.33 s, rank 19),
`ore_ont_8445` (KM 0.91 s, rank 39) — all three are ontologies where Konclude reports `owl:Thing`
unsatisfiable in under 3 s and rustdl DNFs at 120 s. `CLAUDE.md` documents the exact residual:
classify's inconsistency detection is a sound under-approximation that *cannot reach a tableau-only
inconsistency*, with `ddmin_core_residual_divergence` already `#[ignore]`d as the record of it. 3
recoveries, and it does not touch `absorb.rs`. The triage names this as one of its three
"cheap to attack" clusters (`2026-08-04-tail151-peer-triage.md` §8).

### 4. Defect 7 — batch the per-rule `Instant::now()`

Re-verified **still unbuilt** for this review: `crates/owl-dl-tableau/src/saturate.rs` calls
`ctx.check_deadline()` at four sites inside `step!` (lines 88, 97, 118, 161), and
`check_deadline` (`crates/owl-dl-tableau/src/lib.rs:711-719`) reads `std::time::Instant::now()`
unconditionally on every call. **11.28% self-time on `ore_ont_10019`**
(`2026-08-01-dnf257-characterization.md:144`), and the main tableau is **84.6%** of that ontology's
stall. Hours of work; the deadline is a cut, so coarsening it needs an overshoot bound, not a
completeness argument. Not a DNF recovery — ~11% of 97 s — but broad and cheap.

### 5. Guarded absorption, re-gated — this plan, with §*Required changes* applied

Ranked below 1–4 **only** because of the target list and the missing pre-check, not because of the
mechanism. If the deletion pre-check in #2 comes back positive on 3 or more of the re-selected
targets, this moves to **first** and the rest of the plan is well constructed.

### 6. The two ~140 k-class flat-hierarchy ontologies (`ore_ont_16744`, `ore_ont_8737`)

Konclude 52.17 s / 46.58 s — ranks 135 and 134, i.e. the *slowest* Set A members, and the answer is
empty (0 of 142,884 and 0 of 136,612 `SubClassOf` axioms have a non-`Thing` superclass). Pure scale.
Named for completeness; 2 recoveries at the far end of the ranking.

---

## Required changes to the plan

1. **Add the deletion pre-check as Task 0, ahead of the `∃`-rule probe.** `sed`, two arms, ≥6
   ontologies from the re-selected list in #2 above. **Declare the criterion first:** the line
   proceeds only if ≥3 of them either complete under the cut or show a materially improved cost
   profile (decided pairs up / timed-out pairs down at a fixed non-truncating `--pair-timeout-ms`).
   Record my `ore_ont_9944` result as the first row and as a **negative**. This is R2's required
   change #4, still unaddressed, and it is now backed by a measurement rather than a citation.

   **Pair it with a phase attribution.** Coverage is worthless on an ontology whose wall is in
   preparation. For each candidate, report where the wall sits — conversion / EL saturation /
   `HyperCache::build` / label-cache build / pair loop — before treating its `mfg_A` count as a
   prediction. `ore_ont_9944` still DNFs at 200 s with **every pair capped at 50 ms**, which already
   rules out the pair-pathology signature that `ore_ont_10019` has and the mechanism was derived from.

2. **Re-select the target list on `peer wall × coverage`, not on full coverage.** Concretely: drop
   `ore_ont_4669` (Set B — no peer classifies it, so a recovery there is not even a demonstrated
   gap); collapse `3575`/`5218` and `239`/`9739` to one each (their TBoxes differ by 4 and 8 lines);
   and **add** `ore_ont_6485` (0.08 s, 49/52), `6333` (0.18 s, 60/64), `10460` (0.19 s, 31/37),
   `16372` (0.14 s, 89/111). The gate is currently 0-for-20 against the triage's fastest cohort and
   3-for-41 against its sub-second cohort; that is the single largest defect in the plan.

3. **Recalibrate the threshold on distinct problems and state what it is calibrated against.**
   "≥6 of 17" is uncalibrated. The distinct-problem count is 15, and 6 ontologies can be 4 problems.
   Anchor it to the base rate: `RUSTDL_ITERATIVE_DEEPENING` **16** recoveries,
   `RUSTDL_FRAGMENT_BARE_DECL` **44**, v0.4.14 early-abandon **6** (which shipped, with −5.5% wall),
   `RUSTDL_DOMAIN_ABSORPTION` **3–4**. A defensible bar for a change to `absorb.rs` + the tableau rule
   shape is **≥6 distinct TBoxes**, of which **≥2 from the sub-1 s peer cohort**, on a re-selected
   list — and say so in Task 3 before any number is seen.

4. **Add a flag-OFF wall gate to Task 1.** Task 1 Step 3 checks verdict inertness only. `ConceptRule`
   goes from an 8-byte `Copy` struct to a `SmallVec` carrier in a path carrying three shipped
   optimisations, whose in-code comment already names its clone chain as pizza's top exclusive-time
   frame, and whose nearest precedent (Phase 3e) was reverted at **+2.34% GALEN wall** after winning
   7.49 pp on a SIO flame. Pin GALEN + notgalen + pizza + SIO walls flag-OFF, min-of-3, and declare
   the tolerance before measuring.

5. **Price the interaction with `apply_deferred_concept_or_rules`.** `Or(_)` conclusions are
   *already* deferred out of `apply_concept_rules` and fired only at saturate stable state, only when
   no disjunct is present (Lever A, `rules.rs` comment). The "10 disjunctions on a bare `CarbonAtom`"
   framing prices the guard as though nothing else mitigates it. State, in Task 2, what the guard adds
   **on top of** deferral — and whether the guard test can be folded into the existing `label_sig`
   bloom prefilter rather than added beside it.

6. **Fix the census's arithmetic and name the 18.** Its §3.5 says "17 of these 26 need tier A only";
   the predicate over its own committed TSV yields **18**. The plan inherits "17" and then writes
   "and the rest", so the target list is nowhere enumerated in full. Enumerate it.

7. **Drop `ore_ont_4669` and `ore_ont_7646` from every target list.** Both are Set B. Nine of the 13
   Set B members are guard-manufacturable, which is precisely why manufacturability must not be used
   as a target-selection predicate on its own.

8. **Keep Task 0 (the `∃`-rule generation probe) — it is good** — but run it *second*. It is a
   technical kill switch; the deletion probe is a value kill switch, and value is cheaper to test.

---

## One sentence the plan's author will not want to hear

I ran the deletion probe your Task 3 defers to its own failure branch — one `sed` and two `classify`
runs — on the most favourable target in your gated set, and **deleting all 2,110 sufficient directions
from `ore_ont_9944` changed nothing, while capping every pair at 50 ms also changed nothing**: the only
dynamic measurement that now exists on your population is a null, and it took an afternoon that Tasks
0–2 would have spent first.

---

## Appendix — probes run for this review

### Probe 1 — deletion falsification on `ore_ont_9944`

```sh
F=/data/dumontier/ore-run/pool_sample/files/ore_ont_9944.owl
sed 's/^EquivalentClasses(/SubClassOf(/' "$F" > 9944-cut.owl      # 14395 -> 16505 SubClassOf
RAYON_NUM_THREADS=1 timeout 130 ./target/release/rustdl classify "$F"            # rc=124, 0 rows
RAYON_NUM_THREADS=1 timeout 130 ./target/release/rustdl classify 9944-cut.owl    # rc=124, 0 rows, 82.3 MB
```

Both arms: DNF, 0 rows. Same binary (`target/release/rustdl`, Aug 4 20:00), single-thread.
**Caveats, stated because they bound the claim:** contended host (load average 4.88; a concurrent
session was running `rustdl classify` on `13846`/`4412`/`11037` during the runs), single run per arm,
one ontology of 18. Contention biases both arms identically toward DNF, so the *differential* reading
("no rescue from deletion") holds; no absolute wall is claimed. Not a refutation of the mechanism —
a refutation of this instance, which is the instance the plan reaches first.

### Probe 2 — cost profile under a per-pair budget, and where `ore_ont_9944` actually stalls

The `:336-349` correction requires a cost profile, not an outcome, from a non-rescuing cut arm. So
both arms were re-run under a **truncating** per-pair budget, which bounds every individual pair:

```sh
RAYON_NUM_THREADS=1 timeout 200 ./target/release/rustdl classify --pair-timeout-ms 50 <arm>.owl
```

| arm | rc | wall | rows | peak RSS |
|---|---|---|---|---|
| intact | 124 | **200.00 s (cap)** | **0** | 96.4 MB |
| **⇐ deleted (all 2,110)** | 124 | **200.00 s (cap)** | **0** | 82.2 MB |

**Both arms DNF at both budgets.** The cut arm's lower RSS is a smaller KB, not a cost improvement.
So the cost profile is flat under the cut at a truncating budget as well as an unbounded one — the
`:336-349` correction's condition for an informative non-rescue is **not** met in the "improved
profile" direction, and is met in the "no signal anywhere" direction.

**What this licenses.** With every pair capped at 50 ms, no single pair can consume the wall. So
`ore_ont_9944` is **not** bottlenecked on a small set of pathological subsumption tests — the failure
mode a per-pair guard most directly attacks, and the one `ore_ont_10019` exhibits (373,919 branches /
5,001 ms on `KetoneGroup` alone, per the mechanism doc EXP-4). Its cost must be **preparation** or
**aggregate pair volume**.

**What this does NOT license, stated explicitly because the inverse claim is tempting and wrong.**
`classify` emits its hierarchy only on completion, so `rows = 0` means *did not finish*, not *did not
start*. **I have not localised the stall** and do not claim to. Distinguishing prep from volume is one
`RUSTDL_TRACE=1` run plus reading the `# ...` banner on a completing arm, and it belongs in the plan's
Task 0, not in a review.

**Consequence for the plan:** the pre-check must report a *phase attribution*, not an outcome. For
each candidate, run `RUSTDL_TRACE=1` (or a completing `--pair-timeout-ms` arm and read the banner) and
record whether the wall sits in conversion / saturation / `HyperCache::build` / label-cache build
versus the pair loop. An ontology whose wall is preparation-bound is **categorically** out of this
lever's reach regardless of its coverage number — and the highest-coverage, fastest-peer member of the
gated set is at minimum not pair-pathology-bound, which is the closest thing to that signature I could
establish in one afternoon.

### Cross-reference recomputation

```sh
H=/data/dumontier/owl-reasoner-harness
# tail x full-coverage x shared>=5  (cols: 11=or 13=mfg_A 14=B_only 15=C_only 16=synth 18=shared_ge5)
awk -F'\t' 'NR==FNR{t[$1];next} $2=="OK" && ($1 in t) && $11>0 && ($13+$14+$15+$16)==$11 && $18>0' \
  $H/baselines/2026-08-04-tail-v0414-list.txt \
  docs/benchmarks/2026-08-04-guard-manufacturable-census.tsv     # 26 rows; 18 with C_only==0
```

Set/rank/wall joins against `$H/baselines/2026-08-04-setA-138-ranked.txt` and `…-setB-13-list.txt`.
Duplicate detection: `grep -oP '^(SubClassOf|EquivalentClasses|DisjointClasses|SubObjectPropertyOf|
TransitiveObjectProperty|ObjectPropertyDomain|ObjectPropertyRange)\(.*' | sort -u | diff`.
