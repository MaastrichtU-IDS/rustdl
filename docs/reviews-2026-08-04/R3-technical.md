# R3 — adversarial technical review of `docs/superpowers/plans/2026-08-04-guarded-absorption.md`

Reviewer brief: find every way this plan FAILS or produces a WRONG result, with the prior review's
7 blockers (`docs/reviews-2026-08-04/R1-technical.md`) as the specific hazard list to re-check.
Every claim below is backed by `file:line` in this tree or by a command I ran. Where I could not
establish something, I say **cannot verify** rather than infer.

---

## Verdict

**NO-GO as written.** The central design insight is correct and does defuse R1's whole
minting-hazard cluster — but three code paths silently **drop** the new guard and two of them are
**FP** routes, the plan's own code sketch is ill-typed and unsound for an inverse-role conjunct, its
Task 0 acceptance criterion states the edge direction backwards, R1's B3 falsifier (the wedge
already implements a *stronger* form of this exact transformation) is not addressed, and a cheap
pre-check I ran shows **6 of 9** measured tier-A-only targets still DNF at a 1 ms per-pair budget —
i.e. with the phase this lever improves already reduced to ~zero.

---

## Blockers

### B1. `absorb_roles` silently drops the new guard, producing a rule STRICTLY STRONGER than the axiom — an FP.

`absorb_roles` is the last mutator on `concept_rules` and it converts any `ConceptRule` whose
conclusion is `All(R, D)` into a `RoleRule`, reconstructing it field-by-field:

```rust
// crates/owl-dl-core/src/absorb.rs:191-205
pub fn absorb_roles(tbox: &mut AbsorbedTBox, pool: &mut ConceptPool) {
    for rule in std::mem::take(&mut tbox.concept_rules) {
        if let ConceptExpr::All(role, target) = pool.get(rule.conclusion) {
            tbox.role_rules.push(RoleRule {
                role: *role,
                guard: Some(rule.trigger),      // ← only the trigger survives
                target_label: *target,
            });
```

A `guards: SmallVec<…>` field added at `absorb.rs:145-148` is **not read here and not carried into
`RoleRule`** (`RoleRule` has a single `Option<ClassId>` guard, `absorb.rs:163`). So for any GCI whose
head, *after the plan removes the `All(r,¬F)` disjunct*, is a `∀`:

```
A ⊓ ∃r.F ⊑ ∀s.G      →  disjuncts [Not(A), All(r,¬F), All(s,G)]
                      →  plan: remove All(r,¬F), guards=[∃r.F], conclusion = All(s,G)
                      →  absorb_roles: RoleRule{ role: s, guard: Some(A), target: G }   ← ∃r.F GONE
```

That rule fires on `A` + one `s`-edge, with no `∃r.F` requirement. It is strictly stronger than the
axiom, so it can manufacture a clash the KB does not entail — **a false-positive subsumption /
unsatisfiability.** This is not the tautology failing; it is plumbing, which is exactly why the
plan's two-reason FP-safety argument ("adding a guard makes a rule fire *less* often") does not
cover it: `absorb_roles` **removes** the guard.

Note the same shape arises from an ordinary `A ⊑ ∀r.¬C` axiom, whose only non-trigger disjunct
*is* `All(r,¬C)`: today that becomes a `RoleRule` at `absorb.rs:195-200`; under the plan the disjunct
is consumed as a guard, the conclusion collapses to `Bot` via `pool.or([])`, and the ∀-propagation
`RoleRule` **disappears**. That direction is a MISS, not an FP, but it means the plan's pass must
also decide whether `All(r, Not(Atomic))` disjuncts that are the axiom's *consequent* are eligible
at all — and `GuardManufacturability::classify` cannot tell consequent from negated-antecedent
(`crates/owl-dl-core/src/residual_absorbability.rs:161-163` matches `All(_, inner)` structurally),
so the census population includes both.

**Required change:** `absorb_roles` must refuse to convert a `ConceptRule` with a non-empty `guards`
(keep it in `concept_rules`), or `RoleRule` must gain the same extra-guard vector and
`apply_role_rules` must test it. Add a canary on the `A ⊓ ∃r.F ⊑ ∀s.G` shape and sabotage it by
deleting the refusal.

### B2. `build_told_super_closure` reads guarded atomic conclusions as UNCONDITIONAL told subsumptions. Its own doc comment states the invariant the plan breaks.

