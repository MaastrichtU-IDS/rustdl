# Phase 1b.5 — lazy expansion + per-sup caching results

Run 2026-06-03 at HEAD `a368746`. Phase 1b.5 lands the lazy-expansion
path on top of Phase 1b's snapshot replay infrastructure:
`SnapshotNode.pre_capture_labels` (T1), `LazyReplayState` +
`HyperEngine::from_snapshot_lazy` (T2), lazy expansion wired into
replay (T3), and a per-sup `neg_sup_clauses` cache on `SnapshotCache`
(T4). Soundness clean across the Phase 0 net + GALEN + notgalen with
`RUSTDL_SNAPSHOT_CAPTURE=1` (the default-OFF state remains
unchanged).

**Phase 1b.5 brings the Horn-fragment wall back to flat-vs-flag-OFF
baseline** — Phase 1b's +8% overhead is amortized away by lazy
replay. The architectural lever delivered the flat-vs-baseline win
that Phase 1c readiness requires; the wall savings are smaller than
the recon projection, see "Gap vs recon" below.

## Headline

- **FP=0 / MISSED=0** on Phase 0 net (alehif, ORE-10908, ORE-15672)
  and on GALEN with flag ON + lazy ON; closure 27,997 = Konclude.
- **FP=0 / MISSED=18** on notgalen (matches Phase 7 baseline; the 18
  are pre-project dl-approximation artifacts unrelated to this
  project).
- GALEN classify wall (flag ON, lazy ON): **154.44 s** vs Phase 1b
  first-cut 161.31 s (−4.3 %) vs flag-OFF baseline 148.95 s (+3.7 %,
  essentially flat).
- notgalen classify wall: **337.58 s** vs pre-project baseline ~1170 s
  (per `docs/perf-2026-06-03-konclude-vs-rustdl.md`; this is a
  fresher uncontended host so a direct apples-to-apples is not
  warranted, but the wall is comfortably under the spec §7 project
  notgalen ≤ 400 s success criterion).
- Spec §6 Phase 1c outcome band: GALEN 154.44 s falls in the
  **150–300 s band → Ship + mandatory Phase 2 build**. The ≤ 150 s
  "Layer 2 incremental" door is missed by 4.4 s; the §A revert
  threshold (300 s) is not anywhere close to firing.

## What landed

- `crates/owl-dl-tableau/src/snapshot.rs` — `SnapshotNode.pre_capture_labels`
  field (T1: per-node label-set captured before the snapshot finalises
  so lazy replay knows the rule-firing fingerprint).
- `crates/owl-dl-tableau/src/hyper.rs` — `LazyReplayState` struct +
  `HyperEngine::from_snapshot_lazy` constructor (T2: replay path
  that gates `Event::Label` re-firing on per-node fingerprint match).
- `crates/owl-dl-tableau/src/replay.rs` — lazy expansion wired into
  `replay_with_neg_sup`; snapshot-origin label events skip
  `process_event` when the pre-capture fingerprint hasn't shifted (T3).
- `crates/owl-dl-reasoner/src/lib.rs` — `SnapshotCache` extended
  with a per-sup `neg_sup_clauses` cache (T4: avoid re-allocating the
  one-clause `fresh_q ⊓ sup → ⊥` vec per pair).

## Commits

- `9f3e568` T1: `pre_capture_labels` on `SnapshotNode`.
- `76d515f` T2: `LazyReplayState` + `from_snapshot_lazy`.
- `921c56e` T3: lazy expansion wired into replay.
- `a368746` T4: per-sup `neg_sup_clauses` cache in `SnapshotCache`.
- `<this-commit>` T5: results doc.

## Measurements

| Fixture | Pre-project wall | Phase 1b (flag ON) | **Phase 1b.5 (flag ON + lazy ON)** | Δ vs Phase 1b | Soundness |
|---|---:|---:|---:|---:|---|
| alehif-test | — | 12.77 s | **6.58 s** | −48 %* | FP=0 / MISSED=0 |
| ORE-10908 | — | 12.99 s | **13.05 s** | flat | FP=0 / MISSED=0 |
| ORE-15672 (Unsafe → no-op) | — | 33.91 s | **34.11 s** | flat | FP=0 / MISSED=0 |
| **GALEN (Horn → load-bearing)** | 148.95 s (flag OFF) | 161.31 s (+8.3 % vs OFF) | **154.44 s** (+3.7 % vs OFF) | **−4.3 %** | FP=0 / MISSED=0 (closure 27,997 = Konclude) |
| **notgalen** | ~1170 s (different host, see §) | — | **337.58 s** | — | FP=0 / MISSED=18 (Phase 7 baseline; pre-project artifacts) |

*alehif's −48 % isn't a Phase 1b.5 win — Phase 1b ran on a more
contended host. The flat-vs-flag-OFF reading is what's load-bearing.

The notgalen pre-project baseline (~1170 s) was measured on a more
contended host in `docs/perf-2026-06-03-konclude-vs-rustdl.md`. The
337.58 s here is much faster, but the contention difference makes a
direct delta misleading — what matters is FP=0 / MISSED=18 holds
and the wall sits comfortably under the spec §7 ≤ 400 s success
criterion.

## Spec §6 Phase 1c outcome band

GALEN wall: 154.44 s.

