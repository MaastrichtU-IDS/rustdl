# R2 — VALUE / PRIORITY review of `docs/superpowers/plans/2026-08-04-definitorial-absorption.md`

**Reviewer scope:** value and priority only. Nothing here is a claim about code correctness.
**Everything below is grounded in a committed file, a measured number, or a command re-run for this review.**

---

## Verdict

**DON'T DO IT — not as written.** Phase 1's addressable set on its own headline ontology is **provably zero heads**, measured by a shipped instrument and already quoted in the plan's own root-cause document (`ore_ont_10019`: `conclusion_is_or: 29`, `..with extra ¬Atomic: **0**`), while Task 3's decision rule is keyed entirely on that ontology — so the plan as sequenced is scheduled to spend two tasks and arrive at its own "Phase 1 is refuted" branch.

---

## The strongest argument against

### 1. Phase 1 cannot move `ore_ont_10019`, and this is measured, not inferred

Phase 1 is, by the plan's own words (line 48), *"textbook binary absorption (Hudek & Weddell) with surrogates"* over the **atomic** conjuncts. rustdl already ships the instrument that counts exactly that population — `concept_rule_or_with_extra_not_atomic` in `crates/owl-dl-core/src/residual_absorbability.rs:167-173`:

```rust
// residual_absorbability.rs:226-235
if let ConceptExpr::Or(args) = pool.get(rule.conclusion) {
    stats.concept_rule_or += 1;
    let extra = args.iter().any(|&d| matches!(pool.get(d), ConceptExpr::Not(inner)
        if matches!(pool.get(*inner), ConceptExpr::Atomic(_))));
    if extra { stats.concept_rule_or_with_extra_not_atomic += 1; }
}
```

Its doc comment states the purpose verbatim: *"This is where binary absorption pays."*

I re-ran it for this review on the target:

```
$ ./target/release/rustdl residual-absorbability .../ore_ont_10019.owl
# concept_rules:                182
#   conclusion_is_or:           29
#   ..with extra ¬Atomic:       0  (binary-absorption candidates)
```

Confirmed independently by reading the axioms. All 29 `EquivalentClasses` bodies have **exactly one** top-level atomic conjunct, which `as_trigger` already consumes:

```
AcylHalideGroup ≡ CarbonAtom ⊓ =1 hDBW.OxygenAtom ⊓ =1 hSBW.HalogenAtom ⊓ =1 hSBW.OrganicGroup
CarbonylGroup   ≡ =1 hDBW.OxygenAtom ⊓ CarbonAtom
AmideGroup      ≡ CarbonAtom ⊓ ∃hDBW.OxygenAtom ⊓ ∃hSBW.NitrogenAtom
```

There is **no second atomic conjunct to pull into a surrogate chain**. The plan's stated bet (line 51) — *"pulling the atomic conjuncts out of the disjunction shrinks the set of nodes on which it opens — `CarbonAtom` alone triggers ~15 disjunctions today"* — requires ≥2 atomic conjuncts per head. There is 1, and it is already the trigger. A surrogate `S ≡ CarbonAtom` fires on precisely the same node set as `CarbonAtom`, leaves the head width unchanged, and changes nothing about §5's mechanism.

**This number is printed in the plan's own source document**, `docs/2026-08-04-ore-10019-rootcause.md:131-134`, and read there as *"none of the 29 is a binary-absorption candidate"* — the correct reading, whose consequence for the phase ordering was then not drawn.

### 2. The decision rule is keyed on the ontology where the mechanism is inert

Task 3's four branches are all `10019`-relative. Combined with §1, the outcome is determined in advance: **"`10019` unchanged ⇒ Phase 1 is refuted; record it, and do not proceed to Phase 2."** There is no branch for *"10019 unchanged but N other ontologies recovered"* — so even a genuine broad win would land the plan in a stopping rule. A decision rule that cannot record its own success is worse than no rule.

### 3. The plan skips the prerequisite its own lineage declared

`docs/2026-08-01-domain-absorption-results.md`, § *"What this implies for what to build"*, names **binary absorption** as the right target (`concept_rule_or` AUC **0.849**) and then says, in bold:

> **Do not build it yet.** AUC 0.85 is discrimination, not causation, and this project has just been burned by exactly that inference. The next step is the same one-directional falsification that worked before: on a high-`rule_or` ontology, **delete** those rules (strictly stronger than absorbing them) and see whether it completes.

