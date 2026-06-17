# Justification Spec 2 — Property & Individual Query Reductions

**Date:** 2026-06-17
**Status:** design (approved scope), ready for implementation plan
**Builds on:** `2026-06-12-justification-foundation-design.md` (Spec 1, shipped),
the ⊥-locality module pre-pass (`4c71b09`), and `--labels` (`cc43bf5`).

## Goal

Extend the black-box justification engine (`crates/owl-dl-reasoner/src/justify.rs`)
with **six new query types** over object properties and individuals, so a user can
ask *why* an entailment of those kinds holds and get a minimal responsible-axiom
set — the same contract Spec 1 gives for class queries.

New queries:

| Query | Meaning |
|---|---|
| `SubObjectProperty {sub, sup}` | `P ⊑ Q` |
| `EquivalentObjectProperties {a, b}` | `P ≡ Q` |
| `DisjointObjectProperties {a, b}` | `Disjoint(P, Q)` |
| `ObjectPropertyAssertion {source, prop, target}` | `a P b` |
| `SameIndividual {a, b}` | `a = b` |
| `DifferentIndividuals {a, b}` | `a ≠ b` |

## Non-goal: data-property queries (with reason)

Data-property queries (`dp ⊑ dq`, `a dp v`, data equiv/disjoint) are **explicitly
out of scope**, not by preference but because the reasoner cannot back them. At
`convert.rs:1647-1655`, `SubDataPropertyOf`, `EquivalentDataProperties`,
`DisjointDataProperties`, `DataPropertyAssertion`, and
`NegativeDataPropertyAssertion` all convert to `Ok(None)` — dropped as a sound
under-approximation (the IR has no concrete-domain / literal ABox model; see Phase
D1). The D4–D11 preprocessing recovers only data axioms with **class-level**
consequences (ranges, cardinality → derived class axioms / DKey classes); raw data
assertions and data sub/equiv/disjoint produce no class consequence, so they stay
dropped. A query reduction over them would inject axioms the reasoner ignores and
return a vacuous "not entailed." Backing them requires the concrete-domain ABox
reasoner (`owl-dl-datatypes`, not yet wired in) — a separate, larger project.
Revisit only when that lands.

## Approach: inconsistency-with-injected-probe

Every new query reduces to a **consistency check on the ontology plus a fresh
"negation" probe** — the exact pattern Spec 1's `DisjointClasses` query already
uses (inject `X ≡ a ⊓ b`, check `X` unsatisfiable). The probe axioms are injected
*inside* `entails` on whatever subset it is handed; they are **never** candidate
axioms, so they never appear in a justification.

Rejected alternatives:
- **Native reasoner API** (`is_subproperty_of`, …) — changes the engine; black-box
  justify deliberately stays reasoner-agnostic over the public API.
- **Nominal class-encodings** (`P⊑Q` via `∃P.{o} ⊑ ∃Q.{o}`) — heavier, needs
  nominals, no advantage over the probe.

### Reductions

`_a`, `_b` are fresh probe individuals (IRIs `urn:rustdl-justify-probe-a` /
`-b`); `a`, `b`, `P`, `Q` are the user-supplied entities.

| Query | `entails` reduces to |
|---|---|
| `P ⊑ Q` | `¬is_consistent(O ∪ {ObjectPropertyAssertion(P,_a,_b), NegativeObjectPropertyAssertion(Q,_a,_b)})` |
| `P ≡ Q` | `entails(SubObjectProperty{P,Q}) ∧ entails(SubObjectProperty{Q,P})` |
| `Disjoint(P,Q)` | `¬is_consistent(O ∪ {ObjectPropertyAssertion(P,_a,_b), ObjectPropertyAssertion(Q,_a,_b)})` |
| `a P b` | `¬is_consistent(O ∪ {NegativeObjectPropertyAssertion(P,a,b)})` |
| `a = b` | `¬is_consistent(O ∪ {DifferentIndividuals(a,b)})` |
| `a ≠ b` | `¬is_consistent(O ∪ {SameIndividual(a,b)})` |

A single private helper does the injection:

```rust
fn inconsistent_with<A: ForIRI>(
    onto: &SetOntology<A>,
    extra: impl IntoIterator<Item = Component<A>>,
) -> Result<bool, ReasonError> {
    let mut probed = onto.clone();
    for c in extra { probed.insert(c); }
    Ok(!crate::is_consistent(&probed)?)
}
```

## Soundness (the load-bearing argument)

The contract is the Spec 1 contract: **a returned justification always genuinely
entails the query** (minimality exact only on EL/Horn, flagged). FP=0 must hold.

1. **No false-positive entailment.** `entails` returns `true` only when
   `O ∪ probe` is inconsistent. rustdl's consistency is **sound** (it never reports
   a consistent ontology as inconsistent). So a `true` verdict means `O ∪ probe`
   is genuinely inconsistent ⇒ the queried entailment genuinely holds. No spurious
   justification.

