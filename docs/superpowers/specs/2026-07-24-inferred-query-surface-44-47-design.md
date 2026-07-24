# Design: inferred query surface — property hierarchy, property values, same/different, disjointness (issues #44–#47)

**Date:** 2026-07-24
**Status:** design approved (brainstorming); pending spec review → implementation plan.
**Author:** Claude + Michel, session: rustdl API-surface brainstorming.
**Closes:** #44, #45, #46, #47 (the "API surface" bucket of the Protégé-plugin
gap audit, `2026-07-24-protege-plugin-design.md` §8a). One combined spec; one
implementation plan; **separate commits/PRs per issue** so each closes its ticket.

## 1. Goal

Expose four families of inferred reasoner queries — currently backed by internal
machinery but not reachable from the CLI `--json` contract or Python — on the same
three-layer surface every other rustdl query uses (**reasoner API → Python binding
→ CLI `--json`**). This turns the Protégé plugin's empty property-hierarchy /
property-value / same-different / disjoint panels into populated ones as each
ticket lands.

The four queries and their OWLReasoner analogues:

| # | OWLReasoner method | rustdl query |
|---|---|---|
| 44 | `getSub/Super/EquivalentObjectProperties`, data equivalents | inferred object/data property hierarchy |
| 45 | `getObject/DataPropertyValues(ind, prop)` | inferred property values for individuals |
| 46 | `getSameIndividuals(ind)` / `getDifferentIndividuals(ind)` | inferred same/different individuals |
| 47 | `getDisjointClasses(ce)` / disjoint object+data properties | inferred disjointness |

## 2. Semantics — "full inferred within rustdl's sound envelope"

The user chose **full-inferred** semantics over a pure told/structural closure.
In rustdl that means: **entailment reduced to (un)satisfiability / consistency
checks on the existing sound engines**, seeded by a fast structural lower bound.
It is *not* textbook-complete — it inherits the engines' soundness (**FP=0**) and
their near-but-not-guaranteed completeness, surfaced honestly via an `incomplete`
flag (§6). Two of the four are structural-bound by decision (§5).

### 2.1 The load-bearing soundness invariants (non-negotiable)

1. **UNSAT-direction only.** Every entailment is concluded **only** from an
   `unsatisfiable` / `inconsistent` verdict — never from a satisfying model, a
   wedge `Sat`, or any cached completion. Unsat is always sound; a `Sat` is a
   MISS at worst, never a false positive. This is the exact discipline that the
   snapshot-cache FP incident violated (CLAUDE.md, `RUSTDL_SNAPSHOT_CAPTURE`
   default-OFF soundness fix: it trusted one satisfying model and emitted
   spurious subsumptions on the non-Horn/disjunctive fragment). We stay on the
   sound side by construction and state it as an invariant so the
   inconsistency-based reductions (45, 46-same) are never tempted to read the
   near-consistent completion for "extra" edges/merges.

2. **Inconsistent-KB guard (mandatory).** An inconsistent ontology entails
   *everything* — every disjointness, every `a≠b` **and** every `a=b`, every
   `R(a,b)`. So every new query MUST run `abox_saturation::saturate_abox_consistency`
   first and return `Err(ReasonError::Inconsistent)` on a clash, exactly as the
   existing `materialize_*` functions already do
   (`owl-dl-reasoner/src/lib.rs` `materialize_object_property_assertions` /
   `materialize_subobjectproperty_axioms` both guard this). Skipping it produces
   silent, unsound-looking "all-disjoint / all-same-and-different" output.

3. **Reduce via `PreparedOntology::decide`, not the named-class entry points.**
   The compound probes `C ⊓ D` and `{a} ⊓ {b}` **cannot** be expressed through
   `sat_class_probe` / `is_class_satisfiable[_with_timeout]` — those take a
   *named class IRI* only. The correct primitive is the internal
   `PreparedOntology::decide(build_test_concept: FnOnce(&mut ConceptPool) ->
   ConceptId)` closure (`owl-dl-reasoner/src/lib.rs`), which builds the probe on
   a freshly-cloned pool over the frozen snapshot: `pool.and([pool.nominal(a),
   pool.nominal(b)])` (46-different) and `pool.and([C, D])` (47-classes). Note
   the EL-closure fast path fires only for a *named* class id, so it does **not**
   accelerate compound probes — expect classify-class cost per probe. `decide` is
   currently `pub(crate)`; a thin public wrapper (or a set of purpose-built query
   fns that call it internally) is part of the work.

## 3. Shared mechanism — structural seed + budgeted entailment extension

All four use one pattern, reusing only proven-sound engines:

1. **Structural seed (sound lower bound), computed once:** told/equivalent/inverse
   closures (`ToldTables`, `RoleHierarchy`), the `SameIndividual` union-find, and
   the existing `materialize_*` outputs.
