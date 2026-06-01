# Phase 2c — fix target

> **Naming note.** The Phase 2c plan is filed under "functional-role +
> covering"; the diagnosis on which it was based
> (`docs/phase2c-galen-diagnosis.md` Option 3) hypothesised that the
> entailment requires a *covering* axiom (`Z ⊑ A ⊔ B`) or a disjointness
> between target classes. T3's inspection of pair_06 shows that
> hypothesis is wrong: pair_06.ofn contains **zero** disjointness or
> covering axioms (`grep -c Disjoint pair_06.ofn` = 0), yet HermiT
> derives the target subsumption. The actual gap is the
> **functional-property witness-coincidence** step — Phase 2a's
> witness-merge rule with the conclusion propagated back to the
> sub-roles X already has facts on, instead of only to the functional
> super-role. The plan's name is kept for traceability; the rule itself
> needs no covering / disjointness premise. See
> `/tmp/p2c-absorbed-pair06.txt` for the absorbed-TBox dump and
> per-class subsumer closure that drove this finding.

## Absorbed-TBox shape (pair_06 inspection)

The pair_06 saturator-only verdict was inspected by an in-tree throwaway
test (`crates/owl-dl-reasoner/tests/phase2c_debug_inspect_pair_06.rs`,
NOT committed; removed before the T3 commit) that ran
`owl_dl_core::convert::convert_ontology` → `nnf_axioms` →
`owl_dl_core::absorb::absorb`, then dumped `AbsorbedTBox` plus the
saturator's subsumer closure for the IRIs in the
IneffectiveCardiacFunction triangle. The relevant `AbsorbedTBox` fields
hold the data described here (see
[`crates/owl-dl-core/src/absorb.rs`](../crates/owl-dl-core/src/absorb.rs)
for type definitions):

| Field | Role |
|---|---|
| `concept_rules: Vec<ConceptRule { trigger: ClassId, conclusion: ConceptId }>` | Atomic-trigger rules `A ⊑ ψ` extracted from absorbing `⊤ ⊑ ¬A ⊔ ψ`. |
| `residual_gcis: Vec<ConceptId>` | `⊤ ⊑ φ` that couldn't extract a class trigger; applied universally. |
| `role_rules: Vec<RoleRule>` | Conclusions of shape `∀R.D` lifted into edge-fire form. |
| `nominal_rules`, `deferred_or_residuals` | Not relevant for pair_06. |

### How the line-1235 GCI absorbs

```
SubClassOf(
  ObjectIntersectionOf(
    :BodyProcess
    ObjectSomeValuesFrom(:hasEffectiveness
      ObjectIntersectionOf(:Effectiveness ObjectSomeValuesFrom(:hasState :ineffective)))
    ObjectSomeValuesFrom(:hasIntrinsicPathologicalStatus :physiological))
  ObjectSomeValuesFrom(:hasPathologicalStatus :pathological))
```

After NNF + absorption, this becomes a single `ConceptRule` whose
trigger is `:BodyProcess` (the first `Not(Atomic)` disjunct of the
encoded `⊤ ⊑ ¬LHS ⊔ RHS`) and whose conclusion is the disjunction of
all remaining encoded operands (dumped form):

```
trigger=:BodyProcess  conclusion=⊔(
    ∃:hasPathologicalStatus.:pathological,
    ∀:hasIntrinsicPathologicalStatus.¬(:physiological),
    ∀:hasEffectiveness.⊔(¬(:Effectiveness), ∀:hasState.¬(:ineffective)))
```

i.e. a `ConceptExpr::Or` conclusion — the saturator's
`lower_sub_class_of` falls into the `_ => {}` default arm because the
RHS is not atomic / atomic-And / `Some` / `And` of atomic+`Some` and
**this GCI is silently dropped from the saturation rule set**. The
tableau/wedge handles it; the saturator does not.

### What the saturator DOES derive on pair_06

Even with the line-1235 GCI dropped, the saturator closes (relevant
subset for `IneffectiveCardiacFunction`):

