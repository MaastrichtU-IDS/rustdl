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

Recommendation: build it **only** if the three residuals matter for a real workload, or as a
deliberate correctness exercise. If the goal is DNF-tail progress per unit risk, Bucket B is the
better target — and unlike this lever, its blocker is not yet even diagnosed.

## What this does not claim

- It does not help `ore_ont_5368` (genuine collapse via 15 functional properties) — that was Lever 2's
  only remaining justification, and Lever 2 stays parked.
- It is not validated by the curated corpus; the canaries and the R1–R4 fixtures are the net.
- It does not change `definitely_disjoint`, so D11b's FP surface is untouched.
- It is not proven, only un-refuted by two independent adversarial attempts.
