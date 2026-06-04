# ABox Consistency Check — Handoff (2026-06-04)

## Status: PARTIAL

Seven sound clash patterns (P1–P7) shipped and gated by env. All 16 synthetic
unit tests pass (positive + negative per pattern). FP=0 preserved across every
corpus closure-diff fixture. The family/family-stripped stretch goal — the
project's headline target — was **not closed**: those inconsistencies are
multi-step (∃hasSex.Female ⊓ ∃hasSex.Male + Functional(hasSex) → Male⊓Female
disjoint) and P7's domain/range augmentation alone doesn't reach the
functional-collapse step. That's the next scoping target.

## Scope shipped

- **T1 (commit `6866188`)** — `UnionFind<u32>` helper (path compression +
  union by rank, 4 unit tests).
- **T2 (commit `0f46ff7`)** — `abox_check` module skeleton: `AboxVerdict`,
  `ClashReason` (7 variants), env gate `RUSTDL_ABOX_CHECK` (default ON).
- **T3 (commit `0f3a9d3`)** — `abox_verdict: OnceLock<AboxVerdict>` lazy
  field on `PreparedOntology` with `&self` accessor that honours the env
  gate.
- **T4 (commit `163f263`)** — `is_consistent_internal_full` consults
  `abox_verdict()` before falling through to `prepared.decide(Top)`.
- **T5 (commit `cf8da71`)** — `ClassificationStats.inconsistent` field
  + `classify_inconsistent()` helper; pre-check inserted in both
  `classify_top_down_internal` paths (pure-EL / Horn fast path and
  general top-down).
- **T6 (commit `bd62f58`)** — CLI banner line `# abox_check: inconsistent
  | unknown | skipped`.
- **T7 (commit `0d64cd0`)** — **P1 direct-⊥ assertion**. ClassAssertion(C, a)
  with C atomic and `Subsumers::is_unsatisfiable(c)`. Required adding
  `pub(crate) closure: Subsumers` field to `PreparedOntology` (computed
  in `from_internal` over the un-mutated input).
- **T8 (commit `e6d1923`)** — **P2 disjoint types**. Per-individual atomic
  type-set from ClassAssertions + EL subsumer closure; pairwise
  `ToldTables::are_told_disjoint` scan. Required adding `told` field +
  populating in `from_internal`. **Soundness gotcha caught during impl:**
  the EL closure returns Tseitin-introduced ClassIds beyond
  `told.num_classes()` on real corpora (alehif et al.); without a bounds
  guard the `are_told_disjoint` call panics on out-of-range indices.
  The guard `c.index() < told.num_classes()` was added and is reused by
  P7 in T13.
- **T9 (commit `12566aa`)** — **P3 NegOPA vs OPA**. HashSet of positive
  `(a, R, b)` triples; each NegOPA tested against it and against
  `hierarchy.super_roles(R)` propagated assertions. Required adding
  `Abox::negative_property_triples` (raw polarity-normalised triples)
  populated during `collect_abox`; `hierarchy` field widened to
  `pub(crate)`.
- **T10 (commit `6036db5`)** — **P4 SameAs ∩ DifferentFrom (transitive)**.
  Union-find over individual indices from `same_pairs`; each
  `different_pairs` entry queried. The `uf` value stays in scope through
  P5/P6.
- **T11 (commit `1356cc4`)** — **P5 Functional + two distinct witnesses**.
  Functional + InverseFunctional grouping over `property_assertions`;
  multi-target groups merge in the P4 union-find with re-check of all
  `different_pairs` after every successful merge. Required adding
  `PreparedOntology::axioms: Vec<Axiom>` (clone of input axiom list)
  used by P5/P6/P7.
- **T12 (commit `70f7189`)** — **P6 Asymmetric/Irreflexive violations**.
  Asymmetric scan over `pos` set; irreflexive scan including
  SameAs-merge detection via the P4/P5 union-find.
- **T13 (commit `ba8989e`)** — **P7 domain/range stretch**.
  `ObjectPropertyDomain` / `Range` augment the per-individual type set
  (with EL subsumer closure); re-runs P2 pairwise-disjoint scan with
  the T8 bounds guard.
- **T14 (commit `83f324e`)** — `family_inconsistency_detected` +
  `family_stripped_inconsistency_detected` corpus regression tests
  (`#[ignore]`d, documented as stretch goals).
- **T14.5 (commit `6e63c28`)** — perf fix: skip
  `PreparedOntology::from_internal` build for ABox-free inputs on the
  Horn / pure-EL fast path. Initial measurement showed +94 % GALEN
  regression because the build paid ~1.5 s of NNF + absorb + closure
  for an ABox check that early-returns `Unknown` on empty
  `individuals`. New `has_abox_axioms()` helper gates the build behind
  an O(n) axiom scan. Honours the spec's "zero overhead on ABox-free"
  contract.
