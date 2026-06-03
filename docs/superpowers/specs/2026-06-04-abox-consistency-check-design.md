# ABox Consistency Check — Design

**Status:** spec, awaiting plan
**Date:** 2026-06-04
**Trigger:** `family.ofn` / `family-stripped.ofn` are flagged inconsistent by HermiT and Konclude in <1 s. `rustdl consistent` times out at 180 s; `rustdl classify` emits a normal-looking 324-pair hierarchy with no unsat. The user-visible trap: a reasoner that confidently classifies an ontology that two oracles reject.

## Goal

Detect a tractable class of ABox-driven ontology-level inconsistencies via a sound, fast pre-check that runs **before** the tableau, and wire it into both `is_consistent` and `classify` so neither path can emit a normal verdict on an inconsistent input it could have caught cheaply.

Sound under-approximation, same model as our EL saturator and the D1 datatype drop: a positive verdict (`Inconsistent`) is unconditional; an `Unknown` verdict falls through to the existing tableau path.

## Non-goals

- The functional-role-merge inference step needed for family-style multi-step clashes (`∃hasSex.Male ⊓ ∃hasSex.Female + Functional(hasSex) → Male⊓Female → ⊥`). If our seven sound patterns happen to close family, we'll celebrate; otherwise that's a separate scoping target.
- ABox realization (per-individual most-specific type).
- Concrete-domain reasoning on `DataPropertyAssertion` literal values (D5 covers the TBox side; ABox-level datatype conflicts are out of scope).

## Architecture

One new pass, `abox_consistency_check`, runs after `collect_abox` finishes (i.e., after `PreparedOntology::from_internal` has populated the `Abox` struct and the EL saturator has built its closure). The check returns:

```rust
pub(crate) enum AboxVerdict {
    Inconsistent { reason: ClashReason },
    Unknown,
}
```

Cached in a `OnceCell<AboxVerdict>` field on `PreparedOntology` so classify + the per-pair loop don't recompute.

Wiring:

- `is_consistent_internal_full`: consult `prepared.abox_verdict()`. If `Inconsistent`, return `Ok((false, stats))` immediately. If `Unknown`, fall through to the existing `run_satisfiability(Top)`.
- `classify_top_down_with_timeout`: same consult. If `Inconsistent`, return a `Classification` with every named class marked unsatisfiable and a new `inconsistent: true` flag in `ClassificationStats`. Konclude's behaviour on inconsistent input is "every class is unsatisfiable" — we mirror that. If `Unknown`, fall through to today's path.
- New CLI banner line: `# abox_check: inconsistent | unknown | skipped`. `consistent` subcommand benefits transparently.

Sound by construction: each pattern below is a direct semantic clash. No inferred subsumption gets created, so a positive verdict is unconditional.

## Clash patterns

Seven sound patterns, in cost order.

### P1 — Direct `⊥` assertion

`ClassAssertion(C, a)` where the EL closure derives `C ⊑ ⊥`. Iterate `abox.class_assertions`; for each `(individual, class_concept_id)`, decompose to an atomic `ClassId` if the concept is `Atomic(c)` and consult `Subsumers::is_unsatisfiable(c)` (already populated by the saturator). Non-atomic class expressions are ignored at P1 — they'd require running a satisfiability probe, which defeats the cheap-pre-check premise.

### P2 — Pairwise class-disjointness on the same individual

Per individual, compute the set of asserted classes plus their EL-closure subsumers. Test every pair of asserted-class roots against the precomputed `told_disjoint_pairs` table in `owl-dl-core::told`. Per-individual type counts are tiny in realistic ABoxes (single digits), so the inner pairwise cost is negligible.

### P3 — NegativeOPA vs OPA

Build a `HashSet<(IndividualId, RoleId, IndividualId)>` of positive assertions from `abox.property_assertions`. For each `NegativeObjectPropertyAssertion(R, a, b)` recovered from the in-memory `∀R.¬{b}` form in `abox.negative_property_assertions`, test set membership of `(a, R, b)`. Also propagate up the role hierarchy: a positive assertion on any super-role of `R` implies the assertion on `R`.

### P4 — SameAs ∩ DifferentFrom direct

Build union-find over `abox.same_pairs`. For each `(a, b) ∈ abox.different_pairs`, test `find(a) == find(b)` → inconsistent.

### P5 — Functional-role two-distinct-witnesses

