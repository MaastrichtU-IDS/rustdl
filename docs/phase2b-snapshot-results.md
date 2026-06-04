# Phase 2b — Horn fragment short-circuit shipped (project-headline)

> **Filename note.** The snapshot-cache project's "Phase 2b" collides
> with the earlier saturation "Phase 2b" (compound existential-body
> fix, commit `022ca50` — see `docs/phase2b-results.md`). To avoid
> overwriting that doc, this results file is named
> `phase2b-snapshot-results.md`.

Run 2026-06-03 at HEAD `ba07b4e` (T1: widen shortcircuit + env gate +
canary fix). T2 measured the full corpus matrix at both
`RUSTDL_HORN_SHORTCIRCUIT=0` (Phase 1c baseline) and `=1` (Phase 2b
default-on), plus GALEN + notgalen via the soundness-gated
closure-diff harness in both modes. This commit (T2) is the
project-headline results doc + CLAUDE.md defaults bullet.

Phase 2b widens the existing pure-EL short-circuit
(`classify_pure_el` — the saturation-only fast path) to also dispatch
ontologies that `analyze_fragment` classifies as `Horn`. The hyper
Horn fixpoint is complete on Horn workloads, so the saturation closure
IS the full classification — no per-pair verification required.

**Spec §7 acceptance: MET.** **Spec §6 Phase 2b outcome band: GALEN
0.40 s — FAR below the ≤ 150 s "Ship + proceed to Phase 2a" door.**
Phase 2b IS Phase 2 (Layer 2 from the spec); the Phase 2a recon
re-framed it as a tiny widening of the existing fast-path dispatch
instead of a multi-month new component.

## Headline

- **GALEN classify (closure-diff harness): 161.95 s → 0.40 s — ~405×
  speedup.** FP=0 / MISSED=0; closure 27,997 = Konclude. Sound by
  composition (hyper Horn fixpoint complete on Horn).
- **notgalen classify (closure-diff harness): 366.25 s → 0.69 s —
  ~531× speedup.** FP=0 / MISSED=18 (matches Phase 7 / Phase 1c
  baseline; the 18 are pre-project dl-approximation artifacts
  unrelated to this project).
- **alehif-test classify (CLI matrix): 1.63 s → 0.09 s — ~18×
  speedup.** Closure 247 = Konclude on the HORN_ON dispatch (the
  HORN_OFF banner reports `saturation=193 tableau=0` because the
  per-pair loop short-circuits on the wedge before the full closure
  is materialised; both modes pass `konclude_closure_diff`).
- **No out-of-EL fixture regressed** — all sat in run-to-run noise
  (largest delta: sio-fp2 0.44 s → 0.43 s, −2.27 %; ore-15672
  +0.14 %; ore-10908 −0.57 %). Phase 2b doesn't touch the out-of-EL
  dispatch path at all.
- **No fixture moved on FP / MISSED** — all soundness gates clean
  across both modes.

## Per-fixture measurement matrix

T2 ran each small fixture twice (HORN_OFF, HORN_ON) via the classify
CLI with `--pair-timeout-ms 200`. GALEN and notgalen were measured via
`konclude_closure_diff.rs`.

Sorted by ascending class count for readability.

| Fixture | Classes | Fragment | HORN_OFF wall | HORN_ON wall | Δ % | FP / MISSED |
|---|---:|---|---:|---:|---:|---|
| anch-module | 12 | out-of-EL | 0.00 s | 0.00 s | flat | n/a (sub-frame noise) |
| sulo-stripped | 17 | out-of-EL | 0.01 s | 0.01 s | flat | n/a |
| asp-module | 20 | out-of-EL | 0.00 s | 0.00 s | flat | n/a |
| np-module | 34 | out-of-EL | 1.67 s | 1.67 s | flat | n/a |
| ro-stripped | 58 | out-of-EL | 0.50 s | 0.50 s | flat | n/a |
| family-stripped | 58 | out-of-EL | 27.53 s | 27.41 s | −0.44 % | n/a |
| sio-fp2-module | 74 | out-of-EL | 0.44 s | 0.43 s | −2.27 % | n/a |
| shoiq-knowledge | — | — | 0.04 s | 0.05 s | noise | n/a |
| ore-15672-shoin | 82 | out-of-EL | 29.08 s | 29.12 s | +0.14 % | FP=0 / MISSED=0 (Phase 0 net) |
| ore-15516-alchoiq | 84 | out-of-EL | 0.17 s | 0.18 s | flat | n/a |
| pizza | 99 | out-of-EL | 3.47 s | 3.47 s | flat | n/a |
| **alehif-test** | **167** | **Horn** | **1.63 s** | **0.09 s** | **−94.5 % (~18×)** | **FP=0 / MISSED=0 (Phase 0 net)** |
| ore-10908-sroiq | 692 | out-of-EL | 5.27 s | 5.24 s | −0.57 % | FP=0 / MISSED=0 (Phase 0 net) |
| **GALEN** | **(closure-diff harness)** | **Horn (load-bearing)** | **161.95 s** | **0.40 s** | **−99.75 % (~405×)** | **FP=0 / MISSED=0** (closure 27,997 = Konclude) |
| **notgalen** | **(closure-diff harness)** | **Horn** | **366.25 s** | **0.69 s** | **−99.81 % (~531×)** | **FP=0 / MISSED=18** (Phase 7 baseline; pre-project) |

