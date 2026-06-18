# First-Class Data Properties — Engine Arc (design)

**Date:** 2026-06-17
**Status:** design (architecture locked pending POC); sub-project 1 detailed
**Why:** make data properties first-class in the reasoner so classification,
consistency, AND justification queries reason about data — the prerequisite the
user set before continuing Justification Spec 2's data-property queries
(`2026-06-17-justification-spec2-property-individual-queries-design.md`).

## Current state (the gap)

Data is handled **only at the TBox/class level**: data ranges become synthetic
**DKey** atomic classes and a data property in `∃dp.R` is treated as a forward
object **role** to a DKey filler (`convert.rs:582-640`); a refute-only
concrete-domain solver (`owl-dl-datatypes::card_sat`) checks range/cardinality
clashes per tableau node. Everything else is **dropped at convert**
(`convert.rs:1647-1655`): `DataPropertyAssertion`, `NegativeDataPropertyAssertion`,
`SubDataPropertyOf`, `EquivalentDataProperties`, `DisjointDataProperties`,
`FunctionalDataProperty`, domain/range. `data_axioms.rs` (D4–D11) rescans the
source and recovers *some* of these as class-level clash axioms (`C ⊑ ⊥`, `C ⊑ D`),
but there is **no data ABox, no data-property RBox, no per-individual data value,
and no negative-data support** anywhere. (Full inventory: the 2026-06-17 Explore
map in the session record.)

## Locked architecture: extend DKey/dp-as-role to the ABox (Approach B)

Reuse the existing "data property = object role over a DKey value-filler" encoding
and extend it from the TBox to the ABox. Every data-property axiom lowers to an
**existing** object-fragment `Axiom` (no new IR variants), so the battle-tested
object RBox, functional-merge, disjoint-roles, `∀`/complement, and the wired
concrete-domain solver do the reasoning.

| Data axiom | Lowering (reused `vocab.intern_role(dp_iri)` dp-role) | Engine reused |
|---|---|---|
| `DataPropertyAssertion(a,dp,v)` | `ClassAssertion(a, ∃dp.DKey(point v))` | ABox `ClassAssertion` + `∃` |
| `NegativeDataPropertyAssertion(a,dp,v)` | `ClassAssertion(a, ¬∃dp.DKey(point v))` → NNF `∀dp.¬DKey(point v)` | `∀`/complement |
| `SubDataPropertyOf(dp,dq)` | `SubObjectPropertyOf(dp, dq)` | role hierarchy |
| `EquivalentDataProperties(…)` | pairwise `SubObjectPropertyOf` both ways | role hierarchy |
| `DisjointDataProperties(…)` | `DisjointObjectProperties(dp-roles)` | disjoint-roles |
| `FunctionalDataProperty(dp)` | `FunctionalObjectProperty(dp)` | functional-merge + DKey-disjointness |
| `DataPropertyDomain(dp,C)` | `ObjectPropertyDomain(dp, C)` | object domain |
| `DataPropertyRange(dp,R)` | `ObjectPropertyRange(dp, DKey(R))` | object range |

**Why B over a dedicated concrete-domain ABox (A).** B adds *no* new tableau
state, rules, clash, or trail integration — A adds all four (a per-node data store,
data-value successors, a new clash, backtracking hooks), every line of it
soundness-critical. B also turns the data-property-range check into a
*correctly-modelled* clash (value-node carries `DKey(point v) ⊓ DKey(R)`; disjoint
when `v ∉ R`) rather than a special case. **POC gate:** sub-project 1 ships a
small end-to-end proof (a told `dp⊑dq` + asserted value forces the expected
consistency verdict) before B is considered final; if the POC exposes a blocker,
revisit A.

## Soundness contract (the crux: literal value-identity)

FP=0 is sacred. The entire approach rests on **DKey value-identity being exact**:

- A literal `v` lowers to a **singleton** point-range DKey class `DKey(point v)`.
- **Distinct literals within a datatype get disjoint DKey points** (existing D6+
  guarantee: int `5` → `urn:rustdl-dkey:5:5`, `6` → `:6:6`, provably disjoint).
- **Cross-datatype DKeys are disjoint** (existing bucket separation; int ≠ float ≠
  decimal ≠ date ≠ dateTime ≠ string, pinned by the parser-exclusivity canaries).
- Therefore `Functional(dp)` merging two value successors is sound: same literal →
  same DKey point → merge OK; distinct literals → disjoint DKey points → clash
  (correct: a functional data property cannot hold two distinct values).

Other soundness rails:
- **Unrecognized datatype/literal ⇒ drop the whole axiom** (the current sound
  under-approximation, preserved). Dropping loses entailments, never invents them.
- **dp-roles are simple**: OWL forbids transitive/composite data properties, so
  dp-roles never chain; a dp-successor is always a DKey *leaf* value-node, never
  branching object structure. The lowering must never mark a dp-role transitive or
  put it in a role chain.
