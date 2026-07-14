# `ore_ont_10019` hyper-sat stall: blocking-counter + feature-detection findings (SP0)

**Date:** 2026-07-14
**Task:** SP0 / Task 0.1 of the dense-SROIQ tractability plan — pure diagnostic,
no reasoning-behavior change. Extends the existing `hyper-sat` probe
(`rustdl hyper-sat`) with (1) per-class `blk=<fired>/<eligible>` blocking
columns, (2) aggregate `total_is_blocked_calls` / `total_blocks_fired` /
`total_block_eligible` summary lines, and (3) a coarse syntactic
`features: inverse=? nominal=? card=?` scan. See
`crates/owl-dl-cli/src/main.rs` (the `Command::HyperSat` arm) for the code.

## Command run

```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli
./target/release/rustdl hyper-sat ~/data/ore-run/input/ore_ont_10019.ofn --per-class-timeout-ms 300
```

## Raw probe output

```
# features: inverse=false nominal=false card=true
# PERFORMANCE PROBE (not a soundness claim):
#   clausifier defers 0 axiom(s); dropping them only
#   removes constraints, so Unsat is sound for the full
#   ontology but Sat is NOT. See hypertableau-scoping.md §H2b.
# clauses_total:    346
# disjunctive:      25
# deferred:         0
# depth_cap:        256
# per_class_timeout: 300ms
# classes:          47
# sat:              14
# unsat:            0
# stalled:          33
# total_wall_ms:    9943.5
# total_branches:   50373
# max_depth_reached:80
# --- profiling counters (search-quality work) ---
# match_attempts:   103712780  (clause×node Horn match tries)
# node_clones:      50373  (save/restore — trail target)
# fixpoint_passes:  43746
# total_is_blocked_calls: 3137027
# total_blocks_fired:    2380847
# total_block_eligible:  0
# classes_branched: 35   <-- HEADLINE: only these probe the engine
# branched_wall_ms_mean: 284.10
# branched_wall_ms_max:  301.91
# --- top classes by branching ---
#   Stalled wall=300.12ms branches=3449 (disj=3449 merge=0) restores=3449 depth=23 blk=75490/0  http://ontology.dumontierlab.com/AmideGroup
#   Stalled wall=300.19ms branches=3155 (disj=3155 merge=0) restores=3155 depth=22 blk=72206/0  http://ontology.dumontierlab.com/CarboxylicAcidGroup
#   Stalled wall=301.20ms branches=1712 (disj=1712 merge=0) restores=1712 depth=75 blk=93837/0  http://ontology.dumontierlab.com/HydroxylGroup
#   Stalled wall=301.32ms branches=1643 (disj=1643 merge=0) restores=1643 depth=74 blk=87458/0  http://ontology.dumontierlab.com/EtherGroup
#   Stalled wall=301.28ms branches=1604 (disj=1604 merge=0) restores=1604 depth=75 blk=85433/0  http://ontology.dumontierlab.com/SulfoxideGroup
#   Stalled wall=301.46ms branches=1598 (disj=1598 merge=0) restores=1598 depth=75 blk=83764/0  http://ontology.dumontierlab.com/OxygenAtom
#   Stalled wall=301.84ms branches=1577 (disj=1577 merge=0) restores=1577 depth=75 blk=83843/0  http://ontology.dumontierlab.com/SulfonicAcidGroup
#   Stalled wall=301.11ms branches=1549 (disj=1549 merge=0) restores=1549 depth=76 blk=81164/0  http://ontology.dumontierlab.com/ThiolGroup
#   Stalled wall=301.16ms branches=1532 (disj=1532 merge=0) restores=1532 depth=75 blk=80466/0  http://ontology.dumontierlab.com/AcylFluorideGroup
#   Stalled wall=301.46ms branches=1515 (disj=1515 merge=0) restores=1515 depth=80 blk=78336/0  http://ontology.dumontierlab.com/EsterGroup
#   Stalled wall=301.30ms branches=1514 (disj=1514 merge=0) restores=1514 depth=75 blk=79352/0  http://ontology.dumontierlab.com/AcylChlorideGroup
#   Stalled wall=301.28ms branches=1501 (disj=1501 merge=0) restores=1501 depth=74 blk=78038/0  http://ontology.dumontierlab.com/AcylBromideGroup
#   Stalled wall=301.20ms branches=1499 (disj=1499 merge=0) restores=1499 depth=75 blk=76387/0  http://ontology.dumontierlab.com/Alkyl
#   Stalled wall=301.32ms branches=1447 (disj=1447 merge=0) restores=1447 depth=76 blk=72535/0  http://ontology.dumontierlab.com/SecondaryAmineGroup
#   Stalled wall=301.28ms branches=1436 (disj=1436 merge=0) restores=1436 depth=75 blk=72564/0  http://ontology.dumontierlab.com/SulfonicAcidDerivativeGroup
```

