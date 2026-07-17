# Saturator disjointness in the complete fragment (Phase A inc 1) — shipped, results (2026-07-17)

Implements `docs/superpowers/specs/2026-07-17-saturator-disjointness-design.md` per plan
`docs/superpowers/plans/2026-07-17-saturator-disjointness.md`. Branch `feat/saturator-disjointness`.

## What shipped
`DisjointClasses` is now admitted to the saturator's complete fragment (`is_saturator_axiom`)
**iff the ontology has no functional/inverse-functional role** — the
`disjoint_ok` gate. Reuses the existing `DisjointnessClash` rule + `process_unsat` back-prop
(complete on EL+disjoint-no-functional by construction). An allowlist-gate change, no engine change.

## Gate (three tiers, all green)
1. **Non-regression:** `cargo test -p owl-dl-reasoner` = 59 groups / 0 failed (incl. the curated
   oracle tests + the updated fragment tests). Full workspace: only the pre-existing,
   anon/disjoint-unrelated `incremental_matches_baseline_on_fixtures` failure (missing fixture
   `ontologies/regression/funcmerge-cyclic.ofn`). No new failures. FP=0 held.
2. **Empirical fast==hybrid on real ORE disjoint onts:** on 20 small disjoint-no-functional ORE
   onts, `classify` with the fast path (`RUSTDL_HORN_SHORTCIRCUIT=1`) is **byte-identical** to the
   complete hybrid path (`=0`) — **20/20 identical, 0 diverged**. The D10 unsound-completeness
   guard, empirically clean on real onts.
3. **By-construction:** on EL+disjoint-no-functional, disjointness yields only unsat, propagated
   completely by `process_unsat` (subclass + ∃-fact channels) — the tier no oracle scales to on
   the giants.

Plus 4 canaries (clash→unsat, ∃-fact back-prop, satisfiable control, fast==hybrid) + 2 fragment
unit tests (accepts-no-functional / rejects-with-functional), all green.

## Acceptance — foundation, as designed
Standalone DNF-recovery is **~0** (measured: 31/39 disjoint onts also use symmetric). The gate
fires (proven by the unit test), and the fast path is sound+complete on disjointness — but the
giant disjoint onts still DNF because they also need the **symmetric** increment (next). Success
here is the sound, complete, gated foundation, not ontology recovery.

## Follow-up (non-blocking)
Cosmetic: `saturator_complete_fragment` uses `functional_roles.iter().next().is_some()` where
`!functional_roles.is_empty()` is idiomatic.

## Final-review correction (2026-07-17)

The final whole-branch review caught that admitting `DisjointUnion` was **unsound-completeness**:
the saturator's rule-builder registers `disjoint_pairs` from `DisjointClasses` only (no
`DisjointUnion` arm), and `DisjointUnion` additionally carries a disjunctive covering
(`class ≡ ⊔members`) that is out-of-fragment — so routing a `DisjointUnion` ont to the fast path
would silently drop both halves. Fixed (commit 72baa2f): the gate accepts **`DisjointClasses`
only**; `DisjointUnion` stays on the hybrid path. Pinned by `saturator_fragment_rejects_disjoint_union`.

Follow-up (non-blocking): `has_cardinality_role` currently treats every `InverseFunctionalRole`
axiom as blocking, including non-inverse ones the saturator DOES handle — over-conservative (a
missed fast-path opportunity, safe direction), reconcile in a later increment.
