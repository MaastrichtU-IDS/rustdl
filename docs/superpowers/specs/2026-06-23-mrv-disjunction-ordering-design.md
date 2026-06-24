# MRV disjunction ordering — sound wedge feature — Design

**The first real build phase of the nominal/merge rewrite.** The combination spike's diagnostics
isolated a single sound lever: **MRV (most-constrained-variable) ordering of the ⊔ rule** — branch
the open disjunctive clause with the fewest live disjuncts first. Alone it is sound and collapses
wine's hard models 5–54× to the correct `Sat` (FP=0/MISSED=0 on the full wine closure). det-pruning
(the other spike lever) is unsound on the nominal/≤n fragment and contributed ~nothing to the
collapse — **dropped.** This spec promotes MRV from the throwaway spike to a clean, sound,
corpus-FP=0-gated wedge feature.

## Background / evidence

Spike findings (`docs/combination-spike-gate-results-2026-06-23.md`, "FP REFUTATION + diagnostics"):
MRV-only on wine = **FP=0/MISSED=0, full closure 653=653**; `sat(Alsatian⊓¬American)` 66683→1227
branches / 1.2 s / Sat; `sat(SweetWine)` 67459→12366 / 15.6 s / Sat. det-pruning-only = FP=7 (the
deterministic `horn_fixpoint` look-ahead skips the ≤n-merge → drops live disjuncts). MRV adds **no
look-ahead** — just a per-branch scan.

## Mechanism

In `HyperEngine::find_open_disjunction` (`crates/owl-dl-tableau/src/hyper.rs`), which today returns
the **first** open disjunctive `(clause, node, binding)`: when MRV is enabled, instead enumerate all
open candidates (same enumeration — over nodes × non-Horn clauses × bindings where the clause is open,
i.e. `!any_head_satisfied`) and return the candidate minimizing the count of **not-already-satisfied**
disjuncts (`#{k : !head_atom_satisfied(ci, k, node, &binding)}`), ties broken by first encounter. The
count uses the cheap per-disjunct satisfied check **only** — no `horn_fixpoint`, no look-ahead.

This requires the `head_atom_satisfied(&self, ci, k, xnode, binding) -> bool` helper (extracted from
`any_head_satisfied` in the spike); re-extract it cleanly here (behaviour-preserving).

## Soundness

MRV reorders **which** open ⊔ the backtracking search expands first. It does **not** drop, add, or
alter any disjunct or clash — the logical search space and its completeness are unchanged; only the
visitation order differs. A sound, complete backtracking search returns the same Sat/Unsat verdict
regardless of branch order, so MRV is **verdict-invariant by construction** — no FP, no MISS. (This is
a categorically stronger soundness argument than det-pruning's failed-literal claim, which had the
nominal-context hole; reordering has no such hole.) Blocking/termination are order-tolerant (a blocked
node realizes its model via its blocker irrespective of expansion order). The corpus FP=0 gate
confirms the implementation at scale.

## Gating

Env flag `RUSTDL_MRV_ORDERING`. **Default decided by the gate**, not assumed:
- Built default **OFF** so the flag-OFF path is provably byte-identical to current `main` and the gate
  measures ON vs OFF cleanly.
- Flip to default **ON** only if the gate passes: FP=0/MISSED=0 byte-identical corpus-wide AND no
  fixture wall-regresses AND the wine collapse holds.

## Components

- `crates/owl-dl-tableau/src/hyper.rs`: `mrv_ordering: bool` field (mirror an existing bool-flag
  field's scaffolding — set false in all constructors) + `with_mrv_ordering` builder; the
  `head_atom_satisfied` extraction; the MRV branch in `find_open_disjunction` (gated; flag-OFF =
  unchanged first-open body).
- `crates/owl-dl-reasoner/src/lib.rs`: `hyper_mrv_ordering_enabled()` env helper + wiring at the
  engine-construction sites (the `with_precise_card_deps` sites).
- A unit test: two open ⊔ clauses with different live-disjunct counts; assert MRV-ON returns the
  fewer-live clause (and OFF returns first-open), verdict preserved.

## Testing / gate (the proof)

1. **Unit:** the MRV-selection test above + the `head_atom_satisfied` extraction guarded by the full
   `owl-dl-tableau` suite staying green (behaviour-preserving).
2. **Flag-OFF byte-identical:** `cargo test -p owl-dl-tableau` green; `find_open_disjunction` unchanged
   when the flag is off.
3. **Corpus FP=0 / no-regression gate (controller-run, the load-bearing proof):** `konclude_closure_diff`
   with `RUSTDL_MRV_ORDERING=1` across all oracled fixtures (bibtex, pizza, ro, sio, sulo, galen,
   notgalen, ore-15672, ore-10908, wine) — **FP=0/MISSED=0 byte-identical** on every one. Plus a wall
   measurement (MRV-ON vs OFF) on the cardinality/disjunction fixtures (wine, sio, ore) confirming
   **no regression** and capturing the wine improvement. This gate — not the soundness argument — is
   what graduates MRV to default-ON.

## Success criteria

MRV implemented as a sound, gated wedge feature; unit + flag-OFF tests green; **corpus FP=0/MISSED=0
byte-identical with the flag ON**, no wall regression, wine collapse confirmed. On full pass: flip
default ON and it lands on `feat/build-once-redesign` as the rewrite's first shipped increment. If any
fixture FPs or regresses: keep default OFF, record which, and diagnose before shipping.

## What this is NOT

Not det-pruning (unsound, dropped). Not build-once / KPSet / amortized deterministic-expansion (MRV
needs no look-ahead, so there is nothing to amortize). Not the full rewrite — it is the first sound,
shippable increment, and it validates the rewrite's path (a sound search-ordering improvement that
collapses the hard nominal/disjunctive models without touching soundness).