2. **Entailment extension** over the *remaining* candidate pairs (those not
   already in the seed, self-pairs and unsat-class pairs pruned), sharing one
   `PreparedOntology` snapshot:
   - **Class-sat probes** (47-classes, 46-different): `pool.and([..])` unsat via
     `decide`. Each probe = one pool-clone + ABox re-seed + tableau search — the
     **same per-pair cost profile as `classify`**, parallelizable over rayon.
   - **Augment-and-recheck** (46-same, 45): inject one extra ABox fact into the
     seed loop of the *existing* snapshot (a `different_pair` for 46-same; a
     negative property triple / `∀r.¬{b}` label for 45) **before** the tableau's
     `mark_distinct`, then check inconsistency. This reuses the whole prepared
     snapshot — **a `PreparedOntology` rebuild per check is forbidden** (re-running
     saturate + told + hyper + absorb per candidate is intractable, fatally so for
     45's candidate space). This is small snapshot-preserving plumbing on the ABox
     seed path, not a new engine.
3. **Per-check deadline** (a budget, à la classify's `--pair-timeout-ms`); a
   timed-out check yields a sound under-approximation and contributes to the
   result's `incomplete` flag (§6).

## 4. Per-issue design

### 4.1 Issue #47 — disjointness (the cleanest; recommended first)

- **Disjoint classes (full-inferred):** `getDisjointClasses(C)` = every named `D`
  with `C ⊓ D ⊑ ⊥`. Seed from `ToldTables::disjoint_with` (`owl-dl-core/src/told.rs`);
  extend by probing `pool.and([C, D])` unsat via `decide` for candidate pairs.
  **Unsat-class handling:** an unsatisfiable class is disjoint with everything
  (and itself) — semantically true but a flood. Exclude unsatisfiable classes and
  self-pairs, mirroring how `json_out.rs` already excludes unsat classes from
  `equivalent_groups`. Only probe pairs not already told-disjoint and not
  involving an unsat class.
- **Disjoint object/data properties (structural — §5):** told-disjoint-property
  closure only; no property-level entailment probe.

### 4.2 Issue #46 — same / different individuals

- **Different (full-inferred):** `a≠b` entailed iff `{a} ⊓ {b}` unsat — a class-sat
  probe (snapshot-shareable, no UNA assumption: we *prove* distinctness, not assume
  it). Seed from told `DifferentIndividuals` / `AllDifferent`.
- **Same:** the `SameIndividual` union-find in `abox_saturation.rs` captures **only
  asserted** sameness (functional-role Rule 7 propagates *types*, never merges
  identity) — lifting it out yields no derived sameness. Two-tier approach:
  1. **Structural derived-same pass (cheap, preferred):** extend the abox saturator
     to record functional-forced merges — for functional/inverse-functional `R`,
     if derived `edges` contain `R(a,b)` and `R(a,c)` then `b=c`. O(edges), zero
     tableau calls; captures the common `hasSex`-style cases the module already
     reasons over. Union these into the same-closure.
  2. **Augment-and-recheck residual:** `a=b` iff `KB ∪ {DifferentIndividuals(a,b)}`
     inconsistent, via the snapshot-preserving ABox-seed plumbing (§3). Budgeted.

### 4.3 Issue #45 — property values

- `getObjectPropertyValues(a, R)` = `{b : KB ⊨ R(a,b)}`; `R(a,b)` entailed iff
  `KB ∪ {NegativeObjectPropertyAssertion(R,a,b)}` inconsistent (data: negative data
  assertion). Seed from `materialize_{object,data}_property_assertions` (already a
  strong sound lower bound at ~zero marginal cost).
- **Hard-bounded candidate set:** the naive `|I|² × |R|` cross-product is
  intractable regardless of snapshot reuse. Augment-recheck candidates are
  restricted to a small declared/queried set (default: the pattern the plugin
  actually asks — per `(individual, property)` — plus a bounded neighborhood),
  **not** the cross-product. Default budget on (§6).

### 4.4 Issue #44 — property hierarchy (structural — §5)

- `classify_object_property_hierarchy` / `classify_data_property_hierarchy`
  returning a `PropertyClassification` (equivalent groups + direct/Hasse edges),
  mirroring the class `Classification` shape. Object properties use the internal
  `RoleHierarchy` closure (told + inverse + symmetric + equivalence + chains, the
  closure the reasoner actually reasons with — richer than today's
  `materialize_subobjectproperty_axioms` partial rebuild); data properties use the
  told + equivalent closure (no inverses/chains). Complete for the fragment rustdl
  reasons about; no property-subsumption entailment probe.

## 5. Deliberate structural-only boundaries (approved)

- **#44 property hierarchy** and the **#47 disjoint-properties** half are exposed
  as the reasoner's actual structural role-level inference, **not** a full
  entailment probe. rustdl has no property-subsumption / property-disjointness
  entailment engine; building one is substantial new engine work that rarely
  differs from the structural closure and risks the soundness gates — out of scope
  here, documented as "complete for the fragment," not as full inference.
- Class-level queries (**#47-classes, #46, #45**) get the full budgeted entailment
  extension.

## 6. `incomplete` semantics (stronger than classify's)

`classify` sets `incomplete = timed_out_pairs > 0` — a pure *budget* signal. That
is insufficient here: with `trust_sat` on (default), a wedge `Sat` can be a genuine
MISS **with no timeout and no budget cut**, and `is_consistent` on ABox inputs is
itself a trusted-`Sat` under-approximation — so the augment-recheck queries
(45, 46-same) are structurally incomplete **even at unbounded budget**. Therefore
each result's `incomplete` flag is set when **either**:
- a per-check deadline fired (budget cut), **or**
- the answer rests on a trusted-`Sat` / non-saturator-complete verdict (the query
  ran outside the fragment where the engine is complete-by-construction).

This mirrors the existing `FragmentClassification` / `trust_sat` honesty and keeps
the plugin from reporting "complete" over results the soundness contract says may
miss. Structural-only queries (#44, disjoint-properties) report `incomplete: false`
within their documented fragment.

## 7. Surface (three layers, matching existing precedent)

### 7.1 Reasoner API (`crates/owl-dl-reasoner/src/lib.rs`)
New query fns, each running the inconsistent-KB guard first and returning
structured results + an `incomplete` signal:
- 44: `classify_object_property_hierarchy`, `classify_data_property_hierarchy` →
  `PropertyClassification { equivalent_groups, direct_subsumptions }`.
- 45: `inferred_object_property_values`, `inferred_data_property_values` (seed +
  budgeted extension, budget config).
- 46: `same_individuals` (equivalence groups), `different_individuals` (pairs).
- 47: `disjoint_classes` (pairs, entailment-extended), `disjoint_object_properties`
  / `disjoint_data_properties` (structural).
- A thin public entry to the `decide` compound-probe path (or keep it internal and
  expose only the query fns above — plan decides).

### 7.2 Python (`crates/owl-dl-py`)
New `#[pyfunction]`s in `queries.rs`/`materialize.rs`, re-exported in
`python/rustdl/__init__.py` (`__all__`), typed in `__init__.pyi`, guarded by
`tests/python/test_stubs.py` (a `test_stubs` regression has reached `main` before
— the drift guard is mandatory).

### 7.3 CLI `--json` (`crates/owl-dl-cli/src/main.rs` + `src/json_out.rs`)
New `Command` variants parallel to `Realize`/`Instance`, each emitting
`{ "schema_version", "incomplete": bool, … }`. These extend the §4 JSON contract
of the plugin spec (they were the deferred v1.x rows in §8a). Because the plugin
treats the schema as a stability contract, **new query outputs are additive new
top-level result structs**; the plan decides whether to bump `SCHEMA_VERSION`
(currently `1`) or add them as clearly-separate documents (leaning: new documents
keep the existing `classify/consistent/realize` schemas byte-stable, so no bump for
those; the new subcommands carry their own `schema_version`). One golden `--json`
test per subcommand.

## 8. Testing

- **Oracle-backed** (the repo's soundness discipline) for the entailment-extended
  queries (47-classes, 46, 45): HermiT/ROBOT/Konclude oracle fixtures like the
  existing `crates/owl-dl-reasoner/tests/materialize_oracle.rs`. The oracle is the
  FP guard — any pair rustdl reports that the oracle does not is a soundness bug.
- **Negatives-first canaries** per query (including the vacuous-inconsistent-KB
  guard, unsat-class exclusion for 47, no-UNA distinctness for 46-different).
- **Golden `--json`** tests on small fixtures; **Python stub-drift** test.
- **FP=0 corpus re-validation** for anything touching the sat/consistency path
  (the reductions reuse those engines; a closure-diff run confirms no perturbation).

## 9. Risks / open items for the plan

- **Snapshot-preserving ABox-seed plumbing (45, 46-same):** confirm the exact
  injection point before `mark_distinct` in the tableau ABox seed loop and that it
  leaves the frozen snapshot reusable across probes. This is the highest-risk
  mechanical item; a fallback of "structural-only for 45" exists if it proves
  infeasible (but the user chose full-inferred, so exhaust the plumbing first).
- **45 candidate-set bounding:** pin the default candidate policy so 45 is
  tractable on real ABoxes without silently dropping likely values.
- **Cost of 47-classes O(n²) probes** on large TBoxes — same frontier as classify
  (`wine` DNFs unbounded); reuse classify's budget/`incomplete` posture.
- **Derived-same functional-merge pass (46):** scope it to functional /
  inverse-functional roles only; verify it adds no FP via the oracle.
- **Schema-version decision** (bump vs per-subcommand) — §7.3.
