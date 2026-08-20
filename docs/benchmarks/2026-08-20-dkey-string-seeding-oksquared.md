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


---

## CORPUS SWEEP AND DEFAULT FLIP (2026-08-21): both flags DEFAULT ON

Frame: the **651** data-property-bearing ORE ontologies (grep SUPERSET — it can over-include
but cannot miss an affected input; the flags touch only DKey seeding). Arm A = shipped default
(both off), Arm B = both on. `classify`, single-thread, 60 s cap, binary pinned and verified
against a discriminating input (`ore_ont_10929` 130.5 s vs 3.9 s).

| verdict | n |
|---|---:|
| IDENTICAL | **614** |
| BOTH_DNF (uninformative) | 34 |
| **RECOVERY_DNF_TO_OK** | **3** |
| **REGRESSION_OK_TO_DNF** | **0** |
| **HASH_DIFFER** | **0** |

**Recoveries — real tail reduction, not just wall:**

| ontology | arm A | arm B |
|---|---|---|
| `ore_ont_10929` | DNF at 60 s | **4.89 s** |
| `ore_ont_15635` | DNF at 60 s | **7.62 s** |
| `ore_ont_10517` | DNF at 60 s | 48.30 s |

`ore_ont_10517` was **not** in the 9-member grep frame of the companion document — the sweep found
an affected ontology the shape census missed, which is the argument for sweeping rather than
trusting a census.

Among the **347 resolvable** rows (both arms ≥ 0.10 s, so timer quantisation cannot manufacture a
result — an earlier sweep produced 26 phantom "2.00× wins" that were one 10 ms tick):

* **9 wins ≥1.5×**: `ore_ont_9347` 7.8×, `16853` 4.3×, `1685` 4.0×, `7607` 3.2×, `12182` 3.0×,
  `6892` 1.8×, `16542` 1.7×, `9694` 1.6×, `13700` 1.5×.
* **2 losses**: `ore_ont_3795` 0.24 → 0.48 s, `ore_ont_4263` 0.26 → 0.41 s — i.e. **+0.24 s and
  +0.15 s absolute**, on a small base.
* Aggregate **893.5 s → 877.2 s (+1.8%)**.

**Inertness verified, not assumed:** 25 of 25 sampled non-bearing ontologies (of 1,269) are
byte-identical across both settings.

### Decision: DEFAULT ON

Zero `ok → DNF` and zero answer changes across 614 identical comparisons, against 3 tail
recoveries and 9 wins. **Zero `HASH_DIFFER` is the load-bearing result** — it is the evidence for
the exactness arguments (the group skip elides only provably-droppable pairs; the size index only
provably-impossible subset tests). Either being wrong would have shown up as a differing hierarchy,
and 614 clean comparisons plus 347 resolvable timing rows did not produce one.

At the default `ore_ont_10929` now converts in **3.5 s** against **93.7 s** with `=0`, so the
escape hatch is a genuine revert.

**The DNF tail drops 143 → 140.**


## THE FLIP EXPOSED A FLAW IN THE SKIP'S CORRECTNESS ARGUMENT — found by four canaries

Flipping both defaults ON failed the suite in two waves, and the second wave changed the
diagnosis.

**Wave 1** — `dkey_emit_order.rs`: `unrelated_second_property_must_not_lose_the_clash` and
`merging_gate_off_agrees_with_default_on_the_nnf_fixture`, both **negative controls** asserting the
ordering defect is still observable at `RUSTDL_DKEY_EMIT_ORDER=0`. My first reading was that the
skip incidentally fixes that defect, so the controls needed to pin the skip off. I edited them and
they went green.

**Wave 2** — `dkey_flag_defaults.rs`: `emit_order_default_is_on_and_zero_reverts` and
`tbox_stats_told_counters_track_the_emitted_dkey_disjointness`. **Four independent tests objecting
is not precondition drift — it is the codebase reporting a defect in the change.**

**The real fault.** The skip is equivalent to the per-pair path only because a pair spanning two
components is still enumerated in the OTHER component. That holds under `emit_order`, where
declining merely LOOKS at a pair. With `emit_order` **off**, declining **spends** the pair, so
eliding a group changes which component spends it — and the skip stops being behaviour-preserving,
incidentally repairing the very defect `RUSTDL_DKEY_EMIT_ORDER` exists to fix.

I had written that dependency into the code comment and **failed to enforce it in the condition**.
The fix is one clause — `&& emit_order` — after which all nine canaries pass with the test files
**reverted to their original form**, which is the tell that the fault was mine and not theirs.

**Lesson, sharper than "run the tests":** when a test fails, the first hypothesis should be that
the CODE is wrong, not that the test's assumptions are stale. I reached for the second explanation
first; it was locally plausible, it went green, and it would have shipped a real behaviour change
under `EMIT_ORDER=0` while disabling the guards that detect it.

Post-gating, at the default (where `emit_order` is on): `ore_ont_10929` **95.0 s → 3.1 s (30.6×)**,
`ore_ont_15635` **88.1 s → 5.8 s (15.2×)**, suite **1685/0/78**.