2. **The probe encodes exactly the negation.** Fresh `_a`,`_b` are otherwise
   unconstrained, so any forced clash generalizes from "for these two" to the
   universal statement (standard fresh-individual reduction). For the assertion /
   same / different queries the individuals are the user's own and the probe is the
   direct negation.

3. **Object-property domain/range cannot cause an *unsound* positive.** Asserting
   `P(_a,_b)` may type `_a`/`_b` via `domain(P)`/`range(P)`. The only way that alone
   makes `O ∪ probe` inconsistent is if a domain/range is unsatisfiable — but then
   `P` is necessarily **empty**, which makes `P⊑Q` (and `Disjoint(P,Q)`) vacuously
   **true**. So "inconsistent → entailed" stays correct. (This is exactly why the
   reduction is safe for *object* properties but *not* data properties, where a
   fixed literal's datatype could clash with a range without the property being
   empty — another reason data queries are excluded.)

4. **Ontology already inconsistent.** Then every query reports "entailed," which is
   semantically correct (`⊥ ⊨ everything`); the justification is the inconsistency's
   cause. Same behavior as Spec 1's existing `Unsatisfiable`/`Inconsistent` probes.

**Completeness** inherits the reasoner's: the clash must be *detectable*. The A1
ABox pre-check (`P3` NegOPA-vs-OPA with role-hierarchy propagation, `P4`
SameAs∩DifferentFrom) plus the tableau cover these patterns. A miss yields a false
*negative* ("not entailed") — sound, possibly incomplete — never a false positive.

## ⊥-locality module

Extend `query_seed_signature` with the new variants. Unlike global `Inconsistent`
(which returns `None` and disables the module), these queries have **bounded
signatures**, so the ⊥-module stays justification-preserving and they get the same
speedup:

| Query | Seed signature |
|---|---|
| `SubObjectProperty{P,Q}`, `EquivalentObjectProperties{P,Q}`, `DisjointObjectProperties{P,Q}` | `{P, Q}` |
| `ObjectPropertyAssertion{a,P,b}` | `{a, P, b}` |
| `SameIndividual{a,b}`, `DifferentIndividuals{a,b}` | `{a, b}` |

The fresh probe individuals (`_a`,`_b`) do not appear in candidate axioms, so they
need not seed. The find-one safety wrapper (`localized_candidates`: verify
`fixed ∪ module ⊨ q`, else fall back to full) still backstops a locality bug;
find-all completeness rests on `is_bot_local` as before, now also exercised by a
property-query entry in the differential canary.

## Components / files

- **`crates/owl-dl-reasoner/src/justify.rs`**
  - `Entailment` enum: +6 variants.
  - `entails`: +6 match arms (5 via `inconsistent_with`; `EquivalentObjectProperties`
    delegates to two `SubObjectProperty` checks).
  - `query_seed_signature`: +6 arms per the table above.
  - New consts `PROBE_A`/`PROBE_B` (fresh probe individual IRIs).
  - `inconsistent_with` helper.
- **`crates/owl-dl-cli/src/main.rs`**
  - `parse_justify_query`: +6 verbs — `subproperty P Q`, `equiv-property P Q`,
    `disjoint-property P Q`, `property A P B`, `same A B`, `different A B`.
    (Distinct from the class verbs `equivalent`/`disjoint` to avoid ambiguity.)
  - Help/usage text updated.
  - `--labels` needs no change (covers role/individual axioms via
    `component_entities`).

## Testing

In `crates/owl-dl-reasoner/tests/justification.rs` (per-query, entailed + not):
- `SubObjectProperty`: `P⊑Q,Q⊑R ⊨ P⊑R`; justification = `{P⊑Q,Q⊑R}`; and a
  non-entailed `P⊑Q` returns `None`.
- `EquivalentObjectProperties`: `P⊑Q,Q⊑P ⊨ P≡Q`; justification = both.
- `DisjointObjectProperties`: told `Disjoint(P,Q) ⊨ Disjoint(P,Q)`; justification =
  `{Disjoint(P,Q)}`; absent → `None`.
- `ObjectPropertyAssertion`: `(a P b), P⊑Q ⊨ (a Q b)`; justification = both;
  non-entailed → `None`.
- `SameIndividual`: a functional-property merge that forces `a=b`, justified;
  non-entailed → `None`.
- `DifferentIndividuals`: told `Different(a,b) ⊨ a≠b`; absent → `None`.
- **Probe-never-in-output**: assert no returned axiom mentions
  `urn:rustdl-justify-probe-*`.
- **Module differential**: add a property query to a fixture with two
  justifications and assert `find_all` module-on == module-off (extends the
  existing `module_preserves_all_justifications` style).

In `crates/owl-dl-cli/src/main.rs` `label_tests`/a new `query_parse_tests`:
- each new verb parses to the right `Entailment`; wrong arity errors.

## Out of scope (this spec)

- Data-property queries (see the non-goal section — reasoner-limited).
- Laconic/precise justifications + root-vs-derived unsat (Spec 3).
- Glass-box step-proofs for these query types (SROIQ proofs remain future work).