The plan's transformation turns a disjunctive-conclusion rule into an **atomic**-conclusion rule in
the commonest tier-A case. `absorb_gci`'s `pool.or(rest)` collapses a singleton to the operand
(`crates/owl-dl-core/src/absorb.rs:366-370` and the comment at `:365`, "Or normalizations handle
empty (→ Bot), single (→ operand)"), so `D ≡ A ⊓ ∃r.F` goes from

```
ConceptRule { trigger: A, conclusion: Or([All(r,¬F), Atomic(D)]) }     (today — an Or)
```
to
```
ConceptRule { trigger: A, guards: [Some(r,F)], conclusion: Atomic(D) } (plan — ATOMIC)
```

and `Atomic` conclusions are harvested as told subsumptions:

```rust
// crates/owl-dl-tableau/src/lib.rs:772-787
for rule in &tbox.concept_rules {
    if matches!(self.pool.get(rule.conclusion), ConceptExpr::Atomic(_)) {
        direct.entry(rule.trigger).or_default().push(rule.conclusion);
```

whose consumer states the soundness argument that the change invalidates, verbatim:

> "Soundness: an atomic `ConceptRule` (`trigger ClassId → Atomic super`) is an **unconditional** told
> subsumption, so a node carrying `C` is in every model also a `body`" — `lib.rs:740-746`

Consumer chain: `node_entails_filler` (`lib.rs:748-768`) → `apply_min` (`rules.rs:852`). An
`A`-labelled neighbour is counted as a `D`-witness for `≥n r.D` without `∃r.F` holding, generation is
suppressed, and the witnesses are then **marked pairwise distinct** (`rules.rs:884-890`). Unjustified
distinctness marks are what `apply_max` turns into `Bot` when every pair in an over-count set is
distinct (`rules.rs:915-925`, step 5) — i.e. **a spurious clash from an unentailed premise: an FP.**
I did not construct the end-to-end fixture, so call the FP **plausible-not-demonstrated**; the broken
invariant is demonstrated.

**Required change:** `build_told_super_closure` must skip rules with non-empty `guards`. Same audit
for `has_pending_nominal_disjunction` (`lib.rs:1005-1022`), which also reads
`concept_rules_by_trigger` / `concept_rules` and would treat an *unfireable* guarded `Or` as a
pending nominal disjunction, deferring `apply_exists` (`rules.rs:722-724`). Latent only —
`RUSTDL_NOMINAL_FIRST` defaults OFF — but it is a second silent drift site.

### B3. The plan's code sketch is ill-typed, and the naive repair is UNSOUND for an inverse-role conjunct.

The plan writes, three times (plan:11-12, :66, :159):

```rust
RoleRule { role: Role::Inverse(r), guard: Some(F), target_label: pool.some(r, F) }
```

`Role` is `enum Role { Named(RoleId), Inverse(RoleId) }` (`crates/owl-dl-core/src/ir.rs:76-79`), so
`Role::Inverse(r)` requires `r: RoleId`. But the conjunct's role is a `Role`, and it can already be
`Role::Inverse(p)` — `ObjectSomeValuesFrom(ObjectInverseOf(:p) :F)` is legal OWL 2 DL, and the
census's tier-A predicate **ignores the role entirely** (`residual_absorbability.rs:161`,
`ConceptExpr::All(_, inner)`), so such disjuncts are inside the counted population.

For a conjunct `∃p⁻.F` the marker rule must be `role.flip()` = `Role::Named(p)`
(`ir.rs:110-115`). Emitting `Role::Inverse(p)` instead makes the rule fire on the F-node's **in**-`p`
edges, adding `∃p⁻.F` to the node `y` with `y —p→ x`. But `y ⊨ ∃p⁻.F` requires a `z ∈ F` with
`z —p→ y`, which is **not what was observed**. The label is unentailed, `apply_exists`
(`rules.rs:764-767`, `Role::Inverse(r) => ctx.new_predecessor_with_deps(…)`) then *generates* a
p-predecessor seeded with `F`, and any clash that follows is an **FP**.

**Required change:** use `role.flip()`, not `Role::Inverse(role_id)`. Add a canary whose body
conjunct is written with `ObjectInverseOf`. Also: the census reports no named-vs-inverse role split,
so the plan cannot presently state what share of its tier-A population this is — **cannot verify**;
add the column before quoting the population.

