# Definitorial (surrogate) absorption of the sufficient direction — Implementation Plan

> # ⛔ RETRACTED 2026-08-04 — NEVER EXECUTED. DO NOT EXECUTE.
>
> Two independent adversarial reviews returned **NO-GO** and **DON'T DO IT**, on the same
> decisive finding, reached separately: **Phase 1's addressable set on `ore_ont_10019`,
> this plan's own headline target, is ZERO.**
>
> Phase 1 builds a Horn surrogate chain over the **atomic** conjuncts of `C₁ ⊓ … ⊓ Cₙ ⊑ D`.
> A chain needs **≥2** atomic conjuncts — with one, the surrogate `S ≡ A` fires on exactly
> the node set `A` fires on and the residual head is unchanged. **All 26 of `ore_ont_10019`'s
> conjunctive definitions have exactly ONE atomic conjunct** (verified by both reviewers,
> independently, by parsing `canon.owx`). The shipped instrument that counts this population
> — `concept_rule_or_with_extra_not_atomic`, whose own doc comment at
> `crates/owl-dl-core/src/residual_absorbability.rs:47-56` identifies it as *the*
> binary-absorption payoff column — reports:
>
> ```
> #   conclusion_is_or:           29
> #   ..with extra ¬Atomic:       0  (binary-absorption candidates)
> ```
>
> **That zero is quoted verbatim in `docs/2026-08-04-ore-10019-rootcause.md:133` and again in
> this plan's own "defect" section.** I wrote a plan whose lead task cannot fire on its target,
> past a number I had already printed twice. Task 3's decision rule was keyed entirely on that
> ontology, so the plan was scheduled to spend two tasks arriving at its own
> "Phase 1 is refuted" branch.
>
> Three further findings each independently invalidate the plan's architecture:
>
> - **"Preprocessing only / no engine change" is FALSE for the main tableau**, which carries
>   **84.6%** of the stall. `ConceptRule { trigger: ClassId, conclusion: ConceptId }`
>   (`absorb.rs:145-148`) has a **single-atom** trigger, so the Horn rule `C₁ ⊓ C₂ → S₁` is
>   **not expressible in `AbsorbedTBox`** — the only available encoding is another disjunction.
>   `absorb.rs:45` says so in-tree: *"Multi-trigger absorption (`A ⊓ B ⊑ C`) is a Phase 4
>   refinement."* Verified.
> - **The wedge ALREADY implements Phase 1's effect and still stalls.**
>   `absorb_hard_antecedent` (`owl-dl-core/src/clause.rs:442-491`) puts every *soft* antecedent
>   conjunct — including `Some` — into the clause **body**, negating only hard ones into the
>   head. So `A ⊓ B ⊑ D` is already `A(X) ∧ B(X) → D(X)` there. The wedge nonetheless burns
>   **373,919 branches** on `KetoneGroup`. In-tree falsification of the premise, on this very
>   ontology.
> - **Both measurement gates were invalid for this change.** `10019` is **absent** from the
>   400-ontology MISSED-net population (verified), so the primary gate was blind to the primary
>   target; and **7 of 8 curated fixtures have `extra ¬Atomic == 0`**, so "closures exact"
>   would have been an inertness check, not evidence.
>
> Reviews: `docs/reviews-2026-08-04/R1-technical.md` (11 blockers/concerns, with the
> id-space, output-leak, and told-table hazards a surrogate would have introduced) and
> `docs/reviews-2026-08-04/R2-value.md` (population counts from the committed census;
> ranked alternatives).
>
> **Kept, not deleted, as the record of a plan that contradicted its own cited evidence.**
> The replacement is `2026-08-04-multi-trigger-absorption.md`, built on R1's constructive
> finding: port the wedge's existing soft/hard antecedent partition into `absorb.rs` —
> multi-trigger absorption for the main tableau, which introduces **no new class** and so
> inherits none of the hazards below.
>
> Everything after this banner is the retracted draft, unedited.

