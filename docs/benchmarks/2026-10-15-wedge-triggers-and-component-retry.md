# Wedge trigger dispatch, the #137 net, and the #182 component retry (2026-10-15)

Three questions. All three answers are negative or explanatory: no code change ships from this
note. The builds differ by section, so each section states its own:
- §1 and the §3 traces: the root-recovery branch, forked at `091a0d4` (#265). That is before
  the #266 default flip and the #267 blocking prefilter, so `RUSTDL_HYPER_EDGE_PIN=1` was set
  by hand and the CPU figures are pre-#267. (#267's own measurement of `classify ore_ont_2313`,
  9.26 → 7.40 CPU-s, is on a different base.)
- §2's event-level instrumentation: `c588625` (#267 merged, pin on by default).

**Counting terms.** "Net recoveries" (`RUSTDL_RESEED_PROBE=1`, `stats.reseed_recovered`)
counts re-seed PASSES that changed the graph fingerprint. "Recovered firings" counts individual
clause firings that changed something inside those passes, so one recovery is many firings.
Both vary run to run at a truncating per-pair budget.

## 1. Root-recovery dispatch: closes the trigger gaps, saves no CPU

**Idea.** `Event::Edge` fires a clause at fixed nodes:
- the edge's source (`role_trigger`);
- the source's predecessors (`role_back_trigger`);
- the target, for inverse or symmetric first legs.

`Event::Label` fires successor-class clauses at the labelled node's predecessors
(`succ_trigger`).

A binding whose root `X` sits anywhere else is missed by the drain and left to the #137 net.
Known shapes (the edge ones are named after the `gap_a_*` / `gap_c_*` canaries in
`crates/owl-dl-tableau/tests/chain_back_trigger.rs`; a third edge shape, an inverse non-first
leg, was never reproduced):
- (a) the root is behind an edge stored at the middle node;
- (c) a three-hop body;
- a class atom two hops down;
- a class atom on the root's parent.

Root recovery walks the clause's variable tree from the new fact back to `X` through the graph.
It then fires a match pinned to that fact at each recovered root.

**Correctness.** With the #137 net OFF, the default dispatch answers `Sat` on all four gap
canaries; root recovery answers `Unsat`. Two sabotages confirm the canaries guard the
behaviour:
- edge walk truncated to one level of predecessors: 3 tests fail;
- label walk reduced to predecessors: 2 tests fail.

FP controls stay `Sat`. The whole tableau suite passes with the flag on.

**Cost.** One binary, the flag the only difference, `RUSTDL_HYPER_EDGE_PIN=1` in both arms,
single thread, 5 reps with arm order alternated, CPU-s (user + sys), host load ~16–19. Answers
were identical in all 50 runs.

| case | off: median [range] | on: median [range] | change |
|---|---|---|---|
| `classify ore_ont_2313` | 9.41 [8.31–11.27] | 10.93 [9.39–11.16] | +16% |
| `classify wine` | 12.02 [10.79–12.06] | 11.73 [11.06–12.15] | −2% |
| `consistent ore_ont_14375`, 60 s budget | 11.31 [9.80–12.96] | 11.17 [10.34–13.76] | −1% |
| `consistent ore_ont_10127`, 60 s budget | 28.94 [25.41–38.65] | 27.49 [23.50–29.91] | −5% |
| `consistent ore_ont_1182`, 60 s budget | 120.30 [118.28–120.81] | 119.88 [117.84–121.29] | 0% |

**On the stall cases the net still recovers as much.** Net recoveries (passes), single runs, edge
pin on in both columns, edge and label root recovery both on in the second:

| ontology | default dispatch | root recovery |
|---|---|---|
| pizza | 5 | 1 |
| ro | 26 | 11 |
| `ore_ont_2313` | 2083 | 2427 |
| `ore_ont_14375` | 1 | 0 |
| `ore_ont_10127` | 1 | 0 |

The reason is section 2. Parked, not shipped:
`refs/archive/heads/perf/hyper-root-recovery`, flag `RUSTDL_HYPER_ROOT_RECOVERY`.

## 2. What the #137 net recovers: blocking-status changes, not lost events

**Tally by clause shape.** Recovered firings (not passes) on `classify ore_ont_2313`, one run
of the §1 build with the default dispatch and the edge pin off:

