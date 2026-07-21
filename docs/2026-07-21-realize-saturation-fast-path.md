# Realize saturation fast path — fixes the issue-#35 realization hang (2026-07-21)

## Symptom

`materialize_inferred_class_assertions` / CLI `realize` (and `instance` /
`instances`) **did not terminate** (>300 s) on a tiny satisfiable 4-axiom
ontology, while `classify` and `materialize_inferred_property_assertions` on the
same file returned instantly (GitHub issue #35). Minimal reproducer:

```
EquivalentClasses(:Man   ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Male)))
EquivalentClasses(:Woman ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Female)))
ObjectPropertyDomain(:hasParent :Person)
ObjectPropertyAssertion(:isMotherOf :a :b)
```

## Root cause

`realize_internal` ran the full SROIQ tableau (`prepared.decide`, the
`{a} ⊓ ¬C` probe) for **every** (individual, satisfiable-class) pair. On this
EL/Horn ontology the two non-absorbable GCIs from the `≡` definitions
(`Person ⊓ ∃hasSex.Male ⊑ Man`, likewise `Woman`) let every node speculatively
pick `Man`/`Woman` → generate a `∃hasSex` successor → recurse. Blocking capped
the live graph at ~134 nodes but the ⊔-backtracking search burned 100 000+
branches and never finished. **Every** `{x} ⊓ ¬C` probe diverged (even
`¬Male`, no existentials) — the blow-up is model construction, not the query.

`classify` never hits this: it dispatches EL/Horn ontologies to the saturation
fast path (`is_pure_el` / `saturator_complete_fragment` / Lever-1
`tbox_only_saturator_eligible`) and never builds a tableau. `realize` had **no
such gate**.

## Fix (Tier B: nominal-materialization saturation, complete on the fragment)

Two independent, default-ON parts.

### 1. Fast path — `owl_dl_saturation::saturate_for_realize`

Materializes **every named individual** `a` as an opaque nominal class `N_a`
(reusing the existing `introduce_nominal`) and seeds its ABox constraints into
the EL saturator:

- `N_a ⊑ C` for each `ClassAssertion(C, a)` (atomic operands of `C`);
- `N_a ⊑ ∃r.N_b` for each `ObjectPropertyAssertion(r, a, b)` — a **ground**
  edge. Gives `Domain(r)` on `a` for free (fact-time domain propagation walks
  the super-role closure) and lets existential-LHS GCIs fire
  (`Person ⊓ ∃hasSex.Male ⊑ Man` with `Male(b)` ⟹ `a:Man`);
- `N_b ⊑ Rng` for each such edge and each effective `Range(r)`. The saturator
  deliberately omits range propagation through `∃`-facts (unsound for
  *anonymous* witnesses), but for a **ground** nominal successor `b` the range
  obligation genuinely holds, so it is seeded explicitly.

Runs to fixpoint; entailed named types of `a` = `subsumers_of(N_a)` restricted
to declared user classes and satisfiable classes. **Complete == the tableau**
on the saturator-complete EL/Horn fragment — including the conjunctive-LHS case
`x:D1, x:D2, D1 ⊓ D2 ⊑ E ⊨ x:E` that the old closure-only
`realize_saturation_only` (and `abox_saturation`) both drop. **Sound by
construction** (every seeded axiom is entailed).

`realize` / `is_instance_of` / `instances_of` take this path when
`realize_saturation_eligible` holds: the **TBox** is in the saturator-complete
fragment **and** every ABox axiom is a shape the seeding captures exactly
(atomic/⊓ `ClassAssertion`, non-inverse `ObjectPropertyAssertion`;
`NegativeObjectPropertyAssertion` / `DifferentIndividuals` are type-irrelevant
and allowed; `SameIndividual` and inverse-role assertions fall back). Unlike
`classify`, realize **cannot** admit arbitrary ABox shapes — the ABox is
load-bearing for individual types. `RUSTDL_REALIZE_SATURATION=0` forces the
tableau path (A/B isolation).

### 2. Off-fragment backstop — per-pair tableau deadline

`RUSTDL_REALIZE_PAIR_TIMEOUT_MS` (default UNSET ⟹ no bound, verdict-preserving)
bounds each `{a} ⊓ ¬C` probe on the off-fragment tableau path; a deadline hit
yields "not an instance" — a sound under-approximation. Restores the
caller-side bound removed in 0.3.18 so a genuinely-hard SROIQ *pair* can no
longer hang unbounded. (It bounds each pair, not the total wall of a large
individual×class product — off-fragment ABox-heavy inputs like `wine` remain
slow, as before.)

## Result

- Issue-#35 reproducer: `realize` / `instance` / `instances` terminate
  instantly with correct answers (`a`, `b` have no entailed named type).
- FP=0 preserved; fast path complete == tableau on the fragment (byte-identical
  A/B on a terminating fixture).
- `classify` untouched; off-fragment realize is the identical prior logic plus
  the optional deadline.

## Tests

`crates/owl-dl-reasoner/src/realize.rs` (unit): conjunctive-LHS, existential-LHS
(family shape), domain+range, issue-#35 termination (realize + is_instance_of),
and `fast_path_matches_tableau_on_terminating_fixture` (fast-path == tableau
byte-identity).
