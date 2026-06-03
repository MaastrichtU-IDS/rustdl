# Konclude snapshot cache project — completion handoff

Project end: 2026-06-03 at HEAD `b4ddb5d`. Closing the project at
Phase 2b per Phase 3a recon's NO-GO (per-class refinement projected
insufficient to close spec §6 ore-15672 target alone; dead-end §19).

This doc summarizes the full arc + provides the pickup state for
any future revisit.

---

## Headline metrics

| Workload | Pre-project | Post-project | Speedup |
|---|---:|---:|---:|
| **GALEN** (2748 cls, Horn) | 149 s | **0.40 s** | **400×** |
| **notgalen** (3087 cls, Horn) | 1170 s (pre-project contended host) | **0.69 s** | ~**1700×** |
| **alehif** (167 cls, Horn) | 2.28 s | **0.01 s** | **228×** |
| ore-10908 (692 cls, SROIQ) | 7.48 s | 5.28 s | 1.4× |
| ore-15672 (82 cls, SROIQ) | 30.15 s | 29.12 s | 1.0× (flat) |
| pizza (99 cls, SROIQ) | 4.10 s | 3.47 s | 1.2× |

**Konclude ratios** (rustdl wall / Konclude wall):

| Workload | Pre-project ratio | Post-project ratio | Status |
|---|---:|---:|---|
| ore-10908 | 4.32× | **3.05×** | **Project's named target (≤5×) MET** |
| GALEN | 219× | **<10×** (0.40s vs Konclude 44ms reasoning + docker startup) | Approaches Konclude parity ex-docker |
| notgalen | 530× | **<20×** | Similar near-parity |
| alehif | 1.08× | <1× | Already under Konclude |
| pizza | 2.34× | 2.04× | Stable |
| ore-15672 | 17.2× | 16.6× | **Spec §6 Phase 3 target (≤10×) NOT MET** (dead-end §19) |

---

## What shipped (4 phases + 2 recons across 15 commits)

| Phase | Commits | Date | Headline |
|---|---:|---|---|
| Phase 1a | 5 (`26a2a57..89650c1`) | 2026-06-03 | Snapshot types + capture + Phase 0 canary; **no behavior change** |
| Phase 1b | 9 (`610ce86..cb47751`) | 2026-06-03 | Replay driver + sentinel + cache + orchestrator wiring (`RUSTDL_SNAPSHOT_CAPTURE` env, default OFF); +8% GALEN wall |
| Phase 1b.5 recon | 3 (`4675f54..13b930c`) | 2026-06-03 | Pairs-per-sub instrumentation; GO on lazy expansion |
| Phase 1b.5 | 5 (`9f3e568..5640c30`) | 2026-06-03 | Lazy expansion + per-sup `neg_sup_clauses` caching (`RUSTDL_SNAPSHOT_LAZY` env, default ON); flat-vs-baseline |
| Phase 1c | 2 (`5db819a..1090595`) | 2026-06-03 | Default-on shipped; spec §7 acceptance MET |
| Phase 2a recon | 3 (`ba0820e..e5f0519`) | 2026-06-03 | Wall-breakdown instrumentation; empirical kicker (`--saturation-only` GALEN at 0.48s); GO with reframing |
| Phase 2b | 2 (`ba07b4e..b0b23c1`) | 2026-06-03 | **Horn fragment short-circuit (`RUSTDL_HORN_SHORTCIRCUIT` env, default ON)** — the headline; 400-503× speedups on Horn |
| Phase 3a recon | 3 (`1e4fd08..b4ddb5d`) | 2026-06-03 | Per-class `BackPropRisk` instrumentation; **NO-GO** (dead-end §19); project closes here |

---

## What didn't ship (and why)

- **Phase 3 (per-class `BackPropRisk` loosening)**: recon validated
  the architectural lever exists (67-99% of SROIQ classes would be
  per-class Safe) but projected wall savings are bounded by per-pair
  tableau cost on hard classes (ore-15672's tier_walk is 96% of
  wall; snapshot cache doesn't help search-budget-exhausted
  tableau pairs per dead-end §18). Dead-end §19 captures the
  analysis. Could be revisited if a future workload makes SROIQ
  wall a customer-visible priority.

- **Spec §5 Layer 2 (TBox-wide saturation candidate filter)**:
  Phase 2a recon caught spec §5's misframing — label-cache-build
  was 0.2% of wall, not the ~30% the spec hypothesized. The
  empirical kicker (`--saturation-only` on GALEN at 0.48s) showed
  the existing infrastructure already handled Horn workloads if we
  just dispatched to it. Phase 2b is "Layer 2 reframed" — used
  existing components instead of building new ones.

---

## Soundness contract additions

The project added 3 env defaults to the CLAUDE.md soundness contract:

- `RUSTDL_SNAPSHOT_CAPTURE` (Phase 1c, default ON): per-class snapshot
  cache ahead of wedge for `BackPropRisk::Safe` ontologies. Set `=0`
  to revert to pre-project pure-wedge.