Logs: `/tmp/p2b/matrix.log` (small fixtures), `/tmp/p2b/galen-horn-on.log`,
`/tmp/p2b/galen-horn-off.log`, `/tmp/p2b/notgalen-default.log`,
`/tmp/p2b/notgalen-baseline.log`.

## Spec §7 acceptance verification

- ✓ **FP=0 + MISSED=baseline** on every measured fixture, in both
  modes. GALEN closure 27,997 = Konclude (FP=0/MISSED=0); notgalen
  MISSED=18 matches Phase 7 / Phase 1c baseline; Phase 0 net
  (alehif, ORE-10908, ORE-15672) FP=0/MISSED=0.
- ✓ **Phase 0 canary 4/4 pass** — Phase 2b T1 included the canary
  extension that disables `RUSTDL_HORN_SHORTCIRCUIT` in the two
  snapshot-path-firing tests so they continue to exercise the cache
  hot path.
- ✓ **Reasoner-crate clippy clean for changed code** (pre-existing
  saturation errors are out of scope, per the Phase 1b precedent).
- ✓ **No fixture regressed > 10 %** vs the Phase 1c baseline. The
  out-of-EL fixtures are untouched by Phase 2b (largest delta:
  sio-fp2 −2.27 %; all walls within run-to-run noise). The Horn
  fixtures are massively improved, not regressed.

## Spec §6 outcome-band attribution

GALEN wall under Phase 2b default: **0.40 s** (closure-diff harness).

| GALEN wall band | Decision | Status |
|---|---|---|
| ≤ 150 s | Ship + proceed to Phase 2a (Layer 2 incremental) | ✅ **MET — far below the door** (0.40 s vs 150 s gate, 375× headroom) |
| 150–300 s | Ship + mandatory Phase 2 build (Layer 2 is path to headline) | n/a (we are below this band) |
| > 300 s after recon-driven tuning | §A revert | NOT TRIGGERED (far from firing) |

Phase 2b IS the Phase 2 build (Layer 2 from the spec). The Phase 2a
recon re-framed it: spec §5's original hypothesis ("label-cache build
is ~30 % of wall") was invalidated by measurement (0.2 % on GALEN),
and the empirical kicker was that `rustdl classify --saturation-only`
on GALEN already produced the full Konclude-matching closure in
0.48 s — the existing sound-but-incomplete fast path IS already
complete on Horn ontologies; the orchestrator just hadn't been
dispatching it for them. Phase 2b was a tiny widening of that
dispatch decision.

## Project arc summary

| Phase | Headline | GALEN wall | notgalen wall | FP / MISSED (GALEN / notgalen) | Commit |
|---|---|---:|---:|---|---|
| Pre-project (`docs/perf-2026-06-03-konclude-vs-rustdl.md`) | wedge-only baseline | ~149 s (flag OFF, this host) | ~1170 s (more-contended host) | 0/0 / 0/18 | — |
| Phase 0 + 1a (`docs/phase1a-results.md`) | snapshot types + capture + canary; flag default OFF | 148.95 s | — | 0/0 / 0/18 | `143d017`, `b66c9a0` |
| Phase 1b (`docs/phase1b-results.md`) | replay driver + sentinel + cache + orchestrator wiring | 161.31 s (+8.3 % vs OFF, flag-ON only) | — | 0/0 / — | `610ce86` (+lineage) |
| Phase 1b.5 (`docs/phase1b5-results.md`) | lazy expansion + per-sup cache; Horn wall back to flat-vs-OFF | 154.44 s | 337.58 s | 0/0 / 0/18 | `a368746` (+lineage) |
| Phase 1c (`docs/phase1c-results.md`) | snapshot default-on shipped; corpus matrix clean | 153.70 s | 342.15 s | 0/0 / 0/18 | `5db819a` |
| Phase 2a recon (`docs/phase2a-recon.md`) | spec §5 invalidated by measurement; `--saturation-only` already complete on Horn → Phase 2 re-framed as Horn-fragment shortcircuit | — (recon only) | — | — | `b907daf`, `e5f0519` |
| **Phase 2b (this doc)** | **Horn shortcircuit shipped — 400×+ speedups on Horn workloads** | **0.40 s** | **0.69 s** | **0/0 / 0/18** | `ba07b4e` + this commit |

