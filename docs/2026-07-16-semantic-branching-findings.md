# Fix #2 Layer A (wedge semantic branching) — findings & Layer-B decision

**Date:** 2026-07-16. **Branch:** `feat/wedge-semantic-branching`.
**Spec:** `docs/superpowers/specs/2026-07-15-wedge-semantic-branching-design.md`.
**Plan:** `docs/superpowers/plans/2026-07-15-wedge-semantic-branching-layerA.md`.

## What shipped (Layer A, behind default-OFF `RUSTDL_SEMANTIC_BRANCHING`)

In-search disjoint-pruning + unit-forcing at the `⊔` decision in
`crates/owl-dl-tableau/src/hyper.rs` `solve`:
- Filter `live` disjuncts, dropping any `Atom::Class(c,_)` disjoint (via
  `disjoint_pairs`, resolved through union-find) with a current node label.
- All pruned → `Unsat`; exactly one `Atom::Class` survivor → unit-force (no
  decision level, same depth); else → branch over the filtered `live`.
- Commits: `a9f8b89` (flag scaffold), `987a940` (Layer A filter).

## Soundness journey (two independent catches before anything shipped)

1. **Advisor (opus, read the engine):** the plan's `deps = body_deps` at the
   pruned/unit-force sites is **unsound** — this is a dependency-directed-
   backjumping engine, so `clash_deps` must be a *superset* of the responsible
   decisions, including the killing label's deps. Correct dep-set =
   `body_deps ∪ ⋃ deps_of(e)` over each killing label `e`. Also: unit-force at
   same depth is only safe for `Atom::Class` survivors (`head_atom_satisfied`
   is permanently false for `AtLeast/Equal/Role` → non-`Class` would loop).
2. **Verdict-identity gate (during implementation):** the advisor's amendment
   named only two sites; there is a **third** — the 2+-survivor branch loop's
   `combined` dep-set started EMPTY and never folded in `reason_deps`. As
   specified this produced a **real false positive on `pizza`** (`Caprina`
   wrongly proved unsatisfiable → ~50 bogus subsumers), traced to clause
   `Pizza → ∃MeatTopping ⊔ VegetarianPizza ⊔ ∃FishTopping` where the pruned
   `VegetarianPizza` disjunct's dependency on an ancestor `NonVeg ⊔ Veg`
   decision was lost. Fixed by seeding `combined = reason_deps` in the branch
   loop (EMPTY when the flag is OFF → OFF path byte-identical).

The dep-set superset is now correct at all four sites (all-pruned→Unsat,
unit-force, branch-loop seed, early-backjump). Review verdict: APPROVED.

## Gates (all green)

- **Non-Horn FP oracle** (`ore_ont_13723` vs Konclude, the primary Layer-B
  hazard — curated is EL/Horn where disjunction-FP cannot manifest):
  OFF and ON both `rustdl=10166 konclude=10166 FP=0 MISSED=0` (closure
  identical).
- **Verdict-identity (byte-identical OFF vs ON):** `funcmerge-cyclic`, `pizza`,
  `27_eight_way_disjunction_sat`, `18_diamond_subsumption_unsat`, plus `ro`,
  `sulo`. OFF path is byte-identical by construction (`reason_deps = EMPTY`
  off-path; every consumption is `union(EMPTY)` = identity) — reviewer-verified
  structurally.
- Full `owl-dl-tableau` suite, `clippy -D warnings`, `fmt` clean.

## `ore_ont_10019` measurement (the go/no-go substrate)

`classify --pair-timeout-ms 250`, `RUSTDL_AGGREGATE_DEADLINE_MS=60000`:

| flag | classes | tableau-probed | incomplete pairs |
|---|---|---|---|
| OFF | 47 | 33 | 1455 |
| ON  | 47 | 33 | 1459 |

**Layer A alone decides zero additional classes** (the Δ+4 incomplete is
deadline-timing noise). This is the predicted outcome: the reactive
`horn_fixpoint` already clashes disjoint co-occurrence on the next pass, so
Layer A's marginal win is fewer save/restores + unit-forcing, not new decided
classes. **Not a failure signal** — Layer A validates the mechanism and the
soundness gate that Layer B needs.

## Decision

- **Layer A is verdict-preserving and gate-clean → keep it, behind the
  default-OFF flag.** Do NOT flip default-ON (it moves nothing on its own).
- **Proceed to Layer B** (SEPARATE plan): the per-node `excluded: Vec<ClassId>`
  exclusion set with the **load-bearing invariant — exclude a sibling's class
  ONLY if that sibling returned `Unsat`, NEVER `Stalled`** (excluding a merely-
  stalled disjunct asserts an unproven `¬Dⱼ` → false clash → FP). Layer B's
  gate adds the `Stalled`-never-excluded canary alongside the FP oracle +
  byte-identical curated + MISSED=0.
- **Whole-Fix#2 go/no-go is evaluated after Layer B:** flag-ON `ore_ont_10019`
  decides ≥ ~half of the 33 within the Konclude/HermiT budget → corpus gate →
  default-ON; else STOP → bound-the-tail (documented, honest). Advisor's
  candid probability Layer B closes the 33: ~40%.

## Follow-ups (for the final whole-branch review)

- **(Minor)** Add a focused `owl-dl-tableau` unit test for the branch-loop
  `combined = reason_deps` seed (the actual pizza FP fix is currently guarded
  only by the CLI verdict-identity differential, not a unit test).
- **(Important, out of the shipped default-OFF scope)** The
  `RUSTDL_SEMANTIC_BRANCHING` ⊥ `RUSTDL_SAT_LOOKAHEAD` separation is not
  code-enforced. Both ON is unvalidated and latently FP (lookahead-dropped
  disjuncts contribute no `reason_deps` — the same under-approximation class
  fixed here for the prune path). Add a runtime guard (ignore one when the
  other is active) or a documented assertion before either could be
  default-ON.