### B4. Task 0 Step 4's acceptance criterion states the required direction BACKWARDS.

Plan Step 4 asks the worker to confirm that the rule "fires on an `r`-edge **from** the guarded node
**to** the labelled one, in the direction this design needs."

The design needs the opposite: the guard `F` sits on the r-**successor**, and `∃r.F` is labelled on
the r-**predecessor**, so the edge runs **from the labelled node to the guarded node**. The code:

- guard presence is computed from the node `apply_role_rules` is called at
  (`crates/owl-dl-tableau/src/rules.rs:333-343`, `guards_present` from `n.labels`);
- edges are enumerated at that same node (`:346-359`);
- `target_label` is added to the **neighbour** (`:393-397`, `ctx.add_label_with_deps(target, c, …)`).

So for `RoleRule{ role: Inverse(r), guard: F }` the in-edge `y —r→ x` at the guarded node `x` puts
the label on `y`. Correct behaviour, backwards description. A worker verifying the plan's sentence
literally will find it false and either declare the design unimplementable or "fix" it — and the
obvious fix is `Role::Named(r)`, which the plan's own Step 8 correctly expects to fail.

The doc comment the plan leans on is also wrong: `absorb.rs:158-161` says "the role expression to
match against an edge incident on **the labelled node**" — edges are matched at the *guarded* node,
not the labelled one. The plan cites this comment as its evidence (plan:66); the comment does not
establish the property, the code does.

**Required change:** restate Step 4 with the correct direction, and fix `absorb.rs:158-161`.

### B5. "Keep `concept_rules_by_trigger` … the index does not change" is false; both hot paths cannot see a guard.

```rust
// crates/owl-dl-core/src/absorb.rs:94
pub concept_rules_by_trigger: HashMap<ClassId, Vec<ConceptId>>,
```

The value is the **conclusion only**. Both firing sites iterate it as bare `ConceptId`s with no route
back to the rule: `apply_concept_rules` at `rules.rs:222-231` (`for &c in conclusions`) and
`apply_deferred_concept_or_rules` at `rules.rs:601-612`. Guards are therefore unreachable on the
indexed (i.e. production) path; only the linear fallback — reached solely when `finalize()` was never
called, i.e. hand-built unit-test TBoxes (`rules.rs:200-212`, `:582-599`) — has the rule in hand.

So Task 1 Step 1 understates the change: the index value type must become `(ConceptId, guards)` or an
index into `concept_rules`, in the function the project has already tuned twice (Phase 3d,
`rules.rs:575-581`). Two further mechanical consequences the plan does not mention:

- `ConceptRule` derives **`Copy`** (`absorb.rs:144`); a `SmallVec` field removes it.
- `TBoxStats.concept_rules` / `concept_rule_or_count` (`crates/owl-dl-reasoner/src/lib.rs:4767`,
  `:4785-4789`) and `residual_absorbability.rs:464-478` count `Or` conclusions from a TBox they build
  by calling `absorb` themselves — so with the flag ON these instruments measure the
  *post*-transformation TBox and `conclusion_is_or` collapses. Task 2 Step 1's use of
  `rustdl residual-absorbability` must pin the flag OFF, and the committed census TSV is not
  comparable flag-ON.

### B6. R1's B3 is unaddressed, and it is the falsifier for tier A specifically.

The wedge already performs a **stronger** version of exactly this transformation for the tier-A shape.
`encode_antecedent` accepts `ConceptExpr::Some`:

```rust
// crates/owl-dl-core/src/clause.rs:543-559
ConceptExpr::Some(role, inner) => {
    let y = self.fresh_var();
    …  let mut body = vec![Atom::Role(role, var, y)];  body.extend(alt);
```

and `absorb_hard_antecedent` puts every soft conjunct into the clause **body**
(`clause.rs:441-491`). So `A ⊓ ∃r.F ⊑ D` is already the Horn clause
`A(X) ∧ r(X,y) ∧ F(y) → D(X)` in the wedge — a real edge join, no marker, no disjunction, strictly
stronger than a guard that requires the marker to have been materialised. And the wedge still burns
373,919 branches on `KetoneGroup` (`docs/2026-08-04-ore-10019-rootcause.md:92-98`).

