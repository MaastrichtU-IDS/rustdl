# Phase 3a recon — does per-class BackPropRisk refinement pay off on SROIQ?

Run 2026-06-03 at HEAD `a6983ed` (Phase 3a T1 instrumentation
landed). Recon decides whether Phase 3 (per-class BackPropRisk
loosening + runtime sentinel safety net) is worth the 3-5 sessions
of implementation, OR whether the project's natural endpoint is
Phase 2b.

## Headline

**NO-GO. Close project at Phase 2b.**

Per-class refinement is structurally sound — the recon validates
that 82-99% of SROIQ classes WOULD be Safe under per-class
classification, even though the ontology-wide classifier (Phase 1a)
currently flags them all Unsafe. But the projected wall savings
are **bounded by the per-pair tableau cost on hard classes** (which
snapshot cache doesn't address), so Phase 3 cannot deliver the
spec §6 target `ore-15672 ≤ 10× Konclude (~17.5s)`. Current
ore-15672 wall is 29s; projected post-Phase-3 wall is ~25-27s,
still above the spec target.

## Per-class Safe ratios (Phase 3a measurement)

| Fixture | Classes | Safe | Unsafe | Safe ratio | Current wall | tier_walk fraction |
|---|---:|---:|---:|---:|---:|---:|
| ore-15672-shoin | 82 | 67 | 15 | **82%** | 29.10s | 96% (28.0s) |
| ore-10908-sroiq | 692 | 683 | 9 | **99%** | 5.33s | not measured |
| ore-15516-alchoiq | 84 | 82 | 2 | **98%** | 0.18s | n/a (tiny) |
| alehif-test (Horn, control) | 167 | 167 | 0 | 100% | n/a (Phase 2b short-circuits) | n/a |

**The architectural lever exists.** Per-class refinement would unlock
snapshot cache on 67 of 82 ore-15672 classes (vs 0 today), and on
683 of 692 ore-10908 classes. The structural argument from spec §6
holds.

## Why Phase 3 still can't close the spec §6 ≤17.5s target

The dominant cost on ore-15672 is **tier_walk = 28,044 ms (96% of
wall)** per the Phase 2a recon instrumentation. tier_walk is the
top-down hierarchy walk plus per-pair wedge + per-pair tableau
calls. Snapshot cache replaces the WEDGE call ahead of the tableau,
not the tableau itself.

Per dead-end §18 (`docs/hypertableau-dead-ends.md`), ore-15672's
residual cost is **search-budget exhaustion on 3 hard classes** —
intrinsic tableau search, not rule-bound. Snapshot cache on Safe
classes does NOT reduce tableau cost on hard classes.

### ore-15672 projected savings under Phase 3

Best-case scenario:
- 67 Safe classes × ~30ms wedge-savings-per-pair × ~24 pairs/sub
  (per Phase 2a instrumentation: `pairs-per-sub median=24`) ÷ 24× concurrency
  ≈ **1-2 seconds wall savings**.
- Dead-end §18's 3 hard classes: unchanged (snapshot doesn't help
  search-budget exhaustion).

Projected ore-15672 wall after Phase 3: **~27 seconds**.
Spec §6 target: **≤ 17.5s**.
Gap: still ~10s. **Phase 3 insufficient.**

### ore-10908 (would Phase 3 help here?)

ore-10908 current: **5.33s, 3.05× Konclude** — already well under
the project's named target (≤ 5× Konclude, met at Phase 8). Phase 3
might shave ~0.5-1s off via snapshot reuse on the 99% Safe classes;
the absolute savings are small and the workload isn't a problem.

### Other SROIQ fixtures

- ore-15516-alchoiq: 0.18s wall — nothing to optimize.
- pizza, sio-fp2-module, family-stripped: not measured in this recon
  (fixtures not present in the local corpus), but their Phase 1c
  walls (3.47s / 0.43s / 27.41s) suggest similar bounded gains.

## Spec §7 vs §6 framing

**Spec §7 project-level acceptance** (already MET as of Phase 2b):
- FP=0 + MISSED=baseline everywhere ✓
- All tests pass ✓
- No fixture regressed > 10% ✓
- Phase 2b headline: GALEN 400×, notgalen 503×, alehif 600× speedups ✓

**Spec §6 Phase 3 acceptance** (the specific ore-15672 ≤ 17.5s
target): **NOT REACHABLE** via per-class refinement alone, per the
analysis above. Reaching it would require addressing the dead-end
§18 search-budget cluster, which requires Konclude-style
sub-tableau / multi-class-search architecture (not in scope for
the snapshot cache project).

## Recommendation

**Close the snapshot cache project at Phase 2b.**

Three reasons:

1. **The project shipped its named target**: ore-10908 ≤ 5×
   Konclude was met at Phase 8 (currently 3.05×). The snapshot
   cache infrastructure is operational and the headline wall
   improvements are massive on Horn workloads.

2. **Phase 3 can't deliver the spec §6 target on ore-15672**:
   per-class refinement would help, but the dominant cost is
   search-budget-bound (dead-end §18). Per-class snapshot reuse
   saves ~10% wall; spec target needs ~40% reduction.

3. **Remaining SROIQ workloads are at acceptable Konclude ratios**:
   ore-10908 3.05×, pizza 2.04×. No customer-visible problem to
   solve.

The architectural lever (per-class refinement + runtime sentinel)
remains AVAILABLE for future work if SROIQ wall becomes a priority
later — Phase 1b's `BackPropAborted` sentinel is wired and tested;
the per-class classifier from this recon (`BackPropRisk::classify_class`)
is in place. If a future use case needs to push SROIQ walls down,
the infrastructure is sitting there ready to be wired.

## Action items

1. Write **dead-end ledger §19 entry** documenting why per-class
   snapshot refinement doesn't close the spec §6 ore-15672 target
   alone. Captures the recon's measured Safe ratios + the
   architectural-vs-projected-wall analysis for future maintainers.
2. Write **project-completion handoff doc** summarizing the full
   arc (Phase 0+1a → 1b → 1b.5 → 1c → 2a recon → 2b → 3a recon →
   close).
3. Decide whether to **revert the Phase 3a instrumentation**
   (per-class counting) OR keep it as profiling telemetry for
   future use. Recommend keep — it's small, cheap, and documents
   the per-class structure for any future revisit.

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md` §6 Phase 3.
- Phase 2b results (project's main headline): `docs/phase2b-snapshot-results.md`.
- Phase 2a recon (validated tier_walk dominance): `docs/phase2a-recon.md`.
- Phase 1c results (project's stable headline): `docs/phase1c-results.md`.
- Dead-end ledger §18 (ore-15672 hard-class cluster): `docs/hypertableau-dead-ends.md`.
- Pre-project baseline: `docs/perf-2026-06-03-konclude-vs-rustdl.md`.

## What instrumentation kept

The `per_class_safe_count` / `per_class_unsafe_count` fields on
`ClassificationStats` + the `# per-class BackPropRisk:` banner
line (commit `a6983ed`) are useful profiling telemetry — keep them.
They document the per-class structure for any future revisit
without re-instrumenting. The classifier `BackPropRisk::classify_class`
remains available for any consumer.
