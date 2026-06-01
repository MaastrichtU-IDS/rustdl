# Phase 3e results — edge-keyed role-rule indexing (REVERTED)

Run 2026-06-01. See `docs/phase3e-fix-target.md` for the design and
`docs/phase3e-recon.md` for the recon.

## Headline

**SIO `apply_role_rules` top-frame attribution dropped 16.36% → 8.87%
(−7.49pp) — clear algorithmic win. GALEN wall regressed +2.34% across
two consecutive samples (739.71s baseline → 754.91s + 759.11s post-fix).
FP=0 + MISSED=17 unchanged. Reverted at commit a2a4d7f.**

The new HashMap-lookup overhead (four `HashMap<RoleId, Vec<_>>` indices
+ per-edge lookups) exceeded the saved `edge_satisfies` calls on
GALEN's edge-heavy / rule-thin pattern. SIO's rule-density profile is
the opposite — where the algorithm wins.

## Prediction vs measurement

| Dimension | Predicted (T3) | Measured (T5) |
|---|---|---|
| SIO `apply_role_rules` frame | 16.03% → 8-10% | **16.36% → 8.87%** ✓ |
| GALEN wall | −3 to −8% | **+2.34%** ✗ |
| FP / MISSED | unchanged | **unchanged** ✓ |

The recon-identified target (`edge_satisfies` per-edge-per-rule cost,
7.26%) was correctly removed from `apply_role_rules`. The wall didn't
follow because GALEN's workload is index-overhead-dominated, not
edge_satisfies-dominated.

## Soundness gate (Phase 0 net)

| Fixture | FP | MISSED | Wall |
|---|---|---|---|
| alehif-test | 0 | 0 | clean |
| ore-10908-sroiq | 0 | 0 | clean |
| ore-15672-shoin | 0 | 0 | clean |

(Total 39s for all three.)

## Wall lever (GALEN) — the two-sample evidence

| Sample | Wall | Δ vs baseline |
|---|---|---|
| Baseline (T2, pre-3e) | 12.33 min (739.71s) | — |
| Post-3e sample #1 | 12.58 min (754.91s) | +2.05% |
| Post-3e sample #2 | 12.65 min (759.11s) | +2.65% |
| **Mean post-3e** | **12.62 min** | **+2.34%** |

Two consecutive same-sign readings rule out the noise hypothesis.
Phase 3d's +14% on its first GALEN run had a contention explanation
(concurrent build); Phase 3e's regressions had no concurrent load
and held across re-measure.

## Flame delta (SIO, post-3d → post-3e)

| Frame | Post-3d | Post-3e | Δ |
|---|---|---|---|
| `apply_role_rules` (top variant) | 16.36% | **8.87%** | **−7.49pp** |
| `edge_satisfies` (summed across all callers) | ~10.65% | ~9.65% | −1.0pp (residual is from other callers like apply_max) |

The algorithm works where rule density is high. SIO is that workload.

## Why GALEN doesn't benefit

`apply_role_rules` cost = O(rules × matching_edges). The Phase 3e
fix replaces the per-rule-per-edge `edge_satisfies` call with a
per-edge HashMap lookup against the new role-keyed indices. Net wall
delta per call = saved_role_hierarchy_traversal − HashMap_lookup_cost.

- SIO: many rules per role → many edge_satisfies calls saved →
  HashMap overhead amortized → net win.
- GALEN: few rules per role, many edges per node → HashMap lookup
  cost per edge dominates the cheap-to-skip work → net loss.

## What's left for Phase 3f (if pursued)

The recon's analysis is reusable: the matching_edges × edge_satisfies
cost IS the inner cost. A future Phase 3f could:
- Gate the indexing on observed rule density per ontology (run-time
  workload-adaptive dispatch).
- Use a simpler index (single direction-aware HashMap) to reduce
  per-edge lookup cost, accepting more compute per hit.
- Restructure differently (cache matching_edges results per role
  with reset-on-edge-list-change semantics).

See `docs/hypertableau-dead-ends.md` §16 for the workload-dependence
that any reattempt must address.

## Cross-references

- Phase 3e plan: `docs/superpowers/plans/2026-06-01-phase3e-apply-role-rules-inner.md`
- Phase 3e recon: `docs/phase3e-recon.md`
- Phase 3e design: `docs/phase3e-fix-target.md`
- Reverted implementation: `89317e0` (full diff)
- Revert commit: `a2a4d7f`
- T5 measurement logs: `/tmp/p3e-{net,galen,galen-2}.log`, `/tmp/p3e-sio-flame.svg` (transient)