```
ICF ⊑ :BodyProcess
ICF ⊑ :CardiacFunction
ICF ⊑ :NAMEDCirculatoryProcess
ICF ⊑ :NAMEDPhysiologicalProcess
ICF ⊑ :PathologicalBodyProcess         ← derived; PBP ≡ BodyProcess ⊓ ∃hasPathologicalStatus.pathological
ICF ⊑ :PathologicalCondition           ← derived; PathCond ≡ DomainCategory ⊓ ∃hasPathologicalStatus.pathological
ICF ⊑ :DomainCategory, :Process, :TopCategory
```

`:IntrinsicallyPathologicalBodyProcess` is **absent** from ICF's
closure. The saturator does derive `(ICF, hasPathologicalStatus,
pathological)` (because the wedge has separately derived
`∃hasPathologicalStatus.pathological` for ICF — likely through the
GCI on line 1236, `BodyProcess ⊓ ∃hasIntrinsicPathologicalStatus.pathological ⊑ ∃hasPathologicalStatus.pathological`,
chained from one of the IntrinsicallyPathological* equivalences — but
NOT `(ICF, hasIntrinsicPathologicalStatus, pathological)`).

The full T3 dump is at `/tmp/p2c-absorbed-pair06.txt` (133 lines).

### How `IPBP`'s defining equivalence absorbs

```
trigger=:IntrinsicallyPathologicalBodyProcess
        conclusion=⊓(:BodyProcess, ∃:hasIntrinsicPathologicalStatus.:pathological)
trigger=:BodyProcess
        conclusion=⊔(:IntrinsicallyPathologicalBodyProcess,
                     ∀:hasIntrinsicPathologicalStatus.¬(:pathological))
```

The forward direction is the textbook `EquivalentClasses → SubClassOf
[A, B]` decomposition. The saturator's `lower_sub_class_of` will only
collect `IPBP ⊑ ∃hasIntrinsicPathologicalStatus.pathological` as an
`ExistentialFact` and `IPBP ⊑ BodyProcess` as an `AtomicSubsumption`.
The **reverse** direction (`BodyProcess ⊓ ∃hasIntrinsicPathologicalStatus.pathological ⊑ IPBP`)
becomes an `ExistentialTrigger { role: hasIntrinsicPathologicalStatus,
body: pathological, head: IPBP }` paired with a `ConjunctiveTrigger
{ bodies: [BodyProcess, marker(hasIntrinsicPathologicalStatus,
pathological)], head: IPBP }`. So if ICF ever gains a
`(ICF, hasIntrinsicPathologicalStatus, pathological)` existential
fact AND has BodyProcess as a told super, the IPBP entailment fires
mechanically.

**The Phase 2c rule's job is to materialise the missing
`(ICF, hasIntrinsicPathologicalStatus, pathological)` fact.**

## The HermiT derivation (re-read)

`docs/phase2b-galen-pair-analysis.md` §"Pair 06" frames the derivation
as a non-Horn case-split on the `hasIntrinsicPathologicalStatus` value
of `IneffectiveCardiacFunction`, eliminating the `physiological`
branch via "functional-role sibling collapse + `physiological`/`pathological`
covering or disjointness for `PathologicalOrPhysiologicalStatus`". This
T3 inspection shows that framing is **misleading**: the necessary
inference is purely Horn, doesn't need a case-split, and doesn't need
any disjointness or covering axiom.

The clean derivation (every step a sound EL+ Horn rule):

