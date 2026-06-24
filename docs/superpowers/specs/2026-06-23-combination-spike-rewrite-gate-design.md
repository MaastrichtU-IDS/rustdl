# Phase-0 combination spike — nominal/merge rewrite gate — Design

**Throwaway gate for the nominal/merge rewrite's load-bearing premise.** The rewrite's thesis is that
the search levers **compound** — each is weak alone (precise ≤n backjump cracks 1/77 hard wine
classes, deterministic-⊔ resolution collapses 18–34%, MRV 1.85×, lemma-caching 0%), but combined they
resolve wine's thrash. **That compound claim has never been measured.** This spike measures it on one
hard wine model before the rewrite builds on it.

## What the gate decides (correctness / soundness / performance — not effort)

- **Performance:** does combining the levers collapse a hard wine model's search from its thrash
  (~67k branches, DNF) to a small, fast search? If the combination does not deliver the performance,
  building the rewrite on it is wrong.
- **Soundness:** the spike runs **unsound-for-timing on purpose** — it isolates the *performance*
  question from the *soundness* question, exactly as the precise-merge and det-lookahead probes did.
  Soundness is not waived for the rewrite; it re-enters as the non-negotiable **corpus closure-diff /
  FP=0** gate at every real build phase. The spike's **verdict-sanity** check (below) guards against
  mistaking a spurious-Unsat collapse (the FP graveyard) for a real win.
- **Correctness of approach:** a collapse achieved via spurious-Unsat is the wrong approach regardless
  of speed; the spike catches that here, before it becomes the architecture.

## The confirmed thrash shape (grounds the lever choice)

All four hard wine classes, flag-OFF, are **wide-and-shallow with `restores ≡ branches`** (~67k branches
= ~67k restores at max-depth ~28, overwhelmingly disjunctive: e.g. `Alsatian⊓¬American` 67524 branches /
67524 restores / 55746 disj / 11778 merge). Every branch clashes and is undone — the search thrashes
through failing disjunct assignments. That is a **branch-ordering + no-learning** signature, which is
exactly what the three levers target: better disjunct choice (det-pruning), better variable order
(MRV), and not re-exploring after a clash (backjumping).

## Architecture

**Branch:** `spike/combo-rewrite-gate` off **`feat/precise-merge-deps`** — that branch already carries
the aggressive ≤n precise backjump (`RUSTDL_PRECISE_MERGE_DEPS`, the unsound-but-aggressive form, which
is fine here). The spike adds det-pruning + MRV on top, so all three levers run together. Throwaway;
does not merge.

**One combo env flag** `RUSTDL_COMBO_SPIKE` (default OFF; flag-OFF path byte-identical). When ON it:
forces the precise ≤n backjump on, and enables det-pruning + MRV at the ⊔ branch path in `HyperEngine`
(`crates/owl-dl-tableau/src/hyper.rs`).

### Lever 1 — deterministic-look-ahead as PRUNING (not counting)

At the **chosen** ⊔ point (after MRV selects it), for each not-already-satisfied disjunct `Dk`: run the
look-ahead `save() → apply_head_atom(Dk, .., EMPTY) → horn_fixpoint(FIXPOINT_ITERS) → restore()`. If it
clashes (`Unsat`), `Dk` is **dropped** from the branch loop. Branch only the survivors (survivors-first
order is automatic). Outcomes: **0 survivors → immediate clash** (this binding is unsatisfiable);
**1 survivor → assert it deterministically, no branch**; **≥2 → branch over the reduced set.** (The
look-ahead loop already exists on `spike/det-lookahead`; re-add it here as *pruning*, not counting.)

### Lever 2 — MRV variable ordering (cheap count, no look-ahead in the scan)

`find_open_disjunction` currently returns the *first* open ⊔. Change it (under the combo flag) to scan
all open ⊔ points and return the one with the **fewest not-already-satisfied disjuncts** (a cheap count
via the existing per-disjunct satisfied check — **no `horn_fixpoint` in the MRV scan**, so the
expensive look-ahead runs only on the single chosen ⊔, keeping per-step cost bounded by the number of
open ⊔ points). Branch the most-constrained ⊔ first.

### Lever 3 — aggressive backjumping

The precise ≤n backjump (inherited from `feat/precise-merge-deps`) + the existing ⊔ dependency-directed
backjump. Both already in place once the precise flag is on; the spike just runs them alongside levers
1–2.

## Measurement protocol

`sat_class_probe` on `sat(SweetWine)` and `sat(Alsatian⊓¬American)` (= `decide_pair_probe(AlsatianWine,
AmericanWine)`), reusing the big-stack-thread / adaptive-budget-OFF harness shape
(`tests/det_lookahead_gate.rs`). For each: combo-OFF baseline (the ~67k thrash) vs combo-ON. Record
`branches_taken` / `restores` / `disj_branches` / `merge_branches` / wall, and the **verdict**.

**Verdict-sanity (mandatory):** wine is consistent and these classes are satisfiable (corpus oracle).
A combo-ON collapse to **Sat** is a real win; a collapse to **Unsat** is spurious (the unsound spike's
FP) and is **NOT** a win — it must be flagged, because a fast wrong answer is not the goal.

## GO / NO-GO (pre-committed)

- **GO** (the combined approach delivers — proceed to the rewrite's first real build phase: a sound
  single-model construction) iff, on at least one hard wine model: branches collapse from ~67k to
  **small** (<1k, ideally ~tens) **AND** wall **< ~30 s** (even unamortized) **AND** verdict = **Sat**.
- **NO-GO** (the levers do not compound to resolve the thrash) iff the search still thrashes (branches
  stay high / wall stays DNF), or the only collapse is to spurious-Unsat. Then the rewrite as conceived
  does not resolve wine, and the approach must be reconsidered before any build.

## Components

- `crates/owl-dl-tableau/src/hyper.rs`: `combo_spike: bool` engine field + `with_combo_spike` builder +
  `RUSTDL_COMBO_SPIKE` env read (mirror the `precise_merge_deps` scaffolding on this branch); the
  det-pruning loop at the chosen ⊔; the MRV selection in `find_open_disjunction`; forcing
  `precise_merge_deps` on when `combo_spike` is on.
- `crates/owl-dl-reasoner/tests/combo_spike_gate.rs`: throwaway harness (2 hard wine probes ×
  {OFF, ON}, stats + verdict dump).
- `docs/combination-spike-gate-results-2026-06-23.md`: the durable verdict.

## Testing

- A small white-box unit test: a ⊔ ontology where det-pruning + MRV reduce the branch count and the
  verdict is preserved (combo-ON Sat == combo-OFF Sat), proving the combo path runs and is not
  trivially wrong on a controlled case.
- The wine measurement is the gate itself (controller-run).
- Flag-OFF byte-identical: with `combo_spike` off, `find_open_disjunction` and the ⊔ loop are unchanged.

## What this is NOT

Not the rewrite. Not a sound mechanism (it is unsound-for-timing by design). Not build-once / KPSet /
reuse (those are later phases, moot until a single hard wine model is shown to be buildable fast — which
is exactly what this gate tests). On GO, the first real build phase is a **sound** single-model
construction (corpus closure-diff / FP=0 gated), re-deriving the spike's levers soundly.

## Success criteria

A decisive branch/wall/verdict table for the two hard wine models (combo OFF vs ON), a verdict-sanity
confirmation, and a GO/NO-GO call against the pre-committed bar — written to the verdict doc. Spike code
does not merge; only the verdict lands on `feat/build-once-redesign`.
