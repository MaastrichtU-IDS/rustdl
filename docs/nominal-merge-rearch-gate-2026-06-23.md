# Nominal/merge dependency re-architecture — gate verdict: NO-GO (lever targets dead-for-wine code) — 2026-06-23

The user committed to the one remaining wine lever: re-architect nominal/merge
dependency tracking so backjumping survives (the `merge_with_cause` →
`birth_deps` fold blamed for bjgap≈1). Per the advisor, gated with a zero-new-code
experiment BEFORE building. **NO-GO — the lever targets code that does not run on
the wine per-pair wall.**

## The gate (advisor-designed, existing flag, no engine code)

`HyperEngine::with_nn_taint_disabled` (`hyper.rs:802`) drops the merge-causation
`cause_deps` — it IS "the engine without the fold." Wired temporarily behind
`RUSTDL_NN_TAINT_DISABLED` into `decide_with_stats`, ran the matched test
`decide_pair_probe(AlsatianWine, AmericanWine)` = `sat(AlsatianWine ⊓ ¬AmericanWine)`
(depth 256, adaptive OFF, 60s, 2 GiB stack):

| config | result | branches (disj / merge) | wall |
|---|---|---|---|
| FOLD-ON (baseline) | **Sat** | 68796 (56699 / 12097) | 3.1 s |
| FOLD-OFF (fold dropped) | **Sat** | **68796 (56699 / 12097)** | 3.2 s |

**Byte-identical.** Dropping the merge-causation fold changes nothing.

## Why: nominals are not wired into the per-pair wedge

`decide_with_stats` (`reasoner/src/lib.rs:1705-1717`) builds the per-pair engine with
`with_double_blocking` + `with_precise_card_deps` + `with_adaptive_budget` — but
**NOT `with_nominals`** (that is applied only on the consistency path `lib.rs:1010`
and the label-cache wedge `lib.rs:2007`). So on the wine per-pair path the NN-rule
never fires, `merge_with_cause` only ever receives EMPTY `cause_deps` (the `≤n`
caller), and the fold is a **complete no-op** — hence byte-identical branches. The
prior "bjgap≈1 / merge_with_cause defeats backjumping" diagnosis does not apply to
the per-pair wall; it was about a path the wine wedge doesn't take. (Consistent with
the reuse-trap doc's note that `with_nominals` is absent from `decide`, and that
wiring it in "does NOT help" — 166k vs 168k branches.)

## What the wine per-pair wall actually is

The matched pair is a **Sat model-search**: 68796 branches dominated by **disjunction
(56699)** + `≤n`-merge (12097), finding a satisfying model (the model exists; it's
just slow to reach). Implications for the lever families:

- **Dependency-tracking / backjumping / conflict-learning (incl. this re-arch):**
  cannot help a SAT model-search — they prune UNSAT subtrees; here there is no unsat
  to prove, only a satisfying assignment to find. And the `≤n` half already has
  `precise-card-deps`. The nominal-merge fold is a no-op (above).
- **Branch ordering / model-guided heuristics (the only family that fits a SAT
  search):** MOMS reverted, semantic branching 1.0×, forward-checking net-negative
  (all prior measurements). Model-guided ordering remains the one un-fully-explored
  sliver, but it is a SAT-search ordering heuristic — NOT the nominal/merge
  re-architecture, and prior ordering attempts all measured out.

## Verdict

The nominal/merge dependency re-architecture is **NO-GO for wine**: the per-pair
wedge is nominal-free, so the merge-causation fold (the target) is provably inert
there (byte-identical branches). The real per-pair cost is disjunction branching on a
SAT model-search, where the entire backjumping/learning family is the wrong tool and
the ordering family is measured out. **Every wine engine lever is now
measurement-closed** (SP0–SP3, model reuse, and this). Recommendation stands: accept
the wine wall (one fixture, MISSED=0, `--pair-timeout-ms` knob) — adaptive budget
already early-cuts these diverging SAT searches in production.

Throwaway probe (reverted): `RUSTDL_NN_TAINT_DISABLED` gate + `wine_nn_taint_gate`
test; `with_nn_taint_disabled` returned to `#[cfg(test)]`. Working tree clean.
