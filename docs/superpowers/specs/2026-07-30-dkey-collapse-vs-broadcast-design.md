# DKey disjointness: distinguish COLLAPSE from BROADCAST merge sources

**Date:** 2026-07-30
**Status:** Design — the naive version was REFUTED by adversarial review; this is the corrected
design, not yet prototyped
**Flag:** `RUSTDL_DKEY_COLLAPSE_SPLIT`, default **OFF** until the gates below pass, then flip
**Predecessor:** `2026-07-30-dkey-nonmerging-component-gate-design.md` (shipped). This is the same
bug class one level deeper — the third instance.

## Summary

The shipped merging gate asks "does this role component contain a merge-inducing role?" and seeds all
pairs if yes. But the merge sources are not interchangeable:

- **COLLAPSE** (functional / inverse-functional / `≤n`) forces two *distinct* successors onto **one**
  node. Two data **values** can then share a label ⇒ value×value pairs are consumable.
- **BROADCAST** (a `DataPropertyRange` / `∀r.DKey` whose filler is a `DKey`) puts **one** extra key
  onto **every** successor. A value only ever meets the *broadcast key* — never another value.

Three ORE ontologies are merge-inducing **only** via broadcast, so their entire value×value residual
is dead weight: `ore_ont_7607` 5,419,609 axioms, `ore_ont_1685` 5,418,126, `ore_ont_4410` 1,270,101 —
all three currently DNF.

## Measured motivation

| ont | `DataPropertyAssertion` | distinct literals | `FunctionalDataProperty` | `DataPropertyRange` | other collapse sources | after shipped gate | classify |
|---|---|---|---|---|---|---|---|
| `ore_ont_7607` | 18,755 | ~18,027 | **0** | 31 | **none** | 5,419,609 | DNF |
| `ore_ont_1685` | 13,581 | ~12,854 | **0** | 31 | **none** | 5,418,126 | DNF |
| `ore_ont_4410` | 9,343 | — | 3 (minor props) | 25 | **none** | 1,270,101 | DNF |
| `ore_ont_5368` | 6,101 | — | 15 | 14 | none | 18,620,251 (untouched) | DNF |

"other collapse sources" = `DataMaxCardinality`, `DataMinCardinality`, `DataExactCardinality`,
`ObjectMaxCardinality`, `InverseFunctionalDataProperty` — **all zero** on all three.

Arithmetic check: `7607`'s heaviest property has ~3,293 distinct values; C(3293,2) ≈ 5.42M, matching
the residual almost exactly. So one broadcast-only property accounts for essentially the whole figure.

**Both halves are empty on these inputs.** Value×value cannot co-label (nothing collapses). And
value×broadcast is never *emitted*, because every range is bare/unfaceted — `xsd:string` ×33,
`xsd:anyURI` ×2, `xsd:boolean` ×3, `xsd:int` ×1 on `7607`/`1685`; `xsd:string` ×18, `xsd:integer` ×7
on `4410` — so each lowers to a `Top`-like `DKey` that overlaps everything and
`definitely_disjoint` is false. Expected result: `7607`/`1685` → ~0, `4410` → small,
`5368` → unchanged.

`4410` keeps whatever its 3 functional properties legitimately need; those are `haAbstract`,
`haAnnoDiChiusuraPrevisto`, `haPrimoAnnoDiAttivita`, none in its top-5 by assertion count, so the win
should still be large.

## What adversarial review refuted — read this before implementing

Two independent reviewers (semantics lens; code lens) attacked the naive form of this lever. **The
naive form was a per-component early return in `anchor`, and it is wrong.** Their findings are
requirements, not commentary. Each is backed by a fixture that passes today and would break.

### R1 — The drop must be PER-PAIR, not per-component

Broadcast×value and broadcast×broadcast pairs are genuinely consumable:

- two disjoint `DataPropertyRange` on one property ⇒ `∃p.⊤ ⊑ ⊥` (both range keys land on every
  successor, so they meet each other);
- a range vs a conflicting `DataHasValue` ⇒ inconsistent — **this is the flagship D11b clash**;
- `DataPropertyRange(:f DataOneOf("a"))` + `SubDataPropertyOf(:p :f)` + `p(i,"b")` ⇒ inconsistent
  (the broadcast rides down the property hierarchy).

All three pass today under both gate settings and all three die under a per-component drop.
**Requirement: omit a pair only when BOTH of its keys are value-only in that component.**

