# Guarded absorption of the sufficient direction — Implementation Plan

> # ⏸ PARKED 2026-08-04 — NEVER EXECUTED. The design is sound; the addressable set is not established.
>
> Reviews: `docs/reviews-2026-08-04/R3-technical.md` (NO-GO as written),
> `docs/reviews-2026-08-04/R4-value.md` (DO IT AFTER a deletion pre-check on a re-selected list).
>
> **What survives, and it is worth keeping:** the central design insight is **correct and confirmed**.
> The marker can be the interned `∃r.F` itself, minting nothing, and `F ⊑ ∀r⁻.∃r.F` is a genuine
> SROIQ tautology across every edge case checked (self-loops, transitive/symmetric `r`, unsatisfiable
> `F`, nominals, inverse-derived edges). That **retires R1's entire hazard cluster** — id-space suffix
> invariant, `reportable_class_iris` leakage, told-table sizing, digest corruption,
> `num_total_classes²` RSS, `justify` unfolding. Konclude's mechanism remains measured and its
> sufficiency remains isolated by ablation. If this is ever built, build it this way.
>
> **Why it is parked: the same defect as v1, one level out — I gated on a STATIC count again.**
> Both reviewers independently ran cheap *dynamic* probes I had deferred, and both came back adverse:
>
> - **R3 ran `classify --pair-timeout-ms 1`** — which caps to ~zero exactly the phase this lever
>   improves — over 9 of the 18 tier-A targets. **6 of 9 still DNF at 60 s with per-pair search
>   already eliminated**, so no amount of branch reduction rescues them. The 3 that finish spend
>   40–47 s in phases this lever cannot reach (`label_cache_build` — the **wedge** — 32.2 s of
>   `ore_ont_8194`'s 60 s; `saturate`; and `prepare`, which this lever makes *more* expensive).
>   `ore_ont_4412` reports `# fragment: Horn (trust_sat)`: the **wedge** answers its pairs, and the
>   wedge already puts `∃r.F` into clause bodies as a real edge join (`clause.rs:543-559`) — a
>   *strictly stronger* form of this mechanism — and still stalls. That is R1's B3, which this plan
>   never addressed. **Cost of the probe: ~20 minutes.**
> - **R4 ran the deletion falsification** the standing project instruction
>   (`docs/2026-08-01-domain-absorption-results.md`) demands and this plan omitted, on the *most
>   favourable* member of the gated set (`ore_ont_9944`: 100% guard coverage, 2,110/2,110, a shared
>   trigger carrying 149 disjunctive rules, Konclude 0.81 s). Deleting **all 2,110** sufficient
>   directions — strictly stronger than guarding them — leaves it **DNF in both arms, flat**. At
>   `--pair-timeout-ms 50` it also fails, so it is not bottlenecked on a small set of pathological
>   pairs, which is the failure mode a per-pair guard attacks.
>
> **The target list was drawn from the wrong end, and the selector is anti-correlated with the goal.**
> None of the 18 is in the peer-triage top 20; only 3 of the 41 sub-second Set A members are in the
> gated 26; and **`ore_ont_4669`, which this plan names as a target, is Set B — no peer classifies
> it at all.** Decisively: guard-manufacturability holds on 77.5% of Set A **and 69.2% of Set B**, and
> the 31 Set A members with *nothing* manufacturable have a **median Konclude wall of 1.68 s against
> 6.14 s** for those that do. **The predicate does not select tractable ontologies; if anything it
> selects against them.** So the census sized a population correctly and that population is not the
> one worth attacking.
>
> **Three FP routes were also found, none involving the tautology** — recorded because they bind on
> any future attempt: (1) `absorb_roles` (`absorb.rs:191-205`) reconstructs `RoleRule` field-by-field,
> so a new `guards` field is silently dropped and the rule becomes **stronger than its axiom**;
> (2) `build_told_super_closure` (`lib.rs:772-787`) harvests atomic-conclusion `ConceptRule`s as
> **unconditional** told subsumptions — its own doc comment at `:740-746` states the invariant — and
> singleton-`Or` collapse makes tier-A conclusions atomic; (3) `Role::Inverse(r)` is ill-typed here and
> the naive repair is unsound for an `ObjectInverseOf` body conjunct (needs `role.flip()`), and the
> census does not look at the role at all (`residual_absorbability.rs:161`).
>
> Also corrected: **my Task 0 Step 4 states the edge direction backwards**, and so does the doc comment
> it cites (`absorb.rs:158-161`) — a worker verifying it literally would have "fixed" it into FP route
> 3. Task 0 tested the wrong risk (∃-regeneration is *provably* absent; the real graph-growth risk is
> **blocking** — `apply_role_rules` has no `is_blocked` gate and every added label makes subset-blocking
> harder). Task 3's tier-partition branch cannot fire. And the denominator is **18, not 17** — the
> census has an arithmetic slip, `3575`/`5218` are the same ontology (4 differing lines of 115,148) as
> are `239`/`9739`, giving **15 distinct peer-solvable tier-A problems**; "~98% of corpus volume" is
> really **78.5%**.
>
> **Reopen only with a dynamic addressability result**, not a shape count: ontologies where
> `--pair-timeout-ms 1` *does* complete quickly, whose stall *is* per-pair search, and which are
> fast for a peer.
>
> Everything after this banner is the unexecuted draft.

---

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`.
> **DRAFT — awaiting adversarial review. Do not execute until § Adversarial review is filled in.**

**Goal:** Stop a defined class's sufficient (`⇐`) direction from firing a disjunction on every node
carrying its one atomic conjunct, by giving the absorbed rule a **second guard** that is derived only
on nodes that actually have the required successor.

**Architecture:** Two additive preprocessing changes plus one small rule-shape extension. (1) For each
`∃r.F`-entailing conjunct in a `⇐` body, emit the tautology `F ⊑ ∀r⁻.(∃r.F)` as a `RoleRule`, so
`∃r.F` is materialised on exactly the nodes that have an `r`-successor in `F`. (2) Extend
`ConceptRule` with extra guards and absorb the `⇐` direction with `∃r.F` as a guard rather than as a
negated head disjunct. **No new class is minted** — the marker is the existential concept itself.

**Tech Stack:** Rust (`RUSTUP_TOOLCHAIN=stable`), `owl-dl-core` absorption, `owl-dl-tableau` rules,
the guard-manufacturable census, the MISSED net, `sweep-arm.sh`.

---

## Why this plan exists, and what makes it different from the one it replaces

`docs/superpowers/plans/2026-08-04-definitorial-absorption.md` was **retracted** after two
independent reviews found its addressable set on its target was zero. This plan differs on every
axis that mattered:

| | retracted plan | this plan |
|---|---|---|
| mechanism source | inferred from rustdl's own comments | **measured from Konclude's absorbed-TBox dump** |
| addressable set on the tail | 1 ontology reaches full coverage | **26**, and 116 of 120 partially |
| gating ontology | `ore_ont_10019` (addressable set 0) | the **26**, prioritising 17 tier-A-only |
| new classes minted | one per head (up to 15,928) | **none** |
| needs cardinality reverse-derivation | yes (deferred to a Phase 2) | **no** |

**The measured peer mechanism** (`docs/2026-08-04-konclude-cardinality-mechanism.md`): all 47 of
Konclude's absorbed rules carry exactly **two** guards and **zero** fire on a bare node, against
rustdl's one guard with **10** firing on every bare `CarbonAtom`. Konclude's second guard comes from
a cardinality conjunct **using only its `≥1` consequence**; its `≤n` halves stay in the head. So the
cardinality reverse-derivation this project had called the binding sub-problem **was never on the
critical path.** Ablation isolates sufficiency: from a 14-optimisation-disabled DNF floor,
re-enabling **binary absorption alone** restores 0.055 s / 162 pairs while 12 of 13 others stay DNF.

**Population** (`docs/2026-08-04-guard-manufacturable-census.md`, 1,914 ontologies analysed):

| | pool | tail (151) |
|---|---:|---:|
| `concept_rule_or > 0` | 888 | 120 |
| old predicate (`extra ¬Atomic > 0`) | 285 | **55** |
| **new predicate (guard-manufacturable)** | 489 | **116** |
| **→ 0 bare-node disjunctions** | 208 | **26** (vs 1 old) |

Tail rule coverage **93.1%** against 5.3% under the old predicate; per-ontology median coverage
98.3%. 101 tail ontologies have a trigger shared by ≥5 disjunctive rules (max **18,323**;
`ore_ont_10019`'s 10 is at the mild end).

**Why it is worth doing at all:** peers classify **138 of the 151** (91.4%) — Konclude median 5.11 s,
**39 under 1 s, 41 at ≥120×** (`docs/2026-08-04-tail151-peer-triage.md`). The tail is a demonstrated
algorithmic gap, and this is the mechanism the fastest peer is measured to use.

## Global Constraints

- **FP=0 is absolute.** Two independent reasons this change is FP-safe, and **both must be verified,
  not asserted**: (i) `F ⊑ ∀r⁻.(∃r.F)` is a **tautology** — it adds no models; (ii) adding a guard
  makes a rule fire *less* often, so a missing guard is a MISS, never an FP.
- **Mint no class, grow no signature.** `RoleRule.target_label` is already a `ConceptId`
  (`absorb.rs:164`) and `Role::Inverse(r)` already fires on incoming `r`-edges (`absorb.rs:158-161`),
  so the marker is the interned `∃r.F` itself. **This is load-bearing**: it is what makes the entire
  hazard cluster from `docs/reviews-2026-08-04/R1-technical.md` inapplicable — no vocabulary id-space
  suffix invariant to preserve, no `reportable_class_iris` leak (`realize.rs:626` and `:914` build
  their class list with **no filter**), no `told.rs` table pollution, no fragment-gate flip, and no
  surrogate bytes to corrupt the digest-based sweep comparison. **If the design drifts toward
  minting a class, re-read R1 in full before proceeding — every one of those hazards comes back.**
- **Success is DECIDED PAIRS and WALL, never branch count.** The prior `RUSTDL_OR_CARD_SATISFIED`
  attempt halved branches, decided no class, and made generation worse.
- **Two gates for a default flip, neither substituting for the other:** the MISSED net (baseline
  **MISSED = 5,198**, FP = 0, ~10 min/arm) *and* a 1,920-ontology two-arm sweep. The net's frame is
  drawn from completers and structurally cannot see an `ok → dnf`.
- **Gates must be tier-resolved, not aggregate.** In building the census, a deliberately sabotaged
  predicate still produced the correct aggregate on `ore_ont_10019` — only the tier split moved. An
  aggregate acceptance number would have passed a broken instrument.
- Build: `export RUSTUP_TOOLCHAIN=stable; export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`.
  A bare `cargo` fails; a skipped build silently reuses a stale binary. **Pin each binary to a
  uniquely named path immediately after the build that produced it and verify the pin against a
  discriminating input.**
- `cargo fmt --all`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- `grep -c`/`pgrep -c` print `0` **and exit 1**. One output file per concurrent worker, never shared.
- Sabotage every canary, serially, and report counts **as run, including survivors**.

---

## Task 0: Kill the plan cheaply if the ∃-rule regenerates witnesses

**This is the highest technical risk and it comes first, before any absorption work.** Adding
`∃r.F` to node `x`'s label is only free if the tableau's `∃`-rule recognises the **already-existing**
`r`-successor in `F` and does not generate a second one. If it generates, this plan reproduces the
exact failure that killed `RUSTDL_OR_CARD_SATISFIED` — a branch-bound stall converted into a
generation-bound one (nodes 135→257) — and must be abandoned or redesigned.

**Files:** `crates/owl-dl-tableau/src/rules.rs` (the `∃`/`Some` rule), `graph.rs`.

- [ ] **Step 1.** Read the `∃`-rule. Determine whether it checks for an existing witness before
      generating, and whether that check is syntactic (an `r`-edge to a node whose label contains
      `F`) or semantic. Report `file:line` and quote it.
- [ ] **Step 2.** Write a probe: a 3-axiom ontology with `A ⊑ ∃r.F`, and an injected label `∃r.F` on
      a node that already has its witness. Instrument node count. **Pre-declare the criterion:** if
      node count rises when the redundant label is added, the risk is real.
- [ ] **Step 3.** If it regenerates: check whether `RoleRule` target labels bypass the `∃`-rule, or
      whether a "satisfied-existential" marker distinct from `Some(r,F)` is needed (that would
      reintroduce a minted concept — go back and re-read the Global Constraints). **Report and stop
      for a decision rather than improvising.**
- [ ] **Step 4.** Also verify — by test, not by reading the doc comment — that
      `RoleRule { role: Role::Inverse(r), guard: Some(F), target_label: … }` fires on an `r`-edge
      **from** the guarded node **to** the labelled one, in the direction this design needs. The
      comment at `absorb.rs:158-161` says so; confirm it.

**Exit:** a written go/no-go on the generation risk, with the node-count numbers.

---

## Task 1: Multi-guard `ConceptRule`, inert by construction

**Files:** `crates/owl-dl-core/src/absorb.rs`; `crates/owl-dl-tableau/src/rules.rs`;
`crates/owl-dl-tableau/tests/`.

- [ ] **Step 1.** Add `guards: SmallVec<[ConceptId; 2]>` to `ConceptRule` (`absorb.rs:145-148`),
      defaulting empty. Keep `concept_rules_by_trigger` (`absorb.rs:94`) keyed on `trigger` — the
      index does not change; only the firing test gains a conjunct.
- [ ] **Step 2.** In `apply_concept_rules` (`rules.rs:~160-200`) and the deferred-Or path
      (`apply_deferred_concept_or_rules`, `rules.rs:~546`), require every `guard` to be present in
      the node's label set before firing. **Deps:** the fired conclusion's `DepSet` must union the
      trigger's deps **with each guard's** deps, or dependency-directed backjumping becomes unsound.
      An over-approximate dep is sound; start there and say so in the code.
- [ ] **Step 3.** With no producer of guards yet, the whole workspace must be **byte-identical**.
      Run `./scripts/run-soundness-diff.sh` → 11 VERIFIED, closures exact. This is a genuine
      inertness check and the right use of that gate.
- [ ] **Step 4.** Unit tests: a guarded rule fires when all guards present; does **not** fire when
      one is missing; deps include each guard's. Sabotage: drop the guard check (must fail the
      negative test); drop a guard from the dep union (must fail the deps test).
- [ ] **Step 5.** Commit.

---

## Task 2: Emit the marker `RoleRule`, tier A only

Tier A = a disjunct of shape `All(r, ¬F)` (from `∃r.F`) or `Max(0, r, F)` (from `≥1`/`=1 r.F`), `F`
a **named** class. Per the census this is **~98% of corpus volume**. Tiers B (`≥n`, n≥2 → `Max(n-1)`)
and C (complex filler, needs recursive composition) are **out of scope here** — if tier C is never
built the tail full-coverage headline drops 26 → 18, which the plan accepts.

**Files:** `crates/owl-dl-core/src/absorb.rs`; new `crates/owl-dl-reasoner/tests/guarded_absorption.rs`.

- [ ] **Step 1: Failing canary first.** `D ≡ A ⊓ ∃r.F` plus a second definition sharing trigger `A`,
      and a class provably satisfying each body. Assert both subsumptions. Verify **by instrument**
      (`rustdl residual-absorbability`) that the fixture really produces disjunctive-conclusion rules
      with `guard_manufacturable > 0` — two prior fixtures in this area were silently degenerate
      (the `⊔`-rule literal prune, and `ConceptPool::or` flattening).
- [ ] **Step 2.** Run it; confirm it fails, and confirm *why* (a disjunctive head, not a parse error).
- [ ] **Step 3: Implement**, behind `RUSTDL_GUARDED_ABSORPTION`, **default OFF**. For each qualifying
      disjunct: emit `RoleRule { role: Role::Inverse(r), guard: Some(F), target_label: pool.some(r,F) }`,
      **remove that disjunct from the head**, and add `pool.some(r,F)` to the rule's `guards`.
      Dedupe markers — one `RoleRule` per distinct `(r, F)`, not one per occurrence; the census shows
      single ontologies with 15,928 qualifying rules, so per-occurrence emission is a blow-up risk.
- [ ] **Step 4.** Canary passes.
- [ ] **Step 5: Sub-role and inverse correctness.** Two cases the `ore_ont_10019` shape cannot test,
      because **all five of its roles are symmetric**: (a) a conjunct `∃s.F` where the edge present is
      `r` with `r ⊑ s` — the marker must still be derived (the tableau consults sub-role propagation
      for `RoleRule`s, `absorb.rs:161`; verify); (b) a genuinely **asymmetric** role with a declared
      inverse. Write a canary for each. **A MISS here is sound but silent, so these are the tests
      that keep this from becoming a D10 instance** (a gate certifying completeness while the engine
      drops the entailment).
- [ ] **Step 6.** Measure the **17 tier-A-only targets** from the census §3.5: `ore_ont_3575`, `5218`,
      `11745`, `7127`, `11495`, `4669`, `7581`, `7956`, `11037` and the rest. Report wall + decided
      pairs, flag ON vs OFF, pinned binaries, serial. Konclude's wall for each is in
      `baselines/2026-08-04-setA-138-ranked.txt` — report the ratio.
- [ ] **Step 7.** `ore_ont_10019` as a **secondary** observation only. Expect 26 of 29 heads guarded
      and the 3 `ObjectUnionOf` definitions untouched. **It must not gate anything** — its unique
      remaining prize is one pair, and `RUSTDL_CLASSIFY_SAME_TIER=1` already recovers 2 of its 3.
- [ ] **Step 8.** Sabotage: emit the `RoleRule` but do not remove the head disjunct (should be
      verdict-identical but slower — a *control*, not a failure); remove the disjunct without emitting
      the `RoleRule` (**must lose entailments** — if it does not, the canary is not exercising the
      mechanism); use `Role::Named(r)` instead of `Inverse` (must fail).
- [ ] **Step 9.** Gates: FP=0 net flag-ON (11 VERIFIED, closures exact) **and** flag-OFF
      byte-identical to pre-change. MISSED net ΔMISSED. **Predict both in writing first.**
- [ ] **Step 10.** fmt, clippy, `cargo test --workspace --no-fail-fast`. Commit.

---

## Task 3: Decide, by a rule fixed before Task 2's numbers are seen

- **≥6 of the 17 tier-A targets recover (`dnf → ok`), ΔMISSED = 0, no `ok → dnf` in a 1,920-ontology
  two-arm sweep** ⇒ recommend default ON.
- **1–5 recover, ΔMISSED = 0** ⇒ keep OFF, ship as opt-in, report the residual, and decide tier B/C
  on the recovery *mechanism* observed — not on momentum.
- **0 recover** ⇒ the static census over-predicted. Record it as a measured negative and diagnose
  *why* before proposing anything further: the census counts absorbed-TBox shapes, not whether
  removing the disjunct changes the search.
- **ΔMISSED > 0** ⇒ diagnose before judging. Two genuinely different causes: a **missing marker**
  (sub-role/inverse gap — a bug, fix it) versus **tier-partition perturbation** (`classify.rs:2409-2454`
  groups classes into tiers by raw subsumer count and never compares same-tier classes, so a changed
  count changes which pairs are compared — a real trade needing an explicit accept/reject). Do not
  collapse these.
- **Any FP** ⇒ stop. The tautology argument fails and that is a design error, not a tuning matter.

---

## Stopping rules

- **Task 0 can end this plan**, and should if the `∃`-rule regenerates. That is the cheapest possible
  exit and it is deliberately first.
- **A branch-count improvement is not success.**
- **`ore_ont_10019` gates nothing.** It is the diagnostic instance, not the target population.
- If tier A recovers nothing, tiers B and C need independent justification, not inheritance.

## Adversarial review

*(To be filled in from two independent reviews before execution. Do not start Task 0 until this
section records the findings and their resolutions.)*
