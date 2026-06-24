# SP-B saturation-guided model construction — viability gate — Design

**Throwaway research gate** for the build-once redesign's SP-B. Decides GO/NO-GO on
committing months to saturation-guided model construction by measuring whether feeding the
now-complete B1–B2c saturation forcing into the wedge's ⊔ choice **collapses wine's branch
count to Konclude's regime**. Code does NOT merge; only the verdict doc lands.

## Why this gate, why now

The build-once core (SP-B → SP-C → SP-D) is the architecture that scales across all
ontologies (Konclude does wine in ~114 ms; rustdl's per-pair wedge DNFs at 1991 s). The
load-bearing assumption is that **saturation forcing, used as a deterministic search hint,
collapses the wedge's blind ⊔ branching** (wine: ~68796 branches building one model →
Konclude's ~tens/class). A prior read-only SP-B gate (`docs/sp-b-viability-gate-2026-06-23.md`)
was **inconclusive**: it filtered ⊔ choices against *immediate* told-disjointness only and
found 66–90% of wine's ⊔ points "free", concluding deep global forcing was the open question.
**B1–B2c now provide that deep forcing** (the saturator's derived-subsumer closure is complete
on wine — `classify --saturation-only wine` = 201 = full hybrid closure). This gate re-runs the
measurement with the *derived* closure as the oracle and **actually wires it into the search**
(not read-only) so the number is decisive.

This gate exists because the cheap "completeness-gated routing" shortcut was falsified
(`docs/sp-b2c-fp-gate-results-2026-06-23.md` + the corpus census): wine fires hundreds of
incomplete-on-construct rules (33 nominals, 117 ∀, 28 cardinality), so it cannot be soundly
routed to the sat-only path — the FP-of-completeness trap. The *sound* path to wine is
saturation-GUIDED construction where the tableau stays authoritative, which this gate tests.

## Mechanism — live-disjunct filtering at the ⊔ branch point

The wedge (`crates/owl-dl-tableau/src/hyper.rs`) clausifies the ontology; a disjunction
becomes a clause whose head atoms are the disjuncts. `find_open_disjunction` (hyper.rs:1821)
returns a matched `(clause_idx, node, binding)`; `solve` (hyper.rs:1707–1752) iterates the head
disjuncts `k in 0..head_len` in order, taking the first `Sat` with dependency-directed
backjumping.

Thread the saturation closure (the saturator's **derived-subsumer closure** + the **told-disjoint
table** — both already computed before the tableau runs, per classify.rs:541) into `HyperCache`,
the same way SP1.1 threaded `RoleHierarchy` via `with_sub_roles`. At the branch point, before
the `k`-loop, compute the **live** subset of head disjuncts:

> head disjunct `Dₖ` is **dead** at this node iff some class `C` in the node's label has a
> *derived* subsumer `G` (per the saturation closure) such that `G` is told-disjoint from `Dₖ`.

This is exactly B1's derived-subsumer × disjointness forced-disjunct logic, applied dynamically
with the node's full label as the context. Branch only over the live disjuncts:

- **1 live** → assert it deterministically, no branch (`disj_branches += 0`).
- **0 live** → immediate clash for this binding (the disjunction is unsatisfiable here).
- **≥2 live** → branch over the reduced set (already fewer branches than `head_len`).

The decisive difference from the prior inconclusive gate: filtering is against the **derived**
(fixpoint, transitive) subsumer closure — the deep forcing — not immediate told-disjointness.

### Scope of the throwaway wiring

- Env flag `RUSTDL_SAT_GUIDE` (default OFF; flag-OFF path is byte-identical to current `main`).
- Minimal threading: a borrowed/`Arc`'d view of `{derived_subsumers: per-class set, told_disjoint:
  pair-test}` into `HyperCache`, populated from the saturator's closure at `HyperCache::build`.
- The filter is applied only where `find_open_disjunction` yields a branch point. No change to
  generation (∃/≥n), merge (≤n), or blocking.

## Measurement protocol

Matched hard wine pairs (from the gate findings): `sat(AlsatianWine ⊓ ¬AmericanWine)`,
`sat(SweetWine)`, `sat(Zinfandel)`, `sat(RedWine)`. For each, with `RUSTDL_SAT_GUIDE` OFF then
ON, record from `HyperStats`: `branches_taken`, `disj_branches`, `restores`, `node_clones`, and
wall-clock. Use the existing per-pair probe tooling (the wedge `decide` path) under a deadline so
flag-OFF DNFs are bounded.

Report a table (pair × flag) of branch counts + wall, plus the collapse ratio per pair.

## Two soundness checks (distinct, do not conflate)

1. **Gate-internal verdict preservation — validity of the measurement.** For every measured pair,
   the flag-ON Sat/Unsat verdict MUST equal the flag-OFF verdict (run flag-OFF under a generous
   deadline; where flag-OFF DNFs, fall back to the known oracle verdict from the corpus closure).
   If pruning a "dead" disjunct flips a verdict, the prune was unsound (a bug in the filter) and
   the branch-count is meaningless — fix or abandon. This is the gate's own correctness guard.
2. **FP=0 (production-direction, NOT re-run here).** The eventual *production* SP-B is FP-safe by
   construction: the saturation filter is a search hint; the tableau stays authoritative; a bad
   hint costs a backtrack, never a false model; completeness comes from the tableau. Because the
   gate code does not ship, corpus FP=0 is not re-measured in this gate — check (1) guards it.

## GO/NO-GO (pre-committed)

- **GO** (commit to the production build-once core: SP-B wiring → SP-C build-once+KPSet → SP-D
  reuse) iff **near-total Konclude-class collapse**: total branch count drops to the hundreds
  regime (not tens of thousands) on **≥2/3 of the matched pairs**, AND a real wall drop
  (DNF/minutes → single-digit seconds on those pairs), AND **verdict-preserved** (check 1).
- **NO-GO** otherwise → bank B1–B2c as sound foundations on the integration branch; wine stays an
  accepted perf gap (knob `--pair-timeout-ms`, MISSED=0); reassess the build-once arc.

## Honest risk the gate will expose

The hint maps cleanly to **named-class** ⊔ points (the node's label contains named classes the
saturation closure knows). Wine's branch explosion may live on **generated successor** nodes whose
labels are synthetic/Tseitin classes with no named-class forcing. If the collapse is weak because
the explosion is on successors, that is a true NO-GO signal (this mechanism alone is insufficient;
successor-label forcing would be a deeper, separate increment). The gate is designed to reveal
this rather than hide it.

## What this spec is NOT

Only the throwaway gate. The production SP-B wiring (FP=0 corpus gate, full clause coverage,
successor-label handling if needed), SP-C build-once + KPSet classification, and SP-D sound
reuse-through-construction are each separate specs, gated on this GO.

## Components

- `crates/owl-dl-tableau/src/hyper.rs`: `RUSTDL_SAT_GUIDE` flag read; a `sat_guide:
  Option<SatGuide>` field on `HyperCache`/the solver; the live-disjunct filter in the
  `find_open_disjunction` branch path.
- A `SatGuide` view (`derived_subsumers`, `told_disjoint`) populated from the saturator closure
  at `HyperCache::build` (reasoner side, mirroring the `with_sub_roles` threading).
- Measurement harness: a throwaway test/bin that runs the 4 matched wine pairs × {OFF, ON} and
  dumps the stats table.
- Verdict doc `docs/sp-b-saturation-guided-gate-results-2026-06-23.md` (the only durable artifact).

## Success criteria

A decisive branch-count table (4 pairs × 2 flags), a verdict-preservation confirmation, and a
GO/NO-GO call against the pre-committed bar — written to the verdict doc. The gate code is
reverted (does not merge); the branch off `feat/build-once-redesign` is discarded after the
verdict lands.