The plan's comparison table (plan:27-33) presents the mechanism as new relative to rustdl and never
states what each engine already has. R1 required exactly this ("the plan must state, for each engine,
what absorption already exists"). It is still missing.

**Required change:** a per-engine table of existing absorption, plus an explicit argument for why a
*weaker* form in the main tableau rescues ontologies the *stronger* form in the wedge does not.

### B7. The addressable set is again asserted from a static count, and a 20-minute pre-check contradicts it.

This is the same failure mode that retracted the predecessor plan, in a larger denominator. The
census warns about it in its own §4 ("a static count of an absorbed TBox, not a measurement of
reasoning") and the plan quotes the warning in Task 3's `0 recover` branch — but does not act on it
*before* Tasks 1–2.

I ran the cheapest possible addressability probe: `classify --pair-timeout-ms 1`, which bounds
**exactly the phase this lever improves** (per-pair main-tableau/wedge search) to ~zero, while
leaving `saturate`/`prepare`/`label_cache_build` unbounded (Phase 8 decoupled the label-cache
deadline from `per_pair_timeout`). 60 s cap, `target/release/rustdl` (built Aug 4 20:00), **run
strictly sequentially** on the 32-core host, default threading:

| tier-A-only target | `or` rules | `--pair-timeout-ms 1` | wall breakdown (ms) |
|---|---:|---|---|
| `ore_ont_4412` | 642 | **ok 46.5 s** | saturate 3820, precheck 896, prepare 9229, **label_cache_build 13843**, tier_walk 154, **sweeps 11114** |
| `ore_ont_8194` | 315 | **ok 47 s** | saturate 2849, precheck 186, prepare 6294, **label_cache_build 32179**, sweeps 1296 |
| `ore_ont_2111` | 1173 | **ok 40 s** | saturate 2008, precheck 427, prepare 4496, **label_cache_build 12094**, **sweeps 15065** |
| `ore_ont_11311` | 1877 | **DNF 60 s** | — |
| `ore_ont_239` | 1877 | **DNF 60 s** | — |
| `ore_ont_9739` | 1877 | **DNF 60 s** | — |
| `ore_ont_9944` | 2110 | **DNF 60 s** | — |
| `ore_ont_11037` | 3046 | **DNF 60 s** | — |
| `ore_ont_13846` | 281 | **DNF 60 s** | — |

**6 of 9 DNF with per-pair search capped at 1 ms.** No amount of per-pair branch reduction rescues
those six at a 60 s cap — the cost is elsewhere. The three that do complete sit at 40–47 s of
work the lever cannot touch: `label_cache_build` is the wedge's per-class satisfiability
(`crates/owl-dl-reasoner/src/classify.rs:2316`), built from clauses, **not** from `AbsorbedTBox`;
`prepare` is `PreparedOntology::from_internal`, which the lever makes *more* expensive (it adds up to
one `RoleRule` per distinct `(r,F)`); `saturate` is the EL saturator. On `ore_ont_8194`
`label_cache_build` alone is **32.2 s of the 60 s cap**.

And `ore_ont_4412`'s banner reads:

```
# classes: 60972
# mode: hybrid (saturation + tableau)
# fragment: Horn (trust_sat sound by construction; hyper Horn fixpoint is complete)
```

`fragment: Horn` means `trust_sat` is sound by construction and the **wedge** answers the pairs — the
engine whose absorbed TBox the plan is not editing.

Threats to validity, stated: my runs are default-threaded, whereas the harness's tail list is a
single-thread 60 s cap, so these floors are **optimistic** relative to the baseline that defined the
tail. A companion 6-way-concurrent arm read 18/18 DNF and is **not** usable — `ore_ont_4412` DNF'd
there and completes in 46.5 s serially, so contention roughly halves throughput; I report only the
sequential rows. I measured 9 of the 18, not all 18.

**Required change:** run this pre-check over all 18 tier-A-only targets **before** Task 1, serially,
single-thread to match the baseline; exclude every target that DNFs at 1 ms/pair from the gated set;
re-derive Task 3's threshold on the surviving denominator. If the surviving set is small, the plan
needs a different target population or a different lever — and that conclusion costs ~20 minutes
instead of an implementation.

---

## Concerns

### C1. Blocking is not analysed at all, and it is the mechanism that killed the predecessor.

The plan's only graph-size risk is the `∃`-rule (Task 0). Injecting `∃r.F` into labels also perturbs
**blocking**, which is a subset test over label sets:

- conditions (3) `L(y) ⊆ L(x')` and (4) `L(parent(y)) ⊆ L(parent(x'))`, `lib.rs:1120-1200`
  (`is_subset_sorted` at `:1187`, `:1191`) with a `label_sig` bloom prefilter at `:1174-1175`;
- **every label added to `y` can only make `y` harder to block.**

`apply_role_rules` has **no `is_blocked` gate** — contrast `apply_exists` (`rules.rs:713`),
`apply_min` (`:795-800`), `apply_max`, `apply_role_chains` (`:1172`), `apply_self_restriction`
(`:1338`). So markers land on blocked nodes too and can un-block them, resuming generation.

The direction is genuinely non-monotone: markers land on r-**predecessors**, which for
forward-generated trees are ancestors (larger `L(x')` ⇒ *helps* blocking) but for
`new_predecessor` (inverse `∃`, `lib.rs:1085-1095`) are freshly-created children (*hurts*). Net
effect **cannot be predicted**; it must be measured, and Task 0's 3-axiom probe with an
already-witnessed successor cannot see it.

**Required addition:** a Task 0 probe with a generating `∃`-cycle over a role with a declared inverse,
reporting node count plus the `is_blocked_true` / `is_blocked_prefilter_rejects` /
`is_blocked_subset_scans` counters (`crates/owl-dl-tableau/src/counters.rs`), flag ON vs OFF.

### C2. `apply_role_rules` throughput is unpriced, and the project has already reverted one change here.

Phase 3e reverted edge-keyed role-rule indexing at +2.34% GALEN wall (CLAUDE.md; `docs/phase3e-results.md`).
The current implementation re-walks the node's entire edge list **per rule** (`matching_edges`,
`rules.rs:346-359`, invoked inside the per-guard loop at `:368-376`) and clones a `DepSet` per
matching edge. The plan's largest target has 15,928 qualifying rules; after the plan's `(r,F)` dedup
the real count is the number of **distinct `(r,F)` pairs**, which the census does not report —
**cannot verify**. Report it per target before implementing, and treat `prepare` + `apply_role_rules`
as a cost the lever must pay back out of `sweeps`.

### C3. Task 0's headline risk is PROVABLY absent — the plan should argue it, not measure it.

The marker `pool.some(r, F)` is only ever derived because an edge satisfied
`edge_satisfies(seen, wanted)` (`lib.rs:941-952`) for the marker's role. `apply_exists`'s witness
test uses **the same predicate on the same edge set** (`rules.rs:747-756`) before generating. So
whenever the marker is present *because the rule fired*, the witness check succeeds and nothing is
generated. Sub-role and declared-inverse cases both go through the same predicate.

Both arrival orders are also covered: `add_edge_inner` dirties **both** endpoints
(`lib.rs:1401-1404`), and `add_label_with_deps` dirties the labelled node (`lib.rs:1358`), so the
marker rule re-fires whether the edge or the `F` label arrives second.

The residual case is a marker present for a *different* reason — an asserted or ⇒-derived `∃r.F` —
which already generates today. So Task 0 as scoped is a near-certain pass, its budget should go to
C1, and the plan should not present "Task 0 can end this plan" as its cheapest exit when the cheapest
exit is B7.

### C4. Completeness: the plan owns two of at least six derivation gaps, and the two it owns are the two that work.

The guard is derived from a *syntactic* r-edge to an `Atomic(F)`-labelled node. Cases where `∃r.F`
holds without one:

| case | status |
|---|---|
| sub-role edge (`r ⊑ s`, conjunct `∃s.F`) | **works** — `edge_satisfies` → `is_sub_role` on the same polarity axis (`lib.rs:944-948`). Plan Step 5a. |
| declared inverse / symmetric | **works** — cross-polarity branch → `are_declared_inverses` (`lib.rs:950`). Plan Step 5b. |
| transitive role / role chain | **works, with a caveat** — `apply_role_chains` **materialises** the implied edge (`rules.rs:1290-1292`), so the marker fires. But it is skipped at blocked nodes (`:1172`). Not in the plan. |
| `ObjectHasSelf` | `apply_self_restriction` ensures a self-edge (`rules.rs:1340+`), which should yield the marker at the same node — **cannot verify** (not tested). Not in the plan. |
| `≤n` merge | after `merge_into` the marker must survive on the survivor and the redirected `in_edges` must still match — **cannot verify**. Not in the plan. |
| blocked node / loop-back model | see below. Not in the plan. |

On the **blocked-node** case: in the loop-back model a blocked `y` inherits its blocker's successors,
so `y ⊨ ∃r.F` can hold in the model while `y` has no edge and therefore no marker — the guarded rule
never fires at `y` and the completion may not actually be a model. That is MISS-shaped, not FP-shaped
(a spurious `Sat` ⇒ "not subsumed"), and today's disjunctive form arguably has the same exposure
(`apply_deferred_concept_or_rules` has no `is_blocked` gate either, `rules.rs:546-556`). But pair
blocking's soundness argument assumes every universal consequence reaches the blocked node **by label
inclusion**, and a guard derived from edges is not a label-inclusion consequence. **I cannot resolve
this rigorously here.** It needs a written argument in the plan, not a test.

The plan should also *claim* a positive it currently doesn't: because the marker is the interned
`∃r.F` itself, the guard is equally satisfied by an `∃r.F` arriving from the ⇒-direction of a defined
class or from an asserted axiom, with no edge at all. That is strictly wider than "an edge exists"
and is half the completeness argument.

**Net answer to review question C:** relative to today, the change is completeness-**neutral** for
the sub-role, inverse, transitive-chain and disjunctively-derived-`F` cases (each either works or was
already lost); it is **unresolved** for blocked/loop-back nodes and merged nodes; and it introduces
one *new* loss the plan has not noticed — B1's disappearing `∀`-propagation `RoleRule` when the
removed disjunct was the axiom's consequent rather than a negated antecedent conjunct.

### C5. D10 exposure: the fragment gates are safe; the `incomplete: false` surface is not.

I checked and could not construct a gate flip. `is_pure_el` / `saturator_complete_fragment` /
`is_el_axiom` / `analyze_fragment` all read `InternalOntology.axioms`; the fast paths run the **EL
saturator**, which never consumes `AbsorbedTBox`. Since the pass edits only `AbsorbedTBox`, no gate
can newly certify a closure this change perturbs. **This is a genuine safety property and the plan
should record it.**

The real D10 exposure is different: a marker that is silently not derived yields a spurious `Sat` →
a pair reported *decided* `NotSubsumed` with `incomplete: false` — wrong answer plus a false
completeness claim, which is the bug class's signature. Task 2 Step 5 covers two of the six rows in
C4's table and none of B1/B2/B3. **Step 5 is not sufficient**; extend it row by row, one canary each.

### C6. Task 3's decision rule has a dead branch, a wrong denominator, and a wrong FP diagnosis.

- **The tier-partition branch cannot fire from the mechanism the plan describes.** Tier grouping keys
  on `subsumer_counts[i] = closure.subsumers_count(…)` — the **EL saturation closure**
  (`crates/owl-dl-reasoner/src/classify.rs:2409-2413`, tiers at `:2437-2454`). This pass does not
  touch the closure and mints no class, so raw subsumer counts are unchanged **by construction**.
  R1's B7 described a mechanism specific to *minting a class*; the plan copied the text without
  noticing its own design removes the mechanism. (Tiers can still shift *downstream* of an answer
  change, via `unsatisfiable_idxs` at `:2395-2401` filtering `order`, but that is second-order and is
  not what the branch says.) **Rewrite the branch or delete it.**
- **"≥6 of the 17"** — the denominator is 18, not 17 (see G1), and it is unconditioned: it includes
  targets no per-pair mechanism can rescue (B7). Threshold on the pre-checked addressable subset.
- **"Any FP ⇒ the tautology argument fails and that is a design error"** is wrong. B1, B2 and B3 are
  three FP routes that have nothing to do with the tautology. The rule must be: *any FP ⇒ determine
  which of {tautology, a guard-dropping consumer, marker role polarity} failed.*

### C7. The sabotage list is good but does not cover the FP routes it most needs to.

Task 2 Step 8's three sabotages (emit-without-removing = control; remove-without-emitting = must lose
entailments; `Named` instead of `Inverse` = must fail) are well chosen. Missing, and each is a
one-line mutation of an FP route: (a) let `absorb_roles` convert a guarded rule (B1); (b) let
`build_told_super_closure` accept a guarded rule (B2); (c) emit `Role::Inverse(role_id)` for an
inverse-role conjunct (B3); (d) drop the guard test in only **one** of the two firing sites
(`apply_concept_rules` vs `apply_deferred_concept_or_rules`) — they are independent code and will
drift.

