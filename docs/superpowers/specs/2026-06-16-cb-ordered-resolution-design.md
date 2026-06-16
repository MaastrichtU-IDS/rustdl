# CB engine Slice 0 — ordered resolution + redundancy (termination optimization)

**Date:** 2026-06-16
**Crate:** `crates/owl-dl-cb` (consequence-based ALCH classifier, default-OFF)
**Branch:** `feat/cb-b1-integration` stack
**Goal:** Make the engine terminate in practice on real ALCH inputs (alehif's ~480-axiom
core currently times out >120s) WITHOUT changing soundness or the direct read-off.

## Problem

`apply_hyper` runs **unordered** hyperresolution: it indexes *every* atomic
occurrence of each derived clause and builds arbitrary-union resolvents, so a
context accumulates up to `2^|V|` positive disjunctions (the ExpTime blowup).
The B1 docstring claimed unordered was *required* for direct positive read-off.

## ⚠️ OUTCOME (2026-06-16): the design's verdict (A) was EMPIRICALLY FALSIFIED. See bottom.

**TL;DR of what actually happened.** Ordering (verdict A) **breaks completeness** of
the direct read-off — the `disjunctive_subsumption_by_cases` canary failed immediately
(`A⊑B⊔C, B⊑D, C⊑D ⊬ A⊑D`: a single total order can't expose a *maximal* consequence atom
as a unit). And the "alehif doesn't terminate in 120s" premise turned out to be a
**`convert_ontology` preprocessing hang**, NOT a saturation blowup: the CB engine
saturates real lightly-disjunctive ALCH fine (alehif's ABox/EquivalentClasses-stripped
167-class core plateaus at max 25 clauses/context, converges in seconds). **What shipped:
the subsumption-redundancy gate only (unordered resolution kept — it is directly
complete).** No ordering, no goal-directed rewrite. Full analysis at the bottom.

## Original design verdict (research, opus, 2026-06-16) — NOT adopted: **(A)**

SKH-style **ordered resolution with selection** + tautology/forward/backward
subsumption preserves the direct read-off, **provided reportable atomic classes
are order-minimal**. No switch to goal-directed `A ⊓ ¬B` per-pair refutation.

The docstring conflated *refutational* completeness (SKH Remark 5) with the
*model-generation / productivity* property of ordered ground resolution
(Bachmair–Ganzinger, Handbook of Automated Reasoning 2001, §4): a clause set
saturated under ordered resolution with selection + redundancy entails an
order-**minimal** positive atom `B` iff the unit `{B}` (or `{}`) is derived.
This is exactly how ELK / SKH (IJCAI 2011) / Bate et al. (KR 2016) read positive
`H ⊑ B` directly off an *ordered* saturation — not goal-directed.

**Key subtlety:** the order is on **atomic literals only**. `∃R.B`/`∀R.B` are
*selected* side-literals consumed by the structural rules (`apply_succ_and_forall`,
`apply_back_prop`), never resolution targets — they ride along in residuals but
are never the resolution key. So this is "ordered resolution with selection on
the non-atomic literals," within Bachmair–Ganzinger completeness for the atomic
sublanguage; the structural rules' completeness is the orthogonal SKH/Bate result.

## The ordering

`atomic_key(L)`: defined only for non-`Top` atomic literals.
- tier 0 (minimal): reportable atomic class (`ClassId ∈ norm.classes`)
- tier 1: synthetic/complement atom `X` (`ClassId ∉ norm.classes`)
- tiebreak within tier: `ConceptId` (total, well-founded).

`∃R.B`/`∀R.B`/`Top` are NOT atomic keys (filtered out). Reportable-minimal is
**load-bearing**: it guarantees reportable units surface (the productivity result).

## The changes (local; `model.rs` types, structural rules, read-off untouched)

1. **`apply_hyper` — maximal-occurrence indexing.** Index each derived clause's
   residual under its **single maximal atomic literal** (by `atomic_key`), not
   every atomic occurrence. The premise cartesian-product is unchanged in shape.

2. **`add_clause` — redundancy gate (after sort+dedup):**
   - **Tautology deletion:** head containing `Top` ⟹ drop.
   - **Forward subsumption:** an existing clause head ⊆ new head ⟹ drop new.
   - **Backward subsumption:** remove existing **purely-atomic** clauses whose
     head ⊋ new head (and purge from `seen`). Restricted to purely-atomic clauses
     so a `∃/∀`-bearing clause whose structural consequence may not have fired
     yet is never retracted (conservative — MISS-free, never FP).

## Soundness / completeness risk

