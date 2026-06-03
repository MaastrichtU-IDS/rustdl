# Datatype Completeness Project — Handoff (2026-06-03)

## Status: SHIPPED

Tier B + Tier C datatype reasoning is empirically complete on every
data-axiom-bearing fixture in our corpus, with FP=0 maintained
throughout. No remaining failing fixture motivates further extension.

## Scope shipped

- **D1 (commit `e34aeb6`)** — Tier A sound under-approximation:
  silently drop data axioms at convert time (was
  `UnsupportedAxiom`-erroring; this unlocked 4 fixtures that
  previously could not be classified at all).
- **D4 (commit `eb15c74`)** — Tier B preprocessing pass at
  `crates/owl-dl-core/src/data_axioms.rs`. Patterns: Functional+DataMin
  n≥2 → Bot; DataMin>DataMax → Bot; DataPropertyDomain inference;
  SubDataPropertyOf transitivity; intersection-equivalence propagation.
  Companion saturator change: `ElRules::directly_unsat` field +
  `enqueue_unsat` at seed time.
- **D5 (commit `2804cfa`)** — Tier C: `IntegerRange` with closed-form
  intersection over `xsd:integer` facets (`min/maxInclusive`,
  `min/maxExclusive`). New pattern: Functional + ≥2 ranges with empty
  intersection → Bot.
- **D6 (commit `4ede8ca`)** — Two additional corpus fixtures gated
  (ro, sulo). FP=0/MISSED=0 on both.

## Test harness

- `crates/owl-dl-reasoner/tests/datatype_completeness.rs` — 6 synthetic
  fixtures (Tier B + C); all 6 pass. `#[ignore]`d; run with
  `cargo test ... -- --ignored`.
- `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` — corpus
  fixtures alehif, ore-10908, ore-15672, shoiq-knowledge, sio, ro,
  sulo. All FP=0; data-axiom impact: 0 MISSED added by D1 drop.

## Out of scope — separate gap discovered

### family / family-stripped ontology-level inconsistency

Both HermiT and Konclude report `family.ofn` (and the data-axiom-free
`family-stripped.ofn`) as **inconsistent**. rustdl does NOT detect
this:

- `rustdl consistent ontologies/real/family-stripped.ofn` — times out
  at 180 s.
- `rustdl classify --pair-timeout-ms 100 …family-stripped.ofn` —
  returns a normal 324-pair hierarchy with no unsat class.

**Why this is NOT a datatype-completeness gap**: family-stripped has
the data axioms removed and is still inconsistent. The contradiction
sits in: range/domain restrictions + class disjointness (Female/Male,
Marriage/Person/Sex) + 1859 ABox assertions over ~700
DifferentIndividuals.

**What it IS**: a structural / ABox-level inconsistency-detection
gap. Konclude does the consistency check in ~800 ms preprocessing;
rustdl's per-class satisfiability probes don't surface the global
contradiction within budget. Treat as its own scoping target if a
future workload calls for ABox-driven inconsistency detection. Until
then this is a known trap: a reasoner that emits a normal-looking
hierarchy for an ontology two oracles reject.

## Not pursued (YAGNI)

- xsd:decimal / xsd:double / xsd:dateTime range algebras — would
  extend trivially via `IntegerRange`-shaped types sharing the D4
  preprocessing's pattern matcher, but no fixture in the corpus or
  synthetic harness exposes a gap.
- `DataUnionOf` / `DataIntersectionOf` / `DataComplementOf` —
  same reasoning; speculative without a failing fixture.
- ABox-level datatype reasoning (Functional + two distinct literals
  on the same individual) — would require lowering
  DataPropertyAssertion into per-individual literal collisions.
  Not motivated by any current failure.