### R2 — Classify keys by OCCURRENCE POSITION, never by shape or provenance

`DataOneOf("a")` interns to the **same `ClassId`** as the key produced by
`DataPropertyAssertion(p,i,"a")`. So "singleton range ⇒ it's a value key" is wrong, and so is
"came from an assertion ⇒ value". Counterexample that currently works and would regress:

```
SubClassOf(:A DataSomeValuesFrom(:p xsd:string))
SubClassOf(:A DataAllValuesFrom(:p DataOneOf("a")))
SubClassOf(:A DataAllValuesFrom(:p DataOneOf("b")))     ⇒ :A unsatisfiable
```

**Requirement: a key is BROADCAST in a component if it occurs in *any* broadcast position (a range or
`∀` filler) anywhere in that component — a per-key union over all its occurrences. One `ClassId` can
be both VALUE and BROADCAST; if it is broadcast anywhere, it is not "value-only".**

### R3 — Keep ONE union-find gated on the FULL merge set

Only the *value×value drop decision* may be keyed on collapse. If the split is pushed down into step
(d) so that unions are gated on collapse-only supers, the third R1 fixture's component splits into
`{p}`,`{f}` and the value×broadcast pair is lost. **Requirement: leave `m_star` and the union-find
exactly as they are; add the collapse/broadcast distinction as a separate, later decision.**

### R4 — COLLAPSE must be closed DOWNWARD through the role hierarchy

`FunctionalDataProperty(:f)` + `SubDataPropertyOf(:p :f)` + `p(i,"a")` + `p(i,"b")` is inconsistent:
two `p`-successors are also `f`-successors, and `f` functional merges them. A 3-level variant (`g`
functional, `f ⊑ g` broadcast-only, `p ⊑ f` carrying the values) is likewise inconsistent. The
reverse direction is correctly *not* needed (functional sub-role, values on the super).
**Requirement: the collapse set must inherit the same downward closure `m_star` already performs.**

### R5 — A nominal-forcing range/`∀` is a COLLAPSE source, and it is not syntactically detectable

This one found a live bug in the shipped gate and is **already fixed** (`ef41128`): a filler that
forces every successor to be the same individual collapses them via the o-rule, so two distinct value
keys share a label without the filler mentioning a `DKey`. `ObjectPropertyRange(p, ObjectOneOf(o))`
does it; so does `ObjectPropertyRange(p, C)` with `C ⊑ ObjectOneOf(o)`, which no syntactic filler test
can catch. Any range/`∀` is now merge-inducing.

**Consequence for THIS design, and it is the subtlest point here.** Because nominal-forcing is
undecidable syntactically, a range/`∀` may only be classified as *broadcast-only* when its filler is
**provably incapable of collapsing successors**. The sound, decidable, sufficient test:

> the filler consists **exclusively** of `DKey` atomics, combined only by `And` / `Or` / `Not`.

Such a filler contains no nominal and no non-`DKey` class that could be subsumed by one. **Any other
filler contributes COLLAPSE as well** (conservatively), even if it also mentions a `DKey`.

This test is exactly satisfied by the three target ontologies, whose ranges are bare `xsd:*` datatypes
lowering to a single `DKey` atomic — so the conservatism costs nothing where it matters.

### R6 — `unanchored` pairing must be restored (a second latent hole in the shipped gate)

`seed_disjoint_bucket` pairs `global` (unanchored keys) against `anchored`. The shipped gate removes
non-merging keys from `components`, so they never enter `anchored` and fall into the
"neither anchored nor unanchored ⇒ skip entirely" branch. An unanchored key needs **no** collapse
source to reach a label — it is a direct top-level placement — so that pairing is unconditional.

Currently **latent**: no lowering emits a top-level bare `DKey` (`collect_direct_dkeys` stops at
`Some`), so it is reachable only via hand-written `urn:rustdl-dkey:` class IRIs. But it is an
invariant break, not a safe approximation, and this design touches exactly that code.
**Requirement: keys dropped from same-component value×value grouping must still participate in
`global × anchored` pairing.**

## Design

Three additions to `dkey_components` / `seed_disjoint_bucket` in `crates/owl-dl-core/src/convert.rs`.
`m_star`, the union-find, and step (d) are **unchanged** (R3).

**1. A collapse set, parallel to `m_star`, with the same downward closure (R4).**

