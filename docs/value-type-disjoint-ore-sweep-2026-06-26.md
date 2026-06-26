# value-type-disjoint / tautology-skip — ORE sweep (breadth + soundness)

**Status:** results (durable). Prompted by the user correctly challenging the "wine-specific"
claim. Swept ORE 2014 (zenodo 50737, 48 onts) + ORE 2015 DL (zenodo 18578, 315 classification
onts) for the value-disjoint pattern, with Konclude-oracle FP validation where firing.

## Pattern INGREDIENTS are common — not wine-unique

Metadata/grep pre-filter (FunctionalObjectProperty ∧ ObjectHasValue ∧ DifferentIndividuals):
- ORE 2014: ≥8 of 48 (DMOP/PD/DCHARS/DMKB/obsolete_RMOperators medical KBs; NEW/OLD/PREV_fhkb).
- ORE 2015 DL: 11 of 315 (SROIQ/SHOIQ(D)/SHOIN(D)/ALCOIF(D)/SROIF…). 55 have functional roles,
  44 DifferentIndividuals, 53 nominals.

So my earlier "wine-specific" claim (based on 3 ORE onts that lacked ObjectHasValue) was WRONG.

## Meaningful FIRING (≥2 distinct values per functional role) is rare

value-disjoint pairs only when a functional role has ≥2 DifferentIndividuals-distinct nominal
values. Measured firing:
- **wine** (= ORE2015 ore_ont_10702, identical 666 pairs/4 roles/137 classes): 666 pairs.
- **DMOP** (ORE2014): 14 pairs/7 roles. **PD** (ORE2014): 14 pairs.
- All other tested candidates: **0 pairs** — they have only 1 tbox nominal (ORE2015 10908/6934/
  2632/8480/15846 = 0 pairs), so no two values to pair. 10860 parse-error; 10621/7409/16462/9654
  (28–99 MB) too large to classify in-environment (timeout/error) — firing UNKNOWN.

So the firing pattern (multi-value-partition under functional ≤1) is genuinely uncommon; wine is
the standout, DMOP/PD fire modestly. Ingredients ≠ firing.

## SOUNDNESS — FP=0 on every tested firing ontology (vs Konclude oracle)

| ontology | pairs | rustdl(flags-ON) vs Konclude | flags-OFF |
|---|---|---|---|
| wine | 666 | 653=653 FP=0 MISSED=0 | identical |
| DMOP | 14 | closure 5693 FP=0 MISSED=31 | identical (5693/FP0/M31) |
| PD | 14 | closure 5179 FP=0 MISSED=133 | (baseline) |

FP=0 everywhere it fires; on DMOP the flags are **verdict-neutral** (identical closure on/off — the
14 pairs are sound but the ontology lacks wine's disjunctive WALL, so no collapse; MISSED is
baseline incompleteness, not flag-caused). Inert (0-pair) onts are byte-identical.

## Verdict

- **Soundness is GENERAL** — value-disjoint + tautology-skip are FP=0 on every firing ORE ontology
  tested (wine, DMOP, PD), and byte-identical where inert. The features add only entailed
  disjointness / skip only tautologies — sound by construction AND oracle-confirmed at breadth.
- **The WALL-COLLAPSE benefit is wine-class-specific** — only multi-value-partition ontologies with
  the disjunctive-search wall (wine) gain; DMOP/PD fire but are verdict-neutral.
- **Gaps:** 4 large ORE2015 candidates (28–99 MB) couldn't be classified in-environment (firing/FP
  unknown); DCHARS/DMKB (DMOP-family) untested (assumed same as DMOP).

## Default-ON implication

The default-ON bar (FP=0 where it fires + inert elsewhere) is MET on the tested set — materially
stronger than the prior one-ontology evidence. Residual caution: the benefit is narrow (wine-class)
and 4 large candidates are untested. So default-ON is *soundness-safe* but *narrow-value*; staying
default-OFF remains defensible on the "narrow benefit + untested large onts + 3× nominal-pruning FP
history" grounds. Controller's call.