For each `FunctionalObjectProperty(R)` and each pair `(a, R, b1), (a, R, b2) ∈ abox.property_assertions` with `b1 ≠ b2`: functionality forces `same(b1, b2)`. Add the merge edge `(b1, b2)` to the P4 union-find, then re-check P4. Functionality does **not** propagate along role hierarchy in either direction — only merges within `R`'s own asserted pairs count. `InverseFunctionalObjectProperty(R)` is the dual: scan `(a1, R, b)` and `(a2, R, b)` pairs and merge `(a1, a2)`.

### P6 — Asymmetric / Irreflexive violations

For each `AsymmetricObjectProperty(R)`: scan for `R(a,b) ∧ R(b,a)` (after SameAs merge).
For each `IrreflexiveObjectProperty(R)`: scan for `R(a,a)` directly, plus `R(a,b)` where `find(a) == find(b)` in the SameAs union-find.

### P7 — Domain/range disjointness propagation (stretch)

For each `ObjectPropertyDomain(R, D)` and assertion `R(a, _)`, add `D` to `a`'s asserted-class set. Same for `ObjectPropertyRange(R, D)` and the target. Re-run P2 on the augmented per-individual type sets. This is the pattern most likely to close real-corpus clashes (range(`hasFather`)=Man, range(`hasMother`)=Woman, an individual with both relationships, downstream disjointness through `Man⊓Woman` via functional `hasSex` collapse). The functional-collapse step is **out of scope** — so `family-stripped` may or may not close here. Honest stretch goal.

## Data flow

```
                       InternalOntology
                              │
              ┌───────────────┴────────────────┐
              │  PreparedOntology::from_internal
              │  (absorb, NNF, ABox collection,│
              │   EL closure already built)    │
              └───────────────┬────────────────┘
                              ▼
              ┌─────────────────────────────────────┐
              │ abox_consistency_check(&self)       │
              │  → AboxVerdict                      │
              │                                     │
              │  1. Collect per-individual type-set │
              │     - asserted classes              │
              │     - EL-closure subsumers          │
              │  2. Run P1..P6 (cheap)              │
              │  3. If still Unknown:               │
              │     augment via P7 domain/range,    │
              │     re-run P2                       │
              │  4. Return Inconsistent | Unknown   │
              └─────────────────┬───────────────────┘
                                │
            ┌───────────────────┴──────────────────────┐
            ▼                                          ▼
  is_consistent_internal_full                  classify_top_down_*
   ├─ Inconsistent → (false, stats)             ├─ Inconsistent → mark every
   └─ Unknown      → run_satisfiability(Top)    │   class unsatisfiable;
                                                │   set stats.inconsistent
                                                └─ Unknown → existing path
```

## Components

### New file: `crates/owl-dl-reasoner/src/abox_check.rs` (~400 LoC target)

```rust
pub(crate) enum AboxVerdict { Inconsistent { reason: ClashReason }, Unknown }

pub(crate) enum ClashReason {
    AssertedBot     { individual: IndividualId, class: ClassId },
    DisjointTypes   { individual: IndividualId, c: ClassId, d: ClassId },
    NegOpaConflict  { from: IndividualId, role: RoleId, to: IndividualId },
    SameDifferent   { a: IndividualId, b: IndividualId },
    FunctionalDiff  { role: RoleId, a: IndividualId, b1: IndividualId, b2: IndividualId },
    AsymmetricViolation { role: RoleId, a: IndividualId, b: IndividualId },
    IrreflexiveViolation { role: RoleId, a: IndividualId },
}

pub(crate) fn check(prepared: &PreparedOntology) -> AboxVerdict { ... }
```

### Helper: `union_find::UnionFind<IndividualId>`

~50 LoC, path compression + union by rank. Shared by P4 and P5. New module under `crates/owl-dl-reasoner/src/`.

### Helper: `per_individual_types(&Abox, &Subsumers) -> Vec<HashSet<ClassId>>`

Populated lazily on demand from P2/P7; one `HashSet` per `abox.individuals` entry, keyed by the index into `abox.individuals` (not by `IndividualId` directly — avoids a sparse map). For each `ClassAssertion(C, a)` with `C = Atomic(c)`, insert `c` and every element of `Subsumers::subsumers_of(c)`. Non-atomic class expressions in `ClassAssertion` are skipped at P2 (sound: skipping shrinks the type set, which can only miss clashes, never invent them).

### Modified: `PreparedOntology` (`crates/owl-dl-reasoner/src/lib.rs`)

