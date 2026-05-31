# Phase 2b.0 — per-pair GALEN MISSED axiom analysis

Task 4 of the Phase 2b.0 diagnosis plan. For each of the 8 stratified MISSED pairs
(see `docs/phase2b-galen-sample.md`), this doc walks the axioms of the minimal
module to identify what derivation HermiT uses for the **first hop** of the
transitive chain — i.e., the actual rule that rustdl's saturator (or wedge) is
missing. The final pair-level subsumption is downstream of that first hop; fixing
the first hop generally closes the whole chain.

## Critical finding from Task 3 that reshapes this analysis

All 8 HermiT-derivable pairs are **transitive**, not direct. Example:
`FemoralHead ⊑ ExactlyPairedBodyStructure` is found by HermiT via
`FemoralHead ⊑ MirrorImagedBodyStructure ⊑ ExactlyPairedBodyStructure`. The
missing-rule diagnosis therefore targets the first hop, not the literal
final-pair subsumption.

## Headline finding (contradicts spec hypothesis)

The Phase 2b design spec, the Phase 2b.0 plan, and the 2026-05-30 handoff all name
**`≥n + disjointness`** as the candidate rule for Cluster A
(`*PairedBodyStructure / MirrorImagedBodyStructure`). The axioms here tell a
**different story**: across all 8 modules, the cardinality and disjointness
axiom counts are **zero** (grep verified — see cross-cutting summary). The
`≥n + disjointness` derivation is mechanically impossible on these inputs;
whatever rule HermiT uses must live in the EL+ / functional-role-merge / sub-
property / transitive / GCI fragment. The actual missing pattern across all 8 pairs is
**EL+ with compound LHS GCIs whose existential fillers are themselves
intersections containing existentials** — i.e., compound existential-body
lowering (LHS conjunction with nested existential operand) and/or its empirical interaction with CR9 + Tseitin. This is calculus
shape that the rustdl saturator **already documents** support for
(`crates/owl-dl-saturation/src/lib.rs:1263-1331` and `1502+`), so the most
likely root cause is an **implementation gap** in the existing lowering path,
not a missing calculus rule.

Pairs 06–07 do introduce an additional pattern (functional-role +
disjointness + covering) that aligns with the handoff's "functional-role
sibling collapse" lever. But cluster A is **not** the `≥n+disjointness`
hypothesis.

This finding should be re-verified by a saturator developer before being acted
on in Phase 2b proper; this Task 4 is axiom-level analysis, not a closed
investigation.

---

## Pair 01: FemoralHead ⊑ ExactlyPairedBodyStructure

**Full IRIs:** `http://example.org/factkb#FemoralHead` ⊑ `http://example.org/factkb#ExactlyPairedBodyStructure`
**Cluster:** A
**Module size:** 1111 lines (.ofn line count)
**rustdl --saturation-only:** MISS (confirmed via `rustdl subclass --saturation-only`: `no`)
**rustdl default:** MISS (per phase2a sweep)
**HermiT on module:** FOUND (path: `FemoralHead -> MirrorImagedBodyStructure -> ExactlyPairedBodyStructure`)

### Relevant axioms (first hop FemoralHead -> MirrorImagedBodyStructure)

```
EquivalentClasses(:FemoralHead
  ObjectIntersectionOf(:BonyHead ObjectSomeValuesFrom(:isSpecificSolidDivisionOf :Femur)))

EquivalentClasses(:MirrorImagedBodyStructure
  ObjectIntersectionOf(:BodyStructure ObjectSomeValuesFrom(:isPairedOrUnpaired :mirrorImaged)))

SubClassOf(:BonyHead :NAMEDSolidBoneDivisions)
SubClassOf(:NAMEDSolidBoneDivisions :NAMEDBoneDivisions)
SubClassOf(:NAMEDBoneDivisions :NAMEDInternalBodySubPart)
SubClassOf(:NAMEDInternalBodySubPart :BodyPart)
SubClassOf(:BodyPart :BodyStructure)
SubClassOf(:BodyPart ObjectSomeValuesFrom(:hasIntrinsicAbnormalityStatus :normal))

SubClassOf(:Femur :LongBone)
SubClassOf(:LongBone :Bone)
SubClassOf(:Bone :SkeletalStructure)
SubClassOf(:SkeletalStructure :NAMEDInternalBodyPart)
SubClassOf(:NAMEDInternalBodyPart :BodyPart)
SubClassOf(:LongBone ObjectSomeValuesFrom(:isPairedOrUnpaired :mirrorImaged))

SubObjectPropertyOf(:isSpecificSolidDivisionOf :isSolidDivisionOf)

# The "load-bearing" complex LHS GCI:
SubClassOf(
  ObjectIntersectionOf(
    :BodyStructure
    ObjectSomeValuesFrom(:hasIntrinsicAbnormalityStatus :normal)
    ObjectSomeValuesFrom(:isSolidDivisionOf
      ObjectIntersectionOf(
        :BodyStructure
        ObjectSomeValuesFrom(:hasIntrinsicAbnormalityStatus :normal)
        ObjectSomeValuesFrom(:isPairedOrUnpaired :mirrorImaged))))
  ObjectSomeValuesFrom(:isPairedOrUnpaired :mirrorImaged))
```

