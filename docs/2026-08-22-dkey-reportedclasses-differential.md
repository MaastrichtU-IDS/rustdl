# The `ReportedClasses` refactor is answer-inert on the DKey-reachable frame

**Date:** 2026-08-22 · Closes the corpus gate on `fix/dkey-id-aliasing-on-main`.
**Result: 663/663 data-property-bearing ORE ontologies, 0 DIFFER, 0 REGRESSION, 0 RECOVERY.**

## Result summary — measured TWICE, same verdict

| | first measurement | **re-measure (authoritative)** |
|---|---|---|
| base arm | `b796bec` (old merge-base) | **`9a16697`** = `origin/main` incl. **v0.4.22** |
| dkey arm | `60b2e22` | **`14db978`** (rebased) |
| bearing coverage | 663/663 | **663/663** |
| IDENTICAL | 634 | **634** |
| BOTH_TIMEOUT | 19 | 20 |
| NONDET (all adjudicated) | 9 | **8** |
| BOTH_ERR | 1 | 1 |
| **DIFFER / REGRESSION / RECOVERY** | 0 / 0 / 0 | **0 / 0 / 0** |

The re-measure exists because `main` gained **v0.4.22** while this was in flight — a real code
release (`build_told_tables` O(n^2) memset removed, 6.2x conversion; convert sharing told tables
between passes; `realize` surfacing `witness_prune_active`). The first measurement was against
the old merge-base, so it said nothing about the refactor **composed with** those changes. It
now does. The conversion path is exactly where an interaction with index construction would
show, which is why re-running was not optional.

In the re-measure **all 8 NONDET rows adjudicated to 3/3 identical in both arms with the same
hash** — every one a contention artifact of the 4-way sweep, none an answer difference. That is
cleaner than the first pass, where `ore_ont_3281` and `ore_ont_10517` needed deeper attribution;
`10517` is now simply `BOTH_TIMEOUT`. Integrity checks on the re-measure: 663 unique rows, 0
missing, 0 duplicates, **0 empty-hash rows**.

Raw data: `docs/benchmarks/data-2026-08-22-dkey-remeasure-on-v0422.tsv` (+ `-nondet-adjudication`).

## Arms (first measurement — superseded, kept for the record)

| arm | commit | what it is |
|---|---|---|
| base | `b796bec` | `main` minus the DKey work |
| dkey | `60b2e22` | `fix/dkey-id-aliasing-on-main` tip, pre-rebase |

`b796bec` is the merge-base. At the time of that run `origin/main` had advanced 8 commits, all
under `docs/`, so the comparison was behaviourally identical to one against `main` **as it then
stood** — and it avoided rewriting `60b2e22`. That reasoning expired within the hour: `main`
then landed v0.4.22 with real code changes, which is what forced the re-measure above. The
lesson is that "main's lead is docs-only" is a fact with a short shelf life; re-check it at the
moment of measurement, not before.

### Commit hashes here are rebase-volatile

The rebase onto v0.4.22 rewrote all five commits. Anything citing the pre-rebase hashes maps as:

| pre-rebase | post-rebase | commit |
|---|---|---|
| `9cc380c` | `c8ca8b9` | report positions are not ClassIds |
| `ac721de` | `94e49c0` | unsat projection read a ClassId-indexed bitset |
| `5af44c4` | `9caf7d8` | corpus canary in CI; retire `unsatisfiable_bitset` |
| `6bd3904` | `eddaaed` | DKey aliasing resolution (docs) |
| `002c7e8` -> `60b2e22` | `14db978` | finish the probe conversion |

`002c7e8` -> `60b2e22` was an earlier message-only amend. These references have now been
refreshed three times; prefer citing a commit by its subject and role over its hash.

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

**4. A broken harness fails IDENTICALLY in both arms, which reads as agreement.** Setting up
the re-measure, a `sed` double-substitution pointed the sweep at `arms22` instead of `arms2`.
Both binaries were then missing, every invocation returned non-zero, and every row came back
`BOTH_ERR` — indistinguishable at a glance from "the arms agree". A 3-ontology smoke test caught
it on the first row. The harness now **asserts both binaries exist and are executable before
sweeping**, and has a distinct `ERR_ASYM` bucket so a one-sided error cannot hide inside
`BOTH_ERR`. Generalised: every "both arms agree" bucket needs a reason it could not have been
produced by the harness failing to run at all.

## Raw data

- `docs/benchmarks/data-2026-08-22-dkey-remeasure-on-v0422.tsv` — re-measure, 663 rows (authoritative)
- `docs/benchmarks/data-2026-08-22-dkey-remeasure-nondet-adjudication.tsv` — its serial 3x re-tests
- `docs/benchmarks/data-2026-08-22-dkey-reportedclasses-differential.tsv` — first measurement, 914 rows
- `docs/benchmarks/data-2026-08-22-dkey-nondet-adjudication.tsv` — its serial 3x re-tests

## Toolchain note

Everything here ran under `RUSTUP_TOOLCHAIN=stable` (clippy 0.1.96), which **overrides**
`rust-toolchain.toml`'s pinned `1.95.0`. That pin exists specifically to stop clippy/rustfmt
drift from turning CI red, and the pinned toolchain is not installed locally — which is why the
handoff instructions reach for `stable`. For the two `doc_markdown` errors seen here it makes no
difference (CI reproduces them identically on the pin), but the override is the wrong default and
should not be copied forward.