### C8. Task 1 Step 3's inertness gate is the right gate, but its evidential value should be stated.

With no producer of guards the workspace must be byte-identical, so `run-soundness-diff.sh` returning
11 VERIFIED proves the field is inert — that is a valid use, and the plan is right to say so. It is
**not** evidence about the flag-ON path, and R1's C4 measured that 7 of 8 curated fixtures cannot fire
absorption work at all. Task 2 Step 9's flag-ON FP=0 net inherits that: report the
per-fixture firing split (`residual-absorbability` with the flag **OFF**, per B5) alongside the
VERIFIED count, or the gate is an inertness check wearing a soundness label.

---

## Confirmations — verified correct, do not re-litigate

1. **`F ⊑ ∀r⁻.(∃r.F)` IS a tautology in SROIQ, unconditionally.** For any `d ∈ F^I` and any `e` with
   `(d,e) ∈ (r⁻)^I` — i.e. `(e,d) ∈ r^I` — `e` has an `r`-successor `d ∈ F^I`, so `e ∈ (∃r.F)^I`.
   Holds for self-loops (`e = d`), transitive `r`, symmetric `r`, unsatisfiable `F` (vacuous),
   nominal-labelled nodes, and inverse-derived edges. So the plan's FP-safety-by-tautology half is
   **correct**, and every FP route I found is plumbing, not logic.