```rust
// COLLAPSE: forces two DISTINCT successors of r onto ONE node.
//   Functional / InverseFunctional / Max(n, r, _)
//   + any range / ∀ whose filler is NOT provably broadcast-only (R5)
// Closed downward through the role hierarchy, exactly as m_star is.
let mut collapse = vec![false; num_roles];
```

`filler_is_pure_dkey(pool, cid, dkeys)` decides "provably broadcast-only": `Atomic(c)` iff
`dkeys.contains(c)`; `Not`/`And`/`Or` recurse with **all** operands pure; everything else `false`.
(Note this is the *inverse polarity* of the deleted `filler_mentions_dkey`, which asked "any" — the
new test must ask "all". Deleting that function was correct; do not resurrect it.)

**2. Per-key, per-component occurrence classification (R2).** Extend `DkeyComponents`:

```rust
/// Component ids in which this key occurs in a BROADCAST position (a range or ∀
/// filler). A key may be VALUE in one component and BROADCAST in another, and both
/// in the same one — if it is broadcast here, it is not "value-only" here.
broadcast_in: HashMap<ClassId, Vec<usize>>,
```

The step-(e) `anchor` closure already receives the role and filler; it must additionally record
whether this occurrence is a broadcast one. That means distinguishing the call sites: the
`ObjectPropertyRange` loop and the `ConceptExpr::All` arm are broadcast positions;
`Some` / `Min` / `Max` fillers are not.

**3. Per-pair drop at emission (R1).** In `seed_disjoint_bucket`'s per-group loop:

```rust
// Drop iff the component has no COLLAPSE role AND both keys are value-only here.
// Anything involving a broadcast key still gets seeded (R1), and `global × anchored`
// is untouched (R6).
if !collapse_component(c)
    && !is_broadcast_in(a_cid, c)
    && !is_broadcast_in(b_cid, c)
{
    continue;
}
```

Keys must remain in `components` (hence in `anchored`) even when all their pairs are dropped, so R6
holds. This is a change of emission policy, **not** of anchoring — unlike the shipped gate, which
gated `anchor`. That difference is what makes R6 satisfiable here.

## Soundness

**FP-safe by construction, like its predecessor:** the change only ever *removes*
`DisjointClasses` axioms, so it yields fewer clashes, fewer derived `⊥`, and can never produce a
false-positive subsumption. FP=0 is preserved structurally, not empirically.

The exposure is **completeness**, and it rests on exactly one claim:

> In a component with no COLLAPSE role, two keys that are both value-only can never occupy one node
> label.

Adversarial review found no counterexample to this on OWL 2 DL-legal input, having tried role
hierarchies, equivalent and chained properties, inverses, symmetry, transitivity, `ObjectHasSelf`,
nominals, `SameIndividual`, and the o-rule. The one counterexample it did produce required
object/data **punning** (OWL 2 Full) and is closed by R5's conservative filler test plus `ef41128`.

Residual risk is honestly stated: this is an argument plus a failed refutation attempt, not a proof.
The failure mode if wrong is a MISS, never an FP.

## Gates

1. **The R1–R4 fixtures — ALREADY PRESERVED AND VERIFIED.** All 11 are committed at
   `crates/owl-dl-reasoner/tests/fixtures/dkey_collapse_broadcast/` with a README recording each
   one's **measured** verdict on `ef41128` and which requirement it guards. Nine must keep their
   verdict, one is a negative control that must stay satisfiable, and one pins a pre-existing MISS so
   it is not mistaken for a regression. Note the README's warning about the two distinct questions:
   "class unsat" needs `classify --json` + the `unsatisfiable` list, while "inconsistent" needs
   `consistent` — conflating them produced a false alarm during this spec's own preparation. Wiring
   them into a Rust integration test is still to do.
2. **Non-vacuity by sabotage.** Force the drop unconditionally (ignore collapse and broadcast) and
   confirm the R1 fixtures FAIL. If they pass, they are not guarding the emission policy. This is
   mandatory — an earlier gate in this area passed while guarding nothing.
3. **The existing three canaries** (`forall_value_outside_range_clashes`,
   `forall_float_value_outside_clashes`, `forall_string_value_outside_enum_clashes`) plus
   `dkey_nominal_range_merge` must pass.
4. **FP=0 net** — 22/0, closures galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653,
   pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16. Understood to show
   **inertness**: the curated corpus contains no consumed DKey disjointness at all.
