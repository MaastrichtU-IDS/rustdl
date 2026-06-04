# HF2 — inverse roles + double-blocking (scoping)

Drafted 2026-05-27. HF1 made the clausifier sound (`deferred == 0`
corpus-wide). HF2 makes the *engine* sound for **inverse roles**,
which forces a blocking upgrade (anywhere blocking is unsound with
inverses). Part of the full sound+complete roadmap
([`hypertableau-full-scoping.md`](hypertableau-full-scoping.md)).

## §0 — A finding that reframes HF2's validation

The corpus does **not** exercise inverse-role-dependent reasoning:
- The clausifier currently **drops `InverseObjectProperties`** (RBox →
  `_ => {}`), and `role_matches` requires equal polarity (an `R`-edge
  does not satisfy an `R⁻` atom). So inverse semantics are absent.
- Yet **ro-stripped agrees 100 % with Konclude** anyway — its named-
  class subsumptions don't depend on the inverse axioms (the inverses
  are declared but inert for the hierarchy).

**Implication:** HF2 buys *general* SROIQ soundness (correctness on
ontologies where inverses *do* change the hierarchy — beyond the
corpus), not a corpus-agreement gain. Its validation therefore rests
on **crafted tests + conformance to the published calculus**, not the
corpus diff (which is already 100 %). This is the project's raison
d'être (a real native reasoner, not a corpus-passer), but it's honest
to note the remaining phases are generality, not corpus wins.

## §1 — Prerequisite: clausify the RBox

Inverse reasoning needs the role axioms the clausifier currently
ignores. HF2 starts by handling, in `clausify_axiom`:
- **`InverseObjectProperties(R, S)`** (≡ `S ≡ R⁻`): record `S` and `R⁻`
  as the same role at match time (or rewrite `S`-atoms to `R⁻`).
- **`SubObjectPropertyOf` / role hierarchy**: an `R`-edge also counts
  as an `R'`-edge for super-roles `R ⊑ R'` (the existing
  `RoleHierarchy` in `PreparedOntology` is the reference; the hyper
  engine needs its own copy or a shared structure).
- **Role chains, (a)symmetry, (ir)reflexivity, transitivity**: RBox
  characteristics. Transitivity interacts with blocking (the `S` in
  SROIQ); scope carefully — possibly its own sub-phase.

Scope cut for the first HF2 increment: **inverse pairs + the role
hierarchy** (the inert-in-corpus but common constructs). Chains and
transitivity can be a follow-on if they prove deep.

## §2 — Inverse-role propagation in the engine

An `R`-edge `x —R→ y` must satisfy an `R⁻` body/head atom from `y` to
`x`. Concretely:
- `match_body` role-atom matching and `find_open`/`fire_exists` lookups
  become **inverse-aware**: following `R⁻` from `y` means walking `y`'s
  *predecessor* `R`-edges (the `preds` list already exists from the
  semi-naive worklist — reuse it).
- `∀R⁻.C` / `∃R⁻.C` fire across the reverse edge.
- The `RoleHierarchy` is consulted: `role_matches` generalises to "edge
  role is `wanted` or a sub-role of `wanted`, in the right direction."

## §3 — Double-blocking (replace anywhere blocking)

Anywhere blocking (`L(n) ⊆ L(m)`, `m` earlier) is **unsound with
inverse roles**: a blocked node's inverse-role consequences on its
predecessor aren't realised. The SROIQ-sound condition is
**double-blocking** (Motik, Shearer & Horrocks 2009, §3.4 / pairwise
blocking refined for inverses+nominals): block `n` by `m` only when
`L(n)=L(m)`, `L(parent(n))=L(parent(m))`, and the connecting edge
labels match. Go straight to the published condition — pair-blocking
is sound for SHIQ but not once nominals (HF4) interact, and a fragile
intermediate isn't worth it.
- This is the riskiest part: blocking soundness is subtle and
  non-local. Implement against the published rule, not intuition.

## §4 — Validation gate (crafted, since the corpus is inert)

