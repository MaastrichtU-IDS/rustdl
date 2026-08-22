# The `ReportedClasses` refactor is answer-inert on the DKey-reachable frame

**Date:** 2026-08-22 · Closes the corpus gate on `fix/dkey-id-aliasing-on-main`.
**Result: 663/663 data-property-bearing ORE ontologies, 0 DIFFER, 0 REGRESSION, 0 RECOVERY.**

## Arms

| arm | commit | what it is |
|---|---|---|
| base | `b796bec` | `main` minus the DKey work |
| dkey | `60b2e22` | `fix/dkey-id-aliasing-on-main` tip |

`b796bec` is the merge-base. `origin/main` had advanced 8 commits by the time of the run, all
of them under `docs/` — so this comparison is behaviourally identical to one against current
`main`, and it avoids rebasing the branch (which would have rewritten `60b2e22` and re-orphaned
the handoff doc references).

Built in two separate worktrees with separate `CARGO_TARGET_DIR`s. Positive rebuild
confirmation per arm (7 `Compiling owl-dl` lines each, 0 errors) and the binaries were verified
to differ — the shared-target-dir trap that manufactured a false IDENTICAL in the 0.4.21 cycle.

## Why the bearing/non-bearing split is the right frame

A DKey is a **data** key: it can only exist in an ontology that has data properties. In a
non-bearing ontology `reportable_class_iris` filters nothing, so `report_pos` is the identity
and `class_id(i) == ClassId(i)`. The entire defect class this branch fixes **requires a DKey to
exist** in order for the two index spaces to diverge at all. So the 663 bearing ontologies are
the only slice where the refactor can change an answer, and they were swept exhaustively.

Corpus splits **663 bearing / 1258 non-bearing** of 1921 (`DataProperty|DatatypeProperty` by
grep). This is close to the 651/1269 split used by `9c5894f`; the delta is the RDF/XML spelling.

**Stated limit:** 165 of the 1258 non-bearing ontologies were also swept (opportunistically,
all IDENTICAL), leaving ~87% of that frame unmeasured *for this branch*. It rests on the
structural argument above, not on measurement. `9c5894f`'s 1,269-ontology inertness result
covers the DKey **seeding flags**, not this refactor, so it does not transfer.

## Result — bearing frame, 663/663

| verdict | n | meaning |
|---|---|---|
| IDENTICAL | 634 | same hash, both arms stable |
| BOTH_TIMEOUT | 19 | neither arm finished inside 60 s — no information, no asymmetry |
| NONDET | 9 | at least one arm disagreed with ITSELF — adjudicated below |
| BOTH_ERR | 1 | both arms fail identically (`ore_ont_10860`) |
| **DIFFER** | **0** | |
| **REGRESSION** | **0** | |
| **RECOVERY** | **0** | |

Total unique coverage across all passes: **828 of 1921**.

## The 9 NONDET rows, adjudicated serially (3 runs x BOTH arms, no competing load)

Seven resolved to **3/3 identical in both arms with the same hash** — pure contention
artifacts of the 4-way sweep: `12191`, `10894`, `14551`, `16274`, `4141`, `7893`, `9800`.

`ore_ont_3281` is ontology-level nondeterminism, not an arm effect: across sweep and retest
**both** `f7af888c9517` and `6e1159caa848` were produced by **both** arms.

`ore_ont_10517` is the pathological one. Under the wall-bounded config the base arm alone
produced four distinct hashes, so by the rule below no comparison there carries information.
Under the deterministic config (`--pair-timeout-ms 0`) **both arms fail identically** — rc=1,
byte-identical stderr, `tableau bailed out without a verdict (likely an internal limit)`. Same
behaviour in both arms either way.

## Instrumentation findings (both mine, both worth keeping)

**1. `--global-timeout-ms` must never be used to bound a sweep.** It is a known-unfixed
non-bound (`docs/2026-08-16-global-deadline-does-not-bound-wall.md`, 2.9x overshoot at 71k
classes). The first attempt at this sweep wedged on `ore_ont_15687` (21 MB, ~65k classes) for
**~3 hours against a 60 s budget — ~180x**, far outside anything that doc records, which
suggests a phase that never checks the deadline rather than one that overshoots it. Not
isolated here; flagged as its own investigation. Every invocation in this run is wrapped in an
external `timeout -k 5`.

**2. The empty-string hash is a false-IDENTICAL generator.** `sha256("")` is
`e3b0c44298fc...`. A run that exits non-zero with no stdout hashes to it in BOTH arms and
reads as perfect agreement. The sweep harness was correct (`rc != 0 || -z $o` -> `ERR`), but an
ad-hoc probe outside it reported `ore_ont_10517` as IDENTICAL on exactly this basis and it was
believed for one step. Verified afterwards: **0 occurrences of `e3b0c44298fc` in any results
file.** Any future differential must assert `stdout` is non-empty before hashing it.

**3. The rule from `9c5894f` earned its keep again.** *Verify a comparison is stable WITHIN one
arm before comparing two arms.* The harness was restructured to run 4 times per ontology
(base, base', dkey, dkey') so a self-inconsistent row is bucketed `NONDET` and can never be
reported as a DIFFER. Every one of the 9 NONDETs would have been a confident false finding
under a 2-run harness.

## Raw data

- `docs/benchmarks/data-2026-08-22-dkey-reportedclasses-differential.tsv` — all 914 rows
- `docs/benchmarks/data-2026-08-22-dkey-nondet-adjudication.tsv` — the serial 3x re-tests
