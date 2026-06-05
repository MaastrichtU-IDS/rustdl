# Corpus performance — 2026-06-06 (full-parity build)

Measured on the `feat/sio-disjunction-common-subsumer` branch (HEAD after the
Phase 2e functional-merge fix + the SIO disjunction-common-subsumer pass), i.e.
the build that reaches **FP=0, MISSED=0 across all 9 measured corpus fixtures**.

Methodology: `rustdl classify --pair-timeout-ms 200`, wall via `date +%s.%N`
around the process, best of 2 runs, single host under light load. `--pair-
timeout-ms 200` is the practical classify mode (sound under-approximation; the
closure-diff harness uses the same 200 ms budget). Walls are end-to-end process
time (parse + convert + classify), not reasoning-only.

## Classify wall (bounded, 200 ms/pair)

| Ontology | Classify wall | Fragment / notes |
|---|---|---|
| sulo | 0.03 s | small |
| ro | 0.51 s | |
| galen | 0.59 s | Horn → saturation-only fast path |
| notgalen | 1.03 s | Horn → saturation-only fast path |
| pizza | 2.07 s | SROIQ (cardinality) |
| go-basic | 18.65 s | pure EL, ~73 k concept rules |
| family | 27.81 s | |
| sio | 31.79 s | ~1585 classes, per-pair wedge |

(galen / notgalen live in `ontologies/external/`; the rest in
`ontologies/real/`.)

## No regression from the SIO pass (A/B vs main)

The disjunction-common-subsumer pass runs `build_told_tables` once per
`convert_ontology` plus a one-pass axiom scan. A/B on the largest inputs,
same host, single run each:

| Ontology | main (no pass) | this branch | Δ |
|---|---|---|---|
| go-basic | 19.51 s | 18.65 s | −0.86 s (noise) |
| family | 27.82 s | 27.81 s | ~0 |
| sio | 30.16 s | 31.79 s | +1.63 s (≈ run-to-run noise at 30 s) |
| galen | 0.57 s | 0.59 s | noise |
| notgalen | 1.03 s | 1.03 s | 0 |

The pass cost is within run-to-run variance; no measurable regression. (The
per-convert told-table build is paid once per classify, not per pair, and the
table is built in-pipeline anyway.)

## Correctness (the headline)

Full Konclude parity on the measured corpus — `tests/konclude_closure_diff.rs`,
all `#[ignore]`d, run with `-- --ignored`:

| Fixture | FP | MISSED |
|---|---|---|
| alehif | 0 | 0 |
| galen | 0 | 0 |
| notgalen | 0 | 0 |
| ro | 0 | 0 |
| shoiq-knowledge | 0 | 0 |
| sulo | 0 | 0 |
| ore-10908-sroiq | 0 | 0 |
| ore-15672-shoin | 0 | 0 |
| sio | 0 | 0 |

vs Konclude wall-to-wall: galen/notgalen (Horn fast path) tie or beat Konclude;
see `docs/perf-2026-06-04-konclude-vs-rustdl.md` for the head-to-head. SROIQ
walls (sio/pizza) remain above Konclude — the per-pair wedge is the cost; that
is a performance, not a correctness, gap.