- `RUSTDL_SNAPSHOT_LAZY` (Phase 1b.5, default ON): lazy expansion
  in snapshot replay. Set `=0` to revert to Phase 1b full-re-run.
- `RUSTDL_HORN_SHORTCIRCUIT` (Phase 2b, default ON): for Horn
  fragment ontologies, dispatch to saturation fast path instead of
  per-pair loop. Set `=0` to revert to Phase 1c per-pair loop on
  Horn.

All defaults ship "ON" with explicit opt-out for A/B isolation.

---

## Soundness throughout

FP=0 / MISSED=baseline preserved across **every phase** on the soundness
gate fixtures (alehif, ore-10908, ore-15672, GALEN, notgalen). The
project caught two real soundness bugs mid-flight:

1. **Phase 1b T4 commit `d3c5598`**: snapshot counters not merged
   through parallel classify shards (telemetry-only; not a
   correctness bug, but would have hidden later soundness issues
   if not caught at T5 canary).
2. **Phase 1b T6 commit `a1f81ff`**: `¬sup` encoded as global
   `sup(x) → ⊥` instead of root-scoped `fresh_q ⊓ sup → ⊥`.
   Caught by the GALEN soundness gate (25,333 FPs on first run);
   fixed via the wedge's existing `fresh_q` injection pattern.
   This is the second time GALEN-the-soundness-net caught a class
   of bug that synthetic canaries missed (mirrors dead-end §4 from
   pre-project history).

Both bugs landed as separate fix-up commits with explicit attribution
in the commit message. The "every soundness-touching task gets a
GALEN gate before closure" discipline (instituted post-Phase-1b T6)
is now standing project practice.

---

## Recon-first discipline

Two recons in this project; both prevented misallocated work:

- **Phase 1b.5 recon**: projected ~89% CPU reduction from lazy
  expansion; actual ~7% wall savings. The recon was off by an order
  of magnitude. Cause: projection from component-level CPU cost
  assumptions without end-to-end empirical baseline. Lesson: future
  recons should include a 1-pair micro-spike for wall-vs-CPU
  validation before committing.

- **Phase 2a recon**: caught spec §5's misframing (label-cache-build
  is 0.2% of wall, not 30%). Empirical kicker (`--saturation-only`
  at 0.48s on GALEN) validated the alternative architecture before
  spec-following would have built a multi-month new component.
  Lesson: end-to-end empirical baselines are worth more than
  component-level CPU breakdowns.