(Numbers are from the run archived above; a second run showed
`total_branches: 55403`, `total_blocks_fired: 2732399` — a few percent of
run-to-run jitter, presumably from the 300ms wall-clock timeout landing on a
slightly different branch count each time. The `features` line and the
`.../0` `block_eligible` pattern were identical across both runs.)

## (a) Is blocking firing on the depth-75 stalled classes?

**Yes — blocking fires constantly**, not never. Take `HydroxylGroup`
(depth=75, one of the deepest stalls): `blk=93837/0` means `is_blocked`
returned `true` 93,837 times over the course of that one class's 300ms
budget, against 1,712 branch decisions taken — roughly **55 successful
blocks per branch**. The other depth-74–80 classes (`EtherGroup`,
`SulfoxideGroup`, `OxygenAtom`, `SulfonicAcidGroup`, `EsterGroup`, …) show
the same pattern: tens of thousands of `blocks_fired` per class, on the same
order of magnitude as `is_blocked_calls`. Aggregate: `total_blocks_fired /
total_is_blocked_calls ≈ 2,380,847 / 3,137,027 ≈ 76%` — blocking succeeds on
roughly three-quarters of the calls made to it.

So the naive "blocking never fires, the model grows unbounded" hypothesis
(the failure mode the `blocks_fired` doc-comment in `hyper.rs` calls out) is
**not** what is happening here. Blocking is doing real work continuously
throughout the search and still the classes reach depth 74–80 and burn the
full 300ms per-class budget without reaching `Sat`/`Unsat`.

