# A data enumeration is not split into its points (#556), and signed-zero residuals (#590)

**Status:** open, and signalled since #589. Both are sound misses. Before #589 the first read `completeness_guaranteed: true` (the D10 shape); now every surface reports `data_case_split_risk: true` and voids its guarantee (condition 12, see "Signal" below). The entailments themselves are still missed. §2 (#590): the one remaining signed-zero residual, the case split over `[0, 0]`, voids the guarantee through two conditions at once. The fragment gate rejects the ontology (`has_signed_zero_split`, condition 1), and `data_case_split_risk` is set because `[0, 0]` meets the zero points without an edge into them (condition 12). It is one residual, not two.

#556 seeds told `DKey ⊑ DKey` edges between the range bucket and the enumeration bucket of one datatype: `enumeration ⊑ range` and `range ⊑ enumeration`. Two shapes remain out of reach: §1, and the signed-zero cases in §2.

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

## 2. Signed zero (#590)

OWL 2 (Structural Specification §4.2) reads a literal value by **identity**, where `-0` and `+0` are distinct, and a **facet** by equality, where they are equal. So `hasValue(d, -0.0)` and `hasValue(d, 0.0)` are different values, while the facet range `[0, 0]` holds both. Since #590:

- a literal zero point keeps its own key, and the two zero points are disjoint;
- `DataOneOf` members keep their sign;
- the degenerate facet range `[0, 0]` has its own key, written `[-0, +0]`;
- a facet range that contains 0 is above both zero points.

The #556 zero-at-a-bound residual (`{+0} ⊑ hasValue(+0)`) is now derived.

What remains:

- **Case split over `[0, 0]`.** `∃d.[0, 0]` is `∃d.{+0} ⊔ ∃d.{-0}`, and nothing makes that split. With `C ⊑ ∃d.double[0, 0]`, `hasValue(d, 0.0) ⊑ E` and `hasValue(d, -0.0) ⊑ E`, the entailment `C ⊑ E` is missed, and so is `∃d.[0, 0] ⊑ ∃d.{+0, -0}`. The specification says yes to both. HermiT says yes for `xsd:float` and no for `xsd:double`. Whenever a `-0` value or the `[0, 0]` range occurs, the PureEl gate withholds the completeness guarantee (`has_signed_zero_split`), so `completeness_guaranteed` reads `false`. No ORE file has one.
- **Counting the zeros is not a residual.** The concrete-domain bridge counts the IEEE zero point as two values (`PointMultiplicity`), however the range was reached, and counts enumeration members by identity. So `≥2 d.[-1, 0] ⊓ ∀d.[0, 1]` and `≥2 d.{+0, -0}` are satisfiable, and `≥3 d.[0, 0]` is unsat. HermiT reads the `xsd:double` enumeration forms unsat, and the specification decides (§4.2).
- `Functional(d)` with `d(a, 0.0)` and `d(a, -0.0)` is inconsistent (the specification's own example), and rustdl derives it from the disjoint zero points. HermiT reads the `xsd:double` form consistent.

Pinned in `crates/owl-dl-reasoner/tests/signed_zero.rs` (`signed_zero_residuals`) and `concrete_domain_clash.rs` (`float_oneof_signed_zero_dedup_unsat`).

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
- an unrelated superset key does not clear the flag (`t_superkey`), and the degenerate zero
  range `[0, 0]` counts as two values, while a literal zero point is one (section 2, #590);
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