| shape | count |
|---|---|
| `C(X) → C(X)` at a blocked node | 8,943 |
| `C(X) → ∃` at a node not blocked when the net ran | 6,271 |
| `R(X,1) → C(1)` | 2,418 |
| `C(X) ∧ R⁻(X,1) ∧ C(1) → C(X)` at a blocked node | 2,375 |
| `R(X,1) → C(X)` | 169 |
| two-role chains `R(X,1) ∧ R(1,2) → R` | 84 |

The six largest shapes; smaller ones were not tallied.

**Not a deadline artifact.** At `--pair-timeout-ms 1000`, where the run completes
(`incomplete: false`), there are still 1,886 net recoveries (passes). A run at the default 5 ms
on the same build gave 1,629; the 2,083 in §1 is another 5 ms run on a slightly different build,
so the spread between them is run-to-run variance at a truncating budget.

**Event-level instrumentation** (throwaway build on `c588625`, not committed). Its search
differs from §1's build, so its firing counts do not add up to the tally above. Each node records:
- the label events the drain processed;
- each `x_trigger` firing and its outcome;
- which phase queued each label (drain or net).

At every net recovery with a one-atom body:

| head | trigger label queued by | drain processed it? | drain firing outcome | count |
|---|---|---|---|---|
| `C(X)` | the net itself | no (only the net's own event) | — | 6,824 |
| `∃` | the drain | yes | `NoChange` (node blocked then) | 4,437 |
| `∃` | the net itself | no | — | 199 |

The 4,437 are the primary recoveries. In each, the drain fired the clause while the node was
blocked, `fire_exists` correctly declined, and the node was unblocked by the time the net ran.
Nothing re-fires a skipped `∃` on unblocking.

Everything else is downstream of those primary recoveries. The trigger labels were added by the
net in the same pass, on successors its recovered `∃`s created. Those successors start out
blocked, which is why the `C(X) → C(X)` rows read "at a blocked node".

**Every label write goes through `add_label`, which queues an event.** The only direct
`HyperNode::add` calls outside tests are in the constructors, before the one-time seed in
`decide_with_deadline`.

So the net's real job is blocking-status changes. A complete drain would need an unblock
trigger, i.e. blocking tracked rather than recomputed lazily. Turning the net off was already
measured CPU-flat (`ore_ont_2313` 12.2–16.3 CPU-s in both arms, on a pre-#263 build under host
load, so not comparable with §1's figures), so that is not worth building.

## 3. The #182 component retry: the cost is genuine stalls

After a wedge stall and a bounded tableau fall-through, `consistent` re-checks the ABox in
batches of connected components (`RUSTDL_CONSISTENCY_COMPONENTS`, for the #182 ECO+GENEPIO
case).

**Census.** Over the 227-ontology stall population at the default budget, 96 reached the retry.
It decided 0 of them and cost 973 s in total (median 10 s, max 60 s). 38 of the 96 finished in
under 1 s and 58 ran at least one stalled batch. The census does not record which of the 38 the
feasibility gate skipped outright (`prep × batches > 60 s`) and which had only quick,
consistent batches.

**Traces** (`RUSTDL_TRACE=1`):

```
ore_ont_14334: batch 1/3 (60 ABox axioms)   -> consistent in 45 ms
               batch 2/3 (33 ABox axioms)   -> consistent in 47 ms
               batch 3/3 (400 ABox axioms)  -> undetermined in 10061 ms
ore_ont_9540:  batch 1/2 (6 ABox axioms)    -> undetermined in 10174 ms
               batch 2/2 (1998 ABox axioms) -> undetermined in 10075 ms
```

**Preparation is not the cost.** Whole-ontology preparation is 8–2,480 ms on the population,
against ~10 s per stalled batch. Sharing it would save little.

**The stall is in the TBox.** `ore_ont_9540` stalls on a 6-axiom batch.

**No answer-safe cut.** Every remaining cut is a heuristic skip, e.g. "a tiny batch stalled, so
skip the rest". In #182 a 701-axiom batch gave up and a smaller one found the clash, so such a
skip could lose that answer. The retry stays as it is.

## Method notes

- **A debug-only check in a hot loop can hang CI.** #267's first `debug_assert` recomputed four
  label signatures per blocking candidate. The debug `phase_attribution` tests then ran 3 h on
  all three CI platforms, against 9 min on main. It was first misread as a host-load stall.
  The fix checks only on the reject path, where a stale signature could cause a wrong answer:
  308 s locally, against 312 s before.
- **Interleave arms in one loop and compare CPU-s.** The root-recovery A/B ran beside a niced
  workspace test build at load ~16–19. Wall would have been meaningless there; CPU-s medians
  still separate the arms.
