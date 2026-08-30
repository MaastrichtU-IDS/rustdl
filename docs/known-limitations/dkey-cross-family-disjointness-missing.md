# Cross-datatype-family DKey disjointness is never seeded — a missed inconsistency

**Found:** 2026-08-30, root-causing `ore_ont_16321` / `ore_ont_4198` (two-peer-confirmed missed
inconsistency). **Status: OPEN, live on v0.4.24.** **Sound** — rustdl under-reports, never
over-reports — but **silent**: `classify --json` says `"consistent": true` with `"unsatisfiable":
[]` and no incompleteness signal.

## Two-axiom reproducer

```
DataPropertyRange(:STOICHIOMETRIC-COEFFICIENT xsd:double)
DataPropertyAssertion(:STOICHIOMETRIC-COEFFICIENT :sequenceParticipant3 "1.0"^^xsd:float)
```

| | verdict |
|---|---|
| Konclude v0.7.0-1138 | **inconsistent** |
| rustdl v0.4.24 (`classify` and `consistent`) | consistent |

OWL 2 §4.1 makes the value spaces of `xsd:double`, `xsd:float` and `owl:real` **pairwise
disjoint**, so a `float`-typed literal can never satisfy an `xsd:double` range.

**Both ORE ontologies reduce to this same pattern** — `ore_ont_16321` by delta-debugging from 426
candidate axioms down to exactly these two, and `ore_ont_4198` by direct construction from its own
`STOICHIOMETRIC-COEFFICIENT` range plus one of its 67 `float`-typed assertions.

## Root cause

`convert.rs` calls `seed_disjoint_bucket` **once per datatype bucket**:

```rust
seed_disjoint_bucket(out, &int_dkeys,    …);   seed_disjoint_bucket(out, &float_dkeys,  …);
seed_disjoint_bucket(out, &double_dkeys, …);   seed_disjoint_bucket(out, &dec_dkeys,    …);
seed_disjoint_bucket(out, &date_dkeys,   …);   seed_disjoint_bucket(out, &dt_dkeys,     …);
seed_disjoint_bucket(out, &str_dkeys,    …);   seed_disjoint_bucket(out, &lang_dkeys,   …);
```

Disjointness is therefore only ever seeded **within** a bucket. No cross-bucket pair is ever
constructed, so `DKey(double-range)` and `DKey(float-value)` coexist happily and no clash fires.

**This is the completeness twin of the v0.4.9 FALSE POSITIVE.** That bug folded `xsd:float` and
`xsd:double` into one f64 bucket and reported them EQUIVALENT; the fix split the buckets so they
could no longer cross-subsume. Splitting made it sound. Nothing then made the split buckets
**disjoint**, and `CLAUDE.md` records the absence as an FP-safety property — "`seed_disjoint_bucket`
is called once per DATATYPE bucket, so no cross-datatype pair is constructible" — which is exactly
why the clash is missed.

## Scope: broader than float/double, and rustdl is NOT blind to range violations

| probe | range | asserted value | Konclude | rustdl |
|---|---|---|---|---|
| A | `xsd:double` | `"1.0"^^xsd:float` | INCONSISTENT | **consistent** ✗ |
| B | `xsd:double` | `"1.0"^^xsd:double` | consistent | consistent ✓ |
| C | `xsd:float` | `"1.0"^^xsd:float` | consistent | consistent ✓ |
| D | `xsd:int` | `"abc"^^xsd:string` | INCONSISTENT | inconsistent ✓ |
| E | `xsd:int` | `"1.5"^^xsd:double` | INCONSISTENT | **consistent** ✗ |
| F | `xsd:string` | `"1"^^xsd:int` | INCONSISTENT | inconsistent ✓ |

So the gap is **numeric-vs-numeric**; string-vs-numeric is already caught (by a different route —
those two report `dropped` entries, and the range is not fully lowered). Soundness is intact: the
assertion ALONE is `consistent` in both reasoners, so D and F are not reaching `inconsistent`
through a false positive.

## Fix sketch, and the trap in it

The obvious fix — seed cross-bucket pairwise disjointness — is **O(k²) across buckets** and this
subsystem has a documented history of exactly that explosion (`ore_ont_9347`: 49.5 M concept
rules). A **family-marker** design is O(#DKeys) instead: give each datatype FAMILY one synthetic
marker class, seed `DKey ⊑ FamilyMarker` per key, and declare the family markers pairwise disjoint
(a constant ~21 axioms).

Feasibility measured, not assumed: disjointness propagates through subsumption in rustdl
(`A ⊑ M1`, `B ⊑ M2`, `Disjoint(M1,M2)` ⟹ `A ⊓ B` unsat), **and** through the `∃p.KA ⊓ ∀p.KB`
shape the DKey lowering actually builds. Both verified.

**THE TRAP: `int` and `decimal` are separate BUCKETS but the same FAMILY.** `xsd:int ⊆ xsd:integer
⊆ xsd:decimal ⊆ owl:real`, so a naive "different bucket ⇒ disjoint" rule manufactures false
positives. The families are `{int, decimal}` (one family, NOT self-disjoint), and `double`,
`float`, `string`, `date`, `dateTime`, `langString` as separate pairwise-disjoint families.

**Direction of risk is INVERTED for this fix:** it emits MORE disjointness, so the failure mode is
a FALSE POSITIVE, not a miss. Per this repo's own record the curated corpus is INERT for the DKey
area, so a green FP=0 net would demonstrate non-regression only — negatives-first canaries plus a
Konclude ∪ HermiT adjudication are the evidence that would count.