---

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`.
> **DRAFT — awaiting adversarial review. Do not execute until the review section at the bottom is filled in.**

**Goal:** Make a defined class's sufficient (`⇐`) direction fire as a **Horn rule** rather than a disjunction, eliminating both the branching *and* the speculative generation that Konclude avoids.

**Architecture:** Preprocessing only. Introduce fresh surrogate atoms so `C₁ ⊓ … ⊓ Cₙ ⊑ D` becomes a chain of Horn implications instead of the NNF disjunction `¬C₁ ⊔ … ⊔ ¬Cₙ ⊔ D`. No engine change; the tableau and wedge see a different, logically equivalent TBox.

**Tech Stack:** Rust (`RUSTUP_TOOLCHAIN=stable`), `owl-dl-core` absorption, `owl-dl-tableau` clausifier, the MISSED net and `sweep-arm.sh` for gates.

---

## The defect, precisely

`ore_ont_10019` (47 classes, Konclude **20 ms**, rustdl **97 s** for 159 of 162 pairs) was root-caused in `docs/2026-08-04-ore-10019-rootcause.md`:

- A defined class's `⇐` direction absorbs to a **disjunctive head anchored on a shared atomic soft trigger** (`clause.rs:471-477`, `absorb.rs:461-468`) — there is no surrogate atom.
- `search.rs:502-503` decides a disjunction is open by **purely syntactic** label membership.
- The cardinality-complement disjuncts `¬(=1 r.C)` ⇒ `≤0 r.C ⊔ ≥2 r.C` are **semantically true on nearly every node but never syntactically present**, so every node carrying the shared trigger (`CarbonAtom`, a conjunct of ~15 of 29 defined classes) keeps ~15 disjunctions **perpetually open**; choosing the `D` disjunct regenerates the trigger.

**Konclude, measured:** definitorial surrogate body atoms make `⇐` the Horn rule `soft ⊓ M₁ ⊓ … → D`. No nondeterminism, and **no speculative assertion of `D`**, hence no speculative generation.

## What has already been refuted — do not re-attempt

- **Porting the wedge's satisfaction check** (`RUSTDL_OR_CARD_SATISFIED`). Built, fired on its pre-declared criterion (branches halved, `options=5` 1175→6), **decided no class**, and **converted a branch-bound stall into a generation-bound one** (nodes 135→257). Reverted. **This is the failure mode this plan must avoid, and the reason a branch-count improvement is NOT the success criterion.**
- `=2` → `≥2`; `KetoneGroup`'s own `⇐`; `hasSingleBondWith` symmetry; both blocking modes; both mono-valued `Alkyl` fillers.
- Depth caps in both directions; iterative deepening; adaptive early-abandon; domain absorption (drives its residuals to zero and it still stalls); `MAX_BODY_VARS`; the label heuristic (prunes 86% here — healthy).
- The 2026-07-16 "over-branching is pure waste" corollary is **retracted** — that spike had *deleted* the `⇐` clauses.

## Global Constraints

- **FP=0 is absolute.** This is a *semantics-preserving* rewrite: a fresh atom defined by an equivalence over a signature the user never queries adds no entailments over the original signature. **That is the soundness argument and it must be verified, not asserted.**
- **Surrogates must never appear in output.** `reportable_class_iris` already filters synthetic `DKey`/`NomKey` classes — reuse that path. A surrogate leaking into a closure is an FP against the oracle.
- **Success criterion is DECIDED CLASSES and WALL, never branch count.** The refuted attempt halved branches and decided nothing.
- **Two gates for any default flip**, and neither substitutes for the other: the **MISSED net** (`owl-reasoner-harness/scripts/missed-net.*`, baseline **MISSED = 5,198**, FP = 0, ~10 min/arm) *and* a **1,920-ontology two-arm sweep** for `ok → dnf`. The net's frame is drawn from *completers* and structurally cannot see a DNF-tail regression.
- Build with `RUSTUP_TOOLCHAIN=stable cargo build --release` (a bare `cargo` FAILS; a skipped build silently reuses a stale binary). Pin and sha-verify every measured binary.
- Cap every probe in wall AND address space; run serially. Prove every instrument fires by a numeric criterion declared in advance.
- `grep -c`/`pgrep -c` print `0` **and exit 1** on no match. Never `cmd | tail` then read `$?`.
- Sabotage every canary, strictly serially, reporting counts **as run including survivors**.

---

## The decomposition, and why it is in this order

`C₁ ⊓ … ⊓ Cₙ ⊑ D` has two kinds of conjunct, and they are **not** equally tractable:

- **Atomic `Cᵢ`** — a surrogate chain is plain Horn: `C₁ ⊓ C₂ → S₁`, `S₁ ⊓ C₃ → S₂`, …, `Sₖ → D`. Membership is a label lookup. This is textbook binary absorption (Hudek & Weddell) with surrogates.
- **Cardinality `Cᵢ` = `=n r.F`** — a Horn body atom needs "this node satisfies `=n r.F`", which is **not** a label lookup: it requires counting successors, which is what the tableau does nondeterministically. **This is the open sub-problem**, and the refuted attempt is evidence it is genuinely hard.

**So Phase 1 handles atomic conjuncts only, and Phase 2 is gated on Phase 1's measurement.** The bet is that pulling the atomic conjuncts out of the disjunction shrinks the set of nodes on which it opens — `CarbonAtom` alone triggers ~15 disjunctions today. **Phase 1 may be sufficient, and if it is, Phase 2 is never built.** If Phase 1 does not move `10019`, that is a documented result and Phase 2 needs its own justification rather than being assumed.

---

## Task 1: Report-only census — how many ontologies have the shape, and what mix of conjuncts

**Files:** new CLI subcommand alongside `residual-absorbability`; `docs/2026-08-04-definitorial-census.md`.

- [ ] **Step 1.** Add `definitorial-census <file>` printing, per ontology: number of `⇐` directions that currently absorb to a disjunctive head; for each, the count of **atomic** conjuncts, **cardinality** conjuncts, and **other** conjuncts; and the number of distinct **shared soft triggers** (an atomic conjunct appearing in ≥2 such heads — the mechanism that makes this quadratic).
- [ ] **Step 2.** Validate on `ore_ont_10019` before trusting it anywhere: it must report ~29 defined classes, ~15 heads sharing `CarbonAtom`, and cardinality conjuncts present. **If it does not, the instrument is wrong, not the root-cause doc.**
- [ ] **Step 3.** Run over all 1,920. Report: how many ontologies have ≥1 such head; the distribution of shared-trigger counts; **how many heads are all-atomic** (Phase 1 fully absorbs them) versus **mixed** (Phase 1 partially absorbs) versus **all-cardinality** (Phase 1 does nothing). Count and report conversion timeouts per arm — never filter them out.
- [ ] **Step 4.** State the addressable set for Phase 1 **before** building it. If all-atomic and mixed heads are rare outside `10019`, say so — that bounds the whole plan and is a legitimate stopping point.

---

## Task 2: Phase 1 — surrogate absorption of atomic conjuncts

**Files:** `crates/owl-dl-core/src/absorb.rs`; `crates/owl-dl-tableau/src/clause.rs`; new `crates/owl-dl-reasoner/tests/definitorial_absorption.rs`.

- [ ] **Step 1: Write the failing canary first.** A synthetic with a defined class whose `⇐` has ≥2 atomic conjuncts plus one cardinality conjunct, sharing its trigger with a second defined class — the `10019` shape in miniature. Assert the entailment Konclude derives. **Beware two known fixture traps**: the `⊔`-rule literal prune and `ConceptPool::or` flattening have each silently made a fixture degenerate. Verify the fixture actually produces a disjunctive head, by instrument, before believing the test.
- [ ] **Step 2: Run it; confirm it fails** for the right reason (a disjunctive head, not a parse error).
- [ ] **Step 3: Implement.** Introduce fresh surrogate class ids (follow how `DKey`/`NomKey`/Tseitin synthetics are minted and how `num_total_classes` accounts for them — **note the D4 finding that synthetic minting inflated a dense matrix 47×, so check the memory consequence of minting a surrogate per head**). Emit the Horn chain for the atomic prefix; leave the residual disjunction over the non-atomic conjuncts plus `D`. Behind flag `RUSTDL_DEFINITORIAL_ABSORPTION`, **default OFF**.
- [ ] **Step 4: Run the canary; confirm it passes.**
- [ ] **Step 5: Verify surrogates do not leak.** Assert no surrogate IRI appears in `classify` output on the canary and on `10019`; a leak is an FP against the oracle.
- [ ] **Step 6: Measure `10019`.** Report wall and **pair count against the 162 target** (baseline: 159 at 97 s; `RUSTDL_CLASSIFY_SAME_TIER=1` reaches 161 — report both arms, since 2 of the 3 misses are the orthogonal same-tier gap and must not be credited to this work).
- [ ] **Step 7: Sabotage the canary** — at minimum: emit the surrogate chain but never link `Sₖ → D`; reverse the chain order; mint one surrogate shared across two heads that must not share it. Report counts as run.
- [ ] **Step 8: Superset check** on ~15 ontologies the Task 1 census says carry the shape (**not** a random sample — a random ORE sample would be inert here, as it was for the tableau work). Any lost pair is a hard stop.
- [ ] **Step 9: FP=0 net + MISSED net.** `run-soundness-diff.sh` → 11 VERIFIED closures exact; MISSED net ΔMISSED against 5,198 with FP. **Predict both before running, in writing.**
- [ ] **Step 10: Full gates and commit.** fmt; clippy `-D warnings`; `cargo test --workspace --exclude owl-dl-py --no-fail-fast`.

---

## Task 3: Decide, by a rule fixed before Task 2's numbers are seen

- **`10019` reaches 162 (or 161 with `SAME_TIER`) AND ΔMISSED = 0 AND no `ok → dnf` in a full sweep** ⇒ recommend default ON.
- **`10019` improves but does not reach 162, ΔMISSED = 0** ⇒ keep OFF, report the residual, and decide Phase 2 on the Task 1 census — not on this one ontology.
- **ΔMISSED > 0** ⇒ the rewrite is not semantics-preserving as implemented. **That is a bug, not a trade** — the whole soundness argument is that a fresh definitional atom adds nothing over the original signature. Stop and diagnose.
- **`10019` unchanged** ⇒ Phase 1 is refuted; record it, and do **not** proceed to Phase 2 on the assumption that the harder half would have worked. Phase 2 would need independent evidence.

---

## Task 4 (conditional — only if Task 3 says so): Phase 2, cardinality surrogates

Deriving `Q ≡ (=n r.F)` requires detecting cardinality satisfaction, which is **not** a label lookup. The refuted `RUSTDL_OR_CARD_SATISFIED` attempt shows the naive form converts a branch-bound stall into a generation-bound one. **Do not start this without a written design that says how it avoids that**, and without the Task 1 census showing an addressable set beyond `10019`.

---

## Stopping rules

- **Task 1 bounds everything.** If the shape is confined to a handful of ontologies, say so and stop; a mechanism that explains one ontology is a bug report, not a program.
- **A branch-count improvement is not success.** The refuted attempt halved branches and decided nothing.
- **Any ΔMISSED > 0 is a bug here, not a trade.**
- If Phase 1 is refuted, Phase 2 needs fresh justification, not momentum.

## Adversarial review

*(To be filled in from two independent reviews before execution. Do not start Task 1 until this section records the findings and their resolutions.)*
