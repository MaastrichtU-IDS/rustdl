# The remaining tail is an ABSORPTION problem, not a search problem

> # RETRACTED IN ITS CENTRAL CLAIM — 2026-08-01
>
> **The title is wrong and the calibration behind it was confounded.** Read this banner before
> anything below it. Full data: `docs/2026-08-01-domain-absorption-results.md`.
>
> **1. The 300× calibration was a MIS-ATTRIBUTION.** The controlled deletion removed an
> `EquivalentClasses` axiom, which is *two* axioms in one — a `⟸` half and a `⟹` half. Splitting
> them: the `⟸` half alone (28 residuals) runs in **0.02 s**; the `⟹` half alone (26 residuals,
> a disjunctive concept rule) runs in **0.02 s**; only **both together** cost 8.84 s. So
> "two residual disjunctions cost 300×" is false — the deletion changed two things at once, and
> I attributed the whole effect to the residual count. This is exactly the confound I had caught
> two reviewers on days earlier (`SAT_SEED=0` changing rebuild *and* clause volume) and then
> committed myself.
>
> **2. The thesis was tested directly and FAILED.** Domain absorption was implemented and the
> pre-registered experiment run over the 167 survivors:
>
> | group | n | recovery |
> |---|---|---|
> | **Z** — residuals driven to **zero** | 54 | **1 (1.9%)** |
> | **N** — residuals remain | 77 | **3 (3.9%)** |
>
> Zero is **not** a threshold (53 of 54 reach zero and still DNF), and recovery is **not**
> continuous in residual count — the group that *kept* residuals recovered slightly more, and the
> 21 ontologies left with only 1–2 residuals recovered nothing. **Residual count is not the
> operative variable.** Recovery depends on *which* axiom is absorbed, matching the split above.
>
> **What survives.** Domain absorption is sound by logical identity with `ObjectPropertyDomain`,
> verdict-preserving (13/13 curated and 10/10 ORE closures byte-identical, FP=0 net 11/11 exact),
> and it does recover **4** real DNFs — `ore_ont_3281` goes 28 residuals → 0 and
> **DNF@300 s → 11.40 s** at a non-truncating budget. That is a genuine but *narrow* win, not the
> general mechanism this document claimed.
>
> **What it kills, which is the more valuable output.** Qualified-`∃` absorption — 58% of
> residual volume, the obvious next step — is a **NO-GO as motivated**. It would take 31 more
> ontologies to zero residuals; at the measured 1-in-54 rate that predicts **~0.6 recoveries**,
> against building a *backward* role rule `RoleRule` cannot express. The experiment stopped a
> large project that would not have paid, which is what it was designed to do.


**Date:** 2026-08-01 · rustdl v0.4.10 · analysis from first principles on the instances

## Summary

rustdl's absorption is a **single-pass, pick-the-first-`¬Atomic`-disjunct heuristic**
(`absorb.rs:349`, `as_trigger` at `:394`). Anything it cannot trigger on becomes a
**`residual_gci` — a global disjunction applied to every node in the completion graph.**

That is far short of what peer reasoners implement, and it is measurably the dominant cost
in the remaining DNF tail:

- **Two** extra residual disjunctions cost **300×** on a real ontology (measured, below).
- **130 of 160** measured survivors (81%) carry residual GCIs, median **46**, p90 **1639**,
  max **38,135**.

The long-standing framing — that this tail is *disjunctive-search* blowup requiring a
clash-driven search rewrite — describes the symptom. The search explodes because
**preprocessing hands it an exponentially larger problem than it needs to solve.**

## The instance that proves it: `ore_ont_3281`

126 classes, 131 `SubClassOf`, no top-level `∃`, no `∀`, no nominals, no disjointness.
**Konclude classifies it in 0.02 s. rustdl does not finish in 120 s.**

The whole difficulty is three axioms:

```
Relation ≡ (≥1 hasTarget) ⊔ (≥1 hasSource)
Relation ⊑ =1 hasSource
Relation ⊑ =1 hasTarget
ObjectPropertyDomain(hasSource, Relation)     ← already stated
ObjectPropertyDomain(hasTarget, Relation)     ← already stated
```

The `⟸` direction of that equivalence is `(≥1 hasTarget) ⊑ Relation`, whose NNF disjunction is
`[Max(0, hasTarget), Atomic(Relation)]`. Neither disjunct is `Not(Atomic)` or `Not(Nominal)`,
so `as_trigger` returns `None` for both and the axiom lands in **`residual_gcis`** — a
disjunction re-applied at every node. Two roles ⇒ two such disjunctions ⇒ 2^(2n) branching.

But `(≥1 R) ⊑ C` **is a domain axiom.** `ObjectPropertyDomain(hasTarget, Relation)` is
*already in the file*, and rustdl handles that construct natively. The equivalence is therefore
**logically redundant** — which makes for a clean controlled experiment.

### The experiment

Delete only that one axiom. Semantics are unchanged: the `⟸` direction duplicates the stated
domains, and the `⟹` direction follows from `Relation ⊑ =1 hasSource`.

| | residual GCIs | wall (`--pair-timeout-ms 1`) | subsumptions |
|---|---|---|---|
| as shipped | 28 | **8.89 s**, 3432 timed-out pairs | 200 |
| one redundant axiom removed | 26 | **0.03 s**, 0 timed out | 200 |

**Closures byte-identical. ~300× from two residual disjunctions.** That is the calibration
that makes the population numbers alarming: if 2 costs 300×, a median of 46 is hopeless.

