# Phase 2b.0 — GALEN MISSED sample

Selected from the 109 MISSED pairs in `phase2b-galen-missed-pairs.txt`
(Phase 2a measurement, commit f61e06a; pair list captured commit a47d962).
Stratified by local-name family to ensure coverage of distinct patterns.

## IRI histograms

### Sub-class local-name families (top 15)

```
      5 SupraPatellarPouch
      5 Recess
      5 KneeJointRecessus
      3 MeniscusOfKneeJoint
      2 UlcerOfStomach
      2 TibialTuberosity
      2 TibialPlateau
      2 TibialInterCondylarEminence
      2 RadialStylus
      2 NeckOfUlna
      2 NeckOfRadius
      2 NeckOfHumerus
      2 NeckOfFibula
      2 NeckOfFemur
      2 LesserTrochanter
```

### Super-class local-name families (top 15)

```
     20 MirrorImagedBodyStructure
     20 ExactlyPairedBodyStructure
     12 IntrinsicallyPathologicalBodyProcess
     12 DigestiveSystemPathology
      7 GastricPathology
      5 JointStability
      4 IntrinsicallyNormalBodyStructure
      3 TruelyHollowStructure
      3 TruelyHollowBodyStructure
      3 RenalPathology
      3 HollowStructure
      3 CardiacPathology
      3 ActuallyHollowStructure
      3 ActuallyHollowBodyStructure
      3 AbnormalBodyStructure
```

### Pair patterns: local-name → local-name (top 15)

```
      1 UlcerOfStomach ⊑ GastricPathology
      1 UlcerOfStomach ⊑ DigestiveSystemPathology
      1 UlcerOfPylorus ⊑ DigestiveSystemPathology
      1 TibialTuberosity ⊑ MirrorImagedBodyStructure
      1 TibialTuberosity ⊑ ExactlyPairedBodyStructure
      1 TibialPlateau ⊑ MirrorImagedBodyStructure
      1 TibialPlateau ⊑ ExactlyPairedBodyStructure
      1 TibialInterCondylarEminence ⊑ MirrorImagedBodyStructure
      1 TibialInterCondylarEminence ⊑ ExactlyPairedBodyStructure
      1 SupraPatellarPouch ⊑ TruelyHollowStructure
      1 SupraPatellarPouch ⊑ TruelyHollowBodyStructure
      1 SupraPatellarPouch ⊑ HollowStructure
      1 SupraPatellarPouch ⊑ ActuallyHollowStructure
      1 SupraPatellarPouch ⊑ ActuallyHollowBodyStructure
      1 RuptureOfHeart ⊑ CardiacPathology
```

Note: all 109 pairs are unique (1:1 histogram), so the pair-pattern histogram
does not further compress the data — the clustering signal lives entirely in the
super-class histogram.

## Visible clusters

**Cluster A — Bilateral/paired anatomical structures (40 pairs, ~37%).**
Twenty distinct body structures (bone heads, necks, condyles, trochanters,
tibial landmarks, etc.) each appear with two super-classes: `ExactlyPairedBodyStructure`
and `MirrorImagedBodyStructure`. The sub-classes are bilateral musculoskeletal
landmarks for which GALEN expects symmetry-based inheritance, but the engine
misses the chained role/property path that establishes bilateral membership.
`MeniscusOfKneeJoint` additionally hits `IntrinsicallyNormalBodyStructure`,
giving it three MISSED pairs.

**Cluster B — Hollow anatomical recesses (15 pairs, ~14%).**
Three sub-classes (`SupraPatellarPouch`, `Recess`, `KneeJointRecessus`) each
MISS five super-classes: `ActuallyHollowBodyStructure`, `ActuallyHollowStructure`,
`HollowStructure`, `TruelyHollowBodyStructure`, `TruelyHollowStructure`.
The five super-classes appear to be a synonym/hierarchy fan-out for "hollow
structure" in GALEN, so a single blocked inference propagates to all five at once.

**Cluster C — Pathological body processes (12 pairs, ~11%).**
Twelve distinct cardiac and respiratory dysfunction concepts (e.g.,
`CongestiveCardiacFailure`, `Dyspnoea`, `Cheyne-StokesRepiration`,
`IneffectiveCardiacFunction` subtypes) each MISS the single super-class
`IntrinsicallyPathologicalBodyProcess`. All are one-to-one pairs; the shared
pattern suggests a single missing inference rule for the
`IntrinsicallyPathologicalBodyProcess` branch.

