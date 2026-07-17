# Anonymous individuals (D1) — shipped, results (2026-07-17)

Implements `docs/superpowers/specs/2026-07-17-anonymous-individuals-design.md` per the plan
`docs/superpowers/plans/2026-07-17-anonymous-individuals.md`. First item of the deficiency
roadmap (`docs/2026-07-17-deficiency-roadmap.md`). Branch `feat/anonymous-individuals`.

## What shipped

- `convert_individual` now interns each anonymous individual as a first-class `IndividualId`
  under the reserved `urn:rustdl-anon:<label>` namespace (`ANON_IRI_PREFIX`) instead of erroring.
  One change, at the single chokepoint every axiom position routes through — so anonymous
  individuals thread through ClassAssertion / OPA / NegOPA / SameIndividual / DifferentIndividuals
  / DataPropertyAssertion / nominal automatically, participating in SameAs / DifferentFrom /
  `≤n`-functional-merge / disjointness exactly as named individuals (no-UNA, sound).
- Reasoning-internal only (decision (a)): anonymous individuals are filtered from the five
  named-individual output surfaces (`instances_of`, `instances_of_saturation_only`, and the three
  `materialize_*` builders) by the reserved prefix. Classification output (over classes) is
  unaffected.

## Acceptance

**Coverage (the ORE ERR1 subset — the 23 % that was unreadable):** 30 previously-anon-rejecting
ontologies re-classified on a fresh binary — **0 / 30 still reject on anonymous individuals**
(was 100 %). They now produce real classification output. (One, `ore_ont_10860`, hits a
*different* unsupported construct — an `xsd`/DArg datatype-arg parse — which is out of scope for
D1 and acceptable per the plan.) The corpus-wide ERR1 count (446, 100 % anon) drops to the
residual non-anon errors.

**Non-regression:** `cargo test --workspace` is green except one **pre-existing, anon-unrelated**
failure — `incremental_matches_baseline_on_fixtures` panics with `fixture missing:
ontologies/regression/funcmerge-cyclic.ofn` (a missing regression-data file, present before any
anon work; the anon diff touches only `convert.rs`, `lib.rs`, `realize.rs`, and two new anon test
files). No new failures introduced. FP=0 soundness contract held (the change adds interning +
output-filtering only; it alters no reasoning verdict).

**New soundness fixtures (Task 3):** 5 HermiT/Konclude-adjudicated anon-*identity* tests — anon in
two disjoint classes (inconsistent), functional-merge-into-disjoint-clash (inconsistent),
functional + `DifferentIndividuals` (inconsistent), the functional-merge *consistent* control,
and `SameIndividual(anon, named)` propagation to the named individual — all pass with **no
reasoning-source change**, confirming the threading is genuinely automatic.

## Follow-ups (non-blocking, for a later pass)

- The Python `owl-dl-py/src/errors.rs` still maps `ConversionError::AnonymousIndividual` to
  "anonymous individuals are not supported"; that arm is now unreachable from the Rust core
  (retained only because the Python binding references the variant). Update or remove the
  now-stale message.
- Harden the `materialize_object` reporting test: its fixture's edges all involve `_:x`, so after
  filtering the result is empty and the "no anon leaked" assertion passes vacuously — add a
  surviving named→named edge and assert it is present.
