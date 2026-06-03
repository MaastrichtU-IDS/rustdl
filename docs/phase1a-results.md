# Phase 1a — snapshot data structure + capture results

Run 2026-06-03 at HEAD `143d017`. Phase 1a lands the types,
the capture path on HyperEngine, the ontology-wide risk classifier,
and the Phase 0 canary harness. **Zero behavior change in default
classify** — capture is gated behind `RUSTDL_SNAPSHOT_CAPTURE`
(default OFF), and no consumer wires it yet (that's Phase 1b).

## Headline

Phase 1a is plumbing-only. Acceptance is that nothing regressed:

- FP=0 + MISSED=0 on alehif + ORE-10908 + ORE-15672 + GALEN
  (unchanged from Phase 8 baseline).
- GALEN classify wall: 452.34 s (Phase 8 baseline: 453.02 s; delta
  −0.15 %).
- All in-tree tests pass on owl-dl-tableau (96/96).
- Reasoner-crate clippy clean under `-D warnings`.

## What landed

- `crates/owl-dl-tableau/src/snapshot.rs` — `GraphSnapshot`,
  `SnapshotNode`, `SnapshotEdge`, `BackPropRisk`, `UnsafeReason`,
  `SnapshotNodeId`, `RuleFingerprint`. Public accessors: `seed`,
  `is_safe`, `risk`, `node_count`, `root_labels`. Constructor
  `from_parts` (pub(crate)).
- `BackPropRisk::classify_ontology(internal)` — first-cut
  ontology-wide classifier. Scans all axioms in one pass,
  accumulates (inverse, nominal, cardinality) bits, returns in
  priority order. Conservative — Horn ontologies land Safe;
  SROIQ workloads land Unsafe.
- `HyperEngine::satisfiability_snapshot(seed) -> Option<GraphSnapshot>` —
  walks the union-find, collapses merged-away nodes, copies
  node/edge structure. Phase 1a stamps `risk = Safe` (orchestrator
  overrides in Phase 1b).
- `snapshot_capture_enabled()` env helper, default OFF.
- Phase 0 canary at `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs`.
- Risk-classifier unit tests at `crates/owl-dl-tableau/tests/backprop_risk.rs`.
- Snapshot-capture unit tests at `crates/owl-dl-tableau/tests/snapshot_capture.rs`.

## Commits

- T1: `26a2a57` — types + classifier + 4 unit tests.
- T1-fixup: `292c255` — code-quality review feedback: collapsed
  asymmetric short-circuit into uniform shape; spec ref for Inv-1.
- T2: `b66c9a0` — `satisfiability_snapshot` capture + accessors + 2 unit tests.
- T3: `143d017` — env flag + Phase 0 canary.

## Measurements

| Fixture | Pre-Phase-1a wall | Post-Phase-1a wall | Delta |
|---|---:|---:|---:|
| alehif-test (closure_diff) | — | 8.68 s | — |
| ORE-10908 (closure_diff) | — | 9.78 s | — |
| ORE-15672 (closure_diff) | — | 36.03 s | — |
| GALEN (closure_diff) | 453.02 s | 452.34 s | −0.15 % |

Soundness: FP=0 + MISSED=0 on all fixtures (unchanged).
GALEN closure size unchanged at 27 997 = Konclude.

The per-fixture pre-Phase-1a wall for alehif / ORE-10908 / ORE-15672
was not captured at session start (Phase 8 results doc carries only
the GALEN number). The GALEN baseline 453.02 s IS from
`docs/phase8-results.md` and is authoritative.

### Measurement environment caveat

Host was heavily contended during these runs — `top -bn1` at the
start of Step 1 showed load average ~74 with two Python jobs
consuming ~3000 % CPU between them on a many-core box. GALEN still
landed at −0.15 % vs the Phase 8 baseline (also measured under
contention on this host), so the comparison is apples-to-apples
and the delta is well inside both the ±5 % noise band and the
±10 % regression gate.

## Cost-bound check (acceptance criterion from spec §6 Phase 1a)

Spec §6 Phase 1a revert criterion: "Memory/build cost > 30% of
classify wall on GALEN."

Phase 1a has no consumer of the new types — no snapshots are
captured during default classify because `RUSTDL_SNAPSHOT_CAPTURE`
is OFF. Expected build cost: 0 % of classify wall. Measured GALEN
delta: −0.15 %.

Status: **PASS** — measured delta (−0.15 %) is within noise and
two orders of magnitude below the 30 % revert threshold; no
consumer is wired, so no snapshot allocations happen on the hot
path. Soundness gate green across all four fixtures.

## What's next

Phase 1b: `LazyReplayDriver` + `BackPropAborted` sentinel + wiring
into `subsumes_via_tableau` behind the env flag. Separate plan:
`docs/superpowers/plans/2026-06-XX-konclude-snapshot-cache-phase-1b.md`
(written after Phase 1a lands).

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`.
- Phase 0 + 1a plan: `docs/superpowers/plans/2026-06-03-konclude-snapshot-cache-phase-1a.md`.
- Pre-project baseline: `docs/perf-2026-06-03-konclude-vs-rustdl.md`.
- Phase 8 GALEN wall reference: `docs/phase8-results.md`.