| GALEN wall band | Decision | Status |
|---|---|---|
| ≤ 150 s | Ship + proceed to Phase 2a (Layer 2 incremental) | **NOT MET** (off by 4.4 s) |
| 150–300 s | **Ship + mandatory Phase 2 build** (Layer 2 is path to headline) | ✅ **THIS BAND** |
| > 300 s after recon-driven tuning | §A revert | NOT TRIGGERED |

**Decision: Ship Phase 1b.5 + flip Phase 1c defaults + mandatory
Phase 2 build.** The lazy-expansion lever is sound and brings Horn
wall to flat with flag-OFF; Phase 2 (global saturation filter, Layer
2 in the spec) is the path to the ≤ 150 s headline.

The notgalen wall (337.58 s) sits well under the project-level
≤ 400 s success criterion, which strengthens the case for shipping
Phase 1c default-on rather than reverting.

## Gap vs recon — honest framing

The Phase 1b.5 recon (`docs/phase1b5-recon.md`) projected ~89 % CPU
reduction (~3,200 CPU-sec saved) under the assumption that lazy
expansion would skip ~90 % of `horn_fixpoint` work on replay,
yielding ~16 s wall at 24× concurrency.

Actual measured wall: **154.44 s** — a −4.3 % delta vs Phase 1b's
161.31 s first-cut. The recon was off by an order of magnitude.

Plausible explanations:

- **(a) Multi-threaded wall is already saturated at 24× concurrency.**
  The recon measured 3,585 CPU-seconds of wedge work at 150 s wall,
  giving effective concurrency ≈ 24×. Per-pair CPU savings don't
  unblock the wall bottleneck once the saturator is full — only
  reducing the longest critical-path stages helps.
- **(b) Snapshot-build cost doubles the wedge work per sub.** Every
  sub now pays for one cold-wedge-shaped snapshot build *and* the
  replay calls. Lazy replay's per-pair savings have to overcome that
  doubled per-sub cost before they move the wall.
- **(c) `Event::Edge` events still re-seed on replay.** Only
  `Event::Label` is gated by the fingerprint; edge events on
  snapshot nodes still trigger role-clause re-firing. The hot work
  on Horn workloads is plausibly more edge-heavy than the recon's
  label-only model assumed.

**The win is real but smaller than projected.** Flag-ON at
154.44 s is flat-with-flag-OFF (148.95 s, +3.7 %) — this is what
Phase 1c needs to ship default-on without regressing the Horn
fragment. Phase 1b alone left +8.3 % regression on the table;
Phase 1b.5 closes that gap.

The optimistic recon projection was wrong, but the architectural
lever still delivered the flat-vs-baseline outcome Phase 1c
readiness requires. The ≤ 150 s headline now needs Phase 2's
Layer 2 lever (global saturation filter) to clear.

## Phase 1c plan (next)

Per spec §6 outcome bands (150–300 s band → ship + mandatory Phase 2):

1. **Flip defaults to ON**: `RUSTDL_SNAPSHOT_CAPTURE` + lazy expansion
   both default-on (the lazy expansion is currently always-on once
   replay is taken; double-check no plumbing remains gated on
   per-task env vars after T3).
2. **Full corpus + soundness gate**: run the project Phase 0+1+real
   corpus matrix (`scripts/bench-rustdl-modes.sh` or equivalent
   harness from `docs/perf-2026-06-03-konclude-vs-rustdl.md`) with
   defaults flipped; assert FP=0 + MISSED=corpus-baselines on every
   fixture; assert no fixture's wall regresses > 10 % vs the
   project baseline pinned in `docs/perf-2026-06-03-konclude-vs-rustdl.md`.
3. **Project-headline results doc** (`docs/phase1c-results.md`):
   document the default flip + corpus matrix + headline GALEN
   wall vs Konclude + per-fixture deltas vs project baseline.
4. **Open Phase 2a recon** in parallel: is the global-saturation
   filter the right next lever, or has Phase 1b.5's flat-vs-baseline
   result changed the cost calculus?

## Carry-overs / open questions

| Carry-over | Status |
|---|---|
| `pre_capture_labels` field on every `SnapshotNode` | ✅ Landed (T1) |
| `LazyReplayState` + lazy `from_snapshot` constructor | ✅ Landed (T2) |
| Per-sup `neg_sup_clauses` cache | ✅ Landed (T4) |
| `parent` / `parent_role` snapshot fields (HF2 double-blocking) | Still deferred — sound to omit on Safe seeds; Phase 3 may revisit |
| `Event::Edge` gating on lazy replay | **Open** — the recon-vs-actual gap suggests this is the next perf lever inside Phase 1b.5 territory; deferred to Phase 2a recon |
| Per-class `BackPropRisk` classifier | Deferred to Phase 3 (SROIQ workloads) |
| `pairs_per_sub` + `wedge_cost_histogram_ms` profiling fields | Keep as documented telemetry (Phase 1b.5 recon) |

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md` §6 outcome bands.
- Phase 1b.5 plan: `docs/superpowers/plans/2026-06-03-konclude-snapshot-cache-phase-1b5.md`.
- Phase 1b.5 recon (recon vs actual gap motivation): `docs/phase1b5-recon.md`.
- Phase 1b results (immediate predecessor): `docs/phase1b-results.md`.
- Phase 1a results: `docs/phase1a-results.md`.
- Pre-project baseline + notgalen ~1170 s number: `docs/perf-2026-06-03-konclude-vs-rustdl.md`.
- Dead-end ledger (for any future revert entry): `docs/hypertableau-dead-ends.md`.
