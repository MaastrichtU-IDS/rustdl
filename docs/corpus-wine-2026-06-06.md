# Adding the Wine ontology — nominal/value-restriction stressor

Added 2026-06-06. First corpus fixture targeting **nominal + value-restriction
reasoning** (the W3C OWL-guide wine ontology, SHOIN(D)). Surfaces a real
completeness gap that no prior corpus ontology exposed; FP=0 (sound).

## What it is

W3C OWL-guide `wine` merged with its imported `food` ontology
(`http://www.w3.org/TR/2003/PR-owl-guide-20031209/{wine,food}`). wine imports
food, which circularly imports wine — ROBOT hangs resolving those over the
network, so `fetch_wine` strips both `owl:imports` triples and merges the two
files locally into one self-contained ontology.

- 137 classes, 228 SubClassOf, 88 EquivalentClasses, 39 DisjointClasses
- **207 nominal/value restrictions** (`ObjectHasValue` / `ObjectOneOf` over wine
  regions, grapes, colors, sugar/body/flavor levels)
- a handful of datatype axioms (lighter than hoped — wine's datatypes are minimal)

Fragment: out-of-EL. rustdl classify wall: **186.8 s** (`--pair-timeout-ms 200`)
— almost entirely per-pair tableau (`tier_walk` ≈ 184 s); nominal reasoning is
the cost. Small class count, but each pair's satisfiability check is expensive.

## Result

```
wine   rustdl_closure=596   konclude_closure=653   FP=0   MISSED=57
```

- **FP=0** — soundness holds on a nominal/disjointness-rich ontology (the test
  gate). No false subsumptions.
- **MISSED=57** — a substantial completeness gap, all of one shape: **nominal /
  value-restriction subsumptions** the classify path (`trust_sat` / saturation)
  doesn't derive. Examples:
  - `AlsatianWine ⊑ FrenchWine`, `Anjou/Beaujolais/Bordeaux/… ⊑ FrenchWine`
    (region nominals: `locatedIn` value ∈ a French-region `ObjectOneOf` set)
  - `Sancerre ⊑ SauvignonBlanc`, `Beaujolais ⊑ Gamay`, `Margaux ⊑ Merlot`
    (grape `hasValue` restrictions)
  - `Muscadet ⊑ WhiteNonSweetWine`, `DryRiesling ⊑ WhiteNonSweetWine`
    (sugar/color value restrictions)

This is the same *shape* of gap as pre-fix SIO (a trust_sat under-approximation
the full tableau would close but times out at the per-pair budget), except the
missing reasoning is **nominal/value-restriction**, not disjunction. The full
tableau (trust_sat off, unbounded) would recover them but is too slow per pair.

## Why this fixture earns its place

No prior corpus ontology stressed nominal/value-restriction classification:
pizza/ORE have nominals but few entailments hinge on them; the 9 parity fixtures
are EL/Horn/disjunction-shaped. Wine isolates the nominal completeness frontier
with a small, famous, well-understood ground truth — and it does so without a
soundness regression (FP=0).

## Reproduce

```sh
scripts/fetch-real-ontologies.sh        # fetches + merges wine.ofn
docker/robot/classify-oracle.sh ontologies/real/wine.ofn \
    ontologies/real/konclude-input/wine-classified.owx
cargo test -p owl-dl-reasoner --release --test konclude_closure_diff \
    wine_closure_matches_konclude -- --ignored --nocapture
```

## Next (not done here)

The 57 are an open **nominal-reasoning completeness** project, parallel to the
SIO disjunction and notgalen functional-merge work but for `ObjectHasValue` /
`ObjectOneOf`. Likely a sound saturator/preprocessing lever (e.g. propagate
`∃R.{a}` + region-membership nominal facts) — to be scoped separately. The test
asserts only FP=0 today; MISSED is informational, as for sio/ro/sulo.