That falsification has not been run. This plan is the build, with the prerequisite dropped and no replacement. (The prerequisite also needs the correction at `2026-08-01-domain-absorption-results.md:336-349` applied — a deletion probe is only informative if the cut arm's *cost profile* improves — but "run it with the caveat" is a day, not a program.)

### 4. Task 1 largely re-measures a committed artifact

`docs/benchmarks/2026-08-01-residual-absorbability-census.tsv` is 1,921 lines, one row per pool ontology, and carries `concept_rule_or` and `concept_rule_or_with_extra_not_atomic` per ontology. From it (computed for this review, `status == OK`, n = 1,913):

| population | n | `or > 0` | **`extra > 0` = Phase-1 addressable** | `or > 0` **and** `extra == 0` = Phase-1 **inert**, Phase-2-only |
|---|---:|---:|---:|---:|
| whole pool | 1,913 | 888 | **285 (14.9%)** | **603** |
| v0.4.10 DNF survivors | 161 | 137 | **65 (40%)** | **72** |

`ore_ont_10019` is in the **603 / 72** — the Phase-1-inert column. Magnitude among the 285: min 1, **median 6**, p90 493, max 30,509.

Task 1 Steps 1–3 propose a new CLI subcommand to derive numbers that this table already supports for the load-bearing question. What Task 1 would genuinely add is the *shared-trigger* count and the per-head atomic/cardinality mix — worth having, but that is a one-day extension to `residual_absorbability.rs`, not a new instrument, and it should be run **before** any commitment, not as step one of a plan whose step two is implementation.

### 5. The prize on the headline target is at most one subsumption

`docs/2026-08-04-ore-10019-rootcause.md:66-82`: default **159/162 FP=0**; `RUSTDL_CLASSIFY_SAME_TIER=1` → **161/162 FP=0**. Two of the three misses are the already-documented tier-walk gap, recoverable **today with an existing flag**. This plan's unique completeness contribution on its own target is **`SulfoxideGroup ⊑ SulfinicAcidGeneralGroup`** — one pair — and only via **Phase 2**, which the plan gates on Phase 1 succeeding.

### 6. Blast-radius vs prize is badly proportioned

The change is in `absorb.rs`, the preprocessing path every one of 1,920 ontologies traverses, to fix a mechanism confirmed in **one**. Priced against the project's own base rate for behaviour-changing default-OFF flags, this plan pattern-matches strongly. Counted from source (`grep` for the opt-in idiom `var_os(…).is_some_and`), the live opt-in set includes `RUSTDL_DOMAIN_ABSORPTION`, `RUSTDL_CLASSIFY_SAME_TIER`, `RUSTDL_SAT_ENQUEUE_DEDUP`, `RUSTDL_LAZY_ABOX_SATURATION`, `RUSTDL_TABLEAU_ITERATIVE_DEEPENING`, `RUSTDL_SEMANTIC_BRANCHING`, `RUSTDL_SAT_LOOKAHEAD`, `RUSTDL_CLASSIFY_DEFINED_SWEEP`, `RUSTDL_BOUND_DIVERGED_TAIL`, `RUSTDL_PREP_DEADLINE`, `RUSTDL_ANYWHERE_BLOCKING`, plus `RUSTDL_WIDE_BODY_VARS` / `RUSTDL_MODEL_DERIVED_TYPES` / `RUSTDL_NOMINAL_FIRST` / `RUSTDL_SNAPSHOT_CAPTURE` and the retired CB engine.

**Probability this ships default-OFF and is never turned on: high — I would put it above 80%**, and the reason is not vibes. In this project a default flip requires a 1,920-ontology two-arm sweep showing no `ok → dnf` (the plan correctly says so, line 36); the *recommendation* to run that sweep comes from Task 3; and Task 3's rule cannot be satisfied because §1 says `10019` will not move. The flag has no route to ON as the plan is written.

---

## The strongest argument for

Stated as forcefully as the evidence permits, because parts of it are genuinely strong.

1. **It is the only mechanism in this arc that explains a large peer gap at the level of the calculus.** `ore_ont_10019` is 47 classes, 182 concept rules, **0.01 GB** RSS, conversion and saturation each 0.01 s (`docs/benchmarks/2026-08-01-dnf257-characterization.md:104-110`) — Konclude **0.04 s**, rustdl **97 s**. Every constant-factor lever in the 2026-08-03 audit was measured null on it, and the 2026-08-03 early-abandon doc explicitly disowns it: *"`ore_ont_10019` is not addressed, and this lever is the wrong instrument for it."* The plan's §5/§8 diagnosis — a purely **syntactic** ⊔ open-test (`search.rs:502-503`) against cardinality-complement disjuncts that are semantically true and never syntactically present, on a **shared** soft trigger — is the sharpest mechanism statement produced in this arc, and it is corroborated by the single best artifact in it: `sat` over all 47 classes gives **36 hang / 11 complete, and the 11 are exactly the atoms that are not a shared soft trigger** (rootcause §4b).

2. **The population evidence points here and nowhere else.** `concept_rule_or` is the only factor ever measured on this corpus that separates DNF from completing ontologies after size control: AUC **0.849** raw, **0.787** normalised, and **0.808 / 0.815 / 0.880** within `concept_rules` bands — against `residual_gcis` at **0.480, below chance** (`docs/2026-08-01-domain-absorption-results.md:392-415`). **56% of completers have zero disjunctive concept rules against 15% of DNFs.** Definitorial/surrogate absorption is precisely the family of techniques that reduces that quantity. The absorption-of-residuals thesis is dead; this is its measured replacement.

3. **The soundness exposure is genuinely lower than most levers here**, and for a structural reason: a fresh atom defined by an equivalence over a signature the user never queries adds no entailments over the original signature. Preprocessing-only, no engine change. The plan is properly paranoid about the two real hazards (surrogate leakage into `reportable_class_iris`; the D4 47×-matrix-inflation precedent on synthetic minting).

4. **`10019` is a genuine tail member at production budgets, contrary to a framing I was asked to test.** It completes in **97 s**, so it is `dnf` at the 60 s cap CLAUDE.md quotes and `dnf @90 s` in the early-abandon sweep (`2026-08-03-tableau-early-abandon.md:281`), and a completer only at ~120 s. Calling it "an ontology that already works" overstates the case.

5. **Parity is the stated project goal, and this is standard published technique** — Tsarkov & Horrocks on absorption variants, Hudek & Weddell on binary absorption — present in both HermiT and Konclude and absent here. A reasoner claiming parity plausibly must have it.

**Does the steelman win? No — but it wins a different plan.** Every load-bearing item above argues for **cardinality surrogates (Phase 2)**, or for **binary absorption justified on the 285/65 `concept_rule_or` population**. Not one of them argues for *Phase 1 first, gated on `10019`*, which is the only thing this plan actually commits to build.

---

## Ranked alternatives

### 1. Re-triage the current DNF tail against Konclude ∪ HermiT — **do this first, it is nearly free**

- **Prize:** the input to every prioritisation decision in this arc, including this one. The A/B/C partition (**242 / 15 / 25**) is measured on **v0.4.6** over **257** ontologies. Eight releases have since recovered ~90–100 (`CLAUDE.md:1343`: ~157 remain at 60 s). **Which of the current tail peers solve, and how fast, is unknown.** `docs/benchmarks/2026-08-01-dnf257-characterization.md:261-264` still lists *"Re-measure of the 257 against v0.4.8 — **running**"* and *"Phase 2 clustering of the survivors — tooling built, **blocked on the re-measure**."* That block is still in place; no re-triage has been published since.
- **Cost:** one harness run. The runners and scripts exist (`missed-net.sh peer`, `/data/dumontier/reasoners/run-konclude.sh`, the three `2026-08-01-triage-*.jsonl` legs). No new code.
- **Evidence it matters:** the last triage overturned the standing "intrinsic hardness" account and directly produced five shipped fixes and 44 recoveries. Nothing else on this list is cheaper per unit of decision value. **One caution, disclosed in the source:** `missed-net.sh peer` rebuilds `<peer>.jsonl` from a chunk glob and a sub-list run once destroyed ~90 records per leg (`2026-08-03-tableau-early-abandon.md:505-513`) — write to a distinct tag.

### 2. Settle `RUSTDL_DOMAIN_ABSORPTION`'s default

- **Prize:** **4 measured DNF→completes**, each serially re-verified ON vs OFF on an idle host: `ore_ont_3281` 11.49 s / 224 subs, `16372` 6.66 s / 2237, `6132` 33.34 s / 394, `9899` 33.16 s / 487 (`2026-08-01-domain-absorption-results.md:203-213`). Plus closing a dormant flag.
- **Cost:** **zero new code.** It is built, `fmt`/`clippy` clean, 1467 tests pass, FP=0 net **flag ON** with 11 VERIFIED closures exact, verdict byte-identical on 13 curated + 10 ORE ontologies, **6/6 sabotages caught**. It is OFF for exactly one missing measurement — a wall check over the 1,030 affected ontologies — already descoped to ~1/6 the wall as Task E of `2026-08-02-next-block-v2.md:159-163`.
- **Uniquely low risk:** it is the one technique in the census that is *sound and completeness-preserving by logical identity* with `ObjectPropertyDomain` (`residual_absorbability.rs:43-46`). It must change no verdict anywhere, only cost.
- **Honest caveat:** `3281` now also recovers via v0.4.14 early-abandon (19.91 s, `2026-08-03-tableau-early-abandon.md:282`), so the 4 may have shrunk to 3 — which is itself an argument for doing #1 before #2.

### 3. Defect 7 — batch the per-rule `Instant::now()`

- **Prize:** **11.28% self time on `ore_ont_10019`** — this plan's own target — attributed to the vdso clock (`2026-08-01-dnf257-characterization.md:144`). Broad: it touches every deadline-bounded main-tableau workload, and the main tableau is **84.6%** of `10019`'s stall.
- **Cost:** hours. I verified it is **unbuilt**: `crates/owl-dl-tableau/src/saturate.rs` still calls `ctx.check_deadline()` inside `step!` for each of 13 rules per node per pass, and `check_deadline` (`tableau/src/lib.rs:711-719`) reads `Instant::now()` unconditionally. The in-code comment still claims *"a cheap `Instant` comparison, dwarfed by rule bodies"* — measured wrong.
- **Risk:** the deadline is a *cut*; coarsening it can only delay abandonment by a bounded overshoot, never lose a proof or change a verdict. Needs a bound on the overshoot, not a completeness argument.
- **Not a DNF recovery.** ~11% of 97 s. Named because it is the cheapest real improvement to this plan's own target, not because it substitutes for the mechanism.

### 4. Phase 2 alone — cardinality surrogates, with its own design and its own population

- **Prize:** the actual fix. Both the root-cause doc §8 and the plan §8 say so: *"the correct target is … to make the ⇐ direction deterministic, which removes the branching and the speculative generation in one move."* Population, computed above: **603 pool / 72 v0.4.10 survivors** carry `concept_rule_or > 0` with **zero** atomic conjuncts left — the set Phase 1 provably cannot touch and Phase 2 exactly addresses. `10019` is in it.
- **Cost:** high and honestly stated by the plan — deriving `Q ≡ (=n r.F)` is not a label lookup, and the naive form (`RUSTDL_OR_CARD_SATISFIED`) was built, fired on its pre-declared criterion, **decided no class**, and converted a branch-bound stall into a generation-bound one (nodes 135→257, 243→657). Reverted.
- **Why it still ranks above the plan:** it is aimed at the thing that would move the target, and its population is 8× Phase 1's within the survivor set (72 vs 65 is comparable, but 603 vs 285 pool-wide, and `10019` is only in the former).

### 5. Binary absorption, re-justified on the `concept_rule_or` population — i.e. Phase 1 with `10019` removed as its gate

- **Prize:** **285 pool / 65 survivor** ontologies, median 6 heads, max 30,509; the AUC-0.849 discriminator.
- **Cost:** the same build as the plan's Task 2, plus the one-directional deletion falsification the prior doc demanded (a day, and it can kill the line for free).
- **This is the salvage of the plan under review.** It is ranked 5th only because #1 would tell you whether the 65 are still in the tail, and #2 and #3 are cheaper.

---

## Required rewrite if this work proceeds

Not "DO IT NOW", so these are the conditions under which the *mechanism* becomes worth executing.

1. **Delete `ore_ont_10019` from Task 3's decision rule.** It is the wrong gate for Phase 1 by measurement. Keep it as the Phase-2 gate, where it belongs.

2. **Make the census a hard gate, with the threshold declared now — and read the committed TSV first.** Concretely:
   - Re-run `residual-absorbability --tsv` over the **current** (post-v0.4.14) DNF tail, not the v0.4.10 167.
   - **Threshold, fixed before any implementation: ≥15 ontologies in the current tail with `concept_rule_or_with_extra_not_atomic > 0`, of which ≥5 have `≥2` such heads sharing one trigger.** Below that, stop; a mechanism whose population is a handful of shallow heads is a bug report. (For calibration: the v0.4.10 survivors gave **65** with `extra > 0` — so this threshold is plausibly met, and meeting it is the honest reason to build.)
   - Task 1's genuinely new content is the **shared-trigger count** and the per-head atomic/cardinality/other mix. Add it to `residual_absorbability.rs`; do not build a second subcommand that re-derives columns already in `docs/benchmarks/2026-08-01-residual-absorbability-census.tsv`.

3. **Add the missing decision branch:** *"`10019` unchanged AND ≥N ontologies in the current tail recover ⇒ recommend default ON."* Fix N now. Without it the plan cannot record a win.

4. **Run the deletion falsification as Task 0**, per `2026-08-01-domain-absorption-results.md`'s own instruction, with the `:336-349` correction applied: on a high-`concept_rule_or` **small** ontology, delete the Or-conclusion rules and report the cut arm's **cost profile** (timed-out pairs, branch counts), not just its outcome. A non-rescue with an unimproved cost profile is uninformative; a non-rescue with an improved one kills the line for a day's work.

5. **Predict, in writing, that Phase 1 leaves `10019` at 159/162 and 97 s**, and record `..with extra ¬Atomic: 0` as the reason. If it *does* move, the surrogate implementation is doing something other than what the plan describes, and that is a finding.

---

## One sentence to the plan's author

Your own root-cause document measured Phase 1's addressable set on `ore_ont_10019` — `..with extra ¬Atomic: 0`, quoted verbatim in its §4d — and you built the decision rule for Phase 1 around that ontology anyway, so the plan as written is scheduled to spend two tasks proving a number you had already printed.