2. **No class is minted and no `ClassId` grows.** `RoleRule.target_label` is a `ConceptId`
   (`absorb.rs:164`) and `pool.some(r, pool.atomic(F))` interns a `ConceptExpr::Some` in the
   `ConceptPool`, not the `Vocabulary`. R1's B5 (mint site / `absorb` signature), B6
   (`reportable_class_iris`, `realize.rs:626/914`, `convert_back.rs`, `json_out.rs` panics), B7's
   suffix invariant, C6 (digest corruption), C9 (`num_total_classes²` RSS) and C10
   (`justify`/`repair` unfolding) **genuinely do not apply.** This is the plan's real advance over its
   predecessor and it is correctly identified as load-bearing.
3. **The `Role::Inverse` firing direction is what the design needs, for a `Named` conjunct role** —
   guard at the F-node, in-edge matched via `edge_satisfies(Role::Inverse(edge_role), Role::Inverse(r))`,
   label added to the neighbour (`rules.rs:333-343`, `:353-358`, `:393-397`; `lib.rs:941-952`).
   (Half (i) of the review's question A is behaviourally CORRECT — the plan's *description* of it is
   not; see B4.)
4. **`∃r.F` is internable with zero signature growth** — question A(ii) answered yes; `ConceptPool`
   interning is the mechanism, and it makes the marker share an id with any pre-existing `∃r.F`.