1. A **crafted `R⁻` ontology** whose hierarchy *depends* on the inverse:
   e.g. `A ⊑ ∃R.B`, `B ⊑ ∀R⁻.C` ⊨ `A ⊑ C`. The current engine gets
   this **wrong** (no inverse propagation); HF2 must derive `A ⊑ C`.
   This test is the HF2 canary — write it first, watch it fail, make it
   pass.
2. A **role-hierarchy** test: `R ⊑ S`, `A ⊑ ∃R.B` ⊨ `A ⊑ ∃S.B`.
3. A **cyclic + inverse** test that anywhere-blocking would get wrong
   but double-blocking gets right (the blocking-soundness canary).
4. **No corpus regression:** pizza/ro/sulo/SIO agreement unchanged
   (0 FP, completeness held) — the corpus is inert to inverses, so
   this just guards against breakage.

## §5 — Build on, scope cuts, out of scope

Build on the current engine (the `preds` reverse edges are already
there for the worklist; the `RoleHierarchy` exists in the reasoner).
Scope cuts: chains/transitivity deferred within HF2 if deep. Out of
scope: cardinality-in-calculus (HF3), nominals/NN-rule (HF4),
`Sat`-soundness wiring (HF5) — HF2 is inverse + blocking only.

## §6 — Recommended entry point

The **crafted inverse canary test** (§4.1) first — it fails today and
defines "done" for the core of HF2. Then RBox inverse-pair
clausification (§1) + inverse-aware matching (§2) to make it pass,
then double-blocking (§3) with its own cyclic canary. Each step gated
by its crafted test; the corpus is the no-regression guard, not the
completeness gate.

## §7 — Progress

- **Inverse-aware matching (§2) — DONE** (`b5f6762`). `enumerate_matches`
  now unions outgoing edges with incoming `preds` whose `flip()` matches
  the wanted role, so following `R⁻` walks `R`-predecessors. The canary
  (§4.1) passes and is a live regression test. Corpus 0-FP held and
  **counts are identical to baseline** (pizza 695, ro 158, sulo 51) —
  the corpus is inert to inverse propagation, confirming §0. Sound for
  `Unsat` (∀R⁻ derives genuine consequences).
- **RBox inverse-pair clausification (§1) — DONE** (`build_inverse_canon`
  / `canon_role` in clause.rs). `InverseObjectProperties(R,S)` (`S ≡ R⁻`)
  now rewrites role `S` to `R⁻` at every clause site, so named inverses
  reuse the engine's flip-matching. Named-inverse canary
  (`hyper_subsumption_probe_propagates_named_inverse`) passes. Corpus
  unchanged (ro-stripped still 158, 0 FP) — confirms inverse-inertness
  holds even for *named* inverses.
- **Role hierarchy (§1/§4.2) — DONE** (`role_matches` now takes the
  `RoleHierarchy`; `HyperEngine::with_sub_roles`). One-way `R ⊑ S` can't
  be canonicalized (unlike inverse pairs), so it's consulted at match
  time: an `R`-edge satisfies an `S`-atom when `is_sub_role(R, S)`
  (same polarity, since `R ⊑ S ⟹ R⁻ ⊑ S⁻`). Reuses the reasoner's
  `build_role_hierarchy`, cloned per pair. Super-role canary
  (`hyper_subsumption_probe_propagates_super_role`) passes. Corpus
  unchanged (ro/sulo *have* `SubObjectPropertyOf` yet still 158/51, 0
  FP) — hierarchy is wired+active but the corpus's probe subsumptions
  don't depend on it. Chains/transitivity remain HF3.
- **Double-blocking (§3) — DONE** (shipped default-on at commit `5e48382`, 2026-05-29; see `hypertableau-hf2-doubleblocking-scoping.md` §9 for timeline). ~~PENDING. Required for SAT-soundness (anywhere-blocking is unsound with inverses for *model construction*, not for the `Unsat`-only probe). Needs its own cyclic canary (§4.3).~~
