# Phase 2c.0 — cluster shift from Phase 2b.0 to post-Phase-3 residual

Compares the original Phase 2b.0 5-cluster characterization
(`docs/phase2b-galen-sample.md`) against the current 17 GALEN + 27
notgalen residual MISSED (per `docs/phase2c-galen-missed-pairs.txt` +
`docs/phase2c-notgalen-missed-pairs.txt`).

## GALEN residual super-class histogram

```
     12 IntrinsicallyPathologicalBodyProcess
      3 AbnormalBodyStructure
      1 VariantBodyStructure
      1 UnusualBodyStructure
```

All 17 GALEN residual pairs fall into 4 distinct super-classes. There are **0
pairs for `DigestiveSystemPathology`, `GastricPathology`, `ExactlyPairedBodyStructure`,
`MirrorImagedBodyStructure`, `HollowStructure`, or `JointStability`** — clusters A,
B, D, and E are fully recovered by Phase 2b / 2b.5.

## notgalen residual super-class histogram

```
     12 IntrinsicallyPathologicalBodyProcess
     12 Anonymous-324
      3 Anonymous-351
```

The notgalen 27 pairs span 3 distinct super-classes, two of which are anonymous
(blank-node-style) IRIs from the `http://galen.org/galen.owl#` namespace.
Notably, every notgalen sub-class that hits `Anonymous-324` also hits
`IntrinsicallyPathologicalBodyProcess`; 3 of 12 additionally hit `Anonymous-351`.
Sub-class local names: `Anonymous-241`, `Anonymous-331`, `Anonymous-349`,
`Anonymous-93`, `CardiacInsufficiencyDueToProsthesis`,
`CardiacInsufficiencyFollowingCardiacSurgery`, `CongestiveCardiacFailure`,
`IneffectiveCardiacFunction`, `LeftIneffectiveCardiacFunction`,
`PostcardiotomySyndrome`, `PostvalvulotomySyndrome`,
`RightIneffectiveCardiacFunction` — strongly overlapping with GALEN cluster C
sub-classes (same cardiac/respiratory dysfunction concepts).

## Phase 2b.0 super-class histogram (recall, all 109 GALEN MISSED)

| Super-class | Phase 2b.0 (of 109) | Phase 2b.0 cluster | Post-Phase-2b/2b.5/3 status |
|---|---|---|---|
| MirrorImagedBodyStructure | 20 | A (paired anatomy) | **fully recovered** (0 residual) |
| ExactlyPairedBodyStructure | 20 | A (paired anatomy) | **fully recovered** (0 residual) |
| IntrinsicallyPathologicalBodyProcess | 12 | C (pathological process) | **12 GALEN + 12 notgalen residual** |
| DigestiveSystemPathology | 12 | D (digestive pathology) | **fully recovered** (0 residual) |
| GastricPathology | 7 | D (digestive pathology) | **fully recovered** (0 residual) |
| JointStability | 5 | E (joint stability) | **fully recovered** (0 residual) |
| HollowStructure / ActuallyHollowBodyStructure (cluster B) | 15 | B (hollow) | **fully recovered** (0 residual) |
| AbnormalBodyStructure | 3 | F (tail) | **3 residual** (in F tail of original 109) |
| UnusualBodyStructure | 1 | F (tail) | **1 residual** (in F tail of original 109) |
| VariantBodyStructure | 1 | F (tail) | **1 residual** (in F tail of original 109) |

Cluster D recovery is a Phase 2b/2b.5 win beyond the original estimate. The plan
predicted cluster D would still be residual; it was not.

## New super-class appearances vs Phase 2b.0

**No genuinely new super-classes.** All four GALEN residual super-classes were
present in the original 109-pair list:

```bash
# Verified by:
# grep -c "#AbnormalBodyStructure$" docs/phase2b-galen-missed-pairs.txt  => 3
# grep -c "#UnusualBodyStructure$"  docs/phase2b-galen-missed-pairs.txt  => 1
# grep -c "#VariantBodyStructure$"  docs/phase2b-galen-missed-pairs.txt  => 1
```

`AbnormalBodyStructure` (3 original) and `UnusualBodyStructure` / `VariantBodyStructure`
(1 each) were in Phase 2b.0's F-tail. They are surfacing now because Phase 2b/2b.5
cleared clusters A/B/D/E, making the F-tail residue visible. This is expected
behaviour — F-tail emergence, not a genuinely new shape.

The notgalen `Anonymous-324` / `Anonymous-351` super-classes are from a different
OWL namespace (`http://galen.org/galen.owl#`) and were not characterized in Phase
2b.0 (which was GALEN-only / factkb-only). Their co-occurrence with
`IntrinsicallyPathologicalBodyProcess` across the same sub-class set suggests they
are anonymous GCI class expressions that encode the same or closely related
cluster-C shape, but this is **unconfirmed by per-pair analysis** — see cluster
mapping below.

## Cluster mapping for Phase 2c

| Residual cluster | Pairs (GALEN + notgalen) | Phase 2b.0 origin | Phase 2c rule shape |
|---|---|---|---|
| C — IntrinsicallyPathologicalBodyProcess (named) | 12 + 12 = 24 | original C | functional-role + covering / sibling-collapse (EL+ Option 3) |
| notgalen Anonymous-324 / Anonymous-351 (likely C) | 0 + 15 = 15 | not characterized (notgalen) | **new shape — needs per-pair analysis** (strong prior: cluster-C variant given sub-class overlap) |
| F-tail body-structure (AbnormalBodyStructure + UnusualBodyStructure + VariantBodyStructure) | 5 + 0 = 5 | F tail of original 109 | **uncharacterized — likely cluster-C-like; per-pair canary deferred to Phase 2c** (may share cluster-C/D axiom pattern or differ) |

Summary:
- **24 pairs** are confident cluster C (named `IntrinsicallyPathologicalBodyProcess`,
  same sub-classes as Phase 2b.0 pair 06 canonical representative).
- **15 pairs** are likely cluster-C variants (anonymous super-class co-occurrence),
  unconfirmed.
- **5 pairs** are F-tail body-structure residue from the original 109, unconfirmed
  per-pair.
- **0 pairs** are genuinely new shapes not seen in Phase 2b.0.

## Phase 2c scope prediction

If Option 3 (EL+ approximation, pattern-matching the `∃R_i.X ⊑ ∃R_f.Y` triangle)
lands:

- **Lower bound (certain cluster C): 24 of 44** — the 12 GALEN + 12 notgalen
  `IntrinsicallyPathologicalBodyProcess` named pairs.
- **Upper bound (if anonymous + F-tail also match the triangle): 44 of 44** —
  all residual pairs.
- **Most likely: 24-39** — cluster C + most anonymous (15 if shape-confirmed)
  recovered; F-tail (5 pairs) may or may not match depending on per-pair structure.

Cluster D (digestive pathology): **0 pairs expected** — already fully recovered.

## Cross-references

- Phase 2b.0 cluster characterization: `docs/phase2b-galen-sample.md`
- Phase 2b.0 per-pair analysis (pairs 06, 07): `docs/phase2b-galen-pair-analysis.md`
- GALEN residual pair list (17 pairs): `docs/phase2c-galen-missed-pairs.txt`
- notgalen residual pair list (27 pairs): `docs/phase2c-notgalen-missed-pairs.txt`
- Phase 2b.0 original 109 pairs: `docs/phase2b-galen-missed-pairs.txt`
