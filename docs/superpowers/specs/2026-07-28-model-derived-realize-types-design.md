# Model-derived realization types (HermiT-style deterministic read-off)

**Date:** 2026-07-28
**Status:** Design — approved for implementation planning
**Gate flag:** `RUSTDL_MODEL_DERIVED_TYPES` (default **OFF** until the
differential+oracle gate passes; then flip ON, mirroring #57 / backfold)

## Problem

`realize` decides each `(individual, class)` type by an independent
`{a} ⊓ ¬C` unsat probe — O(individuals × classes) separate tableau/wedge
searches. On the ORE ABox tail this is unbounded: of 58 ontologies rustdl
DNFs at realize (750 ms/pair, 120 s cap, isolated), **Konclude completes 54/58
(avg 6.8 s) and HermiT 21/58**, while rustdl completes 0. The gap is
architectural: Konclude reads types off one saturated model; rustdl probes
every pair. Poster child `ore_ont_10080` — rustdl DNF, HermiT DNF, **Konclude
958 ms / 1884 assertions**.

HermiT's optimization is the target: types that are **deterministically
derived** (hold in every model) are read off one model; only the "possible"
(branch-dependent) types need a confirmation probe.

## Scope (increment-1 only)

This spec covers **only** the deterministic read-off. It is explicitly **not**
a fix for ontologies where building even one model is the bottleneck.

**Go/no-go measurement (2026-07-28, temporary `RUSTDL_WITNESS_PROBE`, reverted):**
of the 58 realize-DNF onts, a single witness model builds within 8 s on only
**15/58** (avg 1.5 s, max 17 s); **30/58** Stall (model construction *is* the
search that DNFs) and **13/58** never reach model-build (an upstream
inconsistency pre-check / build overruns its deadline). Therefore:

- **In scope:** the 15 hard-tail onts that build a model yet still DNF realize
  (their DNF is caused by *positive* probing, which read-off eliminates), plus
  the broad "model-builds-but-slow" completing population (benchmark: ~50 onts
  at 5–30 s, 8 at 30–120 s).
- **Out of scope (increment-2):** the ~43 that cannot build one model in time.
  That is Konclude-style incremental model construction — the disjunctive-search
  frontier already measured out along several axes (CDCL, semantic branching;
  memory `wine-wall-bjgap1-genuine`, `fix2-semantic-branching-nogo`).

## Mechanism

The determinism boundary already exists in the engine. Every label on a node
carries a `DepSet` (`owl-dl-tableau/src/graph.rs`, `Node::deps_of_label`,
`DepSet = SmallVec<[u32;1]>`): the set of `branch_id`s whose ⊔/choose decisions
the label's derivation depended on. **Empty DepSet ⟺ derived by deterministic
rules with no branch dependence ⟺ entailed in every model.**

The `#57` pseudo-model (`RUSTDL_PSEUDO_MODEL`, default ON) already builds ONE
`Sat` witness model per realize call (`base_model_types` → `build_seeded_engine`
+ `decide_with_deadline` + `seeded_individual_labels`, `lib.rs:3335`) but
discards the DepSets, keeping only the complete label set, which it uses solely
to **prune** negatives (`class ∉ witness ⇒ Ok(false)`, `realize.rs:294`).

This design keeps the DepSets and reads off the deterministic subset as
positives. `instance_check_with_closure` becomes a three-way decision
(replacing today's told-true → prune-false → probe):

1. `class ∈ deterministic_labels(ind)` → `Ok(true)`  *(new read-off, no probe)*
2. `class ∉ complete_witness_labels(ind)` → `Ok(false)`  *(existing #57 prune)*
3. otherwise → `{a} ⊓ ¬C` probe  *(residual: in-witness but branch-dependent)*

Placed immediately after the existing told-closure `Ok(true)` fast path. The
read-off is a strict superset of told-closure (it also captures deterministic
*tableau* derivations, e.g. ∀-propagation onto the individual).

## Components (isolated units)

**(a) Tableau accessor** — `owl-dl-tableau/src/hyper.rs`
`HyperEngine::seeded_individual_deterministic_labels(idx) -> Option<Vec<ClassId>>`.
The witness model is built by the **hyper engine**, whose `HyperNode` carries
`label_deps: Vec<DepSet>` (parallel to `labels`; `DepSet` = `u128` decision-level
bitset, `EMPTY` ⟺ derived with no branch decision, degrades to `ALL`/overflow —
non-empty — on merge taints / >128 levels, by design "never under-counts").
Emptiness test: `dep.highest_level().is_none()` (correctly excludes `ALL`, which
reports `Some(127)`). Returns `labels[i]` where `label_deps[i]` is empty.

**Merge guard (FP-critical).** `merge_with_cause` folds the merge-causation dep
into moved labels **except for the `≤n`/functional caller, which passes
`cause_deps = EMPTY`** (backjumping soundness there is handled separately via
`card_clash_deps → DepSet::ALL` at clash time, NOT on the resting `label_deps`).
So a branch-triggered `≤n`/functional merge can move a label onto a named
individual's node while it keeps its original `EMPTY` dep — reading that as
entailed would be a false positive. The accessor therefore reads off **only for
individuals whose node was untouched by any merge**: `resolve(idx) == idx` (not
merged away) AND the representative absorbed no merge. This needs a lightweight
per-`HyperNode` `absorbed_merge: bool`, set on the survivor inside
`merge_with_cause`. Merged individuals fall back to full probing (sound; they
just don't get the read-off speedup). Read-only apart from that one flag.

**(b) Reasoner model view** — `owl-dl-reasoner/src/lib.rs`
Refactor the single engine build into one internal method returning both views
as a struct `WitnessModel { complete: Vec<HashSet<ClassId>>, deterministic:
Vec<HashSet<ClassId>> }` — so the model is constructed **once**, not twice. The
existing `base_model_types` becomes a thin wrapper returning `.complete` (its
current prune consumer is untouched); a new `base_model_deterministic_types`
returns `.deterministic`. `Sat`/`None` handling unchanged (`None` on
`Unsat`/`Stalled`/deadline ⇒ no read-off, no prune, fall through to probing).
`PreparedOntology` exposes both via siblings of `realize_base_model_types`.
Depends only on accessor (a).

**(c) Realize loop** — `owl-dl-reasoner/src/realize.rs`
`realize_tableau_internal` builds the witness model once, passes
`deterministic: &HashSet<ClassId>` and `complete: &HashSet<ClassId>` into each
per-individual probe. `instance_check_with_closure` gains the read-off arm.
Synthetic-class filtering (`num_user_classes`, unsat exclusion) is unchanged and
applies to read-off types identically.

**Data flow:** build one `Sat` model → `(complete, deterministic)` label sets →
per `(ind, class)`: told? / deterministic-read-off? / prune? / probe.

## Soundness

Two legs:

1. **Analytical.** Empty DepSet ⟺ no branch-decision dependence. The hyper
   `DepSet` system is designed to **over-approximate** (it "never under-counts",
   degrading to `ALL` on uncertainty) — the exact property the read-off needs
   (reported-empty ⟹ truly-empty requires reported ⊇ true). The one path where
   that over-approximation is *deliberately not applied to the resting
   `label_deps`* is the `≤n`/functional **merge** (`merge_with_cause`,
   `hyper.rs:3759`), which passes `cause_deps = EMPTY` and recovers backjumping
   soundness via `card_clash_deps → DepSet::ALL` at clash time instead. Because a
   read-off inspects `label_deps` (not clash deps), this path is a genuine FP
   risk and is closed structurally by the **merge guard** in component (a): an
   individual touched by any merge is excluded from read-off and probed instead.
   With the guard, a read-off label is on a merge-untouched named node with empty
   `label_deps` ⟹ derived purely from decision-level-0 facts ⟹ entailed.

2. **Empirical (the real gate).** A dep-tracking under-report bug cannot be
   ruled out analytically, and for a positive read-off an under-report is a
   silent false-positive subsumption (the crown-jewel FP=0 invariant). The
   read-off is **verdict-preserving by construction** (a deterministic label is
   entailed, so probing would also return true; branch-dependent labels are
   still probed), therefore **ON-vs-OFF `realize --json` byte-identity is
   equivalent to "every read-off was sound."** That differential is the gate.

## Test plan

- **Differential (default gate).** `RUSTDL_MODEL_DERIVED_TYPES` ON vs OFF,
  `realize --json` byte-identical, on onts where DepSets populate:
  `ore_ont_13723` (non-Horn FP canary), a disjunctive-ORE sample, and the 15
  model-building onts of the 58. *EL/Horn onts are deliberately excluded from
  the gate — everything is empty-dep there, so a dep under-report bug is
  invisible on exactly the inputs one would reach for first.*
- **Oracle.** Read-off types ⊆ HermiT/Konclude realization (FP=0) on the same
  disjunctive set (reuse the ROBOT/Konclude harness in `rustdl-scratch`).
- **Unit (TDD, negatives-first).** Fixtures covering: (i) a deterministic type
  (∀-propagated onto an individual, empty dep → read off); (ii) a
  disjunction-dependent type (non-empty dep → must still probe, must NOT be read
  off); (iii) **the merge-guard FP case** — a branch-triggered `≤n`/functional
  merge that moves a label onto a named individual with `EMPTY` dep; read-off
  must NOT emit it (individual is merge-touched → probed). Cases (ii)/(iii) are
  the FP guards and are written first.
- **Regression.** Curated-corpus `realize` verdict-identical (FP=0/MISSED
  unchanged).

## Parameters & limits (stated, not assumed)

- The witness deadline `RUSTDL_PSEUDO_MODEL_WITNESS_MS` (default 1000 ms) bounds
  which onts build a model in time. Some of the 15 need a higher budget to build
  at all (max observed 17 s). The read-off can only help onts whose model
  builds, so realizing the full 15 may require raising this budget — a tuning
  parameter, deliberately *not* assumed to be captured at 1000 ms. Any change to
  the default is a separate, measured decision.
- Completeness is preserved (read-off adds only entailed positives; residual
  branch-dependent types are still probed). No new MISS is possible from this
  change.

## Rollout

Ship default-OFF. Run the differential+oracle gate. If byte-identical on the
disjunctive set with oracle FP=0, flip `RUSTDL_MODEL_DERIVED_TYPES` default-ON
and record the corpus verdict-identity (mirroring how #57 and the label-cache
backfold shipped). `=0` always reverts to the pre-change per-pair-probe path.