5. **Flag-OFF byte-identity** on wine/family/pizza/ro/alehif-test versus pre-change `main`.
6. **Recovery, from PINNED binaries.** `7607`, `1685`, `4410` (expect large drops) and `5368`
   (expect *unchanged* — the negative control). Report concept_rules, wall and RSS individually, and
   whether any now classifies.
   **Pin each binary immediately after its build and verify the pin against a discriminating input.**
   `ore_ont_9347` cannot discriminate here — it has zero ranges — so use `5368` (must stay
   18,620,251) and `7607`.
7. **Population re-scan** before/after on pinned binaries, to check the change is not silently
   dropping pairs on ontologies that do have collapse sources.

## Scope

**In.** The collapse set + downward closure, the per-key broadcast classification, the per-pair
emission rule, R6's `unanchored` restoration, the flag, and the gates.

**Out.** `m_star`, the union-find, and step (d) (R3 forbids touching them). `definitely_disjoint` and
the range algebra. The `Vocabulary` object/data punning issue — worth its own decision (reject as
OWL 2 Full, or keep accepting), noted at R5.

**Separate pre-existing bug, do not fold in.** `∀f.DataOneOf("a")` with `p ⊑ f` and a conflicting
value on `p` is MISSED at *both* gate settings, while the `ObjectPropertyRange` form works —
asymmetric `∀`-propagation down the data-property hierarchy. Found incidentally by review; not
attributable to any gate; deserves its own ticket.

## MEASURED POPULATION (2026-07-30) — supersedes the "3 ontologies" estimate

**The earlier "addressable set is 3 ontologies" figure was wrong by ~95×.** It selected on
*residual ≥1M concept_rules*, which is a threshold, not the binding predicate — an ontology dropping
200k pairs benefits identically and could never appear in such a list. Measured properly with
report-only instrumentation (`RUSTDL_DKEY_SPLIT_STATS=1`, commit `c1f915f`, which counts what the
split *would* drop and is verified byte-identical in emission):

| | count |
|---|---|
| scanned | **1,915** of 1,920 (5 exceed even a 300 s budget) |
| seed any `DKey` disjointness pairs | 356 |
| **would benefit (drop > 0)** | **286** |
| …of those, 100% droppable | 230 |
| seed pairs but drop NONE (correct — they have collapse sources) | 70 |
| corpus-wide pairs | 197,832,545 total → **18,854,659 droppable (9.53%)** |

> **CORRECTED — an earlier figure of "67,122,352 total → 28.1% droppable" was a SELECTION
> ARTIFACT and is retracted.** It came from the 30 s first pass, which timed out on the 17
> largest ontologies and therefore excluded them. The two biggest `DKey` populations in the
> entire corpus turn out to be almost entirely NON-droppable — `ore_ont_2504` 68,672,720 pairs
> → **98** dropped (0.0001%), `ore_ont_4141` 42,723,215 → **6** — because they have genuine
> collapse sources. Including them nearly triples the denominator and cuts the corpus-wide
> share from 28.1% to **9.53%**. The lesson is the standard one in the wrong direction: a
> per-item timeout is not a neutral sampler, it selects *against* exactly the largest items,
> and here the largest items were the ones that would have deflated the headline. Any future
> corpus-share claim in this area must state its timeout and its exclusions.

**Benefit is heavily skewed, and this is the number that should drive the decision:**

| magnitude | ontologies |
|---|---|
| drop ≥ 1M | 4 |
| drop 100k–1M | 11 |
| drop 10k–100k | 24 |
| drop < 10k | 245 |

So 286 benefit *at all*, but 86% of those drop under 10k pairs, which will not change whether they
complete. The decision-relevant sets are **39** (≥10k) and **15** (≥100k) — still 5–13× the retired
3-ontology estimate, and 282 of the 286 lie below the ≥1M line that estimate could not see.

**The per-ontology counts are robust to the correction above; only the corpus-share is not.** The two
giants move from "unmeasured" into "seed pairs, drop none", which is where they belong — they are
evidence the classification's negative side works at extreme scale (68.7M pairs, 98 droppable), not
evidence against the lever.

Top beneficiaries: `7607` 5,410,094 (100%), `1685` 5,409,365 (100%), `12182` 2,051,471 (100%),
`4410` 1,081,138 of 1,266,274 (85%), `7345` 714,740 (100%), `8989` 525,099 of 1,088,126,
`15288` 373,565, `13052` 361,896, `9899`/`6132` 318,001 each, `5548` 291,029, `443` 281,919.

