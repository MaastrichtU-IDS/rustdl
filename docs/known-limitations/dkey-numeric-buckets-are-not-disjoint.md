# The numeric DKey buckets are non-comparable but not DISJOINT — rustdl misses the clash

**Status: FIXED on the `is_consistent` surface (`RUSTDL_DKEY_CROSS_BUCKET_DISJOINT`,
default ON, `=0` reverts). `classify` STILL MISSES IT — see the residual below.**

Was: live on v0.4.24, sound (a MISS, never an FP), root-caused to a 2-axiom reproducer
adjudicated by Konclude AND HermiT with a discriminating control.

## Residual: `classify` and `consistent` still disagree here

`rustdl consistent ore_ont_16321` now correctly reports **inconsistent**; `classify --json`
on the same file still reports `consistent: true, unsatisfiable: []`.

The clash is ABox-level — an individual's data value violating a declared range — so no
*named class* becomes unsatisfiable, and classify's inconsistency pre-check
(`saturator globally_inconsistent`/`top_is_unsat` plus `abox_saturation`) cannot see it:
`abox_saturation` propagates over NAMED individuals with no witness generation, and the
clash lives at the data successor. Closing it needs either DKey-aware ABox saturation or a
consistency probe on the classify path — different machinery, not this change.

This is the documented classify-vs-`consistent` under-approximation, now with a concrete
instance.

## The 2-axiom reproducer

```
DataPropertyRange(:p xsd:double)
DataPropertyAssertion(:p :a "1.0"^^xsd:float)
```

`xsd:float` and `xsd:double` have disjoint value spaces in OWL 2, so the asserted value
cannot lie in the declared range and the KB is **inconsistent**.

| | float/double | float/float (control) |
|---|---|---|
| HermiT | `owl:Thing is not satisfiable` | satisfiable |
| Konclude | `false` | `true` |
| **rustdl** | **`consistent`** ✗ | `consistent` ✓ |

The control is what makes this a finding rather than a disagreement: all three agree on
float/float, so rustdl is wrong specifically on the cross-datatype case. Nothing is
dropped (`dropped: {}`), so this is not the graceful-degradation path.

## Root cause

`seed_dkey_subsumptions` buckets DKeys by datatype and seeds subsumption edges **only
within a bucket**. That is correct and deliberate — it is precisely what fixed the
v0.4.6–v0.4.9 false positive where `xsd:float` and `xsd:double` were folded into one
f64-keyed bucket and reported EQUIVALENT.

D11b then seeds `DisjointClasses(DKey(ra), DKey(rb))` for provably-disjoint pairs, also
**only within a bucket**. So two DKeys in different buckets are neither subsumption-related
nor disjoint, and `∃p.DKey(f:1.0)` never clashes with an `xsd:double` range.

**The v0.4.9 fix removed the false equivalence and never added the disjointness.** This
defect is its dual, and has been latent since.

## The exact scope, measured

Cross-*family* disjointness already works; only the numeric split is missing.

| range | asserted | Konclude | rustdl | |
|---|---|---|---|---|
| `double` | `float` | inconsistent | consistent | **MISS** |
| `decimal` | `float` | inconsistent | consistent | **MISS** |
| `integer` | `float` | inconsistent | consistent | **MISS** |
| `float` | `double` | inconsistent | consistent | **MISS** |
| `integer` | `decimal` | **consistent** | consistent | agree |
| `decimal` | `integer` | **consistent** | consistent | agree |
| `string` | `integer` | inconsistent | **inconsistent** | agree |

## THE TRAP ANY FIX MUST AVOID

**`xsd:integer ⊂ xsd:decimal` — they are NOT disjoint**, and Konclude confirms it in both
directions above. A fix that simply declares "different numeric bucket ⇒ disjoint" would
emit a false `DisjointClasses` and turn a MISS into a **false positive** — in the one
subsystem this repo documents as having already shipped an FP for months.

The correct table is:

* `float` ⊥ `double`, `decimal`, `integer`
* `double` ⊥ `decimal`, `integer`
* `integer` / `decimal` — **NOT disjoint** (subset)