- **Post-review fix (commit `92bd060`)** — **P3 soundness fix**: final
  code review caught that the role-hierarchy propagation was inverted.
  `NegOPA(R, a, b)` is contradicted by a positive `S(a, b)` when
  `S ⊑ R` (sub-role) — the S-fact entails R(a, b). A super-role fact
  does NOT entail R(a, b) and must not flag inconsistency. The plan
  and spec both said "super-role"; the implementation faithfully
  followed the buggy spec. Latent because no fixture combined NegOPA
  with a role hierarchy. Fix: `super_roles` → `sub_roles` in
  `abox_check.rs` P3 block (`sub_roles` is reflexive-transitive so
  the direct-match case is subsumed). Two new fixtures + tests pin
  the bug both directions:
    - `p3_role_hierarchy_super_neg_is_consistent` — the false-positive
      path the buggy version would have flagged.
    - `p3_role_hierarchy_sub_neg_is_inconsistent` — the genuinely
      inconsistent dual case the fix must still detect.
  Spec §P3 corrected. FP=0 invariant re-verified on alehif +
  ore-10908.

## Test harness

### `crates/owl-dl-reasoner/tests/abox_consistency.rs` — 16 synthetic tests
Each pattern has positive + near-miss negative coverage; not `#[ignore]`d.

| Pattern | Positive | Negative |
|---------|----------|----------|
| P1 direct-Bot | `p1_direct_bot_is_inconsistent` | `p1_no_bot_assertion_is_consistent` |
| P2 disjoint types | `p2_disjoint_types_is_inconsistent` | `p2_disjoint_different_individuals_is_consistent` |
| P3 NegOPA vs OPA | `p3_neg_opa_is_inconsistent` | `p3_neg_opa_no_clash_is_consistent` |
| P4 SameAs∩DifferentFrom | `p4_same_then_different_is_inconsistent` | `p4_same_without_different_is_consistent` |
| P5 Functional+diff witnesses | `p5_functional_distinct_witnesses_is_inconsistent` | `p5_functional_no_different_is_consistent` |
| P6 Asymmetric two-way | `p6_asymmetric_two_way_is_inconsistent` | `p6_asymmetric_one_way_is_consistent` |
| P6 Irreflexive self-loop | `p6_irreflexive_self_loop_is_inconsistent` | `p6_irreflexive_distinct_pair_is_consistent` |
| P7 range disjoint (stretch) | `p7_range_clashes_with_assertion_is_inconsistent` | `p7_range_compatible_is_consistent` |
| P3 role-hierarchy (regression) | `p3_role_hierarchy_sub_neg_is_inconsistent` | `p3_role_hierarchy_super_neg_is_consistent` |

All 18 pass.

### `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` — 2 corpus regressions
Both `#[ignore]`d, both currently FAIL as stretch goals:
- `family_inconsistency_detected` — `is_consistent` on `family.ofn` hangs
- `family_stripped_inconsistency_detected` — same on `family-stripped.ofn`

These are the project's headline targets; the spec correctly predicted that
P7 alone wouldn't close them.

## Soundness invariant

FP=0 vs Konclude preserved across every corpus closure-diff fixture
(alehif, ore-10908, ore-15672, shoiq-knowledge, sio, ro, sulo, galen,
notgalen). Two pre-existing MISSED counts (sio MISSED=2, notgalen
MISSED=18) unchanged from main — those predate this project. The cheap
ABox check never flagged a consistent ontology as inconsistent.

## Performance impact

### GALEN classify wall (single-machine measurement)

| Variant | Wall |
|---------|------|
| Default (`RUSTDL_ABOX_CHECK=1`) | ~0.58 s |
| Disabled (`RUSTDL_ABOX_CHECK=0`) | ~0.62 s |

Within measurement noise of `RUSTDL_ABOX_CHECK=0`. Earlier T14 reported a
+94 % regression (3.07 s vs 1.58 s) — that was a stale-binary measurement;
T14.5 fix and clean rebuild confirmed near-zero overhead.

GALEN is in the Horn fragment (T7-era Phase 2b dispatch), and is ABox-free.
On this profile the inline `has_abox_axioms` scan returns false in
microseconds and the `from_internal` build is skipped entirely.

The Phase 7 baseline of 455.73 s is not comparable — that predates the
Horn-shortcircuit dispatch from the Konclude snapshot cache project, which
already cut GALEN from 8+ minutes to sub-second wall.

### ORE-10908 / ORE-15672