## Honest framing — recon vs actual

The Phase 2a recon projected a ~325× GALEN speedup based on the
`rustdl classify --saturation-only` empirical baseline (0.48 s on a
fresh run). Actual GALEN closure-diff at Phase 2b default-on:
**0.40 s** — slightly faster than projected (closure-diff overhead
turned out to be marginal).

In contrast to the Phase 1b.5 recon (which underestimated the CPU-cost
cliff by an order of magnitude), the Phase 2a recon **landed within
the noise band**. The structural difference: the Phase 2a recon had
an empirical kicker (`--saturation-only` already worked end-to-end on
GALEN), so it was extrapolating from a real run rather than from
CPU-cost assumptions. Phase 1b.5 was projecting from instrumented
component costs and got the integration overhead wrong.

Lesson for future recons: an end-to-end empirical baseline is worth
more than a component-level CPU breakdown, even when the latter is
more thoroughly measured.

## Commits this phase

- T1: `ba07b4e` — widen shortcircuit (Horn dispatch in addition to
  PureEl) + `RUSTDL_HORN_SHORTCIRCUIT` env gate + Phase 0 canary
  extension to disable shortcircuit in the two snapshot-path-firing
  tests.
- T2: *(this commit — full corpus matrix + results doc + CLAUDE.md
  defaults bullet)*.

> **Note (2026-06-04 audit):** Phase 3 was CANCELLED. Phase 3a-recon
> (`docs/phase3a-recon.md`) returned NO-GO on the per-class
> `BackPropRisk` refinement (dead-end §19). The snapshot-cache project
> closed at Phase 2b. The rename TODOs below were not actioned;
> `classify_pure_el` and `pure_el_mode` are still the live names in the
> codebase. Block kept as historical record of what was queued.

## Carry-overs / open items for Phase 3

| Carry-over | Status / disposition |
|---|---|
| Phase 3: loosen `BackPropRisk` classifier for SROIQ workloads | **Green-lit next work.** Snapshot infrastructure shipped Phase 1b/1b.5/1c is the SROIQ safety net but hasn't delivered big wins on those workloads yet. Targets: ore-15672 (29 s), pizza (3.47 s), ore-10908 (5.24 s), family-stripped (27.4 s). |
| Rename `classify_pure_el` → `classify_via_saturation_closure` | Code-cleanup carry-over from the Phase 2a recon doc. The fast path now dispatches for both `PureEl` and `Horn` fragments, so the name is misleading. Schedule for Phase 3 housekeeping. |
| `ClassificationStats::pure_el_mode: bool` field name | Same — now set on Horn dispatch too. Rename to `saturation_only_dispatch` or `fast_path_used`. Phase 3 housekeeping. |
| `Event::Edge` gating on lazy replay | Phase 1b.5 recon-vs-actual gap suggests this is a remaining lever inside snapshot-cache territory; revisit when SROIQ workloads bring back snapshot-cache hot path. |
| `parent` / `parent_role` snapshot fields (HF2 double-blocking) | Still deferred — sound to omit on Safe seeds. Phase 3 may revisit when SROIQ workloads come in scope. |
| `snapshot_replay_used` not in CLI banner | Low-priority telemetry polish; Phase 1c reviewer follow-up, still open. |
| Results-doc filename collision (`docs/phase2b-results.md` already used by saturation Phase 2b — commit `022ca50`) | Resolved by naming this doc `phase2b-snapshot-results.md`. Future phases of the snapshot-cache project should adopt the same `-snapshot-` infix if they collide with other Phase-N labels. |

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md` (§5 + §6 outcome bands, §7 acceptance).
- Phase 2b plan: `docs/superpowers/plans/2026-06-03-konclude-snapshot-cache-phase-2b.md`.
- Phase 2a recon (the re-framing): `docs/phase2a-recon.md`.
- Phase 1c results (immediate predecessor): `docs/phase1c-results.md`.
- Phase 1b.5 results: `docs/phase1b5-results.md`.
- Phase 1b results: `docs/phase1b-results.md`.
- Phase 1a results (Phase 0 canary + snapshot capture): `docs/phase1a-results.md`.
- Pre-project baseline: `docs/perf-2026-06-03-konclude-vs-rustdl.md`.
- Soundness contract + new defaults: `CLAUDE.md` (`## Soundness contract` section).
- Saturation Phase 2b (different project, name-colliding): `docs/phase2b-results.md`.
- Dead-end ledger (for any future revert entry): `docs/hypertableau-dead-ends.md`.