5. **The `∃`-rule does check for an existing witness before generating**, syntactically and
   sub-role/inverse-aware (`rules.rs:743-758`). Task 0's Step 1 will find this.
6. **`add_edge_inner` dirties both endpoints** (`lib.rs:1401-1404`); `add_label_with_deps` dirties the
   labelled node (`lib.rs:1358`). Marker derivation is order-independent.
7. **Singleton-`Or` collapse makes the tier-A rule fully deterministic** — better than Konclude's
   measured 4-way head, and worth stating as a design benefit (it is also what triggers B1/B2).
8. **The census reproduces on this binary.** `ore_ont_10019`: `conclusion_is_or 29`,
   `extra ¬Atomic 0`, tierA 15 / tierB-only 2 / tierC-only 9 / any 26, `max_rules_per_trigger 10`.
   Targets: `3575` 15928/15928 max 4704; `11037` 3046/3046 max 850; `13846` 281/281 max 16;
   `4412` 642/642 max 78. All exact.
9. **"26 → 18 if tier C is never built" IS what the census says** — census §4 threats
   ("the tail headline drops from 26 to 18 and the pool from 208 to 188") and §3.4 ("tier A only 18").
   Question G's suspicion is unfounded here; the plan quotes this correctly.
10. **`search.rs:502-503` is quoted correctly** (as R1 also found): the open-disjunction test is the
    purely syntactic `!args.iter().any(|d| labels.binary_search(d).is_ok())`. A marker present as a
    disjunct will close such an `Or` — which is *sound* (the disjunct is genuinely entailed) and is a
    behaviour change worth noting, not a defect.