- **dp-roles and object roles share the role table** but IRIs are disjoint by
  declaration; data axioms only ever relate dp-roles to dp-roles.

The corpus cannot exercise most of this (few `∃+∀+functional` data clashes), so
**boundary unit tests + negatives-first canaries are the safety net**, mirroring
the D6–D11 discipline.

## Incremental delivery + the FP gate

The whole first-class path is gated behind **`RUSTDL_DATA_PROPERTIES` (default
OFF)**. Default-OFF = converter behaves exactly as today = corpus byte-identical,
so each sub-project merges without regressing FP=0/MISSED=0. The gate flips ON
only after sub-project 3's corpus validation. This is the same env-gate discipline
used for snapshot-capture, horn-shortcircuit, precise-card-deps, etc.

### Sub-projects (each: own spec → plan → implement; FP=0 gate at every merge)
1. **IR + convert lowering** (detailed below) — emit the table above behind the
   gate; literal→value-node encoding; POC.
2. **Tableau/solver validation** — value-nodes interact correctly per individual
   (range clash, functional merge, cardinality, disjoint-dp); blocking/termination
   with DKey value-nodes; negatives-first canaries for each clash shape.
3. **`data_axioms.rs` reconciliation** — confirm the native path catches ≥ what the
   D4–D11 preprocessing catches; disable now-redundant patterns (or keep both —
   redundant is sound, just wasteful); flip the gate ON; **prove FP=0/MISSED=0
   unchanged across the full corpus** (sio, shoiq-knowledge, wine, ore-10908,
   ore-15672, pizza, alehif, galen, notgalen, ore_ont_9054).
4. **Justification Spec 2 (full)** — object/individual queries (already designed)
   plus the now-feasible data-property queries (`dp⊑dq`, `dp≡dq`, `Disjoint(dp,dq)`,
   `a dp v`), whose reductions are now class-level `¬∃dp.DKey` consistency checks.

### Sub-project 2 outcome (2026-06-18)

A discovery spike (`rustdl consistent`, gate OFF vs ON) found most shapes already
reason correctly via the reused object + concrete-domain machinery:
`DataAllValuesFrom` + ABox out-of-range assertion (gate-isolated clash), data
range + out-of-bounds value, qualified `≤n dp.T` + ABox, and termination with
DKey leaf nodes all work. Two gaps surfaced:

- **Gap 1 — unqualified data cardinality: FIXED.** `≥n dp` / `≤n dp` / `=n dp`
  over `rdfs:Literal` (no datatype facet) previously dropped. Now lowered (gate-ON
  only) to the same cardinality over the IR `⊤` filler, so the existing ≤n/≥n
  merge + DKey-disjointness fires. **Soundness-restricted to `rdfs:Literal`**: a
  specific *unrecognized* datatype still drops, because `≤n dp.⊤` over-constrains a
  typed `≤n dp.T` restriction (counts all values, not just typed ones) → a false
  clash = FP. Validated by an adversarial check (`≤1 dp.xsd:string` + two integer
  values stays consistent). `convert.rs::lower_unqualified_data_cardinality`.

- **Gap 2 — `DisjointDataProperties` same-value clash: DEFERRED (sound
  under-approximation).** `Disjoint(dp,dq) + dp(a,v) + dq(a,v)` is not detected:
  the two `DKey(v)` value-nodes are distinct anonymous nodes that are never merged,
  so `DisjointObjectProperties` (the correct lowering, which IS emitted) has no
  shared target to clash on. A missed clash here is **incompleteness, never an
  FP** — sound. Disjoint data properties are rare, and the clean fix (value-node
  canonicalization, so equal literals share one node) touches the tableau
  node-creation model and is out of scope for this arc. Revisit only on a measured
  need. (User-approved defer, 2026-06-18.)

### Sub-project 3 outcome (2026-06-18) — reconciliation + FP gate + default flip

- **`data_axioms.rs` reconciliation = keep both (no code change).** With the gate
  ON, both the D4–D11 preprocessing and the new first-class lowering run; both only
  ADD sound axioms, so their union is sound (no FP from double-handling). A
  gate-ON-vs-gate-OFF classify diff across all 10 real fixtures (incl. data-heavy
  family/bibtex) was **byte-identical** — the new path is inert on classification
  output (its value is data-ABox consistency + the future Spec 2 queries), so
  disabling D4 patterns was unnecessary and would only risk the gate-OFF path.
