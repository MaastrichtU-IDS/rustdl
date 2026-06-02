# Phase 6 post-fix SIO flame — plateau territory

Re-flamegraphed 2026-06-02 against HEAD `1bfed6d` (Phase 6, walk dedup).
Sampling: pprof-rs @ 199 Hz, 60 s window on `ontologies/real/sio-fp2-module.ofn`.
Archived at `docs/flamegraphs/sio-classify-2026-06-02-post-phase6.svg`.

## Top hot frames post-Phase-6

| Frame | % | Previously addressed |
|---|---|---|
| `apply_role_rules` (top variant) | 16.06% | Phase 3e attempt **reverted** — workload-dependent break-even (§16). |
| `apply_max` (top variant) | 11.73% | Phase 3f recon **killed at recon** — irreducible probe cost (§17). |
| `apply_role_chains` (top variant) | 7.60% | Already had one HashMap-pending pass; further wins likely small. |
| `apply_role_rules` (2nd) | 6.99% | (same as above) |
| `apply_concept_rules` (top) | 6.27% | Not previously attacked. |
| `first_clash` | 5.17% | Tableau back-jumping; in core hot loop. |
| `apply_self_restriction` (top) | 4.15% | Not previously attacked. |
| `apply_forall` | 4.03% | Not previously attacked. |
| `are_declared_inverses` | 3.44% | Phase 3b already O(1) HashSet — irreducible. |
| `is_sub_role` | 3.27% | Phase 3b indirect target — irreducible. |
| `apply_exists` | 3.21% | Not previously attacked. |
| `apply_deferred_concept_or_rules` | 3.11% | Phase 3d already hoisted — irreducible. |

**`find_direct_parents_top_down` is gone from the top frames** —
Phase 6's dedup eliminated it from the hot region on SIO. (On GALEN
it was 97.2% pre-Phase-6; the wall reduction confirmed the eliminate
there too.)

## Interpretation

The top three frames (`apply_role_rules` 16.06% + `apply_max` 11.73%
+ `apply_role_chains` 7.60% = 35.4%) are exactly the regions Phases
3e/3f either reverted or killed-at-recon. The dead-end ledger §16+§17
documents why surgical attacks here regress GALEN's edge-heavy /
rule-thin workload pattern.

The next un-attacked candidate is `apply_concept_rules` at 6.27%, which
also lives in the tableau's per-node rule dispatch — likely same
shape as `apply_role_rules` per §16 (HashMap-based dispatch with
workload-dependent break-even). Risky bet.

`first_clash` at 5.17% is core back-jumping logic — touching it would
need a different analytical angle than the perf cycles to date.

## Phase 6 is the natural close of the cheap-perf chapter

The cheap perf wins this session ran 3a → 3b → 3c → 3d → 6 — each
finding a leaf-frame O(n) → O(1) swap or a redundant-work elimination.
That trail's dry: every remaining top frame either has a dead-end
ledger entry warning against re-attempt, or has already been
hoisted/cached as a Phase target, or is irreducible probe cost.

Further perf would need a structurally different angle: workload-
adaptive dispatch (per dead-end §16's "Don't reattempt without first
solving workload-dependent dispatch"), or a redesign of how the
tableau wedge schedules rule firings, or moving more work into
the saturator (Phase 2d-style completeness extensions).

## Recommendation

**Stop the perf chapter on a high note.** Phase 6 delivered −9.3 % on
GALEN under contention (vs Phase 5 T3b probe baseline) while
preserving all completeness gains. The handoff doc captures the new
GALEN baseline and the dead-end ledger captures the constrained
design space for any future Phase 7+.

## Cross-references

- Phase 6 results (the win): `docs/phase6-results.md`.
- Phase 5 chain (the recon that led to Phase 6):
  `docs/phase5-{recon,walltime-probe,variance-check,downstream-probe}.md`.
- Dead-end ledger §16 (apply_role_rules workload-dependence):
  `docs/hypertableau-dead-ends.md`.
- Dead-end ledger §17 (apply_max irreducibility):
  `docs/hypertableau-dead-ends.md`.
- Archived flame: `docs/flamegraphs/sio-classify-2026-06-02-post-phase6.svg`.
