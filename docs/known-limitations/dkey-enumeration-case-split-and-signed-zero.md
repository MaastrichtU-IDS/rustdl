# A data enumeration is not split into its points, and a zero at a bound gets no edge (#556 residuals)

**Status:** open. Both are sound misses. The first can read `completeness_guaranteed: true` (the D10 shape), so it needs an issue.

#556 seeds told `DKey ⊑ DKey` edges between the range bucket and the enumeration bucket of one datatype: `enumeration ⊑ range` and `range ⊑ enumeration`. Two shapes remain out of reach.

## 1. Reasoning by cases over an enumeration

```
ClassAssertion(DataSomeValuesFrom(:d DataOneOf("1"^^xsd:integer "2"^^xsd:integer)) :a)
NegativeDataPropertyAssertion(:d :a "1"^^xsd:integer)
NegativeDataPropertyAssertion(:d :a "2"^^xsd:integer)
```

HermiT and Konclude say inconsistent; rustdl says consistent. The `TBox` form is the same: `C ⊑ ∃d.{1,2}`, `∃d.{1} ⊑ E`, `∃d.{2} ⊑ E` entail `C ⊑ E`, which rustdl misses.

The entailment needs `{1,2} ⊑ {1} ⊔ {2}`, a disjunction, and no `DKey ⊑ DKey` edge can express that. The `TBox` form reads `completeness_guaranteed: true` with nothing dropped, on main as on #556. The same happens inside the range bucket alone, with `int[1,2]` in place of `{1,2}` (HermiT derives it).

**Possible fix (not done).** Emit `DKey(S) ⊑ ⊔ DKey(point v)` for an enumeration whose members all have point keys. This is a disjunctive axiom, so it takes such ontologies out of the EL fragment, and it needs its own measurement on the 7 ORE files with numeric enumerations. Failing that, the fragment gate should refuse to certify a multi-member enumeration that shares a role with point keys.

Pinned (`#[ignore]`d, asserting the right answer): `excluding_every_member_of_a_multi_member_enumeration` in `crates/owl-dl-reasoner/tests/dkey_range_oneof_subsumption.rs`.

## 2. A zero at an IEEE range bound

```
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("0.0"^^xsd:double)))
SubClassOf(DataHasValue(:d "0.0"^^xsd:double) :E)
```

HermiT and Konclude derive `C ⊑ E`; rustdl does not.

The `xsd:float` / `xsd:double` buckets collapse `-0` and `+0` (`OrdF64::new`). That is safe for disjointness, but an edge resting on it could claim `{-0} ⊑ {+0}`, and HermiT keeps the two apart (Konclude does not). So #556 gives a zero member an edge only when zero is strictly inside the range, and gives a zero point no `range ⊑ enumeration` edge.

Pinned (`#[ignore]`d): `an_enumerated_zero_against_a_zero_point` in the same file.

The range bucket alone already derives `hasValue(d, -0.0) ⊑ hasValue(d, +0.0)` on main (HermiT: no; Konclude: yes). That is a separate question about signed zero, recorded for an issue.