- **FP=0 validated at ORE-oracle scale (gate ON).** The `konclude_closure_diff`
  net (vs Konclude-classified `.owx` oracles) ran with `RUSTDL_DATA_PROPERTIES=1`:
  **FP=0 / MISSED=0 on every oracle fixture** — bibtex, galen (27997=27997),
  notgalen (32739), sulo, ro, ore-10908 (6001), alehif, pizza (499), sio (8904),
  ore-15672 (142), wine (653). The data-bearing ORE fixture ore-15516-alchoiq
  (50 data axioms) agrees with Konclude (both find it inconsistent) and is
  gate-ON==gate-OFF. The lone net failure `family_inconsistency_detected` is a
  **pre-existing** ABox-inconsistency stretch goal (fails gate-OFF too; orthogonal
  to data properties; `docs/abox-consistency-check-handoff.md`).
- **Gate flipped default ON.** `data_properties_enabled()` is now
  `var("RUSTDL_DATA_PROPERTIES").map_or(true, |v| v != "0")` — **default ON, `=0`
  opts out** (fixes the prior `is_some()` footgun where `=0` wrongly enabled). The
  two test `DpGuard::off()` helpers were updated to set `"0"` (not `remove_var`) so
  gate-OFF tests stay valid under the new default. Perf note: data-bearing
  ontologies do more tableau work gate-ON (e.g. family more probes, bibtex leaves
  the pure-EL fast path) for identical results — accepted cost of first-class data.

### Sub-project 4 outcome (2026-06-18) — queries + a float-FP revert

Spec 2 queries shipped on the branch (see
`2026-06-18-justification-spec2-queries.md`): 7 sound query types —
`SubObjectProperty`, `EquivalentObjectProperties`, `DisjointObjectProperties`,
`ObjectPropertyAssertion`, `SameIndividual`, `DifferentIndividuals`,
`SubDataProperty`, `EquivalentDataProperties` — via inconsistency-with-probe.
Data sub-property uses a two-check guard (`c1` baseline) to avoid a probe-value
range FP. Deferred: `Disjoint(dp,dq)` query (gap 2) and `a dp v` data assertion
(CLI literal parsing).

**Object-disjoint required an engine fix + exposed a pre-existing latent bug.**
The `DisjointObjectProperties` query needs the consistency checker to detect a
disjoint-role ABox clash (the wedge/trust_sat did not), so ABox pattern **P9**
(`DisjointRolePairViolation`) was added. Adversarial review then found
`collect_disjoint_role_pairs` strips inverse polarity (`.role_id()`), so
`Disjoint(r, Inv(s))` was stored as `(r,s)` → a **false positive** for both P9
*and the latent tableau path* (rules.rs:1447). Fixed by emitting **forward–forward
disjoint pairs only** (inverse-involving pairs = sound under-approximation,
skipped). Canaries pin the inverse no-FP case.

