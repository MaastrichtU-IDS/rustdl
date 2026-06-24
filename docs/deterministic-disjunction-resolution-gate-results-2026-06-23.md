# Deterministic-closure ⊔-resolution gate — RESULTS + VERDICT — 2026-06-23

**Verdict: NO-GO — wine is exhaustively closed (4th convergent NO-GO this session).** The full
deterministic closure (Horn-fixpoint look-ahead) collapses only **18–34%** of wine's free ⊔ branch
points to ≤1 survivor — far below the pre-committed **≥70%** GO bar. Wine's disjunctions are
predominantly (66–82%) genuinely nondeterministic *even after complete deterministic propagation*, so
the build-once deterministic-expansion cache (Konclude edge 2) would not collapse wine's wall. The
build-once arc's wine-collapse pursuit ends here. Throwaway gate code (`spike/det-lookahead`) NOT
merged; only this verdict lands.

## What was measured

Read-only deterministic look-ahead at each ⊔ branch point (`RUSTDL_DET_LOOKAHEAD_PROBE=1`): for each
free disjunct `Dk`, `save()` → apply `Dk` → `horn_fixpoint(FIXPOINT_ITERS)` → record clash →
`restore()`; count ⊔ points, disjuncts killed by deterministic consequence, and ⊔ points collapsed to
≤1 surviving disjunct. The richer-than-told-disjointness oracle SP-B lacked. Wine sat-probes, depth
256, adaptive-budget OFF, 60 s/class (the hard classes DNF, so the probe samples a bounded prefix —
the *kill rate* among sampled ⊔ points is the measure).

## Results (probe ON)

| class | ⊔ points sampled | disjuncts killed | ⊔ points collapsed (≤1 survivor) | collapse ratio | verdict |
|---|---|---|---|---|---|
| Wine | 18769 | 6840 | 3420 | **0.18** | Stalled |
| AlsatianWine | 18495 | 6654 | 3327 | **0.18** | Stalled |
| SweetWine | 17104 | 11518 | 5759 | **0.34** | Stalled |
| Zinfandel | 16808 | 8302 | 4151 | **0.25** | Stalled |

Probe OFF: all four Stalled, counters 0. **Verdict-preserved** (ON Stalled == OFF Stalled; the
look-ahead is read-only — confirmed by the Task-2 terminating unit test `det_lookahead_probe_counts_
kill_and_preserves_verdict` (ON==OFF==Sat) plus the corpus DNF-preservation here: the probe never
spuriously terminated a class).

## Interpretation

The full deterministic closure is genuinely richer than SP-B's told-disjointness — it kills 6.6k–11.5k
disjuncts per class and collapses 18–34% of ⊔ points (vs SP-B's pruned=0). But **the GO bar is missed
on every class**: 66–82% of wine's free ⊔ points retain ≥2 live disjuncts after complete Horn
propagation. Those are irreducibly nondeterministic choices — no deterministic mechanism (the strongest
form being this full fixpoint look-ahead) resolves them. So the build-once deterministic-expansion
cache, even built perfectly, would leave the large majority of wine's branching intact ⟹ no wall
collapse.

## The wine arc is exhausted — 4 convergent NO-GOs

| # | mechanism | result |
|---|---|---|
| 1 | SP-B saturation-guided ⊔ forcing (told-disjointness) | pruned=0 (0% of free ⊔) |
| 2 | Precise ≤n merge-causation backjumping | corpus FP=232; even FP-safe → 1/77 hard classes |
| 3 | Per-fact dependency graph (edge 1) | would crack 1/77 even if FP-safe |
| 4 | Deterministic ⊔-resolution / full closure (edge 2) | 18–34% collapse, < 70% bar |

All four converge on the same root, consistent with the prior `wine-wall-bjgap1-genuine` analysis:
**wine's wall is irreducible disjunctive nondeterminism in a nominal architecture.** It is not a
forcing gap (1, 4), not a backjumping gap (2, 3). No single sound lever collapses it; the only
remaining theoretical option is Konclude's *entire* combined architecture (many mechanisms at once), a
ground-up reasoner rewrite repeatedly assessed as not-worth-it for one fixture's wall.

## Consequence

- **NO-GO**: do not build the build-once deterministic-expansion cache for wine. The `spike/det-lookahead`
  code is discarded (not merged).
- **The build-once arc's wine-collapse goal is closed.** Positive deliverable retained: **B2c** made
  the saturator complete-in-output on wine (FP=0, banked foundation, merged on `feat/build-once-redesign`).
- Wine stays an accepted perf gap (`--pair-timeout-ms`, MISSED=0). `main` pristine; the full research
  record (specs, plans, 4 gate verdicts, the FP diagnosis) is on `feat/build-once-redesign`.

## Corpus extension — is det-resolution a PERF lever on the *terminating* SROIQ fixtures? (NO)

Follow-up (option 1): the wine result is a wall-DNF case. Does the same deterministic look-ahead
collapse ⊔ points on the SROIQ fixtures that DO terminate (where a branch cut → wall win)?
Per-class sweep, `RUSTDL_DET_LOOKAHEAD_PROBE=1`, 5 s/class (`tests/det_corpus_gate.rs`, throwaway):

| fixture | wall (hist.) | classes | ⊔ points (total) | collapse ratio | det-resolution |
|---|---|---|---|---|---|
| ore-15672 | fast | 82 | 119 | 0.14 | little branching to prune |
| ore-10908 | ~5 s | 692 | 1833 | **0.995** | effective — but already fast |
| sio | ~32 s | 1585 | 2376 | **0.000** | useless (nondeterministic) |
| wine | DNF | 137 | ~18k/class | 0.18–0.34 | useless |

**The catch-22, confirmed corpus-wide:** deterministic resolution collapses disjunctions *exactly
where they are already cheap* (ore-10908: 99.5%, but the fixture is already ~5 s with only 1833 ⊔
points — pruning saves negligible wall) and *fails exactly where the wall is* (sio 32 s → 0% collapse;
wine DNF → 18–34%). No fixture has **both** a wall **and** collapsible disjunctions. The walls are made
of irreducibly-nondeterministic ⊔ choices (value-partitions / genuine choice) that no deterministic
closure resolves.

**NO-GO for the build-once deterministic-expansion cache as a corpus perf mechanism.** ore-10908
clears the 70% bar numerically, but the bar was a proxy for "a fixture where pruning wins wall" — which
does not exist. (5th convergent NO-GO.) **Positive characterization retained:** the probe cleanly
partitions the corpus's disjunctions into *deterministic* (ore-10908, structurally determined) vs
*genuinely nondeterministic* (sio/wine) — and shows Konclude's deterministic-expansion edge is NOT the
source of its sio/wine speed either (those are 0%/low-collapse). Konclude's sio/wine win must come from
its other mechanisms (nominal architecture + the combination), reinforcing that no single edge is the
lever.

## Method note

Four cheap viability gates this session, each catching a multi-month dead-end *before* building it
(zero months sunk on a dead lever). The discipline — measure the load-bearing assumption first,
corpus-gate every soundness claim, pre-commit the bar — is what let the arc exhaust wine's lever space
quickly and honestly rather than sinking a quarter into any one of them.
