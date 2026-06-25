# Marker-saturator ⊔ failed-literal look-ahead — GATE RESULTS + VERDICT (2026-06-25)

**VERDICT: UNSOUND.** Wiring the full marker saturator (NomKey/ForallKey/MaxKey/
functional-merge) as a failed-literal propagator at the wedge's ⊔ points produces
**23 spurious unsatisfiable wine classes** (Konclude oracle: 0). The ~2× branch
reduction it appeared to deliver was achieved by dropping **live** disjuncts. The
strongest sound consequence relation we have, used as a per-branch failed-literal,
still has the branch-dependent-nominal-context hole — the 5th "sound-looking" wine
pruning lever this session refuted by the wine oracle (after increment-3, snapshot
cache, precise-merge-deps, det-pruning). → commit **SP-B** (closure-only
classification), since Konclude-class is mandatory.

## Setup

Branch `feat/marker-saturator-lookahead-gate` off `feat/build-once-redesign`.
- Task 1 (`665df92` + fix `7a50eda`): `owl_dl_saturation::seed_sat` — `build_base`
  (once) + `seed_unsat(atomic_seed, exists_seed)` (clone base, inject reserved
  synthetic `X`, run, read `X ⊑ ⊥`). The fix makes `seed_unsat` total: out-of-universe
  class ids (the tableau's complement/runtime synthetics) are skipped — a sound
  under-approximation for the drop direction.
- Task 2 (`4e26697`): `RUSTDL_SAT_LOOKAHEAD` (default OFF) ⊔ failed-literal hook in
  `hyper.rs` — at the MRV-chosen disjunction, drop `Dₖ` iff `seed_unsat(label-atomic +
  ∃-markers, Dₖ)`; branch-scoped, sound-under-seeded. Counters `lookahead_{calls,
  dropped,forced_single}`.
- Flag-OFF byte-identical confirmed (OFF run reproduces the MRV baseline exactly,
  `lookahead_calls=0`).

## Branch collapse (depth 256, adaptive budget OFF, 60 s/probe)

| probe | OFF (MRV baseline) | ON | factor | verdict |
|---|---|---|---|---|
| `sat(AlsatianWine ⊓ ¬AmericanWine)` | 1227 br | 582 br (calls 913, dropped 90, forced_single 0) | 2.1× | Sat |
| `sat(SweetWine)` | 12 366 br | 6345 br (calls 6645, dropped 540, forced_single 0) | 1.95× | Sat |

Only ~2× — short of the order-of-magnitude GO bar (SweetWine → low hundreds;
Alsatian → tens). `forced_single=0` on both: the drops never reduced a ⊔ to one
survivor, so the genuine nondeterministic core was untouched. On the branch numbers
alone this read as FLOOR. **But the wine FP gate — run first by mandate — overrides
it.**

## Wine FP gate (run FIRST — the proof) — apples-to-apples at 25 ms/pair

| | rustdl closure | unsat classes | FP* | MISSED |
|---|---|---|---|---|
| **OFF** (MRV baseline) | 653 = konclude 653 | **0** = konclude 0 | 0 | 0 |
| **ON** (look-ahead) | 434 | **23** (konclude 0) | 0\* | 0\* |

\* FP=0 is misleading here: the closure-diff harness excludes a class's pairs once
it is reported unsatisfiable (to avoid the `C ⊑ everything` blow-up), so the 23
spurious unsats *shrink* both reported closures (653 → 434) instead of inflating the
subsumption-FP count. The real soundness signal is **unsat: rustdl=23 vs konclude=0** —
23 wine classes the look-ahead drives to `⊑ ⊥` that are satisfiable per the
HermiT/Konclude oracle. A sound reasoner never reports a satisfiable class unsat.
**This is a false-positive subsumption (`C ⊑ Bot`) — unsound.** The OFF run at the
identical 25 ms deadline has unsat=0 and the full 653 closure, so the 23 are
introduced *by the look-ahead*, not by the deadline.

## Root cause

The failed-literal drop is computed from the node's **current label**, which deep in
the search contains branch **assumptions** (disjuncts chosen at earlier ⊔ points),
not just entailed types. `seed_unsat(assumptions ⊓ Dₖ)` correctly says "under these
assumptions, `Dₖ` clashes" — but the resulting drop, and especially the 0-survivor
clash, is returned with `clash_deps = body_deps`, which does **not** carry the
assumption-dependencies that caused the drops. So a backjump propagates the clash
*above* the assumptions and the class is marked unsat unconditionally. Same family as
the precise-merge-deps FP (`docs/precise-merge-deps-gate-results-2026-06-23.md`):
narrowing/mis-attributing clash dependencies on the nominal+merge fragment. The
marker saturator's TBox-level ⊥ is genuine; the unsoundness is in the *per-branch
dependency tracking* of the drop, not in the saturator.

The fix would require threading the node's branch-assumption dependency set into every
look-ahead drop's clash deps — i.e. the full per-fact dependency graph, the
FP-dangerous dep-tracking direction already NO-GO'd this session (per-fact dep graph:
1/77; precise-merge-deps: wine FP=232). The plan's optional retry (seed only
non-branch-dependent markers) collapses to the birth-label-only seed ≈ told-
disjointness, which SP-B already measured at pruned=0. No sound, collapsing variant
remains.

## Consequence

Per the pre-committed verdict and the user's "Konclude-class is mandatory":
**commit SP-B — closure-only classification.** The saturator already *outputs* wine's
complete closure (653) fast and FP=0; SP-B sidesteps model-building (and therefore
this per-branch dependency hole) entirely by establishing, via a per-construct
completeness audit of the saturator on wine's actual constructs, that `D ∉ closure(C)
⟹ not-subsumed` — skipping every refutation. Its gate is a completeness proof, not a
measurement.

Reinforced lesson (now 5×): on wine's nominal+merge architecture, **no per-branch
pruning lever is sound without full per-fact dependency tracking** (the NO-GO'd
direction); the corpus oracle — wine unsat count, run first — is the only ground
truth, and per-probe `Sat` results (both probes here returned Sat) do **not** witness
soundness.

## Disposition

Spike code stays on `feat/marker-saturator-lookahead-gate`, **unmerged** (the
`seed_sat` API is retained there in case SP-B or a future per-fact-dep effort reuses
it). Only this verdict doc is durable. `main` untouched.
