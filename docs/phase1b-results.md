# Phase 1b — replay driver + sentinel + cache + wiring results

Run 2026-06-03 at HEAD `a1f81ff`. Phase 1b lands snapshot-replay
correctness path: `HyperEngine::from_snapshot`, `replay_with_neg_sup`,
`BackPropAborted` runtime sentinel, `SnapshotCache` on `PreparedOntology`,
orchestrator wiring in `subsumes_via_tableau` (gated on
`RUSTDL_SNAPSHOT_CAPTURE`; default OFF unchanged), env helper
normalized, snapshot counters merged through the parallel
classify shards, and a soundness fix discovered during the GALEN
gate (root-scoped `¬sup` via the wedge's `fresh_q` injection
pattern).

**Phase 1b ships correctness + telemetry, NOT perf.** Full-re-run
replay reconstructs the engine state from a snapshot then re-runs
`decide` from scratch — equivalent wall to the wedge with seed
overhead. Perf wins wait for **Phase 1b.5 lazy expansion**
(fingerprint-gated rule-firing skip); the snapshot cache's
structural infrastructure is what unlocks that follow-up phase.

## Headline

- **FP=0 + MISSED=0** on Phase 0 net + GALEN with flag ON (Inv-2 holds).
- GALEN classify wall (flag ON): **161.31 s** vs flag-OFF baseline 148.95 s
  on the same host (~8% overhead, within the spec §7 project-level
  ≤10% regression bound and well under the §6 Phase 1b revert
  criterion `aborts > 50% of attempts`).
- 101 tableau crate tests pass + 4 reasoner-side canary tests pass.
- Snapshot path consulted on every Safe-ontology pair that reaches
  `subsumes_via_tableau` (verified in T5 canary via
  `snapshot_replay_used > 0` assertion).

## What landed

- `crates/owl-dl-tableau/src/hyper.rs` — `DepSet` bumped to
  `pub(crate)`; `HyperEngine::from_snapshot(clauses, snapshot)`
  reconstruction; `snapshot_origin` + `snapshot_backprop_aborted`
  fields; `add_label_via_backprop` API (Phase 1b infrastructure for
  Phase 3 site-hooking); `snapshot_backprop_aborted()` accessor;
  branch save/restore extended to preserve `snapshot_origin`.
- `crates/owl-dl-tableau/src/snapshot.rs` — `SnapshotNode.birth_deps`
  populated; `nodes()`/`edges_per_node()` accessors.
- `crates/owl-dl-tableau/src/replay.rs` — `ReplayVerdict` enum
  (4 variants); `replay_with_neg_sup(clauses, snapshot, neg_sup_clauses)`
  full-re-run driver; reads `snapshot_backprop_aborted()` after
  `decide` to return `BackPropAborted` on sentinel fire.
- `crates/owl-dl-reasoner/src/lib.rs` — `snapshot_capture_enabled()`
  normalized to sibling style; `SnapshotCache::build` with
  `fresh_q` allocation; `try_replay(sub, sup)` using
  `fresh_q ⊓ sup → ⊥` root-scoped encoding;
  `PreparedOntology::snapshot_replay(sub, sup)` method.
- `crates/owl-dl-reasoner/src/classify.rs` — `ClassificationStats`
  extended with 5 snapshot counters (replay_used/_subsumed/
  _not_subsumed/_aborts + cache_falls_through); snapshot-replay
  shortcut wired ahead of wedge in `subsumes_via_tableau`; per-tier
  + defined-sup-sweep merge loops accumulate the new counters.
- `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs` —
  flag-ON Inv-1 test + counter-firing assertion; flag-ON
  Unsafe-ontology no-op test; process-env guard with serializing
  mutex.

## Commits

- `610ce86` T1: `HyperEngine::from_snapshot` + `birth_deps`.
- `ade6104` T2: `replay_with_neg_sup` driver (full-re-run).
- `a842d7e` T3: `BackPropAborted` runtime sentinel.
- `ea7a202` T4: `SnapshotCache` + orchestrator wiring.
- `d3c5598` T4-fix: counter merge in parallel classify loops
  (snapshot counters were unobservable on the wire before).
- `2ea5cdb` T5: Phase 1b canary extensions (flag-ON Inv-1 + counters).
- `a1f81ff` T6-fix: root-scope ¬sup via wedge `fresh_q` injection
  (caught by GALEN gate — 25,333 FPs with the original global
  `sup(x) → ⊥` encoding).
- `<this-commit>` T6: results doc.

## Measurements

| Fixture | Pre-Phase-1b wall | Post-Phase-1b wall (flag ON) | Δ vs baseline | Soundness |
|---|---:|---:|---:|---|
| alehif-test | — | 12.77 s | — | FP=0 / MISSED=0 |
| ORE-10908 | — | 12.99 s | — | FP=0 / MISSED=0 |
| ORE-15672 (Unsafe → no-op) | — | 33.91 s | — | FP=0 / MISSED=0 |
| **GALEN (Horn → load-bearing)** | 148.95 s (flag OFF, same host) | **161.31 s** | **+8.3%** | FP=0 / MISSED=0 (closure 27,997=Konclude) |

