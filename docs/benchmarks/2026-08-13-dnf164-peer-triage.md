# Peer triage of the current DNF tail: still 91% gap, not intrinsic

**Date:** 2026-08-13 · **Population:** the 164 ontologies rustdl fails to classify
(60 s cap, harness `--threads 1`) · **Peer:** Konclude 0.7.0 native, `PEER_CAP=120`,
`JOBS=4`, same host (`fsesrv-g1`)

## Result

| outcome | n | share |
|---|---|---|
| **CLASSIFIED** | **150** | **91%** |
| EMPTY_STUB (exited 0, classified nothing) | 1 | 1% |
| DNF for Konclude too | 13 | 8% |
| NO_OUTPUT | 0 | — |
| STALE (pre-existing file, not this run) | 0 | — |

Classified walls: **median 5.80 s**, p90 39.3 s, max 105.3 s. **97 under 10 s**, 130 under
30 s.

**Outcome judged from output CONTENT, not exit code.** Konclude exits 0 on missing or junk
input and writes an ~896-byte `Thing`/`Nothing`-only hierarchy; a prior triage leg read 58
of 60 as "ok" from exit codes before that was caught. Files were additionally filtered by
mtime against the run manifest (`2026-08-13T15:01:02`), because `raw/konclude` also holds
the older oracle leg — 0 stale reads.

## The gap has not changed character

| | 2026-08-01 | **2026-08-13** |
|---|---|---|
| rustdl DNF | 257 | **164** (−36%) |
| peer-solvable | 242 (94%) | **150 (91%)** |
| peer median wall | 3.57 s | 5.80 s |
| plausibly intrinsic (Set B) | ≤ 15 | **≤ 14** |

So six default flips and four shipped fixes have **shrunk the tail by 36% without changing
its character**: it remains overwhelmingly a gap against a peer that solves it in seconds,
and Set B — the plausibly-intrinsic residue — is essentially unchanged at ≤14 ontologies.

Two readings follow, and both matter:

* **The work is justified.** 150 ontologies are demonstrably classifiable in a median 5.8 s
  by a peer on the same host. Nothing about this tail is a law of nature.
* **But the levers tried are exhausted at the level they were tried.** perf shows the
  remaining cost is diffuse — largest single area 14%, five roughly-equal ~10% slices — and
  `ore_ont_6134`'s label cache alone is 6,412 s of CPU, 200 s even at perfect 32-way
  parallelism against a 60 s cap. Closing a 20×-to-100× gap does not come from shaving 10%
  slices; it comes from not doing the work at all.

## Implication for what to try next

The per-ontology, per-phase optimisation approach has produced four real fixes this arc
(env-flag hot loops, duplicate saturation, and two correctness fixes) and then hit a floor.
The measurements now say the residual is **algorithmic, not constant-factor**: rustdl is
doing orders of magnitude more work than Konclude on the same input, and the profile is flat.

That points at the architectural items already in the design record rather than at another
profile-guided micro-lever — build-once/classify-many, and the diagnosed-but-unbuilt
surrogate-atom absorption for the defined-class over-branching (`ore_ont_10019`).

**What would falsify that framing:** a single ontology in this 150 where rustdl's cost is
dominated by one symbol worth >30%. None of the 16 profiled so far is.
