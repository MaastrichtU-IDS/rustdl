# SIO classify hot-path findings (Phase 3 prep, SIO confirmation)

Profiled 2026-06-01 against branch `plan/soundness-completeness-perf`
post-Phase-2b/2b.5 (commit 91edd2e). Sampling: pprof-rs @ 199Hz,
RUSTDL_PROFILE_SECONDS=60, on `ontologies/real/sio-stripped.ofn`.
Total samples: 14,544.

## Top hot frames

Unique frames by inclusive %, filtering out generic rayon/std thread infrastructure:

| Rank |       % | Samples | Frame                                                                 |
|------|--------:|--------:|-----------------------------------------------------------------------|
|  1   |  55.91% |   8,131 | `search` (tableau backtracking driver, `owl-dl-tableau/src/search.rs`) |
|  2   |  55.91% |   8,131 | `branch` (tableau branch step, same callsite as search — recursive)  |
|  3   |  55.46% |   8,066 | `saturate` (tableau rule-saturation sub-loop, `owl-dl-tableau/src/saturate.rs`) |
|  4   |  27.93% |   4,062 | `apply_max` (`rules.rs` max-cardinality rule)                        |
|  5   |  25.76% |   3,746 | `edge_satisfies` (role-edge predicate inside apply_max)              |
|  6   |  25.76% |   3,746 | `are_declared_inverses` (inverse-role check, `owl_dl_tableau::{impl#2}`) |
|  7   |  25.76% |   3,746 | `any<(RoleId, RoleId), owl_dl_tableau::{impl#2}::are_declared_inverses::{closure_...>` (linear scan in inverse check) |
|  8   |  22.23% |   3,233 | `eq` (leaf — `ConceptId` / `RoleId` comparisons inside apply_max + are_declared_inverses) |
|  9   |  21.87% |   3,181 | `apply_role_rules` (universal/role propagation, `rules.rs`)          |
| 10   |  18.54% |   2,696 | rayon thread start / idle (parallel pair-loop overhead)              |
| 11   |   9.58% |   1,394 | `apply_role_axioms`                                                   |
| 12   |   9.27% |   1,348 | `find_map` / `try_fold` (iterator chain inside apply_role_rules — linear scan over ConceptExprs) |
| 13   |   9.27% |   1,348 | `bot_id` (accessed inside the `find_map` loop — constant but not inlined) |
| 14   |   9.01% |   1,310 | `apply_self_restriction`                                              |
| 15   |   8.22% |   1,196 | `spec_extend` / `next` (heap allocation in apply_max / apply_role_rules) |
| 16   |   3.31% |     482 | `apply_deferred_concept_or_rules` (deferred-OR rule)                 |
| 17   |   1.19% |     173 | `apply_deferred_or_residuals`                                         |
| 18   |   1.10% |     160 | `from_iter<ConceptId, Filter<...>>` (DepSet clone inside deferred-OR)|
| 19   |   0.95% |     138 | `first_clash` (`saturate.rs:175`)                                     |
| 20   |   0.95% |     138 | `clash_deps_at`                                                       |

EL saturation crate (`owl_dl_saturation`): **0 samples (0.00%)** in the entire profile.
`apply_deferred_concept_or_rules`: 482 samples (3.31%) — present but a minor contributor.

## Spec verification

Per `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`
§"Phase 3", the spec named SIO as saturator-dominated. This flamegraph
**refutes** that claim. The hot path is tableau-dominated: tableau frames
(`search`/`branch`/`saturate`) account for ~55.9% inclusive; `apply_max`
alone at 27.9% plus `are_declared_inverses` at 25.8% represent the real
bottleneck. The EL saturation crate (`owl_dl_saturation`) has 0 samples
(0.00%) in the entire profile — SIO has the same tableau-dominated regime
as GALEN post-Phase-2b, not the saturator-dominated regime the spec anticipated.

Within the tableau hot path, SIO's dominant cost differs from GALEN: on GALEN
`apply_deferred_concept_or_rules` was 31.4% and `PartialEq::eq` inside
`needs_deferred_or` was 18.1%; on SIO those frames are only 3.3% and the
`eq` cost at 22.2% is attributable to `apply_max` + `are_declared_inverses`
instead. SIO's primary bottleneck is the cardinality rule (`apply_max` 27.9%)
together with the inverse-role linear scan inside `edge_satisfies`
(25.8%) — a pattern consistent with SIO's heavy use of functional properties
and role hierarchies.

## How this informs Phase 3

Both the GALEN flamegraph (tableau-dominated at 73% `search/saturate`, with
31.4% `apply_deferred_concept_or_rules`) and the SIO flamegraph (tableau-
dominated at 55.9%, with 27.9% `apply_max`) confirm that the spec's Phase 3
target of "attacking the EL saturator" is entirely wrong for the current
workloads. Phase 3 must target the tableau. The two ontologies suggest
different first fixes: for GALEN, `apply_deferred_concept_or_rules` /
`needs_deferred_or` is the top target (31.4%); for SIO, `apply_max` + the
linear scan in `are_declared_inverses` / `edge_satisfies` (25.8%) is dominant.
The spec's "Or-body regression at commit fddf2ee" is visible in the GALEN
flamegraph (31.4% `apply_deferred_concept_or_rules`) but not in the SIO one
(only 3.3%) — confirming that the Or-body regression is GALEN-specific, driven
by GALEN's larger absorbed TBox triggering more deferred-OR iterations.
Phase 3's first fix should therefore target `apply_deferred_concept_or_rules` /
`needs_deferred_or` to recover GALEN wall time, with a follow-on fix for
`apply_max`'s `are_declared_inverses` linear scan to recover SIO wall time.