**Caveat — the `block_eligible` denominator is structurally zero here, and
that is itself a finding, not a "no eligible nodes" result.** Reading
`crates/owl-dl-tableau/src/hyper.rs:1436-1508`, `is_blocked` has two branches:
the HF2 double-blocking branch (`self.double_blocking == true`) increments
`block_eligible` (line 1451) before searching same-parent-role candidates;
the legacy "anywhere blocking" branch (the `else` at line 1488) never
increments it, only `is_blocked_calls`/`block_compares`/`blocks_fired`.
`hyper_sat_probe` (`crates/owl-dl-reasoner/src/lib.rs:685`) constructs the
engine via the plain `HyperEngine::new(&clauses, class_id)`, which defaults
`double_blocking: false` (`hyper.rs:937`) — it never calls
`.with_double_blocking()`. So this probe run exercises the **legacy
anywhere-blocking path**, where `block_eligible` is dead code (structurally
always 0), while `blocks_fired`/`is_blocked_calls`/`block_compares` are live.
The `blk=<fired>/0` pattern on every line is exactly what that code path
predicts — it is not evidence of "zero eligible nodes." Per Step 1 of the
task brief, this does not block the task (the counter has a real `+= 1` for
its defined semantics, just conditioned on a flag the probe doesn't set); it
is flagged here as a loose end for whoever revisits blocking work, since a
reader could easily misparse `blk=X/0` as "no candidates were ever eligible."
No code change was made to fix this (out of scope — SP0 is diagnostic-only,
"no reasoning-behavior change").

## (b) Features line

```
# features: inverse=false nominal=false card=true
```

`ore_ont_10019` has **no** `InverseObjectProperties`/`ObjectInverseOf` and
**no** `ObjectOneOf`/`ObjectHasValue` (nominals), but **does** have
`ObjectMinCardinality`/`ObjectMaxCardinality`/`ObjectExactCardinality`
(cardinality restrictions — consistent with `merge_branches=0` everywhere in
the top-classes table not being reachable purely from Horn propagation, and
with `disjunctive: 25` in the clause stats: cardinality lowers to
disjunctive-branching clauses in this engine).

For the SP2 question ("are pure label-set no-goods admissible?"): the
absence of inverse roles and nominals on this ontology is exactly the
precondition SP2's no-good caching would want — nothing here forces the
double-blocking / inverse-aware label refinement (H3) to be in play. The
presence of cardinality restrictions is the caveat: SP2 label-set no-goods
need to be sound with respect to the `≤n` merge-branch semantics too, not
just plain conjunctive label sets — this ontology will exercise that
interaction (even though the observed branching here is 100% `disj`, 0%
`merge`, per the `(disj=… merge=0)` columns — the cardinality restrictions
are present in the axioms but are not the ones driving the branch counts on
these particular top-15 stalled classes).

## (c) Verdict: SP1 (per-branch cost) vs blocking fix (SP2a) vs both

**The evidence points to SP1 (per-branch cost) as the primary lever, not a
blocking fix.** Blocking is already firing on a large majority of its calls
(76% aggregate hit rate, tens of thousands of fires per stalled class) — it
is not silently disabled or starved of candidates, so "make blocking fire
more" is not the lever that unblocks these 33 stalled classes. What actually
dominates the 300ms-per-class budget is raw per-branch cost: 50,373 total
branches for 103,712,780 total `match_attempts` — **≈2,058 Horn clause×node
match attempts per branch decision**, on top of a `node_clones` cost equal to
`branches_taken` (one full node-vec clone per branch, the save/restore cost
the trail is meant to eliminate per the doc comment on `node_clones`). At
~2,000+ match attempts and a full graph clone per branch, only ~1,500–3,700
branches fit in a 300ms wall before the deadline fires — nowhere near enough
to exhaust the search space at depth 75–80, hence `Stalled` rather than a
genuine `Sat`/`Unsat`. This is a raw-throughput problem (SP1's stated target:
incremental `horn_fixpoint` across save/restore, replacing the apparent
full-clone-per-branch pattern implied by `node_clones == branches_taken`),
not a blocking-effectiveness problem. A secondary, low-priority cleanup is
worth carrying into SP2 scoping: the `block_eligible` counter is dead
(always 0) unless double-blocking is enabled, so any future SP2 work that
reasons from this probe's `blk=fired/eligible` column should either enable
`.with_double_blocking()` in the probe or read the ratio as `fired/
is_blocked_calls` instead of `fired/eligible`.

---

## SP1 result (2026-07-14): incremental `horn_fixpoint` landed

SP1 Task 1.4 made `horn_fixpoint` incremental under
`RUSTDL_HYPER_INCREMENTAL_FIXPOINT` (default OFF). In incremental mode the
per-pass `worklist.clear()` + full-graph re-seed is skipped; each `solve`
frame drains only the delta its own decision pushed, relying on the parent's
saturated worklist carried across `save`/`restore` (Task 1.3). The root query
graph is built by direct field writes that bypass `worklist.push`, so
`decide_with_deadline` seeds the worklist once, before the first `solve`.

Measured on `ore_ont_10019` (`hyper-sat --per-class-timeout-ms 300`), OFF vs ON:

| metric          | incremental=0 | incremental=1 | delta        |
|-----------------|--------------:|--------------:|-------------:|
| `match_attempts`| 111,145,427   | 1,973,881     | **−98.2% (~56×)** |
| `sat`           | 14            | 14            | unchanged    |
| `unsat`         | 0             | 0             | unchanged    |
| `stalled`       | 33            | 33            | unchanged    |
| branches / stalled class | ~1,500–3,600 | ~16,500       | ~5–10× more  |
| max depth reached | ~75–80      | ~137–138 (cap) | deeper      |

The redundant re-saturation that dominated the per-branch cost is gone:
per-branch Horn match tries collapse ~56×, so within the identical 300 ms
budget the search now explores ~5–10× more branches and reaches the depth cap
(138) instead of stalling at depth ~75–80. The verdict counts are unchanged
(same 14 sat / 33 stalled): these 33 classes are genuinely hard — the deeper,
cheaper search still does not exhaust them inside 300 ms, confirming the doc's
diagnosis that this is a raw-throughput problem. Closing the 33 stalls needs
the remaining SP levers (branch ordering / lookahead / a higher budget), not
more per-branch throughput; the throughput lever itself is now largely spent.

Note: `node_clones == branches_taken` still holds — the full node-vec clone per
branch in `save`/`restore` is unchanged by this task (only the worklist is now
carried; the graph clone is a separate SP lever).

Correctness: the classify OFF-vs-ON differential harness
(`crates/owl-dl-cli/tests/incremental_fixpoint_identity.rs`) is byte-identical
across `funcmerge-cyclic`, `pizza`, `27_eight_way_disjunction_sat`, and
`18_diamond_subsumption_unsat` (the last three added here to cover disjunctive
branching + `≤n` merging across `save`/`restore`).