### What the profile shows, and what it overturns

```
# label heuristic: pruned=0 pass_through=0 misses=3467
# timed-out pairs: 3432 (defaulted to not-subsumed)
# fallthrough (wedge-stall→tableau): ran=3432 rescued=0 noverdict=3432
```

The Phase-7 label heuristic — documented at **96–100% prune rates** — prunes **zero** here,
and every lookup is a *miss*: the per-class oracle recorded nothing usable for any of the 126
classes. Confirmed inert by control (`RUSTDL_LABEL_HEURISTIC=0` gives byte-identical stats).

So the "wall is linear in the per-pair budget" signature, long read as *a tail of hard pairs*,
is nothing of the kind. It is a **collapsed pruning oracle**: every pair falls through to a
full probe, and each probe burns its whole budget finding nothing. A single pair
(`Adjective ⊑ Adposition`, a plain non-subsumption between two taxonomy classes) takes
**294 s** to answer — and it *does* terminate, so this is search-space explosion, not a
termination bug.

## The general diagnosis

`as_trigger` recognises exactly two shapes: `Not(Atomic)` and `Not(Nominal)`. Everything else
is residual. Three standard absorption techniques are therefore absent:

1. **Domain / role absorption.** `∃R.⊤ ⊑ C` and `≥1 R ⊑ C` are domain axioms and should fire
   on *edge creation*, not on every node. **Sound and complete** — logically identical to
   `ObjectPropertyDomain(R, C)`. Note `≥n R ⊑ C` for **n > 1 is NOT** a domain axiom;
   treating it as one would be unsound (too strong). Qualified `∃R.D ⊑ C` also does not
   reduce to a domain axiom — it needs a filler check, a separate and harder case.
2. **Binary absorption** (Hudek & Weddell). `A ⊓ B ⊑ C` yields `[¬A, ¬B, C]`; `as_trigger`
   picks `¬A` and produces a *disjunction* `A → (¬B ⊔ C)` fired on every `A`-node. Binary
   absorption fires only when both `A` and `B` are present.
3. **Nominal absorption** beyond the single `Not(Nominal)` case.

## CORRECTION (2026-08-01, from the census that followed)

Two claims above are **wrong**, and the census caught both.

1. **"rustdl handles `ObjectPropertyDomain` natively" is FALSE as implemented.** `absorb.rs:290`
   lowers *every* `ObjectPropertyDomain(R, D)` to `⊤ ⊑ ∀R.⊥ ⊔ D` — i.e. straight into
   `residual_gcis`, a global disjunction. So the problem is far more common than the analysis
   assumed: **every domain axiom in every ontology already pays this cost.** That makes the
   redundancy argument for `ore_ont_3281` still correct, but for a different reason than stated.
2. **`ore_ont_10019` is NOT a negative control.** It reports 5 residuals, all
   domain-absorbable — from its 5 `ObjectPropertyDomain` axioms. My check was a *source-syntax*
   scan; the residual population is a strict superset of what that scan can see. The lesson is
   the recorded one: **grep is not the gate**, and I applied it to the population but not to my
   own control.

The census also **bounds the claim much harder than the calibration suggested** — see
`docs/2026-08-01-residual-absorbability-census.md`. Domain absorption removes only **6.3%** of
survivor residual *volume*; the mass is `qualified_exists_antecedent` (58%) and
`genuinely_disjunctive` (36%). Only **54 of 167** survivors reach zero residuals under it.
And critically: **the residual-count → wall relationship is calibrated at exactly one point.**
Two residuals costing 300× on one ontology does not establish that residual count predicts wall.

## Scope — stated honestly

**This is one mechanism, not the mechanism.**
`ore_ont_10407` and `ore_ont_8666` do have it. Any claim that absorption explains the whole
tail would be over-generalisation from one confirming case.

A grep-level candidate scan finds the antecedent shape in **145 of 167** survivors (87%) and
784 of the 1,920 pool — but **grep is not the gate** (a prior grep estimate gave 67 where the
real gate-probe found ~40), and the precise problematic case is narrower: the antecedent must
be *purely* existential/cardinality, with no atomic conjunct for `as_trigger` to latch onto.

## What to do next, in order

1. **Report-only instrumentation first.** Classify each residual GCI by whether it carries an
   absorbable disjunct (`Max(0,R)` / `All(R,Bot)` ⇒ domain-absorbable; ≥2 `Not(Atomic)`
   disjuncts ⇒ binary-absorbable), and report how many residuals each technique would remove.
   Run across the 167 and the 784. **Only then** decide what to build. This is the project's
   own Phase-4 discipline, and it is what separates this from the cheap-lever pattern that
   produced a chain of NO-GOs here.
2. Implement domain absorption for the **unqualified, n = 1** case only. Sound by logical
   identity with `ObjectPropertyDomain`.
3. Re-measure. `ore_ont_3281` should reach ~0.03 s **without** editing the ontology.
4. Then evaluate binary absorption on the measured residual population.

## Method note

The load-bearing evidence here is a **controlled deletion of a logically redundant axiom**
giving byte-identical closures at 300× the speed — not a profile. Three prior "obvious"
hotspots in this codebase were refuted by measurement, and the reason to trust this one is
that it changes a *count* (28 → 26 residuals) with a *predicted* and *observed* consequence,
and the negative control (`ore_ont_10019`, zero such axioms) behaves as the hypothesis says
it should: unexplained.