**THE DEFAULT FLIP (sub-project 3) WAS REVERTED — gate is opt-in again.** The
full `owl-dl-reasoner` test suite run gate-ON surfaced a **confined `xsd:float`
consistency false positive**: 3 tests in `datatype_inconsistency.rs`
(`float_boundary_f32_f64_mismatch_stays_consistent`,
`dp2_functional_xsd_float_excluded_is_consistent`,
`float_clearly_outside_range_is_dropped_consistent`) report inconsistent gate-ON
where the ontology is consistent — a float value-identity issue (f32/f64
representation) in the gate-ON ABox functional/cardinality path. The ORE
*classification* closure-diff net missed it (it's a *consistency* FP); the unit
suite caught it. `data_properties_enabled()` reverted to **default-OFF, `=1` to
opt in**. The float FP is the blocker for re-flipping: it must be fixed (exclude
float from the functional/cardinality merge, matching the conservative DP-2
exclusion, OR make the float DKey identity exact across f32/f64) before default-ON.

**xsd:float consistency FP — FIXED (2026-06-18).** The gate-ON data-property arms
(`DataPropertyAssertion`/`NegativeDataPropertyAssertion`/`DataPropertyRange`) now
drop `xsd:float` literals/ranges (matching the DP-1/DP-2 `is_float_datatype`
exclusion); `xsd:double` is kept and the class path is untouched. The **full unit
suite is now green gate-ON** (the validation the first flip skipped); the
float-drop is strictly conservative so the ORE classification net stays FP=0.

**DEFAULT-ON SHIPPED (2026-06-18, commit 5db0c8b).** After the float-FP fix,
`data_properties_enabled()` is `map_or(true, |v| v != "0")` — **default ON, `=0`
opts out**, matching how an OWL 2 DL reasoner (Konclude/HermiT) treats data
properties. Re-validated at default-ON: full unit suite green + Konclude-oracle
closure-diff net **FP=0/MISSED=0 on all 12 fixtures**. rustdl is now a sound
(under-approximate) data-property reasoner by default — `xsd:float`,
disjoint-dp value clashes, and `a dp v` queries remain deliberate sound
under-approximations.

**Status: the whole arc is COMPLETE on branch `feat/data-properties-subproject1`
(25 commits, not merged/pushed): engine (1–3), queries (4), float-FP fix, and the
default-ON flip — all FP=0-validated. Deferred (sound, documented): gap-2
`Disjoint(dp,dq)` query + `a dp v` data assertion.**

---

## Sub-project 1 — IR + convert lowering (detailed)

**Goal:** behind `RUSTDL_DATA_PROPERTIES=1`, lower each dropped data-property axiom
to its object-fragment `Axiom` per the architecture table, reusing the existing
DKey/dp-role machinery. No new `Axiom` variants. Default OFF ⇒ no behavior change.

**Files:**
- `crates/owl-dl-core/src/convert.rs` — replace the `Ok(None)` drop arms
  (`1647-1655`) with gated lowerings; add a literal→point-DKey helper.
- `crates/owl-dl-core/src/data_axioms.rs` — reuse its literal/range parsers
  (`parse_*_range`, point-range construction) for the value-node encoding; no
  behavior change to its existing emit (still runs; reconciled in sub-project 3).
- Tests: `crates/owl-dl-core/src/convert.rs` unit tests + a new
  `crates/owl-dl-reasoner/tests/data_properties.rs` POC integration test.

**Literal → value-node helper.** Generalize the `DataHasValue` path
(`convert.rs:812-865`): a literal `v` of a recognized datatype → its singleton
point range → `DKey(point v)` atomic class. Reuse `data_range_dkey`'s parser
cascade. Unrecognized datatype ⇒ return `None` ⇒ caller drops the axiom.

**Lowering arms** (gated; when gate OFF, keep `Ok(None)`):
- `DataPropertyAssertion(a,dp,v)`: build `∃dp.DKey(point v)` via
  `pool.some(Role::named(vocab.intern_role(dp)), dkey_filler)`; emit
  `Axiom::ClassAssertion { class, individual: a }`. Drop if `v` unrecognized.
- `NegativeDataPropertyAssertion(a,dp,v)`: build `∃dp.DKey(point v)`, wrap in
  `pool.complement(...)` (NNF lowers to `∀dp.¬DKey`); emit `ClassAssertion`. Drop
  if `v` unrecognized.
- `SubDataPropertyOf(dp,dq)`: `Axiom::SubObjectPropertyOf { sub: Role::named(dp),
  sup: Role::named(dq) }`.
- `EquivalentDataProperties(dps)`: emit `SubObjectPropertyOf` for every ordered
  pair (or `Axiom::EquivalentObjectProperties` if the IR has it — use whichever the
  object path already uses).
- `DisjointDataProperties(dps)`: `Axiom::DisjointObjectProperties(roles)`.
- `FunctionalDataProperty(dp)`: `Axiom::FunctionalObjectProperty(Role::named(dp))`.
- `DataPropertyDomain(dp,C)`: `Axiom::ObjectPropertyDomain { role, domain: C }`
  (`C` via `convert_class_expression`; drop on unsupported `C`).
- `DataPropertyRange(dp,R)`: if `R` recognized → `DKey(R)` filler →
  `Axiom::ObjectPropertyRange { role, range: dkey_filler }`; else drop the range
  axiom (keep others).
- `DatatypeDefinition`: still `Ok(None)` (range aliasing; deferred).

**POC integration test** (`data_properties.rs`, gate ON): ontology
`{ SubDataPropertyOf(dp,dq), DataPropertyAssertion(a, dp, 5), ClassAssertion(a,
¬∃dq.DKey(point 5))-via-NegativeDataPropertyAssertion(a,dq,5) }` must be
**inconsistent** (dp⊑dq forces the dq value, contradicting the negative). The
mirror with `6` instead of the negated `5` must be **consistent**. This proves the
role-hierarchy + value-node + ∀/complement path end-to-end and is the B-vs-A gate.

**Sub-project 1 tests (TDD):**
- convert unit tests: each lowering arm produces the expected `Axiom` (gate ON);
  each produces `Ok(None)` (gate OFF) — byte-identical to today.
- unrecognized-datatype literal in an assertion ⇒ axiom dropped (gate ON).
- POC integration test above (inconsistent / consistent pair).
- **Gate-OFF corpus byte-identity**: classify a data-bearing fixture
  (shoiq-knowledge / sio) with gate OFF ⇒ identical to current `main`.

**Out of scope for sub-project 1:** corpus FP gate with the gate ON (that's
sub-project 3), termination proofs (sub-project 2), the query types (sub-project 4).

## Non-goals (whole arc)

- `DatatypeDefinition` (datatype aliasing) and `HasKey` over data properties.
- Datatypes the DKey machinery doesn't recognize (still dropped — sound).
- Data-property *chains* / transitive data properties (OWL-illegal anyway).
- Reworking the concrete-domain solver itself (it stays refute-only; B feeds it
  per-individual constraints through the existing node aggregation).