**Cluster D — Digestive/gastric pathology (12 pairs, ~11%).**
Gastric, duodenal, and esophageal condition classes (ulcers, erosions,
inflammation, polyps, diverticula) MISS `DigestiveSystemPathology` and/or
organ-specific sub-pathology classes (`GastricPathology`, `DuodenalPathology`,
`EsophagealPathology`). Several sub-classes appear twice (once for each
relevant super-class), accounting for 12 pairs from roughly 8 distinct conditions.

**Cluster E — Joint stability (5 pairs, ~5%).**
Five knee-joint stability concepts (`AnteriorStabilityOfKneeJoint`,
`KneeJointStability`, `LateralCollateralStabilityOfKneeJoint`,
`MedialCollateralStabilityOfKneeJoint`, `PosteriorStabilityOfKneeJoint`) all
MISS the single super-class `JointStability`.

**Cluster F — Miscellaneous (25 pairs, ~23%).**
The remainder: renal pathology (`Nephritis` and two specific nephritides →
`RenalPathology`), cardiac pathology (`AneurysmOfHeart`, `ArteriovenousFistulaOfHeart`,
`RuptureOfHeart` → `CardiacPathology`), lung pathology (`PulmonaryInfarction`),
abnormal/unusual/variant body structures (`Diverticulum`, `Polyp`,
`IntrinsicallyAbnormalBodyStructure`, `IntrinsicallyUnusualBodyStructure`,
`IntrinsicallyVariantBodyStructure`), intrinsically normal structures
(`JointMeniscus`, `LateralMeniscus`, `MedialMeniscus`), and one-off patterns.

## Selected pairs

| # | Sub IRI | Sup IRI | Cluster |
|---|---------|---------|---------|
| 1 | `http://example.org/factkb#FemoralHead` | `http://example.org/factkb#ExactlyPairedBodyStructure` | A |
| 2 | `http://example.org/factkb#HeadOfHumerus` | `http://example.org/factkb#MirrorImagedBodyStructure` | A |
| 3 | `http://example.org/factkb#MeniscusOfKneeJoint` | `http://example.org/factkb#ExactlyPairedBodyStructure` | A |
| 4 | `http://example.org/factkb#KneeJointRecessus` | `http://example.org/factkb#HollowStructure` | B |
| 5 | `http://example.org/factkb#SupraPatellarPouch` | `http://example.org/factkb#ActuallyHollowBodyStructure` | B |
| 6 | `http://example.org/factkb#CongestiveCardiacFailure` | `http://example.org/factkb#IntrinsicallyPathologicalBodyProcess` | C |
| 7 | `http://example.org/factkb#AcuteGastricUlcer` | `http://example.org/factkb#DigestiveSystemPathology` | D |
| 8 | `http://example.org/factkb#KneeJointStability` | `http://example.org/factkb#JointStability` | E |

## Rationale

Pairs 1–3 sample Cluster A (the dominant 40-pair bilateral-structures cluster)
using three distinct sub-class shapes: a femur head (long bone), a humerus head
(upper limb), and MeniscusOfKneeJoint which uniquely accumulates three MISSED
super-classes including `IntrinsicallyNormalBodyStructure`. Pairs 4–5 cover the
hollow-structure fan-out cluster (B) with two of the three recurrence sub-classes
(`KneeJointRecessus` and `SupraPatellarPouch`) and two different super-class
synonyms (`HollowStructure` vs `ActuallyHollowBodyStructure`), exposing whether
the miss is symmetric across the synonym fan. Pair 6 is a clean representative
of the pathological-process cluster (C) with a well-known cardiac failure concept.
Pair 7 covers the digestive-pathology cluster (D). Pair 8 covers the
joint-stability cluster (E). The long tail of Cluster F (renal, lung, abnormal
body structure, etc.) is intentionally excluded from the sample: each is a
singleton or small group with a distinct super-class, and the 8-pair budget is
better spent confirming the four largest clusters; F can be revisited if the root
causes identified for A–E do not explain those pairs.