1. `IneffectiveCardiacFunction ⊑ CardiacFunction` (from its equivalence,
   already in the saturator's closure).
2. `CardiacFunction ⊑ NAMEDCirculatoryProcess ⊑ NAMEDPhysiologicalProcess`
   (lines 641, 868 — `:CardiacFunction → :NAMEDCirculatoryProcess →
   :NAMEDPhysiologicalProcess`, already in the closure).
3. `NAMEDPhysiologicalProcess ⊑ ∃hasIntrinsicPathologicalStatus.physiological`
   (line 879 — already in the closure as an `ExistentialFact`).
4. **Independently:** `IneffectiveCardiacFunction ⊑ ∃hasPathologicalStatus.pathological`
   (already derived by the saturator: ICF ⊑ PathologicalBodyProcess in
   the dump). [Cross-check: this depends on the saturator deriving an
   `∃hasPathologicalStatus.pathological` fact for ICF. The exact
   provenance is via GCIs on lines 1236, 1241, etc. — the line-1235 GCI
   was dropped (Or conclusion), so this fact reaches ICF through the
   secondary chain involving the other Intrinsically*/Pathological*
   equivalences. For Phase 2c, all that matters is that the fact
   exists.]
5. `hasIntrinsicPathologicalStatus ⊑ StatusAttribute`,
   `hasPathologicalStatus ⊑ StatusAttribute`,
   `FunctionalObjectProperty(StatusAttribute)` (lines 372–373, 442–443,
   452–453).
6. **Functional-property witness coincidence (the missing step):**
   facts (3) and (4) each commit ICF to having a
   `StatusAttribute`-witness; functionality of StatusAttribute forces
   them to coincide. That single individual is in both `physiological`
   and `pathological`, hence in `physiological ⊓ pathological`.
7. Since that individual is the `hasIntrinsicPathologicalStatus`-witness:
   `ICF ⊑ ∃hasIntrinsicPathologicalStatus.(physiological ⊓ pathological)`,
   and by ⊓-elim `ICF ⊑ ∃hasIntrinsicPathologicalStatus.pathological`.
8. With BodyProcess (told) plus the new fact (7), the IPBP
   `ExistentialTrigger` fires: `ICF ⊑ IntrinsicallyPathologicalBodyProcess`. ✓

No disjointness, no covering, no case-split. The argument is identical
to Phase 2a's witness-merge rationale — but the **payoff is on the
sub-role, not on the functional super-role**.

### Why Phase 2a doesn't already close this

Phase 2a's witness-merge rule (`crates/owl-dl-saturation/src/lib.rs`
around line 707-760) fires on every new `ExistentialFact (X, R, T)`:
for each functional super `R_f` of `R`, it accumulates `T`'s atomic
content into `merged_atom_sets[(X, R_f)]`, allocates a synthetic
`F ≡ ⊓(accumulated atoms)`, and emits the merged fact
**`(X, R_f, F)`**. On pair_06 this produces `(ICF, StatusAttribute, F)`
where `F ≡ physiological ⊓ pathological ⊓ ...` — sound, but the
downstream triggers fire on `R = hasIntrinsicPathologicalStatus`
(IPBP's defining trigger), not on `R = StatusAttribute`. The merged
fact `(ICF, StatusAttribute, F)` does not feed into IPBP's trigger,
because no axiom says
`∃StatusAttribute.pathological ⊑ IntrinsicallyPathologicalBodyProcess`.

Phase 2c is a **strictly additive** extension: when Phase 2a's merge
grows the accumulator for `(X, R_f)`, also emit the synthetic on
every sub-role `R_k ⊑ R_f` for which X already has an existential
fact.

## Rule design

### Trigger condition (runtime, fact-time)

In the existing witness-merge block of `WorklistEngine::process_fact`
(the "Phase 2a EL++ functional-role witness-merge rule (T4.5 atom-set
redesign)" block), wherever the rule currently emits
`(X, R_f, synthetic)`:

1. The arriving fact is `(X, R_arr, T_arr)`.
2. `funcs = functional_supers_of(R_arr)` — every functional super-role
   of `R_arr` (including `R_arr` itself if it's functional).
3. For each `R_f ∈ funcs`, `merged_atom_sets[(X, R_f)]` is updated as
   in Phase 2a, and the existing rule emits `(X, R_f, synthetic)` when
   the set GREW and was non-empty before (`!was_first && grew`).
4. **Phase 2c addition:** under that same condition, additionally
   iterate every existing fact `(X, R_k, _)` already in
   `facts_by_sub[X]` whose role `R_k` satisfies `R_f ∈
   functional_supers_of(R_k)`, and emit `(X, R_k, synthetic)` for each
   distinct such `R_k`.

The "iterate sibling sub-roles via existing facts" formulation
guarantees the precondition (see Soundness section): we only emit
`(X, R_k, synthetic)` when X provably has at least one `R_k`-witness
(namely, the witness of the fact we found in `facts_by_sub[X]`).

### Soundness preconditions (must hold for every emission)

For the emission `(X, R_k, synthetic_for_atomset_at_(X,R_f))` to be
sound, ALL of the following must hold at emission time:

1. **Functional ancestor:** `R_f ∈ functional_supers_of(R_k)` (i.e.
   `R_k ⊑ R_f` reflexive-transitive AND `R_f` is functional). This is
   inherited from Phase 2a's existing `functional_supers_of(R_arr)`
   precondition; the new condition is the same for `R_k`.
2. **Witness exists on R_k:** X has at least one
   `(X, R_k, _)` fact in `facts_by_sub[X]`. (Without this, the rule
   would claim "X has an `R_k`-witness in `synthetic`" when X has no
   `R_k`-edge at all — unsound: `R_f` being functional only forces
   coincidence of witnesses that exist.)
3. **Atomset captures the witness:** `synthetic` denotes
   `⊓(merged_atom_sets[(X, R_f)])`, i.e. the conjunction of every
   atomic class that some `R_f`-witness of X has been told to be in.
   Phase 2a guarantees this invariant.

The covering / disjointness premise the plan named is **not required**
and is not checked. Pair_06 has no covering / disjointness axioms; the
entailment follows from preconditions 1–3 alone.

### Soundness argument (semantics)

Let X be a class with two existential facts `(X, R_i, A)` and
`(X, R_j, B)` such that `R_i ⊑ R_f` and `R_j ⊑ R_f` with `R_f`
functional. Pick any model `M` and individual `x ∈ M(X)`:

- By fact 1, `x` has an `R_i`-successor `y_i ∈ M(A)`. Since `R_i ⊑
  R_f`, `(x, y_i) ∈ M(R_f)`.
- By fact 2, `x` has an `R_j`-successor `y_j ∈ M(B)`. Since `R_j ⊑
  R_f`, `(x, y_j) ∈ M(R_f)`.
- `R_f` is functional: every `x` has at most one `R_f`-successor.
  Therefore `y_i = y_j`. Call it `y`.
- `y ∈ M(A) ∩ M(B) = M(A ⊓ B)`.
- The successor `y` is reachable from `x` via both `R_i` and `R_j`.
  Therefore `x ∈ M(∃R_i.(A ⊓ B))` and `x ∈ M(∃R_j.(A ⊓ B))`.

Generalising from `{A, B}` to the running `merged_atom_sets[(X, R_f)]`
accumulator: every fact `(X, R_n, T_n)` in the saturator's history
with `R_n ⊑ R_f` contributes its atomic content to the single
`R_f`-witness, so that witness is in `⊓(atomic_content)`. Every
sub-role `R_k ⊑ R_f` on which X has at least one fact reuses that
same witness. ⌷

### Why the "covering" framing was wrong

The Phase 2b.0 trace observed HermiT eliminating a *case-split* over
`hasIntrinsicPathologicalStatus`'s value via apparent
`physiological`/`pathological` covering on
`PathologicalOrPhysiologicalStatus`. That trace was over-interpreted:
HermiT's BRANCHES are an artefact of its hypertableau procedure (it
materialises a witness, then branches over its class membership), not
a logical requirement of the derivation. The same conclusion is
reachable by a single Horn step (the witness-coincidence argument
above) once both `∃R_i.A` and `∃R_j.B` are present. Pair_06's
absence of disjointness axioms is the empirical confirmation: a
covering-based derivation could not run on it, yet HermiT closes the
entailment.

## Pattern detection algorithm (concrete)

The rule lives inside `process_fact` in
`crates/owl-dl-saturation/src/lib.rs`, immediately following the
existing Phase 2a witness-merge emission. Pseudocode:

```text
# After Phase 2a's emission of (X, R_f, synthetic) — same control flow,
# same `synthetic` value:
for each fact_idx in facts_by_sub[X.index()]:
    let other = self.facts[fact_idx]
    if other.role == R_arr:
        continue   # R_arr's emission is already covered by Phase 2a's R_f path
    if !functional_supers_of(other.role).contains(R_f):
        continue   # other.role is not under the same functional super
    let new_fact = ExistentialFact { sub: X, role: other.role, target: synthetic }
    if seen_facts.insert((new_fact.sub, new_fact.role, new_fact.target)):
        push new_fact onto facts / facts_by_sub / facts_by_target / todo_fact
```

The arriving-fact's role `R_arr` is already handled by Phase 2a's
existing emission (which fires on `R_f`, not `R_arr`, but the
downstream effect for `R_arr` is implicit because `R_arr ⊑ R_f` and
the synthetic ≡ `⊓(atomic_content)` is at least `T_arr`'s content).
We exclude `R_arr` from the loop just to avoid the trivially
redundant emission `(X, R_arr, synthetic)` when X already has
`(X, R_arr, T_arr)` and Phase 2a's worklist will redrive it.

(Implementation detail T4 will pin down: whether to also emit on
`R_arr` defensively when `T_arr ⊊ synthetic` atomically. Probably
yes — `synthetic ⊑ T_arr` via atomic_subsumption emitted at
allocation, but propagation requires the (X, R_arr, synthetic) fact
to be seen.)

### Complexity

- Outer trigger: one merge-growth event per `(X, R_f)` per growth-step.
  Phase 2a already bounds this by `|atomic_vocabulary|` per `(X, R_f)`.
- Inner loop: `O(|facts_by_sub[X]|)`. Dominated by other saturator
  passes; not a hot path.
- Soundness check (`functional_supers_of(R_k).contains(R_f)`): O(small)
  via the precomputed dense per-role functional-supers list.

No new data structures. Reuses Phase 2a's `merged_atom_sets`,
`introduce_runtime_synthetic`, and the existing
`functional_supers_of` infrastructure.

## Soundness argument for pair_06 (concrete walkthrough)

Pair_06's saturator-only derivation, post-Phase-2c rule:

1. The saturator's seed + closure derives (already happens, pre-Phase-2c):
   - `(ICF, hasIntrinsicPathologicalStatus, physiological)`. Reached
     via ICF ⊑ CardiacFunction ⊑ NAMEDCirculatoryProcess ⊑
     NAMEDPhysiologicalProcess, then the named-class atomic-existential
     fact `NAMEDPhysiologicalProcess ⊑ ∃hasIntrinsicPathologicalStatus.physiological`.
   - `(ICF, hasPathologicalStatus, pathological)`. Reached via
     ICF ⊑ BodyProcess and one of the lines 1241/1244-style GCIs
     producing `∃hasPathologicalStatus.pathological` for BodyProcess
     instances under specific premises that ICF satisfies. (The exact
     reach is not load-bearing for the Phase 2c argument — Phase 2c's
     rule only requires the fact to exist.)
2. Phase 2a triggers on fact-1's arrival:
   - `functional_supers_of(hasIntrinsicPathologicalStatus) ⊇
     {hasIntrinsicPathologicalStatus, StatusAttribute}` (both
     functional).
   - `merged_atom_sets[(ICF, hasIntrinsicPathologicalStatus)] := {physiological-content}`.
   - `merged_atom_sets[(ICF, StatusAttribute)] := {physiological-content}`.
   - Both `was_first=true` → no emission.
3. Phase 2a triggers on fact-2's arrival:
   - `functional_supers_of(hasPathologicalStatus) ⊇
     {hasPathologicalStatus, StatusAttribute}` (both functional).
   - `merged_atom_sets[(ICF, hasPathologicalStatus)] := {pathological-content}`.
     First, no emission.
   - `merged_atom_sets[(ICF, StatusAttribute)] grows from {physiological-content}
     to {physiological-content ∪ pathological-content}`. `grew=true`,
     `was_first=false`. Emits `(ICF, StatusAttribute, F)` where
     `F ≡ ⊓(physiological-content ∪ pathological-content)`.
4. **Phase 2c addition (new):** at the same emission point, the inner
   loop iterates `facts_by_sub[ICF]`:
   - Finds `(ICF, hasIntrinsicPathologicalStatus, physiological)`.
     Sibling: `functional_supers_of(hasIntrinsicPathologicalStatus)`
     contains `StatusAttribute` ✓ — emits `(ICF, hasIntrinsicPathologicalStatus, F)`.
   - Finds `(ICF, hasPathologicalStatus, pathological)`. Sibling check
     passes — emits `(ICF, hasPathologicalStatus, F)`.
5. The new fact `(ICF, hasIntrinsicPathologicalStatus, F)` propagates
   through `F ⊑ pathological` (an atomic-subsumption emitted at
   synthetic-allocation time, because `pathological ∈ atomic_content(F)`)
   via CR5's target-subsumer propagation: any class that has F as a
   subsumer (only F itself initially, but the saturator's `subsumed_by`
   loop catches this) feeds the existential trigger
   `∃hasIntrinsicPathologicalStatus.pathological ⊑ IPBP`.

   More precisely: the existential-trigger machinery scans
   `subsumers_of(F)`; F's subsumers include `pathological`; the
   trigger `(role=hasIntrinsicPathologicalStatus, body=pathological,
   head=IPBP)` matches → ICF gains `IPBP` as a subsumer.

6. Conjunctive trigger `{BodyProcess, marker(hasIntrinsicPathologicalStatus,
   pathological)} ⊑ IPBP` also fires (ICF has BodyProcess told, and the
   F → marker chain via the new fact arrives).
7. `ICF ⊑ IntrinsicallyPathologicalBodyProcess` lands. `CCF ⊑ ICF` was
   already there; ⊑-transitivity gives `CCF ⊑ IPBP`. ✓

No false-positive surface introduced: every emission is a sound
consequence of functional-property witness coincidence, and the
synthetic `F` already lives in the saturator's class universe with
sound atomic-subsumption clauses (`F ⊑ atomic` for each atomic in
`F`'s body, contributed by `TseitinAllocator::introduce`).

## Expected impact

Per Phase 2c.0 diagnosis (`docs/phase2c-galen-diagnosis.md`):

- **Confident floor: 24 pairs** recovered (the 12 GALEN + 12 notgalen
  pure-cluster-C pairs that match pair_06's shape exactly: two-prop
  functional-merge with the missing sub-role-side fact).
- **Most-likely: 39 pairs**, picking up the notgalen anonymous-super-class
  variants if their shape is the same modulo class-name anonymisation.
- **Upper bound: 44** (the full residual MISSED count).

The Phase 2c rule is **strictly more general** than the Phase 2c.0
plan envisioned (no covering precondition), so the upper bound is
empirically plausible. Phase 2c.0 may also recover pairs the diagnosis
didn't catalogue if they exhibit the witness-coincidence shape on
non-StatusAttribute functional super-roles.

## Soundness on the FP=0 net

The rule emits `(X, R_k, F)` where:
- `F` is a Phase-2a-allocated synthetic with sound atomic-subsumption
  clauses (`F ⊑ atomic` for every atomic in its body, established at
  allocation time by `TseitinAllocator::introduce`).
- `R_k` is a role X already has an existential fact on.
- `R_k`'s reachability via the functional super `R_f` is what makes
  the emission sound (the witness-coincidence argument).

Soundness threats considered and dismissed:

- **`F` could be unsound on a class C ≠ X.** F's only entry into other
  classes' closures is via atomic-subsumption (`F ⊑ a_i`) and existential
  triggers matching `∃R.F` (no such trigger is emitted by Phase 2c;
  the synthetics are heads, not bodies). The fact `(X, R_k, F)` only
  ever propagates `F` as a target subsumer for X's `R_k`-witnesses,
  exactly the semantics asserted by `F ≡ ⊓(atomic_content)`.
- **`functional_supers_of` could be wrong.** Phase 2a established this
  precomputation; canaries exercise the 4-sub-property fan-in case.
  Phase 2c reuses the same data with no extension.
- **`facts_by_sub[X]` could include facts whose role isn't actually
  R_k ⊑ R_f.** Each fact's role is checked explicitly via
  `functional_supers_of(other.role).contains(R_f)`. False positives
  here would mean `functional_supers_of` itself is wrong, contradicting
  Phase 2a.
- **The "guard" precondition fails on synthetics.** Synthetic facts
  emitted by Phase 2a / 2b / 2c also live in `facts_by_sub`. If an
  arriving synthetic fact triggers the inner loop, it iterates all
  facts on X including older synthetic facts — sound, because every
  synthetic fact represents a real witness existence. (Acknowledged
  open question: do synthetic-on-synthetic emissions cascade in a
  way that breaks termination? Phase 2a's atomset-bounded design
  proves termination for the underlying merge; Phase 2c adds at most
  `|facts_by_sub[X]|` emissions per merge step, each itself a fact
  whose target is a synthetic from a bounded set — so total emissions
  per X are bounded by `|atomic_vocabulary|² · |sibling_roles|`.
  Termination held in Phase 2a canaries; T4 will re-verify on the
  3-prop and 4-prop synthetics.)

T5 runs the Phase 0 net (alehif, ORE-10908, ORE-15672) to empirically
confirm FP=0. T4 is gated on the synthetic canary passing.

## What this design does NOT close

- **Pairs where the saturator never derives the second sub-role fact.**
  The rule fires only when `(X, R_i, A)` AND `(X, R_j, B)` both exist
  in the saturator's closure under a common functional super. If
  saturation can't reach the second fact (e.g. it required the
  line-1235-style Or-headed GCI that the saturator drops), the rule
  doesn't fire. Pair 07 (per
  `docs/phase2b-galen-pair-analysis.md` §"Pair 07") may fall into
  this bucket — its R-witness for `Ulcer ⊑ ∃hasIntrinsicPathologicalStatus.pathological`
  depends on a GCI chain whose intermediate facts the saturator may
  not establish.
- **Pairs without a functional super-role.** The rule's premise is
  R_f functional. Pairs whose MISSED step is via non-functional
  role hierarchies need a different rule.
- **Non-witness-coincidence MISSED.** The cluster-D and tail pairs
  in `docs/phase2c-galen-diagnosis.md` that aren't cluster-C are
  out of scope for Phase 2c; Phase 2d (if scope-justified) would
  re-diagnose them.

The 24–44 range from Phase 2c.0 reflects this honestly: the floor
counts only the pure cluster-C pairs we can predict the rule will
match; the upper bound acknowledges some notgalen variants will
match too. If T5 measures below 24, the suspect is that one or more
cluster-C pairs lack the sibling sub-role fact at saturation time
(symptom 1 above) — Phase 2d would need to seed that fact first.

## Cross-references

- Phase 2c plan (this T3 closes the design step):
  `docs/superpowers/plans/2026-06-01-phase2c-functional-role-covering.md`.
- Phase 2c.0 diagnosis (Option 3 framing; partially superseded by
  this doc's reframing): `docs/phase2c-galen-diagnosis.md`.
- Phase 2b.0 per-pair analysis (pair 06 trace; the "case-split /
  covering" framing is superseded here):
  `docs/phase2b-galen-pair-analysis.md`.
- Phase 2a results + rule:
  `docs/phase2a-results.md` + `crates/owl-dl-saturation/src/lib.rs`
  around line 707 (witness-merge block).
- pair_06 absorbed-TBox + saturator dump (throwaway):
  `/tmp/p2c-absorbed-pair06.txt`.
- pair_06 HermiT verification:
  `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_06.hermit.owx`.
- Phase 2c pair_06 canary:
  `crates/owl-dl-reasoner/tests/phase2c_pair_06_canary.rs`.
