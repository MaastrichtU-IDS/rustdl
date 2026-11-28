# A data enumeration is not split into its points, and a zero at a bound gets no edge (#556 residuals)

**Status:** open, and signalled since #589. Both are sound misses. Before #589 the first read `completeness_guaranteed: true` (the D10 shape); now every surface reports `data_case_split_risk: true` and voids its guarantee (condition 12, see "Signal" below). The entailments themselves are still missed.

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

## Signal (#589)

Both shapes, and the wider family they belong to, now set `data_case_split_risk`
(`owl_dl_core::convert::data_case_split_risk`): a data key that is not a single value meets
another key of its value space without a seeded `⊑` edge into it. The family, from the #589
plan review (probes in `~/rustdl/sweeps/wB/589/fable/`):

- a range or enumeration covered by others: `{1,2}` by its points, `int[1,10]` by `[1,5]` and
  `[6,10]`, an integer range by decimal ranges, two overlapping decimal ranges, `xsd:integer`
  by `≤0` and `≥1`, `xsd:boolean` and a string enumeration by their values, two adjacent float
  points;
- a range narrowing a filler: `Range(d, [9,20])` with `∃d.[1,10]`, or
  `Range(d, nonNegativeInteger)` with `∃d.(≤1)`, and the points of the intersection;
- an unrelated superset key does not clear the flag (`t_superkey`), and a zero point counts as
  two values (section 2);
- an empty key (`int[5,1]`, `decimal(1.0, 1.0]`, float bounds with no `f32` between them),
  which needs no split but is an atom to the engines, so `C ⊑ ∃d.int[5,1]` left `C`
  satisfiable (Fable review of #589; pre-existing on main);
- a plain string against bare `rdf:langString`, which `HermiT` and Konclude read as holding it
  (no `str ⊑ lang` edge is seeded).

The detector is coarse (role- and polarity-blind), so it also flags shapes that entail nothing,
such as a cover on another property (`t_otherprop`). On the 1,920-file ORE pool (2026-11-28, through `unlowered`; 25 files cut at
90 s, 8 rejected) it flags 424 files, but only 2 of the 944 the classify fast-path gate admits
(`ore_ont_11949`, `ore_ont_15635`) and 1 of the 911 realize admits, so the certificates it
costs are few.

**Not done (deferred):** stating the split as a told disjunction
(`DKey(K) ⊑ DKey(K₁) ⊔ … ⊔ DKey(Kₙ)`). It answers right where it applies (hand-checked on the
plan review's `s2_*` probes), but it does not cover the range-intersection shape, takes the
ontology off the saturation fast path, and needs an arity cap. The in-EL intersection axiom for
range narrowing (`DKey[1,5] ⊓ DKey[5,10] ⊑ DKey[5,5]`, `s2_range_inter_horn`) is the better
first completeness step.