The combined recon outcomes: Phase 1b.5 didn't deliver the headline
the recon promised, but Phase 2a did. Net: the discipline saved at
least one multi-month wasted implementation (Phase 2 as originally
spec'd).

---

## Carry-overs for future work

If a future session revisits this area, these are the queued items:

### High-impact open work
- **Phase 3 implementation** (if a customer-visible SROIQ wall
  becomes priority): per-class `BackPropRisk::classify_class`
  exists (commit `a6983ed`); wire it into `SnapshotCache::try_replay`.
  Projected ~10% wall savings on ore-15672 alone; would NOT close
  spec §6 ≤17.5s target (dead-end §19). Useful only if combined
  with Konclude-style sub-tableau caching (§2 territory) to address
  the hard-class cluster.

- **Konclude-style sub-tableau caching** (dead-end §2 + §19): the
  structurally-different lever for SROIQ workloads. Multi-month;
  requires uncertain-benefit measurement first. Not currently scoped.

### Code-cleanup carry-overs
- Rename `classify_pure_el` → `classify_via_saturation_closure`.
  After Phase 2b widened it to also handle Horn, the name is
  misleading.
- Rename `ClassificationStats::pure_el_mode: bool` → something like
  `saturation_only_dispatch: bool`. Same issue.
- `Event::Edge` lazy gating in `horn_fixpoint` (Phase 1b.5 deferred):
  only `Event::Label` is gated by the lazy guard today. Could yield
  further snapshot-replay wall savings on workloads where the
  snapshot path actually runs (i.e., post-Phase-3 only).
- Snapshot `parent`/`parent_role` capture (Phase 1b T1 deferred):
  HF2 double-blocking restoration on snapshot-seeded nodes. Useful
  only for SROIQ workloads that exercise the snapshot path
  (post-Phase-3 only).
- CLI banner `snapshot_replay_used` telemetry (Phase 1c T2 reviewer
  note): never implemented, but Phase 3a instrumentation kept
  similar fields. Low priority.

---

## Pre-project vs post-project comparison

| Metric | Pre-project | Post-project |
|---|---|---|
| Snapshot infrastructure | none | sound, tested, default-on, A/B-toggleable |
| Horn workload wall (GALEN) | 149 s | 0.40 s (400× speedup) |
| Horn workload wall (notgalen) | ~1170 s (contended host) | 0.69 s (~1700×) |
| SROIQ workload walls | various | unchanged (snapshot doesn't engage on Unsafe ontologies) |
| ore-10908 Konclude ratio | 4.32× | 3.05× (under named ≤5× target) |
| Number of env defaults | (project-pre defaults) | +3 (`RUSTDL_SNAPSHOT_CAPTURE`/`_LAZY`/`HORN_SHORTCIRCUIT`) |
| Soundness gates run | manual ad-hoc | GALEN + notgalen + Phase 0 net at every soundness-touching commit |
| Dead-end ledger entries | §1-§18 | +§19 |

---

## Files committed by this project

`docs/superpowers/specs/`:
- `2026-06-03-konclude-style-global-classification-design.md` — project-level spec

`docs/superpowers/plans/`:
- `2026-06-03-konclude-snapshot-cache-phase-1a.md`
- `2026-06-03-konclude-snapshot-cache-phase-1b.md`
- `2026-06-03-konclude-snapshot-cache-phase-1b5-recon.md`
- `2026-06-03-konclude-snapshot-cache-phase-1b5.md`
- `2026-06-03-konclude-snapshot-cache-phase-1c.md`
- `2026-06-03-konclude-snapshot-cache-phase-2a-recon.md`
- `2026-06-03-konclude-snapshot-cache-phase-2b.md`
- `2026-06-03-konclude-snapshot-cache-phase-3a-recon.md`

`docs/`:
- `phase1a-results.md` / `phase1b-results.md` / `phase1b5-results.md` / `phase1c-results.md` / `phase2a-recon.md` / `phase2b-snapshot-results.md` / `phase3a-recon.md`
- `handoff-2026-06-03-snapshot-cache-project-complete.md` (this doc)
- `hypertableau-dead-ends.md` extended with §19

Code:
- `crates/owl-dl-tableau/src/snapshot.rs` — types + classifier (Phase 1a/1b/1b.5/3a)
- `crates/owl-dl-tableau/src/replay.rs` — replay driver + variants (Phase 1b/1b.5)
- `crates/owl-dl-tableau/src/hyper.rs` — `from_snapshot` + `from_snapshot_lazy` + sentinel hooks (Phase 1b/1b.5)
- `crates/owl-dl-reasoner/src/lib.rs` — `SnapshotCache`, env helpers, per-class counter (Phase 1b/1b.5/2b/3a)
- `crates/owl-dl-reasoner/src/classify.rs` — orchestrator wiring + counters (Phase 1b/1b.5/2a/2b/3a)
- `crates/owl-dl-cli/src/main.rs` — diagnostic banners (Phase 1b/2a/3a)
- `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs` — canary harness (Phase 0/1b/1c/2b)
- Plus tableau unit tests under `crates/owl-dl-tableau/tests/`

---

## Re-run commands

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH

# Full reasoner test suite (~30s):
cargo test -p owl-dl-reasoner

# Phase 0 net soundness gate (default-on; ~35s):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude \
    ore_10908_sroiq ore_15672_shoin

# GALEN soundness gate (default-on, ~1s post-Phase-2b):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture

# notgalen soundness gate (default-on, ~1s post-Phase-2b):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    notgalen_closure_matches_konclude -- --exact --ignored --nocapture

# A/B revert to pre-project behavior (Horn workloads):
RUSTDL_HORN_SHORTCIRCUIT=0 ./target/release/rustdl classify --pair-timeout-ms 200 \
    ontologies/external/galen.ofn

# Diagnostic CLI banner now shows: classes, fragment, subsumption,
# label heuristic, wall breakdown (Phase 2a), per-class BackPropRisk
# (Phase 3a). Useful for profiling future workloads.
```

---

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`
- Pre-project baseline: `docs/perf-2026-06-03-konclude-vs-rustdl.md`
- Project handoff (this doc): `docs/handoff-2026-06-03-snapshot-cache-project-complete.md`
- Engine handoff (prior session): `docs/handoff-2026-06-03.md`
- Dead-end ledger: `docs/hypertableau-dead-ends.md` (§19 added by this project)

---

## Net assessment

**Project shipped.** The named target (ore-10908 ≤ 5× Konclude) was
already met at Phase 8 before this project started; this project
delivered the Horn-fragment headline speedups (400-1700× on
GALEN/notgalen). The architectural infrastructure for SROIQ
acceleration is in place and tested — Phase 3a recon validated the
per-class lever exists but won't close the spec §6 ore-15672 target
alone (dead-end §19 captures why). Future SROIQ work, if scoped,
has the components ready.

The recon-first discipline (Phase 1b.5 + Phase 2a recons) prevented
at least one multi-month wasted implementation. Phase 1b.5 recon was
wrong about magnitude but enabled the Phase 1c shipping; Phase 2a
recon caught spec §5's misframing and reframed Phase 2 from
multi-month to 2 commits.

Closing at Phase 2b is the right call: shipping target met, headline
delivered, infrastructure ready for revisit if SROIQ priorities
shift.