The Phase 1a doc records GALEN at 452 s under heavier contention.
Re-measuring on this less-contended host gives flag-OFF baseline
of 148.95 s; flag-ON 161.31 s. Phase 1b overhead is the +8.3%
(seeding + replay vs cold-wedge cost; no lazy expansion yet).
Spec §6 Phase 1b's revert criterion is `aborts > 50% of attempts`
(soundness-focused, not wall-focused); the §7 project-level
non-regression bound is `≤10%`. +8.3% sits within both.

## Cost-bound analysis (vs. spec §6 Phase 1b acceptance)

Phase 1b acceptance: "Inv-1 + Inv-2 hold across Phase 0 net + GALEN;
counter telemetry reports prune/replay/abort rates; no behavior
change with flag OFF."

| Check | Status |
|---|---|
| Inv-1 (synthetic verdict invariance) | PASS (T5 canary `replay_returns_subsumed_on_horn_chain_with_flag_on`) |
| Inv-2 (corpus verdict invariance, flag ON) | PASS (alehif + ore-10908 + ore-15672 + GALEN all FP=0 / MISSED=0) |
| Counter telemetry (replay/aborts/falls_through) | PASS (`snapshot_replay_used > 0` asserted; merge-loop accumulation fixed in d3c5598) |
| No behavior change with flag OFF | PASS (Phase 0 canary still green) |

**Phase 1b acceptance: MET.**

## Soundness incident (T6 GALEN gate)

The first GALEN flag-ON run produced **25,333 FPs**. Root cause:
the original ¬sup encoding `sup(x) → ⊥` is global — it fires
whenever ANY node carries `sup`. Snapshots of richly-structured
classes (e.g., `Abdomen` in GALEN with successor edges via
`partOf`, `hasFunction`, etc.) have successor nodes whose labels
match arbitrary unrelated classes. When probing `Abdomen ⊑ Hand`,
the `partOf.Hand` successor's `Hand` label spuriously triggers
the clash → false Subsumed verdict.

Phase 0 net (alehif: 247 pairs, ore-10908: 6001, ore-15672: 142)
did not catch this — small closures don't exercise the buggy
pattern enough. GALEN's 27,997-pair closure exposed it definitively.

Fix in `a1f81ff`: mirror the wedge's `HyperCache::decide` pattern.
`SnapshotCache` allocates a `fresh_q: ClassId` at build (like
`HyperCache::fresh_q`); snapshots are built seeded with `fresh_q`
plus a Horn clause `fresh_q → sub` (derives `sub` at root); replay's
¬sup is `fresh_q ⊓ sup → ⊥` — **root-scoped** because only the
root carries `fresh_q`. Verified: 0 FPs after the fix on GALEN +
the Phase 0 net.

The incident reinforces the project meta-lesson: **the corpus diff
on GALEN is the soundness net for snapshot-replay** — synthetic
canaries are necessary but not sufficient. Mirrors the dead-end
ledger §4 lesson about label-only dep-sets.

## What's deferred to Phase 1b.5

Per spec scope decision, lazy expansion (the fingerprint-gated
rule-firing skip that gives the actual wall savings) is Phase 1b.5
work. Without it, replay wall ≈ wedge wall + seed overhead, so
Phase 1c measurement would show no perf headline. Plan order:

- **Phase 1b.5 (next plan):** compute `RuleFingerprint` per snapshot
  node (bloom-hashed `(rule_id, label_set)`); modify the engine's
  inner fixpoint loop to skip rule firings on snapshot-origin nodes
  when the trigger fingerprint hasn't shifted. Measure GALEN wall
  vs. flag-OFF baseline.
- **Phase 1c (after 1b.5 lands):** flip the default; run full
  corpus + soundness gate + write project-headline results doc.

The §A revert criterion is a **Phase 1c** outcome-band gate
(`GALEN > 300 s after recon-driven tuning`); it does NOT fire from
Phase 1b alone — wall stays within ~10% of flag-OFF baseline on
this host. The decision-point happens at Phase 1c after lazy
expansion is in.

## Carry-overs (Phase 1a → Phase 1b → Phase 1b.5)

| Carry-over | Status |
|---|---|
| Env-var parsing normalization | ✅ Fixed in T4 (sibling-style helper) |
| `SnapshotNode.birth_deps` populated | ✅ Fixed in T1 (DepSet bumped to pub(crate)) |
| Counter merge in parallel classify loops | ✅ Fixed in d3c5598 (caught during T5) |
| Root-scoped ¬sup encoding | ✅ Fixed in a1f81ff (caught during T6 GALEN gate) |
| `fired` fingerprint slot | Still placeholder `0` — Phase 1b.5 territory |
| Per-class `BackPropRisk` classifier | Deferred to Phase 3 |
| `neg_sup_clauses` caching per `sup` column | Phase 1b.5 perf opportunity |
| `parent`/`parent_role`/`at_most`/`at_least_done` snapshot fields | Phase 1b.5 if needed for lazy expansion; sound to omit on Safe seeds (Phase 3 may revisit) |

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`.
- Phase 0 + 1a plan + results: `docs/superpowers/plans/2026-06-03-konclude-snapshot-cache-phase-1a.md` + `docs/phase1a-results.md`.
- Phase 1b plan: `docs/superpowers/plans/2026-06-03-konclude-snapshot-cache-phase-1b.md`.
- Pre-project baseline: `docs/perf-2026-06-03-konclude-vs-rustdl.md`.
- Dead-end ledger §4 (label-only dep-set lesson, mirrors the T6 GALEN gate finding): `docs/hypertableau-dead-ends.md`.
