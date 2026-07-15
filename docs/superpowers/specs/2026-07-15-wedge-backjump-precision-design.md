# Dense-SROIQ deep completeness — wedge backjump-precision (design)

**Status:** advisor-reviewed (2026-07-15) — for user review
**Parent roadmap:** `docs/superpowers/specs/2026-07-13-dense-sroiq-tractability-roadmap.md`
(SP2 pivot / deep-R&D branch). Predecessors: SP0/SP1 (`docs/2026-07-13-ore_ont_10019-stall-findings.md`), SP2 (`docs/2026-07-14-sp2-nogood-findings.md`, DEAD).
**Goal:** decide `ore_ont_10019`'s 33 stalled classes (and the dense-SROIQ tail like it) in the wedge within budget — closing the completeness gap Konclude (90 ms) / HermiT (360 ms) already clear, where rustdl stalls.

## Diagnosis (established — no further H1-vs-H2 measurement needed)

**The stall is H2: disjunctive-DFS thrash over a blocking-bounded, redundantly
re-explored state space. H1 (unbounded model / blocking failure) is ruled out.**
Evidence (advisor-verified against code):
- **"depth ~142" is the disjunctive-decision stack, not model/tree depth.**
  `solve` decrements `depth` from `HYPER_WEDGE_DEPTH=256` once per ⊔/≤n decision;
  `track_depth` records `init_depth - depth + 1` = branch-decision-stack length.
  ∃-expansion happens *inside* `horn_fixpoint` per frame, independent of it. So
  the depth number is an H2 quantity by construction.
- **Blocking gates ALL generation, including the ⊔ rule.** `find_open_disjunction`
  skips `is_blocked` nodes (both MRV and legacy scans); a docstring records the
  earlier fix of the exact H1 pathology (blocked-node ⊔ once drove depth
  256→32768, all-stalled). So blocking is not failing to cap the tree.
- **Per-frame saturation terminates far under `FIXPOINT_ITERS=100_000`** (SP0:
  ~2000 match-attempts/branch; SP1: ~40). The `Stalled` comes from `solve`
  reaching `depth==0` with an open disjunction, or the deadline / `is_diverging`
  cut — a *search-depth* stall, not a saturation stall.
- **`restores ≈ branches`** (near-every branch clashes and is undone = DFS
  backtracking); **SP2 `revisit_frac ≈ 1.0`, `reusable_nogood_frac ≈ 0.9998`** (a
  *bounded* set of clash states re-explored, not a stream of novel deep
  contexts). **SP1** made branches 56× cheaper → search went *deeper* (75→138)
  with zero verdict change ⇒ limiter is disjunctive depth/breadth, not model size.

MRV disjunct-ordering (default-ON) and `sat_lookahead` (tested ON here) are both
exhausted — 14 sat / 33 stalled / depth 142 unchanged. SP2's node-local UNSAT
no-good caching is DEAD (152-ont sweep: zero benefit, net-negative). Those levers
are spent; the remaining ones are of a different kind.

## The actionable hypothesis (H3b) — backjump degradation

`solve`'s dependency-directed backjumping is only as strong as the `clash_deps` it
propagates. Two sites deliberately widen to `DepSet::ALL`: the `≤n`
partition-exhaustion site (`card_clash_deps`) and merge-taint. **When a
disjunctive clash inherits an `ALL` dep-set, the backjump test
`!child_deps.contains(d)` is always false ⇒ no backjump ⇒ chronological DFS to
the depth cap.** SP2's large `bjgap_shadow` (the *precise* ideal jump distance)
alongside `restores ≈ branches` hints the *real* backjump under-performs the
precise one on these classes. **This is distinct from what SP2 killed** (that was
node-local no-good caching; this is dependency precision for the existing
backjumper). If confirmed, tightening those dep-sets restores dependency-directed
backjumping, which prunes *depth* — exactly what no-goods could not.

Also in scope as success-criterion context (not the wedge fix itself):
- **H3a — the completeness tail.** A wedge `Stalled` → `HyperVerdict::Unknown` /
  `LabelOracle::NoVerdict` → classify's `search.rs` fallback (roadmap:
  non-terminating). Any wedge win must be read against what happens to
  still-`NoVerdict` classes; bounding that fallback is the honest floor.