### Missing derivation step

HermiT's derivation:

1. From `FemoralHead ≡ BonyHead ⊓ ∃isSpecificSolidDivisionOf.Femur`, EL conjunction
   distribution gives `FemoralHead ⊑ BonyHead` and `FemoralHead ⊑ ∃isSpecificSolidDivisionOf.Femur`.
2. Up the named-class chain, `BonyHead ⊑ BodyStructure` (via `NAMEDSolidBoneDivisions
   → ... → BodyPart → BodyStructure`) and `BonyHead ⊑ ∃hasIntrinsicAbnormalityStatus.normal`
   (via `BodyPart ⊑ ∃hasIntrinsicAbnormalityStatus.normal`).
3. CR9 (sub-role) lifts the existential to `FemoralHead ⊑ ∃isSolidDivisionOf.Femur`.
4. The witness `Femur` itself is shown (by the same chain) to be `BodyStructure ⊓
   ∃hasIntrinsicAbnormalityStatus.normal ⊓ ∃isPairedOrUnpaired.mirrorImaged`
   (the `mirrorImaged` conjunct comes from `Femur ⊑ LongBone ⊑ ∃isPairedOrUnpaired.mirrorImaged`).
5. The complex LHS GCI now fires with `FemoralHead` matched on the outer
   `BodyStructure ⊓ normal ⊓ ∃isSolidDivisionOf.(inner)` and `Femur` matched on
   the inner triple: concludes `FemoralHead ⊑ ∃isPairedOrUnpaired.mirrorImaged`.
6. With `FemoralHead ⊑ BodyStructure` already in hand, the `MirrorImagedBodyStructure`
   definition fires by conjunctive trigger.

The missing rule for rustdl is therefore the ability to fire the complex LHS GCI
shape `A ⊓ ∃R₁.B₁ ⊓ ∃R₂.(C ⊓ ∃R₃.B₂ ⊓ ∃R₄.B₃) ⊑ ∃R₅.B₄`. The saturator does
have machinery for this (`atomic_classes_with_existential_markers`,
`atomic_or_tseitin_body`, the `by_existential` marker cache) — so the gap is
likely an **implementation incompleteness** in how compound-LHS-existential
bodies with nested existentials are lowered, **not** a missing calculus rule.

### Candidate Phase 2b rule shape

Either fix the existing compound existential-body lowering (LHS conjunction with nested existential operand) (most likely the
right move, given the calculus is supposedly already in scope), or — if the
existing lowering is correct — add a specifically-targeted CR-rule for the
`A ⊓ ∃R.(B ⊓ ∃S.C) ⊑ D` shape that GALEN's "if a body part's solid-division-of
is paired, then the body part is paired" pattern needs.

---

## Pair 02: HeadOfHumerus ⊑ MirrorImagedBodyStructure

**Full IRIs:** `http://example.org/factkb#HeadOfHumerus` ⊑ `http://example.org/factkb#MirrorImagedBodyStructure`
**Cluster:** A
**Module size:** 957 lines
**rustdl --saturation-only:** MISS (confirmed: `no`)
**rustdl default:** MISS; `rustdl explain` reports `yes — answered by tableau`,
i.e. the tableau finds it but the wedge with `trust_sat=true` accepts the
wedge's `Sat` verdict and never asks the tableau. Same `trust_sat` failure mode
the handoff describes for the GALEN 109.
**HermiT on module:** FOUND (path: `HeadOfHumerus -> MirrorImagedBodyStructure`, 1 hop)

### Relevant axioms (single direct hop)

