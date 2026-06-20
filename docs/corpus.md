# Canonical evaluation corpus (single source of truth)

Every soundness gate (FP=0/MISSED=0 vs the Konclude∩HermiT oracle) and every perf
sweep MUST use exactly this set. Do not hand-pick subsets per experiment — a missing
fixture is not "neutral", it can be the worst data point (see
`memory: evaluate-innovations-full-corpus`). Paths are verified by
`scripts/perf-flag-sweep.sh` (it aborts on any unresolved path).

## Oracle-backed (soundness corpus — `tests/konclude_closure_diff.rs`)

| fixture | input | oracle | fragment | notes |
|---|---|---|---|---|
| galen | ontologies/external/galen.ofn | ontologies/external/galen-classified.owx | EL+ | large EL |
| notgalen | ontologies/external/notgalen.ofn | ontologies/external/notgalen-classified.owx | EL+ | |
| alehif-test | ontologies/alehif-test.ofn | ontologies/external/alehif-test-classified.owx | ALCHIF | ∀ |
| ore-10908-sroiq | ontologies/external/ore-10908-sroiq-classified.owx | (self) | SROIQ | |
| ore-15672-shoin | ontologies/external/ore-15672-shoin-classified.owx | (self) | SHOIN | disjunction-heavy |
| sio | ontologies/real/sio.ofn | ontologies/real/konclude-input/sio-classified.owx | SROIQ | disjunction-heavy |
| wine | ontologies/real/wine.ofn | ontologies/real/konclude-input/wine-classified.owx | SHOIN(D) | nominal+card; WALL outlier |
| pizza | ontologies/real/pizza.ofn | ontologies/real/konclude-input/pizza-classified.owx | SHOIN | nominal |
| bibtex | ontologies/real/bibtex.ofn | ontologies/real/konclude-input/bibtex-classified.owx | small | |
| ro | ontologies/real/ro.ofn | ontologies/real/konclude-input/ro-classified.owx | EL+ | |
| sulo | ontologies/real/sulo.ofn | ontologies/real/konclude-input/sulo-classified.owx | EL+ | |
| shoiq-knowledge | (not present locally) | — | SHOIQ | SKIP if absent |

## Inconsistency sentinel (CORRECTNESS gap — excluded from MISSED=0 claims)

| fixture | input | oracle verdict | rustdl | status |
|---|---|---|---|---|
| family | ontologies/real/family.ofn | inconsistent (<1s) | reports **consistent** (timeout-default, not reachable @120s) | `#[ignore]`d sentinel `family_inconsistency_detected`; the documented exception to "MISSED=0" |

## Perf-only (no oracle — wall measurement, not soundness)

| fixture | input | fragment | notes |
|---|---|---|---|
| go-basic | ontologies/real/go-basic.ofn | EL | large real-world EL; ~10s; perf control only |

## Wedge-exercising perf subset (disjunctive/SROIQ — where wedge changes show up)
sio, ore-10908-sroiq, ore-15672-shoin, alehif-test, wine, pizza
EL controls: galen, go-basic