- Every uncertainty biases to a **MISS, never an FP**: resolving on fewer
  literals, forward-skipping weaker clauses, and backward-removing strictly-weaker
  clauses can only drop *entailed* (redundant) consequences, never add one.
- The reportable-minimal order is the one MISS hazard (a reportable atom ordered
  above a synthetic could stay buried) — pinned by the disjunctive canaries +
  the differential gate.

## Acceptance

1. All 56 CB tests green (+ a new `ordering_*` canary).
2. **alehif** `cb-diff` *completes* `<<120s` (target sub-second) — primary perf gate.
3. **alehif** `cb-diff` `identical:true` (only_in_cb=[] ⟹ FP=0; only_in_current=[] ⟹ MISSED=0).
4. **bibtex** `cb-diff` stays `identical:true`; synthetic 15-class ALCH gate stays identical.
5. `cargo fmt --check` + `clippy -p owl-dl-cb -- -D warnings` clean.
6. Independent opus review of the FP-impossibility argument before merge.

If alehif completes but `only_in_current` non-empty ⟹ order tiering wrong (MISS) —
recheck reportable-minimal. If `only_in_cb` ever non-empty ⟹ STOP, an FP — revert
backward subsumption to atomic-only (or disable it).

---

# OUTCOME / FALSIFICATION (2026-06-16)

## 1. Ordering breaks the direct read-off (verdict A is wrong as specified)

Implemented `atomic_key` (reportable tier-0-minimal, synthetic tier-1) + maximal-occurrence
indexing in `apply_hyper`. The `disjunctive_subsumption_by_cases` canary failed instantly:
`A⊑B⊔C, B⊑D, C⊑D ⟹ A⊑D` was MISSED. Root cause: reasoning-by-cases needs *both* disjuncts
of `{B,C}` resolved, but ordered resolution only resolves the ⊔-maximal atom; under any
single total order, whenever the consequence atom (`D`) is itself maximal the entailed unit
`{D}` is never derived. The design's appeal to Bachmair–Ganzinger model-generation
guarantees only *minimal*-atom units — but the read-off queries *every* reportable atom as a
potential subsumer, and they can't all be minimal under one order. **(A) reverted.**

## 2. The "doesn't scale" premise was a convert hang, not saturation blowup

Per the advisor, instrumented the fixpoint (`RUSTDL_CB_DEBUG=1`) to discriminate
naive-resaturation vs genuine antichain blowup before any rewrite. Findings on stripped alehif:
- **ABox+role-characteristics+EquivalentClasses-stripped 167-class ALCH core**: saturates in
  seconds — `total_clauses` plateaus ~2700, **max 25 clauses/context**, queue drains. No blowup.
- **With EquivalentClasses (even just 20 injected)**: hangs with ZERO `[cb]` output — i.e.
  *before* `classify()` is even entered ⟹ the hang is in `convert_ontology`, not CB. Yet the
  **full** alehif (all 112 EC + ABox + inverse + functional) converts instantly. So
  `convert_ontology` loops/explodes on alehif's nested-existential defined-class equivalences
  (`C ≡ ∃R.(∃S.X ⊓ Y)`) specifically in the ABox-stripped form — a **separate, likely
  pre-existing preprocessing bug**, orthogonal to the CB engine. (Filed as a follow-up.)

The earlier "CB doesn't terminate in 120s on alehif's stripped core" (B1 integration report)
was almost certainly this same convert hang, misattributed to saturation.

## 3. What shipped

Unordered resolution **kept** (directly complete for the positive read-off). Added only the
**redundancy gate** in `add_clause` — tautology deletion + forward subsumption + backward
subsumption (restricted to purely-atomic heads). Sound + completeness-preserving (redundant-
clause elimination; every drop is an entailed clause). Plus `RUSTDL_CB_DEBUG` fixpoint
diagnostics. 56 CB tests green; bibtex differential stays `identical:true` (FP=0/MISSED=0);
clippy/fmt clean.

## 4. Consequences for the plan

- **No optimization rewrite is warranted now** — there is no measured saturation blowup on
  real ALCH. The subsumption gate is headroom for the more-disjunctive B2–B4 slices.
- **Open follow-up (separate from CB):** the `convert_ontology` hang on alehif's stripped
  EC subset. Until fixed, CB can't be exercised on that ontology's defined-class core.
- **B2/B3/B4 discipline:** add real in-fragment differential measurement per slice; only
  pay for ordering/goal-directed refutation if a slice surfaces a *measured* blowup (the
  advisor's rule — don't pull the rewrite forward on "we'll probably need it").
