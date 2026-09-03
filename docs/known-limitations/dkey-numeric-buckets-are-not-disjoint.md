# The numeric DKey buckets are non-comparable but not DISJOINT — rustdl misses the clash

**Status: FIXED on BOTH surfaces.** `is_consistent` by
`RUSTDL_DKEY_CROSS_BUCKET_DISJOINT` (default ON, `=0` reverts); `classify` by **#97**
(2026-09-03), which consults the wedge consistency route in classify's inconsistency
pre-check. The classify residual this page recorded is CLOSED — re-verified, both
`ore_ont_16321` and `ore_ont_4198` now report `consistent: false` with 40 unsatisfiable
classes from `classify --json`, agreeing with `rustdl consistent`, Konclude and KM.
A DIFFERENT residual remains open — `xsd:integer` vs a non-integral `xsd:decimal`, below.

Was: live on v0.4.24, sound (a MISS, never an FP), root-caused to a 2-axiom reproducer
adjudicated by Konclude AND HermiT with a discriminating control.

## CLOSED by #97: `classify` and `consistent` agreed here from 2026-09-03

**Kept for the mechanism, not as an open item.** Re-measured on the merged binary:
`classify --json` reports `consistent: false` / 40 unsatisfiable on both `ore_ont_16321`
and `ore_ont_4198`, matching `rustdl consistent`. Do not plan against the text below as a
live gap — this page said "classify STILL MISSES IT" after it had been fixed, which is the
pessimistic form of the design-record drift `CLAUDE.md` records as this repo's dominant
failure mode, and the more expensive form: it invites work on a closed problem.

**#97's fix was also smaller than this page's analysis implied.** The paragraph below
correctly identifies why `abox_saturation` cannot reach a data-successor clash — but the
conclusion drawn from it (that closing this needed DKey-aware ABox saturation or a
`decide(Top)` probe) was wrong. The verdict already existed as `consistency: wedge Unsat`
and simply was not consulted from classify. The recorded dead-end for `decide(Top)`
("hangs on consistent alehif/pizza") is about an UNBOUNDED probe; the wedge route is
bounded and measures 2.34 ms on pizza. Checking the recorded dead-end rather than
inheriting it is what made the cheap fix visible.

*Historical description of the mechanism follows.*

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

## Why pairwise seeding, not marker classes — measured

A reviewer built a duplicate fix using per-family MARKER classes (`DKey ⊑ marker`, markers
pairwise disjoint) to get O(#DKeys) instead of pairwise seeding, and hit an
**8.7×–78× slowdown on ontologies with ZERO numeric datatypes** (`ore_ont_1016`). Their
diagnosis was that interning markers shifted class ids corpus-wide. **That is not the
mechanism** — measured on `ore_ont_1016` (3,087 classes, no numeric datatypes at all):

| variant | wall | mode |
|---|---|---|
| baseline | 0.12 s | `pure EL (saturation-only)` |
| +3 classes, prepended (shifts every id) | 0.11 s | `pure EL` |
| +3 classes, appended | 0.11 s | `pure EL` |
| **+3 classes AND 3 `DisjointClasses`** | **1.04 s** | **`hybrid`** |

The id shift costs nothing. **The `DisjointClasses` axioms are what bite**, and the route
is `saturator_complete_fragment`'s `disjoint_ok` gate: disjointness is admitted only when
no functional or inverse-functional role is present, because the disjoint×functional-merge
interaction is unproven. `ore_ont_1016` is galen-derived — 150 `FunctionalObjectProperty`,
0 `DisjointClasses` — i.e. exactly the "functional, no disjoint" profile that gate's own
comment names as staying on the fast path. Injecting any disjointness flips
`has_cardinality_role`-bearing ontologies off saturation-only into the hybrid path.

**The design constraint, for anyone adding disjointness anywhere:** an *unconditional*
`DisjointClasses` injection knocks EVERY functional-role-bearing pure-EL ontology off the
fast path. That population is large (the galen family is well represented in ORE), and the
cost is invisible to canaries — it is a wall change on ontologies the feature does not
touch at all, so only a two-arm sweep sees it.

`seed_cross_bucket_disjoint` is immune **by construction**, not by luck: it emits only
between actual DKeys inside merge-inducing role components, so an ontology with no numeric
DKeys receives nothing and its mode is unchanged. Verified — `ore_ont_1016` still reports
`# mode: pure EL` with this PR applied, and the 637-ontology two-arm classify sweep found
0 differences.

## Two adjacent cases that are NOT gaps (verified 2026-08-31)

Both look like the fix is incomplete. Neither is.

### `xsd:integer` / `xsd:decimal` — a residual no bucket rule can reach

| range | value | Konclude | HermiT | rustdl |
|---|---|---|---|---|
| `xsd:decimal` | `"1"^^xsd:integer` | consistent | consistent | consistent ✓ |
| `xsd:integer` | `"1"^^xsd:decimal` | consistent | consistent | consistent ✓ |
| `xsd:integer` | `"1.5"^^xsd:decimal` | **inconsistent** | **inconsistent** | **consistent** ✗ |

The `1.5` row is a genuine rustdl MISS, and it is **value-dependent, not
datatype-dependent**. `xsd:integer ⊆ xsd:decimal`, so the buckets genuinely overlap and
no disjointness rule can close it — it needs VALUE MEMBERSHIP (`1.5 ∉ integer`), which is
different machinery.

**Do not "complete the matrix" from it.** Seeding `int × dec` to catch `1.5` makes
`"1"^^xsd:integer` in a decimal range a false UNSAT. The canaries
`integer_value_in_decimal_range_is_consistent` and
`decimal_value_in_integer_range_is_consistent` both fail if anyone tries, and the second
one's docstring explains why.

This is also distinct from the classify-surface residual above — that one is about which
*surface* reports the clash; this one is about a clash rustdl cannot derive at all.

### `date` / `dateTime` — no peer supports the entailment

rustdl has a `date:` and a `dt:` bucket, which makes them an inviting-looking pair for
anyone extending `seed_cross_bucket_disjoint`. Measured:

| range | value | Konclude | HermiT |
|---|---|---|---|
| `date` | `dateTime` | consistent | `UnsupportedDatatypeException` |
| `dateTime` | `date` | consistent | `UnsupportedDatatypeException` |
| `date` | `date` | consistent | `UnsupportedDatatypeException` |
| **`dateTime`** | **`dateTime`** | consistent | **is satisfiable** |

The last row is the discriminator: HermiT handles `dateTime` fine, so the exception is
specifically that **`xsd:date` is not in the OWL 2 datatype map** — not an artefact of the
test shape. Konclude reports consistent in both directions. Seeding `date ⊥ dateTime`
would manufacture a false positive no peer supports.

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

**As of #97 that silence is gone for the two corpus instances**; it persists only for the
still-open `xsd:integer` vs non-integral-`xsd:decimal` residual, re-verified 2026-09-03
(rustdl `consistent`, Konclude `false`), which no bucket-disjointness rule can reach
because `integer ⊂ decimal` — it needs value-membership checking.


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
