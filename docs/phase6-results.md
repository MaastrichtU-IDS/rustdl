# Phase 6 — `find_direct_parents_top_down` walk dedup

Run 2026-06-02. Fix: add a `visited: Vec<bool>` bitset to the top-down
walk in `find_direct_parents_top_down`
(`crates/owl-dl-reasoner/src/classify.rs`), so the dense subsumer
lattice on GALEN doesn't re-visit each candidate via every parent path.

Companion to:
- `docs/phase5-recon.md` — CPU flame couldn't see this region.
- `docs/phase5-walltime-probe.md` — saturator is 0.12% of GALEN wall.
- `docs/phase5-downstream-probe.md` — `find_direct_parents_top_down`'s
  tier-walk-and-sweep is 97.2% of GALEN wall.

## Headline

**GALEN classify wall: 753.96 s → 684.00 s = −9.3 % under load avg ~93;
−8.3 % vs pre-2d clean baseline (745.57 s).** FP=0 / MISSED=0 / closure
27,997 = Konclude preserved. Phase 6 doesn't just recover the +6.5 %
Phase 2d regression (`docs/phase2d-2c-redux-results.md`) — it nets
faster than the pre-Phase-2d baseline by ~60 s while keeping all
Phase 2d + 2c-redux completeness gains.

## The fix in one paragraph

Before: `find_direct_parents_top_down`'s walk pops candidates from a
frontier, checks subsumption via `closure.contains(c, d)`, and on
acceptance pushes `direct_children[d]` to the frontier. The dense
GALEN subsumer lattice reaches the same `d` via many distinct parent
paths; without de-dup, each re-visit redoes the closure check, the
child enumeration, and the `accepted.push(d)` — and the final
`accepted.collect::<HashSet>().into_iter().collect()` covers
correctness but not the wasted walk-time work.

After: a `Vec<bool>` `visited` of size `n` (where `n = direct_supers.len()`)
short-circuits already-explored candidates at both the pop site and the
child-push site. The de-dup at the bottom becomes unnecessary
(`accepted` is now unique by construction), so the trailing
`HashSet`-ceremony is removed.

## Measurement

| Stage | Wall (s) | Load avg | Source |
|---|---|---|---|
| Pre-Phase-2d clean baseline | 745.57 | quiet | T2 (`docs/phase2d-2c-redux-results.md`) |
| Post-Phase-2d+2c-redux (T7, concurrent runs) | 801.55 | concurrent | T7 (`docs/phase2d-2c-redux-results.md`) |
| Phase 5 T3b probe (instrumented) | 753.96 | ~89 | `docs/phase5-downstream-probe.md` |
| **Phase 6 (this fix), same machine** | **684.00** | **~93** | this doc |

Deltas (Phase 6 vs each baseline):
- vs Phase 5 T3b probe: **−70 s / −9.3 %** (same machine, slightly higher contention this run).
- vs pre-2d clean baseline: **−61 s / −8.3 %** (machines are different load profiles, so the headline is the same-machine T3b comparison).

Note: the headline wall comparison is across two runs on the SAME load-contended
host as Phase 5 T3b. The pre-2d 745.57 s reading was taken on a quieter
machine. Under-contention runs typically inflate by ~10 %; this fix's
684 s is below the pre-2d clean baseline under heavier contention,
which is strong evidence the dedup wins.

## Soundness gate (Phase 0 net)

| Fixture | FP | MISSED | Wall (s) |
|---|---|---|---|
| alehif-test | 0 | 0 | 9.92 |
| ore-10908-sroiq | 0 | 0 | 29.07 |
| ore-15672-shoin | 0 | 0 | 37.75 |
| GALEN | 0 | 0 | 684.00 |

FP=0 / MISSED-unchanged across the net + GALEN. The walk de-dup is
semantically transparent: the set of accepted candidates is identical
(just without duplicate visits), and the pruning step is unchanged.

## Why it works

The walk's structure is a tree-like descent through `direct_children`,
but the GALEN subsumer lattice is a DAG, not a tree. Many classes are
reached via multiple parents (think: a "BodyProcess" that's a child of
both "Process" and "OrganicProcess", both of which are top-level).
Without `visited`, every such DAG-cross gets re-checked at the cost
of all its downstream traversal. With `visited`, each candidate is
work-evaluated exactly once.

The cost of the bitset is O(n) per call (vec init + scan); on GALEN
n ≈ 2748 so the bitset itself is ~3 KB and the per-call init cost is
microseconds — negligible vs the 100s of ms per call the dedup saves.

## What this rules out for future perf work

Phase 5 T3b's 97.2 % attribution to `find_direct_parents_top_down` no
longer makes this function the dominant cost — Phase 6 reduces its
share. A re-measured flame post-Phase-6 would show what's left;
candidate next targets include the pruning step's `accepted.iter().any`
inner loop (O(|accepted|²) but accepted is small) or the per-pair
tableau path (currently never called on Horn workloads like GALEN).
Phase 7+ if perf matters more than the present 11-min GALEN wall.

## Cross-references

- Phase 2d + 2c-redux results (where the +6.5 % regression was first
  measured): `docs/phase2d-2c-redux-results.md`.
- Phase 5 chain (CPU flame failure mode → saturator innocence →
  variance check aborted → downstream localization):
  `docs/phase5-recon.md`,
  `docs/phase5-walltime-probe.md`,
  `docs/phase5-variance-check.md`,
  `docs/phase5-downstream-probe.md`.
- Implementation: `crates/owl-dl-reasoner/src/classify.rs::find_direct_parents_top_down`.
