# `DatatypeDefinition` over a `DataIntersectionOf` is dropped (a signalled miss on `ore_ont_15682`)

**Date:** 2026-11-28 · **Status:** known, signalled, not fixed ·
**Severity:** a sound miss; every surface reports the drop

## Shape

`ore_ont_15682` (an ORE copy of the W3C wine ontology) names its value ranges with
`DatatypeDefinition`s over a `DataIntersectionOf` of two double facets, and uses those
names in class definitions:

```
DatatypeDefinition(:HighPriceForWine
    DataIntersectionOf(DatatypeRestriction(xsd:double xsd:maxInclusive "10000.0"^^xsd:double)
                       DatatypeRestriction(xsd:double xsd:minInclusive "0.0"^^xsd:double)))
DatatypeDefinition(:LowPriceForWine  …the same intersection…)
EquivalentClasses(:HighPriceWine DataSomeValuesFrom(:hasPrice :HighPriceForWine))
EquivalentClasses(:LowPriceWine  DataSomeValuesFrom(:hasPrice :LowPriceForWine))
```

Three of the price datatypes (and three of the UWS-score ones) are defined by the same
intersection, so `HermiT` derives `HighPriceWine ≡ LowPriceWine ≡ MediumPriceWine` (and the
same for the UWS classes): 12 rows in all. Konclude reports none of them.

## What rustdl does

Conversion drops the data range it cannot lower and says so (main `ac7ff1ee`,
`classify --json`):

```
"dropped": {"DatatypeDefinition: unsupported data range": 23,
            "EquivalentClasses: unsupported data range": 7,
            "SubClassOf: unsupported data range": 9},
"completeness_guaranteed": false
```

The 12 rows are missing, and no completeness is claimed. It is the only missed row in the
full-closure diff of the #589 flagged files (132 of 133 files with an oracle closure miss
nothing), and it is **not** a data case split (#589): the fix is to lower
`DataIntersectionOf` (and `DatatypeDefinition` naming it) to the intersection key of its
operands, a single `DKey` that the existing seeding then relates. That lowering would also be
the first use of intersection keys, if #589's Step 2 is ever built.

## Evidence

- `HermiT` rows: `~/rustdl/missed-net/tsv/hermit/ore_ont_15682.tsv.gz`.
- rustdl output: `~/rustdl/sweeps/bugfix/589/step2/15682.json`.
- Found by the Fable review of the #589 Step 2 plan (`~/rustdl/sweeps/bugfix/589/step2/plan.md`).