- **H3c — `solve_at_most` partition blowup** — real but minority (the top-33
  stalled classes are `merge=0`; only `PrimaryAmineGroup` branches on `≤n`). Not
  the first target.

## Phase 1 — backjump-precision probe (the decisive, fix-selecting measurement)

A small, read-only, env-gated (`RUSTDL_BACKJUMP_PROBE`, default OFF) instrument in
`solve`, run on ONE stalled class (`HydroxylGroup`), logging per disjunctive
clash / frame:
- `(depth, self.nodes.len())` — confirms the model plateaus while depth climbs
  (closes residual H1: bounded model + rising decision depth = H2, definitively).
- the **real `clash_deps`**: its cardinality and its `overflow`/`ALL` flag, vs the
  **shadow (precise) dep-set** at the same clash (the shadow-dep infra from SP2's
  probe already computes the precise twin) — i.e. how often the real backjump
  dep-set is `ALL` (or much larger than the shadow), and the resulting real vs
  shadow backjump distance (`bjgap`).

**Decision criterion:**
- **Real deps frequently `ALL`/over-wide while the shadow is precise** ⇒ backjump
  is being crippled ⇒ **Phase 2 = Fix #1 (dependency-precision / backjump repair).**
- **Real deps already precise & small, yet DFS still can't prune** ⇒ the
  disjunctive breadth is intrinsic ⇒ **Phase 2 = Fix #2 (absorption / unit
  propagation)**, with **bound-the-tail** as the honest fallback.

Deliverable: a findings note + the go-to fix. Cheap (~5–10 lines under a flag, one
class, one run), like SP0.

## Phase 2 — the fix (deferred until Phase 1 selects it)

- **Fix #1 — dependency-precision / backjump repair.** Replace the blanket
  `DepSet::ALL` at disjunctive-clash-relevant sites with a precise dep-set where
  one is soundly derivable, so `solve` can backjump. **FP-CRITICAL (opposite of
  SP2's failure mode):** a dep-set that is too *small* (drops a decision level
  that actually contributed) makes the backjump skip a branch that mattered → a
  branch is pruned that was reachable → **unsound → FP**. So Fix #1's gate is
  FP=0 (curated + non-Horn `ore_ont_13723` oracle) with over-tightening as the
  hazard; the precise dep-set must be a proven *superset* of the true cause, never
  a guess. (The existing `precise_card_deps` work — default-ON, a sound
  over-approximation of the `≤n` clash cause — is the model to extend.)
- **Fix #2 — absorption / unit propagation (BCP).** Make disjunctions not become
  branch points (Konclude's real speed source): propagate forced literals across
  the 25 disjunctive clauses + 55 disjointness axioms before branching. Larger
  build; orthogonal to no-goods; gated FP=0/MISSED=0.
- **Bound-the-tail (honest floor).** If #1/#2 underperform, make the
  `Stalled → NoVerdict → search.rs` path bounded so classify returns
  sound-incomplete fast instead of burning the deadline. A legitimate outcome.

## Soundness invariant

- **FP=0 must never regress**, gated on curated **and** the non-Horn adversarial
  oracle (`ore_ont_13723` vs Konclude). For **Fix #1 this is the primary hazard**
  (over-tight deps → unsound backjump → FP) — the inverse of SP2, where over-prune
  was only a MISS. Every dep-set narrowing must be a proven sound over-approximation
  of the clash cause.
- **MISSED=0 / byte-identical curated closures** as the completeness guard.
- Each shipped fix: default-OFF flag → validate FP=0 AND MISSED=0 (curated +
  non-Horn oracle) + no curated wall regression → default-ON in a separate
  reviewed commit.

## Out of scope

- Node-local UNSAT no-good caching (SP2 — DEAD, 152-ont sweep; do not revive).
- Konclude-style global/status caching (reintroduces the `reuse-trap-A1`
  cross-context FP surface on the non-Horn fragment — high risk, not this branch).
- Inverse/nominal-specific completeness (this ontology has neither).
- `≤n` partition-blowup (H3c) as the first target (minority on the stalled 33).

## What "done" means

Phase 1 always ships (a findings note + the selected fix direction, like SP0).
Phase 2 ships iff its gate passes; if the selected fix underperforms, the honest
deliverable is the bound-the-tail floor + a documented "dense-SROIQ tail needs
Konclude-class absorption, deferred" — a legitimate, evidence-backed stopping
point, not a failure.