11. **The wedge and its clause-index amortisation are untouched.** `clausify` takes an
    `&InternalOntology` (`clause.rs:834`, `:873`) and every caller passes `internal`
    (`crates/owl-dl-reasoner/src/lib.rs:658, 728, 1033, 3119, 4324, 4547`), so the wedge never
    consumes `AbsorbedTBox` despite the stale module doc at `clause.rs:23`.
    `RUSTDL_CLASSIFY_AMORTIZE_IDX` / `RUSTDL_CLASSIFY_LABELS_AMORTIZE` operate on clause indexes and
    **do not break**. That is the answer to review question E — and it is also why B6/B7 bite: the
    lever cannot reach the engine that is doing the work on the measured targets.

---

## Errors of fact (question G)

- **G1. "the 17 tier-A-only targets" is 18.** Counting `C == 0` rows in census §3.5 gives **18**
  (`11495, 4669, 7127, 7581, 7956, 3575, 5218, 11745, 11037, 8194, 2111, 6608, 9944, 11311, 239,
  9739, 4412, 13846`), and census §3.4 independently reports "tier A only 18". The census's own
  sentence "17 of these 26 need tier A only" is an arithmetic error; the plan inherited it into
  Task 2 Step 6 and into Task 3's acceptance threshold.
- **G2. "Per the census this is ~98% of corpus volume" (Task 2 preamble) overstates it.** The census
  says tier A is ~98% of the **manufacturable** volume (827,201 of 841,355 covered pool rules). As a
  share of all `Or`-conclusion rules it is **78.5%** (827,201 / 1,054,027) on the pool and **91.4%**
  (592,840 / 648,373) on the tail.
- **G3. `absorb.rs:158-161` does not say what the plan says it says.** It says the role matches "an
  edge incident on **the labelled node**"; edges are matched at the **guarded** node. The plan uses
  this comment as its evidence for the whole "minting nothing" argument (plan:66). The property is
  true; the citation is not the reason.
- **G4. The `reportable_class_iris` parenthetical is inverted.** Plan:69-70 reads as though "no
  leak" follows from `realize.rs:626`/`:914` having no filter. Those two sites are R1's evidence for
  why a filter *would be needed if a class were minted*; the actual reason there is no leak is
  Confirmation 2. Cosmetic, but this is the kind of inverted reasoning that gets quoted later as
  license.
- **G5. The comparison table's "addressable set on the tail: 26" is a static count relabelled as an
  addressable set** — the exact substitution the census's §4 forbids and the predecessor plan was
  retracted for. B7's measurement is the correction.

---

## The single highest-value finding

**Run `classify --pair-timeout-ms 1` over the 18 tier-A-only targets before writing any code.** It
caps the one phase this lever improves at ~zero, so it is a direct upper bound on what the lever can
buy. On the 9 I measured sequentially, **6 DNF at 60 s** with per-pair search already eliminated, and
the 3 that finish do so at **40–47 s** of work the lever cannot reach — `label_cache_build` (the
**wedge's** per-class satisfiability: 32.2 s of `ore_ont_8194`'s 60 s budget), `prepare` (which this
lever makes *more* expensive), and `saturate`. `ore_ont_4412`'s banner reads
`# fragment: Horn (trust_sat sound by construction; hyper Horn fixpoint is complete)`, i.e. the
**wedge** answers its pairs — and the wedge already puts `∃r.F` conjuncts in clause **bodies** as a
real edge join (`clause.rs:543-559`, `:441-491`), a strictly stronger form of the plan's mechanism,
and still stalls.

The plan's mechanism may well be correct. Its **target population is the unverified part**, exactly
as last time, and this check costs ~20 minutes against an implementation plus two corpus gates.

Second-highest, because it is a soundness defect rather than a value one: **B1** — adding a `guards`
field to `ConceptRule` without touching `absorb_roles` (`absorb.rs:191-205`) converts a guarded rule
into an **unguarded** `RoleRule`, a rule strictly stronger than the axiom it came from, and therefore
an FP. `build_told_super_closure` (`lib.rs:772-787`) is the same defect a second time, against a doc
comment that names the invariant being broken.