- Add field: `abox_verdict: OnceCell<AboxVerdict>`.
- Add method: `pub(crate) fn abox_verdict(&self) -> &AboxVerdict { self.abox_verdict.get_or_init(|| abox_check::check(self)) }`.
- `is_consistent_internal_full` and `classify_top_down_with_timeout`: consult `abox_verdict()` first.

### Modified: `Classification` / `ClassificationStats`

Add `pub inconsistent: bool` to `ClassificationStats`. When `true`, every class is in the unsatisfiable set; the CLI banner adds `# abox_check: inconsistent`.

## Error handling

The check is total. EL closure lookups return `Option<&HashSet<ClassId>>`; a missing entry treats as "no derived subsumers" — sound, only means we don't claim a clash. Every individual that appears in any ABox axiom is interned by `collect_abox`, so union-find lookups can't miss. No `Result` return; `AboxVerdict` is the only output.

## Performance contract

| Metric                          | Target                                                          |
|---------------------------------|-----------------------------------------------------------------|
| family-stripped `is_consistent` | ≤ 1 s (currently 180 s timeout) — **stretch, depends on P7**    |
| GALEN classify wall             | ≤ ±2 % vs Phase 7 baseline (455.73 s); ABox-free → early return |
| ORE-10908 classify wall         | ≤ ±5 %; ABox present, check should be sub-second                |
| ORE-15672 classify wall         | ≤ ±5 %                                                          |
| ABox-free ontologies            | Zero overhead via `abox.individuals.is_empty()` early return    |

## Testing

### Tier 1 — pattern unit tests

`crates/owl-dl-reasoner/tests/abox_consistency.rs` — 7 synthetic `.ofn` fixtures under `tests/fixtures/abox/`, one per pattern P1–P7. Each ~10 lines, hand-authored, asserts `is_consistent → false`. Run on every `cargo test` (not `#[ignore]`d).

Companion negative tests: 7 near-miss fixtures (e.g., disjoint classes asserted on *different* individuals; functional role with two assertions to the *same* individual; SameAs without DifferentFrom). Assert `is_consistent → true`. Sound-positive AND sound-negative coverage.

### Tier 2 — corpus closure-diff regression

`family-stripped.ofn` and `family.ofn` added to `konclude_closure_diff.rs` with a new assertion: `if konclude_inconsistent { rustdl_inconsistent }`. `#[ignore]`d (matches the corpus-test convention). If P1–P7 don't close family, the test stays as a documented target for follow-up functional-merge work. If we do close it, the test is a regression guard.

### Tier 3 — existing corpus FP=0 invariant

Re-run `cargo test ... konclude_closure_diff` for shoiq-knowledge, sio, ro, sulo, alehif, ore-10908, ore-15672. If the ABox check ever flags a *consistent* ontology as inconsistent, every closure-diff for that ontology dies (because all class subsumptions trivially hold under inconsistency). This is our primary unsoundness tripwire — no new harness needed.

## Env gate

`RUSTDL_ABOX_CHECK` defaults `ON`. Set `=0` to skip the check entirely (falls back to today's tableau-only behaviour). Same pattern as `RUSTDL_HORN_SHORTCIRCUIT` / `RUSTDL_SNAPSHOT_CAPTURE`. Lets A/B isolation in regressions and ships an opt-out for the soundness-paranoid.

## Diagnostics

- `RUSTDL_TRACE=1` and check fires → one stderr line: `abox_check: inconsistent — <ClashReason debug>`.
- Classify banner adds: `# abox_check: inconsistent | unknown | skipped`.

## Recon findings (informing the design)

- Family TBox alone is consistent (Konclude confirms).
- Family inconsistency requires `DifferentIndividuals` (in 2nd ABox half) + assertions from BOTH halves of the first ABox half. No single quarter alone is inconsistent.
- No individual has 2+ asserted `hasFather` / `hasMother` / `hasSex`; no child has multiple inferred fathers via `hasFather ∪ isFatherOf⁻¹`. Direct functional-collision (P5) does not close family.
- HermiT's own `explain --mode inconsistency` times out at 120 s — family's clash is genuinely multi-step.
- The candidate path: range(`hasFather`)=Man + range(`hasMother`)=Woman + an individual asserted as both target → `Man ⊓ Woman` via functional `hasSex` collapse. P7 handles the range step but not the functional-collapse step. Family-as-closed is a stretch, not a commitment.
