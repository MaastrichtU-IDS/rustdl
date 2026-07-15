# Bound-the-tail Phase 0 — fallthrough rescue rate (2026-07-16)

**Plan:** `docs/superpowers/plans/2026-07-15-bound-the-tail-exploration.md` Phase 0.
**Question:** how often does the main-tableau fallthrough, AFTER a wedge `Stalled`,
RESCUE a subsumption (return `Subsumed`) the wedge missed? This decides whether
skipping the fallthrough (the −44 % wall reclaim) is MISSED-safe.

**Instrumentation:** diagnostic counters in `subsumes_via_tableau` (a "stall
fallthrough" = wedge `Unknown`/`UnknownDiverged` reaching the tableau, NOT a
fast-refute/counting-verify), surfaced as the `# fallthrough:` banner line. Run with
`RUSTDL_BOUND_DIVERGED_TAIL` OFF so every stall falls through and its tableau outcome
is observed.

## Results

**Curated hybrid onts (sio, ore-10908, ore-15672, alehif, shoiq-knowledge, pizza):**
**zero** stall-fallthroughs — every pair is wedge-decided or label-heuristic-pruned;
no wedge `Stalled` reaches the tableau. So on curated, skipping the fallthrough is
trivially MISSED-safe (there is nothing to skip).

**`ore_ont_10019`** (`--pair-timeout-ms 250`, aggregate 120 s):

| metric | value |
|---|---|
| stall fallthroughs run | 1265 |
| **rescued (tableau found `Subsumed`)** | **11** |
| — of which on a *divergence*-stall | **0** |
| not-subsumed by tableau | 1 |
| no-verdict (tableau also timed out) | 1253 |
| stalls that were divergence-stalls | 2 |

## Decision — the fallthrough is NOT redundant; there is no sound wall win

The −44 % stub reclaim (skip all fallthroughs) was **not free**: it silently drops the
**11 real subsumptions** the main tableau rescues on `ore_ont_10019` (the wedge is
incomplete there; the tableau completes those pairs within a fresh budget). Concretely:

- **Phase 1 (deadline-keyed skip / budget-sharing) — REJECTED by measurement.** All 11
  rescues are on **deadline**-stalls, so any variant that curtails the deadline-stall
  fallthrough loses them → differential MISSED (ON vs OFF) = 11 > 0 → fails its own
  MISSED=0 gate on the very target it was meant to speed up.
- **Phase 2 (earlier divergence detection) — cannot help.** The 11 rescuable pairs are
  **not diverging** — the tableau *solves* them in < 250 ms; they are wedge-incomplete-
  but-tableau-completable, not thrash. Detecting them as "diverging" to cut them would
  MISS them; not detecting them leaves them falling through (no wall saved). Only 2
  pairs are genuine divergence-stalls, and neither rescues.
- **The shipped sound divergence-keyed skip** (`RUSTDL_BOUND_DIVERGED_TAIL`) is
  MISSED-safe (0 diverged-stall rescues) but **near-inert** (2 diverged-stalls → no
  measurable wall win).

**Conclusion:** the main-tableau fallthrough *earns its cost* — it rescues real
subsumptions the incomplete wedge cannot. The dense-SROIQ wall is a genuine
completeness/wall **trade**, not a redundant-work reclaim. The only "win" (−44 % on
`ore_ont_10019`) costs +11 MISSED — a completeness regression the user must explicitly
accept; it is not sound-and-free.

**Arc closed.** Every dense-SROIQ lever is now measured out with evidence:
- Fix #1 backjump-precision — ruled out (bit-identical bjgap).
- Fix #2 semantic branching (Layer A+B) — sound, NO-GO (77 842 exclusions, decides 0).
- Bound-the-tail — sound form inert; the wall reclaim is a real completeness trade
  (rescues 11), not free.

The remaining closer for the dense-SROIQ disjunctive tail is Konclude-class whole-model
caching / CDCL clause-learning, deferred on its reuse-trap FP surface. The aggregate
deadline (`RUSTDL_AGGREGATE_DEADLINE_MS` / `--global-timeout-s`) remains the sound way
to bound wall on pathological inputs.

## Disposition of the Phase 0 instrumentation

The `# fallthrough:` rescue-rate counters are cheap, diagnostic-only, and useful for
future dense-SROIQ triage — kept. The sound `RUSTDL_BOUND_DIVERGED_TAIL` flag is kept
(default-OFF, correct, MISSED-safe) as banked plumbing. No risky knob shipped.