### It touches the DNF tail — including Bucket B

Cross-referenced against the 14 known DNFs:

| DNF | droppable | note |
|---|---|---|
| `7607`, `1685` | 100% | volume-bound, the original targets |
| `4410` | 85% | volume-bound |
| **`5548`** | **55%** (291,029 / 530,605) | **Bucket B — label-cache-build-bound** |
| 9 other search-bound DNFs | `total = 0` | no `DKey` pairs at all; this lever provably cannot touch them |

`5548` is the material surprise. Bucket B was characterised as cost *outside* the per-pair loop and
recovered 0 of 5 across five weeks of matcher/search work — and it was the alternative recommended
*over* this lever. If its label-cache build is slow partly because it carries 530k disjointness
axioms, this is the first mechanism-level lead on that bucket. **Not** a promise: 55% of its pairs
going away may still leave it DNF. But it is testable, and cheaply, once the lever exists.

That the 9 remaining search-bound DNFs show `total = 0` is itself a useful negative: it confirms they
are not data-driven, so no amount of DKey work will help them.

### Second pass on the 17 timeouts (partial)

`11287` 198,313 total → **0** droppable, `14351` 495,077 → **0** — both have genuine collapse
sources, confirming the classification's negative side at scale. `10689`, `14459` seed no pairs.
**Completed.** The decisive rows: `2504` 68,672,720 → 98, `4141` 42,723,215 → 6, `5368`
18,608,050 → 0, `14351` 495,077 → 0, `11287` 198,313 → 0, `20` 12,791 → 0, `5753` 27 → 0; `10689`,
`14459`, `8486`, `868`, `9674` seed none. Five (`10860`, `10929`, `15635`, `4572`, `8445`) exceed
300 s and remain unmeasured — 0.26% of the pool, and they cannot change the ontology counts by more
than 5.

This pass did change one headline (the corpus share, see the correction box) and it strengthened the
classification: at 68.7M and 42.7M pairs, the two largest cases in the corpus are correctly identified
as non-droppable.

## Cost/benefit — read before building

The addressable set is **3 ontologies** (`7607`, `1685`, `4410`), all currently DNF. `5368` is
explicitly not helped. Against that: six side conditions, four of which are load-bearing enough that
getting one wrong silently loses a working clash — including the D11b flagship. This is a materially
larger and subtler change than its ~8-line predecessor.

Two honest alternatives to weigh first:

- **Bucket B of the DNF tail** (`5438 5548 7499 7712 10080`) recovered **0 of 5** across five weeks of
  matcher and search work. It is the least-understood bucket and 5 ontologies, i.e. a larger
  population than this lever's 3.
- **Do nothing.** These 3 stay DNF. They are ORE corpus entries, not user-facing workloads.

**RECOMMENDATION REVERSED by the measurement above (2026-07-30).** The earlier text read: "build it
only if the three residuals matter for a real workload … Bucket B is the better target." That rested
on the retired 3-ontology figure. With 284 beneficiaries (39 at ≥10k, 15 at ≥100k), 28.1% of all
corpus `DKey` pairs droppable, and 4 of 14 known DNFs affected **including a Bucket B case**, the
recommendation is now: **build it.** The side conditions are unchanged and still the hard part — but
the payoff is an order of magnitude larger than when they were judged not worth paying.

Note the two alternatives are no longer exclusive: `5548` means this lever *is* partly a Bucket B
probe. Cheapest sequencing is therefore to build the lever and use `5548` to test whether Bucket B's
label-cache cost is partly axiom-volume, which is information no amount of further profiling has
produced.

## What this does not claim

- It does not help `ore_ont_5368` (genuine collapse via 15 functional properties) — that was Lever 2's
  only remaining justification, and Lever 2 stays parked.
- It does not help the 9 search-bound DNFs that seed no `DKey` pairs at all (`total = 0` measured):
  `5964 6485 8273 8666 13545 5438 7499 7712 10080`. They are not data-driven.
- It is not validated by the curated corpus; the canaries and the R1–R4 fixtures are the net.
- It does not change `definitely_disjoint`, so D11b's FP surface is untouched.
- It is not proven, only un-refuted by two independent adversarial attempts.