```
EquivalentClasses(:HeadOfHumerus
  ObjectIntersectionOf(:BonyHead ObjectSomeValuesFrom(:isSpecificSolidDivisionOf :Humerus)))

# Same chain as pair 01:
SubClassOf(:Humerus :LongBone)
SubClassOf(:LongBone ObjectSomeValuesFrom(:isPairedOrUnpaired :mirrorImaged))
SubClassOf(:BonyHead :NAMEDSolidBoneDivisions)  # → ... → BodyPart → BodyStructure, normal
SubObjectPropertyOf(:isSpecificSolidDivisionOf :isSolidDivisionOf)

# Same complex LHS GCI as pair 01 (verbatim):
SubClassOf(
  ObjectIntersectionOf(:BodyStructure ∃hasIntrinsicAbnormalityStatus.normal
    ∃isSolidDivisionOf.(BodyStructure ⊓ ∃hasIntrinsicAbnormalityStatus.normal ⊓ ∃isPairedOrUnpaired.mirrorImaged))
  ∃isPairedOrUnpaired.mirrorImaged)
```

### Missing derivation step

Same as pair 01 — the complex LHS GCI fires on
`(HeadOfHumerus, Humerus)` instead of `(FemoralHead, Femur)`. Pair 02 is
structurally identical to pair 01, just with the chain truncated (only one hop
because `MirrorImagedBodyStructure ⊑ ExactlyPairedBodyStructure` isn't needed).

Empirical confirmation that this is implementation-level: the saturator does
derive `LongBone ⊑ MirrorImagedBodyStructure` and `Humerus ⊑ LongBone`
(verified via `rustdl classify --saturation-only` on pair_02), so the closure
machinery for the `LongBone` case works; only the `BonyHead ⊓
∃isSpecificSolidDivisionOf.LongBone` shape fails to lift.

### Candidate Phase 2b rule shape

Same as pair 01.

---

## Pair 03: MeniscusOfKneeJoint ⊑ ExactlyPairedBodyStructure

**Full IRIs:** `http://example.org/factkb#MeniscusOfKneeJoint` ⊑ `http://example.org/factkb#ExactlyPairedBodyStructure`
**Cluster:** A
**Module size:** 1720 lines (shared with pairs 04, 05, 08 per Task 3)
**rustdl --saturation-only:** MISS
**rustdl default:** MISS
**HermiT on module:** FOUND (path: `MeniscusOfKneeJoint -> MirrorImagedBodyStructure -> ExactlyPairedBodyStructure`)

### Relevant axioms (first hop MeniscusOfKneeJoint -> MirrorImagedBodyStructure)

```
EquivalentClasses(:MeniscusOfKneeJoint
  ObjectIntersectionOf(:Meniscus ObjectSomeValuesFrom(:isSpecificStructuralComponentOf :KneeJoint)))

SubClassOf(:MeniscusOfKneeJoint ObjectSomeValuesFrom(:isPairedOrUnpaired :atLeastPaired))

EquivalentClasses(:MirrorImagedBodyStructure
  ObjectIntersectionOf(:BodyStructure ObjectSomeValuesFrom(:isPairedOrUnpaired :mirrorImaged)))

SubClassOf(:Meniscus :GenericInternalStructure)  # → ... → BodyStructure / BodyPart

# Analogous complex LHS GCI (here keyed on isStructuralComponentOf, not isSolidDivisionOf):
SubClassOf(
  ObjectIntersectionOf(
    :BodyPart
    ObjectSomeValuesFrom(:isStructuralComponentOf
      ObjectIntersectionOf(:BodyPart ObjectSomeValuesFrom(:isPairedOrUnpaired :mirrorImaged))))
  ObjectSomeValuesFrom(:isPairedOrUnpaired :mirrorImaged))

# (Note: MeniscusOfKneeJoint has explicit ∃isPairedOrUnpaired.atLeastPaired, but
# atLeastPaired ⊑ leftRightPaired, NOT atLeastPaired ⊑ mirrorImaged — so the
# explicit told axiom alone doesn't give MirrorImagedBodyStructure. The complex
# LHS GCI above is what HermiT uses.)
```

### Missing derivation step

Same compound existential-body lowering (LHS conjunction with nested existential operand) pattern as pairs 01–02, just with a different
role (`isStructuralComponentOf` instead of `isSolidDivisionOf`) and slightly
different conjunction shape (one nested existential in the filler, not two).
HermiT derives: `MeniscusOfKneeJoint ⊑ Meniscus ⊑ ... ⊑ BodyPart`, lifts
`∃isSpecificStructuralComponentOf.KneeJoint` to `∃isStructuralComponentOf.KneeJoint`
via sub-role, shows `KneeJoint ⊑ BodyPart` and `KneeJoint ⊑ ∃isPairedOrUnpaired.mirrorImaged`
(the latter likely via a similar GCI keyed on KneeJoint's own structure), then
fires the boxed GCI to conclude `MeniscusOfKneeJoint ⊑ ∃isPairedOrUnpaired.mirrorImaged`.

### Candidate Phase 2b rule shape

Same as pair 01.

---

## Pair 04: KneeJointRecessus ⊑ HollowStructure

**Full IRIs:** `http://example.org/factkb#KneeJointRecessus` ⊑ `http://example.org/factkb#HollowStructure`
**Cluster:** B
**Module size:** 1720 lines (shared with 03, 05, 08)
**rustdl --saturation-only:** MISS
**rustdl default:** MISS
**HermiT on module:** FOUND (path: `KneeJointRecessus -> Recess -> ActuallyHollowBodyStructure -> ActuallyHollowStructure -> TruelyHollowStructure -> HollowStructure`)

### Relevant axioms (first hop KneeJointRecessus -> Recess)

```
EquivalentClasses(:KneeJointRecessus
  ObjectIntersectionOf(:InternalRegion ObjectSomeValuesFrom(:isBlindPouchDivisionOf :KneeJointCavity)))

EquivalentClasses(:Recess
  ObjectIntersectionOf(:InternalRegion ObjectSomeValuesFrom(:isBlindPouchDivisionOf :ActualCavity)))

# So we need :KneeJointCavity ⊑ :ActualCavity.

EquivalentClasses(:KneeJointCavity
  ObjectIntersectionOf(:BodyCavity ObjectSomeValuesFrom(:isSpaceDefinedBy :KneeJoint)))

EquivalentClasses(:ActualCavity
  ObjectIntersectionOf(:BodyCavity
    ObjectSomeValuesFrom(:isSpaceDefinedBy
      ObjectIntersectionOf(:BodyStructure
        ObjectSomeValuesFrom(:hasTopology
          ObjectIntersectionOf(:Topology ObjectSomeValuesFrom(:hasState :actuallyHollow)))))))

SubClassOf(:KneeJoint :LimbJoint)   # → :Joint → :SkeletalStructure → ... → :BodyStructure
SubClassOf(:SynovialJoint
  ObjectSomeValuesFrom(:hasTopology
    ObjectIntersectionOf(:Topology ObjectSomeValuesFrom(:hasState :actuallyHollow))))
```

(For the downstream hops: `actuallyHollow ⊑ trulyHollow ⊑ hollow` told
subsumptions on the state classes, plus the `*HollowStructure` /
`*HollowBodyStructure` equivalents fan out — pure EL.)

### Missing derivation step

`KneeJointRecessus ⊑ Recess` requires showing `KneeJointCavity ⊑ ActualCavity`,
which decomposes (both being EquivalentClasses with the same outer
`BodyCavity ⊓ ∃isSpaceDefinedBy.X` pattern) into showing
`KneeJoint ⊑ BodyStructure ⊓ ∃hasTopology.(Topology ⊓ ∃hasState.actuallyHollow)`.
The body-structure part is the named-class chain; the topology part requires
`KneeJoint ⊑ SynovialJoint` (which it is, transitively) and then CR5
propagation on the synovial-joint topology axiom into a witness whose
subsumers include the inner `Topology ⊓ ∃hasState.actuallyHollow` synthetic.

This is again **compound RHS existential body + the witness needs to be
classified into a compound synthetic**. Same calculus shape as pairs 01–03,
applied to `hasTopology` instead of `isPairedOrUnpaired`.

### Candidate Phase 2b rule shape

Same as pair 01 — the implementation fix on the compound existential-body
lowering (LHS conjunction with nested existential operand) should subsume this case.

---

## Pair 05: SupraPatellarPouch ⊑ ActuallyHollowBodyStructure

**Full IRIs:** `http://example.org/factkb#SupraPatellarPouch` ⊑ `http://example.org/factkb#ActuallyHollowBodyStructure`
**Cluster:** B
**Module size:** 1720 lines (shared with 03, 04, 08)
**rustdl --saturation-only:** MISS
**rustdl default:** MISS
**HermiT on module:** FOUND (path: `SupraPatellarPouch -> KneeJointRecessus -> Recess -> ActuallyHollowBodyStructure`)

### Relevant axioms (first hop SupraPatellarPouch -> KneeJointRecessus)

```
EquivalentClasses(:SupraPatellarPouch
  ObjectIntersectionOf(:KneeJointRecessus
    ObjectSomeValuesFrom(:hasSuperiorInferiorPosition
      ObjectIntersectionOf(:SuperiorInferiorPosition
        ObjectSomeValuesFrom(:hasChangeInState :superiorly)
        ObjectSomeValuesFrom(:hasFrameOfReference :Patella)))))
```

### Missing derivation step

The first hop is **trivial EL conjunction-elimination**:
`SupraPatellarPouch ≡ KneeJointRecessus ⊓ X` immediately gives
`SupraPatellarPouch ⊑ KneeJointRecessus`. So pair 05 has no first-hop calculus
gap. The actual blocker is **all the downstream hops** —
`KneeJointRecessus ⊑ Recess ⊑ ActuallyHollowBodyStructure` — which is exactly
what pair 04 diagnoses. Fixing the gap identified for pair 04 closes pair 05.

### Candidate Phase 2b rule shape

Same as pair 04 (no incremental rule needed beyond what 04 already calls out).

---

## Pair 06: CongestiveCardiacFailure ⊑ IntrinsicallyPathologicalBodyProcess

**Full IRIs:** `http://example.org/factkb#CongestiveCardiacFailure` ⊑ `http://example.org/factkb#IntrinsicallyPathologicalBodyProcess`
**Cluster:** C
**Module size:** 1250 lines
**rustdl --saturation-only:** MISS
**rustdl default:** MISS
**HermiT on module:** FOUND (path: `CongestiveCardiacFailure -> IneffectiveCardiacFunction -> IntrinsicallyPathologicalBodyProcess`)

### Relevant axioms (first hop is trivial EL; the calculus gap lives in hop 2)

The first hop `CongestiveCardiacFailure -> IneffectiveCardiacFunction` is
trivial EL conjunction-elimination from
`EquivalentClasses(:CongestiveCardiacFailure
ObjectIntersectionOf(:IneffectiveCardiacFunction ∃hasConsequence.RaisedVenousPressure))`.
The real diagnostic load is the second hop
`IneffectiveCardiacFunction -> IntrinsicallyPathologicalBodyProcess`:

```
EquivalentClasses(:IneffectiveCardiacFunction
  ObjectIntersectionOf(:CardiacFunction
    ObjectSomeValuesFrom(:hasEffectiveness
      ObjectIntersectionOf(:Effectiveness ObjectSomeValuesFrom(:hasState :ineffective)))))

SubClassOf(:CardiacFunction :NAMEDCirculatoryProcess)   # → ... → :BodyProcess

EquivalentClasses(:IntrinsicallyPathologicalBodyProcess
  ObjectIntersectionOf(:BodyProcess
    ObjectSomeValuesFrom(:hasIntrinsicPathologicalStatus :pathological)))

# Key axioms — functional roles all under :StatusAttribute (a functional super-property):
FunctionalObjectProperty(:StatusAttribute)
FunctionalObjectProperty(:hasIntrinsicPathologicalStatus)
FunctionalObjectProperty(:hasPathologicalStatus)
SubObjectPropertyOf(:hasIntrinsicPathologicalStatus :StatusAttribute)
SubObjectPropertyOf(:hasPathologicalStatus :StatusAttribute)
SubClassOf(:pathological :PathologicalOrPhysiologicalStatus)

# A relevant GCI — but it requires ALREADY having ∃hasIntrinsicPathologicalStatus.physiological:
SubClassOf(
  ObjectIntersectionOf(:BodyProcess
    ObjectSomeValuesFrom(:hasEffectiveness
      ObjectIntersectionOf(:Effectiveness ObjectSomeValuesFrom(:hasState :ineffective)))
    ObjectSomeValuesFrom(:hasIntrinsicPathologicalStatus :physiological))
  ObjectSomeValuesFrom(:hasPathologicalStatus :pathological))
```

### Missing derivation step

HermiT's derivation for `IneffectiveCardiacFunction ⊑
∃hasIntrinsicPathologicalStatus.pathological` is **non-Horn**. There is no told
EL chain from `∃hasEffectiveness....ineffective` to `∃hasIntrinsicPathologicalStatus.pathological`.
HermiT does it via tableau-style negation + functional-role sibling collapse:

- `hasIntrinsicPathologicalStatus` and `hasPathologicalStatus` are both sub-properties
  of the **functional** `StatusAttribute`. Functionality of the super-property
  forces a single witness for both sub-property fillers, so any class with
  `∃hasIntrinsicPathologicalStatus.A ⊓ ∃hasPathologicalStatus.B` has
  `∃StatusAttribute.(A ⊓ B)`.
- The boxed GCI above gives `IneffectiveCardiacFunction ⊑ ∃hasPathologicalStatus.pathological`
  conditional on `∃hasIntrinsicPathologicalStatus.physiological` being assumed.
- HermiT enumerates the case split on intrinsic-pathological-status. The
  `physiological` branch contradicts (via functional-role merge and
  `physiological`/`pathological` covering or disjointness for
  `PathologicalOrPhysiologicalStatus`), forcing the `pathological` branch.

This is **exactly** the "functional-role sibling collapse via `StatusAttribute`"
pattern named in `docs/handoff-2026-05-30.md` §2 (`PathologicalCondition` trace).
It requires:

1. Functional-role inference (merge witnesses of `R_i, R_j ⊑ R_f` when `R_f`
   functional).
2. Negation / case-splitting (HermiT-style tableau or hypertableau disjunctive
   branching with the right disjointness + covering axioms made visible to the
   wedge).
3. Or: an EL+ approximation that materialises the consequence `∃hasIntrinsicPathologicalStatus.pathological`
   directly when the conditions of the boxed GCI fire and the `physiological`
   alternative is provably-disjoint.

### Candidate Phase 2b rule shape

Functional-role witness-merge for sibling sub-properties of a functional super-property
(`R_i, R_j ⊑ R_f`, `R_f` functional ⇒ shared witness), combined with disjointness
propagation through the merged witness. This is open-lever #2 from the handoff.

---

## Pair 07: AcuteGastricUlcer ⊑ DigestiveSystemPathology

**Full IRIs:** `http://example.org/factkb#AcuteGastricUlcer` ⊑ `http://example.org/factkb#DigestiveSystemPathology`
**Cluster:** D
**Module size:** 1071 lines
**rustdl --saturation-only:** MISS
**rustdl default:** MISS
**HermiT on module:** FOUND (path: `AcuteGastricUlcer -> UlcerOfStomach -> GastricPathology -> DigestiveSystemPathology`)

### Relevant axioms

```
EquivalentClasses(:AcuteGastricUlcer
  ObjectIntersectionOf(:UlcerOfStomach
    ObjectSomeValuesFrom(:hasChronicity
      ObjectIntersectionOf(:Chronicity ObjectSomeValuesFrom(:hasState :acute)))))

EquivalentClasses(:UlcerOfStomach
  ObjectIntersectionOf(:Ulcer ObjectSomeValuesFrom(:hasSpecificLocation :Stomach)))

EquivalentClasses(:GastricPathology
  ObjectIntersectionOf(:PathologicalCondition ObjectSomeValuesFrom(:LocativeAttribute :Stomach)))

EquivalentClasses(:DigestiveSystemPathology
  ObjectIntersectionOf(:PathologicalCondition ObjectSomeValuesFrom(:LocativeAttribute :NAMEDGITractBodyPart)))

EquivalentClasses(:PathologicalCondition
  ObjectIntersectionOf(:DomainCategory ObjectSomeValuesFrom(:hasPathologicalStatus :pathological)))

SubClassOf(:Ulcer :UlcerOrErosion)
SubClassOf(:UlcerOrErosion :InflammatoryLesion)
SubClassOf(:Ulcer ObjectSomeValuesFrom(:isOutcomeOf :UlcerationProcess))

SubObjectPropertyOf(:hasSpecificLocation :hasLocation)
SubObjectPropertyOf(:hasLocation :AnatomicalLocativeAttribute)
SubObjectPropertyOf(:AnatomicalLocativeAttribute :LocativeAttribute)
TransitiveObjectProperty(:LocativeAttribute)

SubClassOf(:Stomach :NAMEDGITractBodyPart)
```

### Missing derivation step

The third hop `GastricPathology -> DigestiveSystemPathology` is **pure EL**:
both are `PathologicalCondition ⊓ ∃LocativeAttribute.X`; the only difference is
`Stomach` vs `NAMEDGITractBodyPart`, and `Stomach ⊑ NAMEDGITractBodyPart` is
told. This hop should work in the saturator if the earlier hops do.

The second hop `UlcerOfStomach -> GastricPathology` requires:

1. `Ulcer ⊑ DomainCategory ⊓ ∃hasPathologicalStatus.pathological`. Ulcer →
   UlcerOrErosion → InflammatoryLesion → … but the chain to `DomainCategory` and
   to `∃hasPathologicalStatus.pathological` is **not direct**. `Ulcer ⊑
   ∃isOutcomeOf.UlcerationProcess` and `UlcerationProcess ≡ BodyProcess ⊓
   ∃hasOutcome.Ulcer` give a recursive relationship, but the `pathological`
   status doesn't follow from EL alone — the module's only GCIs that conclude
   `∃hasPathologicalStatus.pathological` for a non-`Behaviour`/`Process` body
   require the LHS to already have `∃hasIntrinsicPathologicalStatus.pathological`
   on it (same functional-role + disjointness pattern as pair 06).
2. `hasSpecificLocation ⊑ LocativeAttribute` via the sub-property chain
   `hasSpecificLocation ⊑ hasLocation ⊑ AnatomicalLocativeAttribute ⊑
   LocativeAttribute`. That's pure CR9.

So pair 07 reduces to **the same functional-role + disjointness gap as pair 06**,
in service of establishing `Ulcer ⊑ PathologicalCondition`. The path Ulcer →
PathologicalCondition probably runs through `Ulcer ⊑ BodyStructure ⊓
∃isOutcomeOf.(BodyProcess ⊓ ∃hasIntrinsicPathologicalStatus.pathological) ⊑
∃hasIntrinsicPathologicalStatus.pathological` (one of the listed GCIs), but
seeding the inner `∃hasIntrinsicPathologicalStatus.pathological` on the
UlcerationProcess witness again requires the negation-driven step.

### Candidate Phase 2b rule shape

Same as pair 06 — functional-role witness-merge + covering / sibling-collapse, plus reliance
on the compound existential-body lowering (LHS conjunction with nested existential operand) for the wrapping shape.

---

## Pair 08: KneeJointStability ⊑ JointStability

**Full IRIs:** `http://example.org/factkb#KneeJointStability` ⊑ `http://example.org/factkb#JointStability`
**Cluster:** E
**Module size:** 1765 lines (related to but slightly different from pairs 03/04/05's 1720-line module)
**rustdl --saturation-only:** MISS
**rustdl default:** MISS
**HermiT on module:** FOUND (path: `KneeJointStability -> JointStability`, 1 hop)

### Relevant axioms (the only hop)

```
EquivalentClasses(:KneeJointStability
  ObjectIntersectionOf(:Scope
    ObjectSomeValuesFrom(:isScopeOf
      ObjectIntersectionOf(:JointArticulationProcess
        ObjectSomeValuesFrom(:actsSpecificallyOn :KneeJoint)))))

EquivalentClasses(:JointStability
  ObjectIntersectionOf(:Scope
    ObjectSomeValuesFrom(:isScopeOf
      ObjectIntersectionOf(:JointArticulationProcess
        ObjectSomeValuesFrom(:actsOn :Joint)))))

SubObjectPropertyOf(:actsSpecificallyOn :actsOn)
SubClassOf(:KneeJoint :LimbJoint)
SubClassOf(:LimbJoint :Joint)
```

### Missing derivation step

Pure EL+ derivation:

1. From the `KneeJointStability` equivalence, the conjunctive trigger gives
   `KneeJointStability ⊑ ∃isScopeOf.W` where `W = JointArticulationProcess ⊓
   ∃actsSpecificallyOn.KneeJoint`.
2. By CR9 + the witness subsumption rule, `W ⊑ JointArticulationProcess`
   (atomic) and `W ⊑ ∃actsOn.KneeJoint` (via sub-property) and via told
   subsumption `KneeJoint ⊑ Joint`, the witness of the `actsOn` existential has
   `Joint` as a subsumer, so `W ⊑ ∃actsOn.Joint` (modulo the witness's
   subsumer set).
3. Therefore `W` should be a subsumer of the Tseitin synthetic for
   `JointArticulationProcess ⊓ ∃actsOn.Joint` (call it `F`), making
   `KneeJointStability ⊑ ∃isScopeOf.F` and hence `KneeJointStability ⊑
   JointStability` by the `JointStability` conjunctive trigger.

The catch: step 3 requires the **inner-witness's** subsumer set to include the
**marker** for `∃actsOn.Joint` (which is itself a Tseitin existential marker
introduced when lowering the `JointStability` definition's compound body). This
is the same "compound existential body in a compound trigger" interaction as
pairs 01–04 — the calculus is straightforward EL+ with CR9 and Tseitin, but it
exercises the same code paths whose empirical failure on pair 01 suggests an
implementation gap.

### Candidate Phase 2b rule shape

Same as pair 01 — implementation fix on compound existential-body lowering (LHS conjunction with nested existential operand) and
its CR5/CR9/Tseitin interaction. Pair 08 is the cleanest, smallest test case
for that fix (only one hop, no `mirrorImaged`/`normal` clutter, no `isStructuralComponentOf`
recursion).

---

## Cross-cutting summary

### Distinct rule shapes that emerged (3)

1. **Compound LHS / RHS existential bodies with nested existentials, lowered
   through CR5 + CR9 + Tseitin synthetics.** Pairs 01, 02, 03, 04, 05 (via 04),
   08. This is the dominant pattern (6 of 8 pairs). On paper the saturator
   already supports this calculus; the empirical failure on pair 08 (the
   simplest instance) is strong evidence of an **implementation gap, not a
   missing calculus rule**.
2. **Functional-role witness-merge + disjointness / covering propagation
   through the merged witness** (the "sibling sub-properties of a functional
   super-property" pattern from the handoff). Pairs 06, 07. Genuine open
   calculus lever, matches handoff §2 ("functional-role inference").
3. **Trivial EL conjunction-elimination** as the first hop, with all the
   calculus work in downstream hops. Pairs 05 and 06 (first hops only) —
   these reduce to patterns 1 and 2 respectively.

### Did Cluster A consistently look like ≥n + disjointness (the spec's named target)?

**No, and the verification is stronger than just "we found a different derivation".**
The `≥n + disjointness` derivation HermiT would need is **mechanically impossible
on these modules** — the axioms simply aren't present. Direct grep across all
8 modules:

```
pair_01: cardinality=0 disjointness=0
pair_02: cardinality=0 disjointness=0
pair_03: cardinality=0 disjointness=0   (cluster A)
pair_04: cardinality=0 disjointness=0
pair_05: cardinality=0 disjointness=0   (cluster B)
pair_06: cardinality=0 disjointness=0   (cluster C)
pair_07: cardinality=0 disjointness=0   (cluster D)
pair_08: cardinality=0 disjointness=0   (cluster E)
```
(Patterns searched: `ObjectMinCardinality|ObjectMaxCardinality|ObjectExactCardinality|
MinCardinality|MaxCardinality|ExactCardinality` and `DisjointClasses|DisjointUnion`.)

Across all eight modules HermiT had **no** `≥n`, `≤n`, `=n` cardinality
restrictions and **no** `DisjointClasses` / `DisjointUnion` axioms available.
Whatever derivation HermiT used must therefore live entirely in the EL+ /
functional-role-merge / sub-property / transitive / domain/range / GCI fragment.
The compound existential-body lowering (LHS conjunction with nested existential operand) GCIs identified per-pair are the only
candidate mechanism that fits the available axioms.

The spec hypothesis (and the handoff §3 lever ranking "≥n cardinality with
disjointness") **cannot** be the GALEN mechanism for this sample, regardless
of how the saturator's compound existential-body lowering is fixed. The "pizza
InterestingPizza" pattern (`Pizza ⊓ ≥3 hasTopping`) cited in the handoff is
genuinely a `≥n + disjointness` case, but the GALEN PairedBodyStructure
derivations are not analogous — GALEN doesn't build paired-ness from
cardinality, it builds it from the compound LHS GCI above.

This is the most important finding of Task 4 and the highest-confidence
contradiction with the prior planning documents. Phase 2b proper should
**not** invest in `≥n + disjointness` work expecting GALEN coverage —
the GALEN MISSED sample's axioms cannot be closed by that rule.

### Verification caveats (honesty above completeness)

- Saturation-only verdicts (`rustdl subclass --saturation-only`) were confirmed
  empirically on pairs 01 and 02; remaining pairs are taken as MISSED based on
  the Task 1 measurement, not re-verified per-pair here.
- The "implementation gap, not calculus gap" attribution for pattern 1 is a
  reasoned inference from reading `crates/owl-dl-saturation/src/lib.rs:1263-1331`
  and §1502 (the LHS-conjunction-with-existential-operand path and the
  Tseitin allocator) plus the empirical pair_02 result. A definitive
  attribution requires either (a) tracing the saturator's lowering on the
  specific GCI to find where it bails out, or (b) finding a calculus
  counterexample. Neither was attempted in Task 4 — Phase 2b proper should
  start from a focused trace on pair 02 or pair 08 (the smallest reproductions).
- Pair 04's chain-end downstream hops (`ActuallyHollowBodyStructure ⊑
  ActuallyHollowStructure ⊑ ... ⊑ HollowStructure`) were assumed pure EL given
  the equivalent-class shapes and the `actuallyHollow ⊑ trulyHollow ⊑ hollow`
  state-class subsumptions, but were not traced step-by-step.
- For pair 06 the "non-Horn / requires negation" attribution was double-checked
  by grepping for any axiom containing both `hasEffectiveness` and
  `hasIntrinsicPathologicalStatus`. The grep returned exactly one axiom — the
  same GCI cited above, where `∃hasIntrinsicPathologicalStatus.physiological`
  appears in the LHS (the assumed branch) and `∃hasPathologicalStatus.pathological`
  (the wrong role) in the RHS. No direct EL chain from `∃hasEffectiveness.ineffective`
  to `∃hasIntrinsicPathologicalStatus.pathological` exists in the module. The
  functional-role attribution is consistent with the handoff §2 trace and with
  this no-such-chain grep.
