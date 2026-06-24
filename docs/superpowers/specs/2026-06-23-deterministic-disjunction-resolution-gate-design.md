# Deterministic-closure ⊔-resolution viability gate (edge-2-amortized) — Design

**Throwaway research gate.** Discriminates the one untested wine mechanism: does a **full
deterministic closure** (Horn-propagation to fixpoint — richer than told-disjointness, ∀-propagated)
resolve wine's free ⊔ branch points? If yes, Konclude's edge-2 (build-once deterministic-concept-
expansion that avoids generating ⊔ branches) is wine's lever and is worth building. If it kills ~none
(like told-disjointness did in SP-B), wine's disjunctions are genuinely nondeterministic and **wine is
exhaustively closed** (4th convergent NO-GO). Code does NOT merge; only the verdict doc lands.

## Why this gate (and why it's the only data-supported wine probe left)

Wine's wall is **disjunctive-branching explosion**: the SP-B viability gate found **66–90% of wine's
⊔ branch points are "free"** (≥2 disjuncts survive), and the precise-merge-deps probe showed
**backjumping cracks 1/77 hard classes** — so the lever is NOT pruning clashes after the fact, it is
**not generating the free ⊔ branches in the first place**. SP-B tested the weak form of that
(told-disjointness ⊔ pruning) → **pruned=0**. Prior per-branch forward-checking was net-negative *on
wall*. The genuinely untested form is the **full deterministic closure** (Horn fixpoint to completion,
∀-propagated, ≤n/role-successor-propagated) — Konclude's `CSaturationNodeAssociatedDeterministicConcept-
Expansion`. This gate measures whether that richer oracle resolves wine's free ⊔ points, before any
multi-month build. (Spec history: [[sp-b-saturation-guided-gate]], precise-merge-deps NO-GO + viability
probe in `docs/precise-merge-deps-gate-results-2026-06-23.md`.)

## Mechanism — read-only deterministic look-ahead at ⊔ points

Hook `find_open_disjunction` in `HyperEngine` (the SP-B harness shape). When the wedge reaches a ⊔
branch point — where the node's label is **already Horn-saturated** (`solve` runs `horn_fixpoint`
before `find_open_disjunction`) — for each **free** head disjunct `Dk` (not already satisfied), run a
read-only deterministic look-ahead:

1. `let saved = self.save();` — snapshot the graph (nodes / representative / neq / block_index / origin).
2. apply `Dk`'s head atom to the resolved target node (the same `apply_head_atom` the real ⊔ loop uses).
3. `let r = self.horn_fixpoint(MAX);` — run ONLY the deterministic Horn fixpoint (no ⊔ / ≤n branching).
   `horn_fixpoint` clears + re-seeds the worklist from the graph, so it is self-contained.
4. record `killed = matches!(r, HyperResult::Unsat)` — `Dk` is dead by deterministic consequence.
5. `self.restore(saved);` — restore the graph. (The worklist is left stale but is re-seeded by the next
   real `horn_fixpoint` in `solve`; verdict-preservation, below, is the safety net.)

Tally, per ⊔ point: free-disjunct count, deterministically-killed count, and whether the look-ahead
**collapses the point to ≤1 surviving disjunct** (the resolution signal). Aggregate over a wine
sat-probe. Gated `RUSTDL_DET_LOOKAHEAD_PROBE`, default OFF; the probe only COUNTS — it does NOT change
which disjunct the real search takes (read-only).

### `MAX` iterations

`horn_fixpoint(MAX)` with `MAX` = the same cap the engine uses for its own pre-branch fixpoint (read it
from the `solve` call site, do not invent a number). A look-ahead that hits the cap without clashing
counts as "not killed" (conservative — under-counts kills, biases toward NO-GO, safe for the gate).

## Two distinct correctness checks

1. **Read-only / verdict-preserving (validity of the measurement).** With the probe ON, every measured
   wine class's Sat/Unsat verdict MUST equal probe-OFF. The probe only counts; it must not alter the
   search. If a verdict flips, the look-ahead's `save`/`restore`/worklist handling is leaking state —
   the measurement is invalid until fixed. This is the gate's correctness guard (mirrors the SP-B /
   precise-merge-deps verdict-preservation discipline).
2. **Soundness of a "kill" (for the eventual build, not re-run here).** A `horn_fixpoint` → `Unsat`
   on (label ∪ `Dk`) is a genuine deterministic clash ⟹ `Dk` is truly unsatisfiable in this context ⟹
   sound to prune in a future build-once mechanism. The gate doesn't ship, so this is argued, not gated.

## Measurement protocol

Wine sat-probes via `sat_class_probe` (reuse the SP-B/`precise_merge_fp_diag` harness shape: 2 GiB
stack, adaptive-budget OFF, depth 256, a deadline). Target classes: `vin:Wine` (the root that the
precise-merge probe showed is the hard core) + 2–3 hard subclasses (e.g. `AlsatianWine`, `SweetWine`,
`Zinfandel`). For each, with the probe ON, dump: total ⊔ points seen, free-disjunct total,
deterministically-killed total, and **#⊔ points collapsed to ≤1 survivor / #⊔ points seen** (the
headline ratio). Run probe-OFF too for the verdict-preservation check.

Because the hard wine classes DNF at the deadline, the probe runs over a *bounded prefix* of the search
(the ⊔ points hit before the deadline) — that is fine: the question is the *kill rate* among the ⊔
points encountered, not exhaustive coverage. Log the deadline + #⊔-points-sampled so the ratio is
interpreted against its sample.

## GO / NO-GO (pre-committed)

- **GO — build edge-2-amortized** iff the full deterministic look-ahead **collapses most of wine's
  sampled free ⊔ points to ≤1 survivor** (a clear majority, say ≥70%), i.e. wine's disjunctions ARE
  deterministically resolvable and SP-B only missed it for using told-disjointness. Then the build-once
  deterministic-expansion cache is the real wine lever.
- **NO-GO — wine exhaustively closed** iff the look-ahead kills ~none / collapses few (comparable to
  SP-B's pruned=0), i.e. wine's free ⊔ points are genuinely nondeterministic and no deterministic
  mechanism resolves them. This is the honest end of the wine arc.

## Components

- `crates/owl-dl-tableau/src/hyper.rs`: `RUSTDL_DET_LOOKAHEAD_PROBE` engine flag (`det_lookahead_probe:
  bool`, mirror `precise_merge_deps` scaffolding); 3 `SearchStats` counters (`det_or_points`,
  `det_disjuncts_killed`, `det_or_points_collapsed`); the read-only look-ahead loop in
  `find_open_disjunction` (or just before the real ⊔ loop in `solve`), guarded by the flag — it counts,
  then control proceeds to the UNCHANGED real ⊔ branching.
- `crates/owl-dl-reasoner/tests/det_lookahead_gate.rs`: throwaway harness (wine sat-probes × probe
  OFF/ON, stats dump + verdict-preservation assertion).
- `docs/deterministic-disjunction-resolution-gate-results-2026-06-23.md`: the durable verdict.

## Success criteria

A decisive collapse-ratio per wine class (probe ON), verdict-preservation confirmed (ON==OFF), and a
GO/NO-GO call against the pre-committed bar — written to the verdict doc. Code reverted (does not
merge). On GO: the build-once deterministic-expansion cache becomes the next spec. On NO-GO: wine is
exhaustively closed across every known mechanism, and the build-once arc ends.

## What this is NOT

Not the build-once deterministic-expansion cache itself (that is the GO follow-on). Not a per-fact
dependency graph (edge 1 — probed dead, 1/77). Not a pruning mechanism that ships — the look-ahead is a
*measurement* of the prunable fraction; the production form would amortize it (build-once), which is
exactly why the per-branch wall-cost of the prior forward-checking attempt is irrelevant to this
go/no-go.
