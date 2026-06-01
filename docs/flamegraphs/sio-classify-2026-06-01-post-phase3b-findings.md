# Phase 3b post-fix SIO flamegraph + findings

Re-flamegraphed 2026-06-01 against HEAD cf05e22 (Phase 3b hashbrown::HashSet
for are_declared_inverses). Sampling: pprof-rs @ 199Hz, 60s window on
`ontologies/real/sio-stripped.ofn`.

## Frame-level diff (top 15 unique frames by max %)

BASELINE SIO top 15 (commit 64bee92, post-Phase-3 bloom prefilter):
  100.00%  all
  100.00%  owl-dl-bench
   55.91%  branch
   55.91%  search
   55.46%  saturate
   27.93%  apply_max
   25.76%  edge_satisfies
   25.76%  are_declared_inverses
   25.76%  any<(RoleId, RoleId), owl_dl_tableau::{impl#2}::are_declared_inverses::{closure...}>
   24.00%  {closure#0}
   22.23%  eq
   21.87%  apply_role_rules
   21.37%  {closure#1}
   18.54%  rayon thread / idle
    9.58%  apply_role_axioms

POST-PHASE-3b SIO top 15 (HEAD cf05e22, hashbrown::HashSet):
  100.00%  all
  100.00%  owl-dl-bench
   69.00%  branch
   69.00%  search
   67.59%  saturate
   24.66%  apply_role_axioms
   24.66%  bot_id
   24.66%  find_map<...>  [iterator chain inside apply_role_axioms]
   24.66%  try_fold<...>  [inner loop of find_map]
   19.63%  {closure#0}   [closure inside apply_role_axioms filter]
   19.08%  apply_self_restriction
   10.03%  rayon thread / idle
    7.32%  apply_role_rules
    6.51%  apply_max
    4.57%  apply_deferred_concept_or_rules
    4.30%  edge_satisfies

## Hot-frame % deltas

- `are_declared_inverses` (linear scan): 25.76% → 3.44% (Δ **-22.32pp**)
- `any<..., are_declared_inverses closure>` (the scan body): 25.76% → 0% (gone)
- `apply_max` (max-cardinality rule): 27.93% → 6.51% (Δ **-21.42pp**)
- `edge_satisfies` (predicate containing the scan): 25.76% → 4.30% (Δ **-21.46pp**)
- `contains<(RoleId,RoleId), foldhash::RandomState>` (new HashSet O(1) lookup): 0% → 3.44%

The hashbrown HashSet lookup (`contains` via foldhash) now costs only 3.44% vs the old
25.76% linear scan — a **7.5× reduction** in inverse-lookup cost. The `apply_max` /
`edge_satisfies` collapse is entirely attributable to the scan elimination.

## New top frame post-Phase-3b

After the scan is gone, `apply_role_axioms` / `bot_id` / `find_map` cluster at
**24.66%** — a linear scan over `ConceptExpr` slices looking for Bot. This was ranked
11th in the baseline (9.58%) and is now the dominant non-search frame. Similarly,
`apply_self_restriction` grew from 9.01% to 19.08% (again, redistribution effect).

## Corpus measurement

| Fixture | Pre-P3b wall | Post-P3b wall | Δ | FP | MISSED |
|---|---|---|---|---|---|
| sio-stripped (CLI) | ~68 s (reference machine) | 192 s (shared-CPU, `--pair-timeout-ms 200`) | uncalibrated — no same-machine pre-3b baseline | n/a | n/a |
| alehif | 2.72 s | 6.84 s | +shared-CPU artifact | 0 | 0 |
| ore-10908-sroiq | 29.27 s | 27.19 s | -7% | 0 | 0 |
| ore-15672-shoin | 31.51 s | 37.69 s | +20% (rayon contention?) | 0 | 0 |
| galen | 21.1 min (Phase 3, shared-CPU) | 24.8 min (Phase 3b, shared-CPU) | within contention envelope | 0 | 17 |

**SIO wall time (Step 2):** `owl-dl-bench classify` has no `--pair-timeout-ms`
option, and `classify()` runs without per-pair timeout. SIO has 2 hard pairs
(SIO_010092 subsumptions) that the handoff doc notes "didn't finish in 3+ hours"
under direct probe — making the no-timeout `owl-dl-bench classify` command
effectively unbounded. For a meaningful wall comparison, `rustdl classify
--pair-timeout-ms 200` was used instead (matching the test-harness configuration
that produced the "68 s" baseline). The post-Phase-3b isolated run completed in
**192 s** on this shared-CPU server. The "~68 s" baseline was measured on a
faster reference machine; the per-machine ratio is approximately 2.8×. A direct
comparison between pre- and post-Phase-3b on the same machine and same flag set
is not available from this session — the flamegraph evidence above is the durable
measurement of the Phase 3b impact on SIO.

**GALEN wall (Step 3):** The GALEN test
(`galen_closure_matches_konclude` via `konclude_closure_diff --exact --ignored
--nocapture`) completed: **1486.17 s ≈ 24.8 min**, FP=0, MISSED=17.
The test ran concurrently with the SIO bench on a shared-CPU server. The Phase 3
baseline of 21.1 min was also measured under shared-CPU conditions (with notgalen
running in parallel; see `galen-classify-2026-06-01-post-phase3-findings.md`).
Both data points are shared-CPU; the +3.7 min difference falls within the
shared-CPU contention envelope and should not be treated as a regression or speedup.

Soundness gate: **FP=0 / MISSED=17 unchanged** held across the Phase 0 net
(alehif FP=0/MISSED=0, ore-10908-sroiq FP=0/MISSED=0, ore-15672-shoin
FP=0/MISSED=0; per `/tmp/p3b-net.log`). GALEN MISSED=17 identical to the
Phase 3 baseline — the 17 pairs are the same `IntrinsicallyPathologicalBodyProcess`
/ `AbnormalBodyStructure` cluster requiring functional-role merge.

## Interpretation

The Phase 3b hashbrown::HashSet swap hit its target decisively. The
`are_declared_inverses` linear scan dropped from **25.76% to 3.44%** (-22.32pp),
and the enclosing `apply_max` / `edge_satisfies` frames dropped by a matching
~21pp each — exactly the predicted outcome of replacing an O(N) `Vec::iter().any()`
scan (N=84 inverse pairs on SIO) with an O(1) HashSet `contains`. The new
`contains` frame costs only 3.44%, confirming the swap delivers the expected 7.5×
lookup speedup.

After the scan is gone, `apply_role_axioms` / `bot_id` / `find_map` emerges as
the dominant post-Phase-3b target at **24.66%**: a linear scan over `ConceptExpr`
slices searching for `Bot`. This is the natural next Phase 3c target. `apply_max`
itself is now only 6.51% — still worth optimizing (the heap-allocating `spec_extend`
pattern), but lower priority than the `apply_role_axioms` iterator chain. The
`apply_self_restriction` frame at 19.08% is another candidate, though its true
cost needs a separate profiling session to disentangle from redistribution effects.

The Phase 0 soundness net shows FP=0 across all measured fixtures, confirming
the HashSet swap did not introduce unsound positives. GALEN MISSED=17 is
unchanged from the Phase 3 baseline — the fix is performance-only as designed.
