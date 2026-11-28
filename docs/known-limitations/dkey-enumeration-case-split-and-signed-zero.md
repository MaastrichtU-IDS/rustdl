# A data enumeration is not split into its points (#556), and signed-zero residuals (#590)

**Status:** open. §1 is a sound miss that can read `completeness_guaranteed: true` (the D10 shape), so it needs an issue. §2's remaining residual is a sound miss under a withheld guarantee.

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
