# The DKey conversion stall is `seed_bucket`'s O(k²) string seeding — measured, then fixed exactly

**Date:** 2026-08-20 · Continues `2026-08-20-dkey-residual-class-unpark-case.md`, which established
that 8 DNF-tail members are conversion-bound but left the cost attributed only to "the
data-property path as a whole", with `seed_bucket` named as the leading suspect **on arithmetic,
not measurement**. This measures it and fixes it.

## The probe

Temporary timing around the two seeding groups in `seed_dkey_subsumptions`, `ore_ont_10929`:

```
DKEYPROBE buckets int=0 float=0 double=0 dec=0 date=0 dt=0 str=57342
DKEYPROBE subsumption_ms=73872
DKEYPROBE disjointness_ms=20484
```

**The suspicion was right and now it is measured:** the `xsd:string` bucket holds **57,342** keys,
so `seed_bucket`'s walk is ~3.29 × 10⁹ ordered pairs and costs **73.9 s of the ~94 s
`convert_ms`** — 78%. Disjointness seeding is the other 20.5 s, which is what the
already-shipped `RUSTDL_DKEY_GROUP_SKIP` addresses (20.5 s → ~1.5 s, consistent with the 19 s it
was measured to save).

Every other datatype bucket on this ontology is **empty**. The cost is strings, specifically.

## Why all 3.29 × 10⁹ tests are provably futile

`StrSet::subset` is:

```rust
(_, StrSet::Top)              => true,
(StrSet::Top, StrSet::Set(_)) => false,
(StrSet::Set(a), StrSet::Set(b)) => a.is_subset(b),
```

and the call site's own comment supplies the rest: *"distinct keys ⟹ strict subset, since equal
ranges share one `ClassId`"*. So a `Set ⊆ Set` edge between distinct keys requires
**|a| < |b|**. Every string DKey minted from a `DataPropertyAssertion` is a **singleton**, so on
this ontology **no `Set ⊆ Set` edge can exist at all** — the walk performs 3.29 billion
comparisons that cannot succeed.

## The fix: size-indexed seeding (`RUSTDL_DKEY_STR_SIZE_INDEX`, default OFF)

`seed_str_bucket_indexed` enumerates only what can succeed:

* `Set ⊆ Top` for every finite set × every `Top`;
* `Top ⊆ Set` never;
* `Set(a) ⊆ Set(b)` only across **strictly increasing cardinalities**.

**Exact, not an approximation** — it is the same relation, enumerated over a partition that
excludes only provable non-edges. Emission order differs from `seed_bucket`, which is immaterial
because `out.axioms` is sorted downstream.

### Measured

Phase-level, `ore_ont_10929` (with the group skip also on):

| | `subsumption_ms` | `disjointness_ms` | `concept_rules` |
|---|---:|---:|---:|
| off | 72,192 | 9 | 57,355 |
| **on** | **9** | 5 | **57,355** |

**8,000× on the dominant phase, rules identical.**

End-to-end conversion wall over all 9 candidates, both fixes off vs both on:

| ontology | off | on | speedup | concept_rules |
|---|---:|---:|---:|---|
| `ore_ont_10929` | 102.5 s | **3.9 s** | **26.3×** | identical (57,355) |
| `ore_ont_15635` | 125.9 s | **7.3 s** | **17.2×** | identical (54,248) |
| `ore_ont_9347` | 6.9 s | **0.6 s** | **11.5×** | identical (113) |
| `ore_ont_2504` | 235.0 s | 238.6 s | 1.0× | identical (**68,761,866**) |
| `ore_ont_4141` | 134.7 s | 129.9 s | 1.0× | identical (**42,738,529**) |
| `ore_ont_5368` | 52.9 s | 50.6 s | 1.0× | identical (18,620,251) |
| `ore_ont_1833` | 39.1 s | 37.0 s | 1.1× | identical (14,030,936) |
| `ore_ont_4572` | 300.2 s | 301.6 s | 1.0× | both timeout |
| `ore_ont_8445` | 301.1 s | 301.5 s | 1.0× | both timeout |

**THE FIXES HELP 3 OF 9, AND THAT IS THE HONEST SCOPE.** An earlier draft of this document
reported only the first two rows and read as though the class were solved. It is not.

**Why the other six are flat, and it is not a shortcoming of the fix.** They materialise
**14–68 million** concept rules. Both fixes eliminate *futile* work — enumeration whose result is
provably discarded. Where the axioms are genuinely emitted there is nothing to skip, and no
enumeration trick can help. This maps exactly onto the sub-classification in the companion
document: the 2 members whose pairs are **100% droppable** are the 2 the fixes rescue; the 4 whose
pairs are **~0% droppable** are untouched, as are the 2 that never finish enumerating.

So the split is now measured rather than predicted:

| sub-class | n | fix |
|---|---:|---|
| futile enumeration | **2** (+`9347`, already fast) | **these two flags — done** |
| genuinely materialised axioms | 4 measured + 2 timeouts | on-demand disjointness oracle (**still parked**) |

That leaves the oracle's addressable set at **~6**, which is close to the spec's original "~4" and
**does not overturn its work-to-reward parking decision.** The unpark case in the companion
document should be read with that correction.

## Scope, and what is NOT fixed

* **6 of the 9 are unaffected** — see the table above. Those need the parked oracle, not this.

* **Only the string bucket is specialised.** `seed_bucket` remains O(k²) for the other six
  datatype buckets. That is deliberate — every other bucket measured **0** on this population — but
  an integer- or decimal-heavy ontology with tens of thousands of distinct values would hit the
  same wall, and the monotonic-cardinality argument **does not transfer to interval ranges**. That
  needs its own reasoning; do not assume it generalises.
* The two fixes are complementary, not alternatives: the group skip handles disjointness seeding
  (20.5 s), this handles subsumption seeding (73.9 s). Neither alone is sufficient.
* Both ship **default OFF** pending a corpus sweep. They change conversion output volume on every
  data-property-bearing ontology, and this repo's record contains a 12-ontology benchmark that hid
  four `ok → DNF` regressions.

## Method note

The previous document named `seed_bucket` as the suspect from arithmetic (~3.6 × 10⁹ pairs at
~26 ns ≈ the residual) and explicitly said *"probe the two seeding calls before touching the
loop."* That instruction was followed and the arithmetic held — but it was checked first, which is
the only reason the fix landed on the right loop. Three causal stories elsewhere in this session
were plausible and wrong.
