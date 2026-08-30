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

## Scope, measured with SUPPORTED datatype spellings and an empty `dropped`

The first version of this matrix used `xsd:int`, which rustdl reports as
`DataPropertyRange: unsupported data range` — so those rows measured "the range never lowered",
not the disjointness gap. Re-probed with `xsd:integer`, `dropped` empty throughout, and with
**both** oracles (HermiT IS reachable on these two-axiom files even though it returns `NO_OUTPUT`
on the full ontologies — which is what upgrades this from the Konclude+KM evidence the earlier
record had to the repo's Konclude ∪ HermiT standard):

| range | asserted value | Konclude | HermiT | rustdl |
|---|---|---|---|---|
| `xsd:double` | `"1.0"^^xsd:float` | INCONSISTENT | INCONSISTENT | **consistent** ✗ |
| `xsd:integer` | `"1.0"^^xsd:float` | INCONSISTENT | INCONSISTENT | **consistent** ✗ |
| `xsd:integer` | `"1.5"^^xsd:double` | INCONSISTENT | INCONSISTENT | **consistent** ✗ |
| `xsd:double` | `"1.0"^^xsd:double` | consistent | consistent | consistent ✓ |
| `xsd:float` | `"1.0"^^xsd:float` | consistent | consistent | consistent ✓ |
| `xsd:decimal` | `"1"^^xsd:integer` | consistent | consistent | consistent ✓ |

**rustdl is NOT blind to range violations.** A full 7×7 range×value matrix shows its existing
mechanism works at a COARSE-GROUP granularity — `{numerics}`, `{string}`, `{date,dateTime}` clash
ACROSS groups and never WITHIN:

```
range\val  integer decimal double  float   string  date    dateTime
integer     con     con     con     con     INC     INC     INC
decimal     con     con     con     con     INC     INC     INC
double      con     con     con     con     INC     INC     INC
float       con     con     con     con     INC     INC     INC
string      INC     INC     INC     INC     con     INC     INC
date        INC     INC     INC     INC     INC     con     con
dateTime    INC     INC     INC     INC     INC     con     con
```

So the gap is exactly the **numeric block's interior**.

**TWO ASSUMPTIONS WERE REFUTED BY MEASURING, AND BOTH WOULD HAVE CAUSED FALSE POSITIVES.**

1. **`date` vs `dateTime` must NOT be made disjoint.** They look like a second gap in the matrix
   above, but `xsd:date` is **not in the OWL 2 datatype map** — HermiT refuses the probe outright
   with `UnsupportedDatatypeException`, and Konclude reports `consistent`. No peer supports the
   entailment, so seeding it would manufacture an FP.
2. **`integer` × `"1.5"^^xsd:decimal` IS inconsistent (both oracles) but is NOT a family
   question.** `integer` and `decimal` are the same family (`owl:real`), and the reverse direction
   `decimal` × `"1"^^xsd:integer` is correctly consistent. That asymmetry is VALUE membership
   (`1.5 ∉ integer`), a separate gap needing the integer range to reject a non-integral value —
   recorded here so it is not conflated with, or silently folded into, the family fix.

**Fix scope is therefore THREE families and THREE disjoint pairs:** `real = {integer, decimal}`,
`double`, `float`, pairwise disjoint. Nothing temporal, nothing involving `string` (already
caught).

## Fix sketch, and the trap in it

The obvious fix — seed cross-bucket pairwise disjointness — is **O(k²) across buckets** and this
subsystem has a documented history of exactly that explosion (`ore_ont_9347`: 49.5 M concept
rules). A **family-marker** design is O(#DKeys) instead: give each datatype FAMILY one synthetic
marker class, seed `DKey ⊑ FamilyMarker` per key, and declare the family markers pairwise disjoint
(a constant ~21 axioms).

Feasibility measured, not assumed: disjointness propagates through subsumption in rustdl
(`A ⊑ M1`, `B ⊑ M2`, `Disjoint(M1,M2)` ⟹ `A ⊓ B` unsat), **and** through the `∃p.KA ⊓ ∀p.KB`
shape the DKey lowering actually builds. Both verified.

**THE TRAP: `integer` and `decimal` are separate BUCKETS but the same FAMILY.** `xsd:integer ⊆
xsd:decimal ⊆ owl:real`, so a naive "different bucket ⇒ disjoint" rule manufactures false
positives — and the oracles confirm it, since `decimal` range with an `integer` value is
CONSISTENT in both. The seeded families are exactly `real = {integer, decimal}`, `double` and
`float`: three families, three disjoint pairs. Nothing temporal (refuted above), nothing involving
`string` (already caught by the existing coarse-group mechanism).

**Direction of risk is INVERTED for this fix:** it emits MORE disjointness, so the failure mode is
a FALSE POSITIVE, not a miss. Per this repo's own record the curated corpus is INERT for the DKey
area, so a green FP=0 net would demonstrate non-regression only — negatives-first canaries plus a
Konclude ∪ HermiT adjudication are the evidence that would count.