Not cleanly measured this session (machine load 100-130 contaminated the
runs). Expected overhead: negligible. For non-Horn / non-pure-EL inputs
the general-path `PreparedOntology::from_internal` was already being
built — adding the abox_verdict() consultation is a single O(|ABox|)
scan that returns `Unknown` in microseconds when there's no clash.
Re-measure on a quiet machine if a concrete number is needed.

### family-stripped `is_consistent`

Before: timed out at 180 s (Phase 0 baseline pre-this-project).
After: timed out at 60 s (T13 + T14 stretch goal validation).

P7 doesn't catch the family clash. The next scoping target (functional-
collapse step) would.

## Soundness contract

Sound under-approximation, same model as the EL saturator and the D1
datatype drop:

- `Inconsistent` is unconditional — any reported clash is a direct
  semantic contradiction on the ABox.
- `Unknown` falls through to the existing tableau path; no claim made
  about consistency.

All seven patterns inspect direct semantic invariants. No inferred
subsumption is created, so a positive verdict requires only that the
input ontology itself contains the clash structure.

## Env gate

`RUSTDL_ABOX_CHECK` (default ON). Set `=0` (or empty) to skip the
check entirely; the runtime reverts to pre-Phase-A1 tableau-only
behaviour.

## CLI surface

`./target/release/rustdl classify <ont>` now prints one of:
- `# abox_check: inconsistent` — check fired; classification mirrors
  Konclude (every class marked unsatisfiable).
- `# abox_check: unknown` — check ran and found no clash.
- `# abox_check: skipped` — `RUSTDL_ABOX_CHECK=0` (or empty).

`./target/release/rustdl consistent <ont>` benefits transparently: a
positive `is_consistent` short-circuits the tableau via the same
pre-check.

## Known gaps (not addressed — out of scope)

### Functional-role merge step (the family gap)

Family-style multi-step clashes need:
```
range(hasMother) = Woman ≡ Person ⊓ ∃hasSex.Female
range(hasFather) = Man   ≡ Person ⊓ ∃hasSex.Male
Functional(hasSex)
Female ⊓ Male ⊑ ⊥
```
P7 covers the range augmentation (Woman / Man propagate into the type
set). What's missing is "collapse the two ∃hasSex witnesses via
functionality" and then "detect the disjoint types on the merged
witness". That's a second pass over the augmented graph; a follow-up
project.

### ABox-level realization

Per-individual most-specific entailed type. Different problem.

### Concrete-domain reasoning on `DataPropertyAssertion` literals

D5 covers the TBox side (integer-range facets via preprocessing); ABox-
level literal conflicts (Functional(hasBirthYear) + two distinct year
values on one individual) are out of scope.

### Pre-existing failing lib tests

6 tests on `main` predate this project (Phase 7/8 env-gate / selective-verify
drift). Their count is unchanged on this branch — `83 passed; 6 failed`
matches both pre- and post-shipment.

## Commit map

```
92bd060 fix(abox-check): P3 — sub_roles, not super_roles (soundness fix)
6e63c28 perf(abox-check): skip PreparedOntology build for ABox-free fast-path inputs
83f324e test(abox-check): T14 — family / family-stripped inconsistency regression
ba8989e feat(abox-check): T13 — P7 domain/range disjointness (stretch)
70f7189 feat(abox-check): T12 — P6 Asymmetric / Irreflexive violations
1356cc4 feat(abox-check): T11 — P5 functional + two distinct witnesses
6036db5 feat(abox-check): T10 — P4 SameAs ∩ DifferentFrom
12566aa feat(abox-check): T9 — P3 NegOPA vs OPA
e6d1923 feat(abox-check): T8 — P2 disjoint types per individual
0d64cd0 feat(abox-check): T7 — P1 direct-Bot assertion
bd62f58 feat(abox-check): T6 — CLI banner surfaces abox_check verdict
cf8da71 feat(abox-check): T5 — wire abox_verdict into classify
163f263 feat(abox-check): T4 — consult abox_verdict in is_consistent
0f3a9d3 feat(abox-check): T3 — wire abox_verdict OnceLock on PreparedOntology
d4b65c6 plan(abox-check): fix type paths — RoleId/IndividualId live in ir, not role_hierarchy/vocab
0f46ff7 feat(abox-check): T2 — module skeleton, AboxVerdict, env gate
6866188 feat(abox-check): T1 — UnionFind<u32> helper
```

Plus three project-scoping commits on `main` before the feature branch:
```
c480647 plan: ABox consistency check — 15-task implementation plan
b9c43da spec: ABox consistency check — design
bd54ce6 docs: datatype completeness project — handoff (shipped)  [prior project]
```