**Direction of risk is inverted here.** Emitting more disjointness risks FPs, so a fix
needs canaries first, a Konclude ∪ HermiT adjudication, and a corpus sweep — the curated
corpus is inert for DKey work by `datatype_value_membership.rs`'s own admission, so a green
FP=0 net would show non-regression only.

## Corpus reach — measured, and it explains most of the unsat misses

A census over all 1,920 ORE ontologies for `DataPropertyRange(p, D1)` +
`DataPropertyAssertion(p, _, "v"^^D2)` with `D1`, `D2` in different disjoint families finds
**7 ontologies**. All 7 are inconsistent per Konclude. rustdl misses **3**
(`ore_ont_16321`, `ore_ont_4198`, `ore_ont_5014`), detects 2 by other routes, and 2 time out
in conversion (`ore_ont_4141`, `ore_ont_8445` — the known DKey conversion-bound pair).

`ore_ont_16321` and `ore_ont_4198` are the two ontologies where rustdl reports
`consistent: true` against **both** Konclude and KM
(`missed-inconsistency-ore-16321-4198.md`). Between them they account for **82 of the 89**
corpus-wide missed-unsat classes in the 391-ontology MISSED-net sample — as ONE defect each,
not 41 apiece, since all 41 follow from the KB being inconsistent.

**So this single 2-axiom defect is the largest identified contributor to rustdl's measured
unsat-completeness gap.**

Note the census over-attributes: `ore_ont_6446` carries the shape but its inconsistency comes
from elsewhere (rustdl already detects it, and the isolated `anyURI`/`string` probe is
consistent for both reasoners once the unsupported `anyURI` range is dropped).

## Severity

Sound — rustdl under-reports, never over-reports. But **silent**: a consumer reading
`consistent: true` gets no signal, and every downstream entailment rests on a KB with no
model.


---

## Gates run on the fix (2026-08-30)

| gate | result |
|---|---|
| canaries (`dkey_cross_bucket_disjointness.rs`) | 9/9, incl. 3 FP guards |
| suite | 1856 passed / 0 failed |
| clippy / fmt | clean |
| FP=0 net | 20 VERIFIED, every closure exact and **unchanged** |
| two-arm classify sweep, 637 data-property-bearing ORE | 603 both-ok, **603 identical, 0 DIFFER, 0 `ok→dnf`** |
| two-arm **consistency** sweep, 268 ORE with ≥2 numeric datatypes | **3 changed, all correct; 0 wrong-direction** |
| conversion volume, 3 worst DKey ontologies | **identical both arms** (`ore_ont_5368` 18,620,251 rules / 18,608,050 disjoint pairs) |

The classify sweep shows **non-regression, not benefit** — classify's output is unchanged
precisely because the fix lands on the `consistent` surface (see the residual above). The
consistency sweep is where the benefit shows: the only three verdict changes across 268
ontologies are `ore_ont_16321`, `ore_ont_4198` and `ore_ont_5014`, all going
consistent → inconsistent, and all three confirmed inconsistent by Konclude (the first two
by KM as well).

**The FP=0 net's green is non-regression only** — `datatype_value_membership.rs` states the
curated corpus is inert for DKey work. The positive evidence is the canaries plus the
Konclude ∪ HermiT adjudication, done on each fixture individually including the two
inverted tests.

### Two pre-existing tests asserted the wrong answer

Both were adjudicated on their **exact** fixtures before being touched, because a failing
test is usually the codebase reporting a defect in the change, not the reverse:

* `float_value_vs_double_range_no_cross_bucket_clash` reasoned from the implementation
  ("the float assertion simply drops into the float bucket, which has no range constraint")
  to a semantic conclusion. Konclude `false`; HermiT `owl:Thing is not satisfiable`.
* `forall_cross_datatype_no_clash` described itself as a "sound under-approx" — which it
  was, and this is the fix for that MISS. Konclude `C is UNSATISFIABLE`; HermiT lists `C`
  alongside `owl:Nothing`.

The half of the first test's rationale that stands — cross-bucket **subsumption** must never
be seeded — is untouched by this change and is preserved in the inverted test's docstring.
