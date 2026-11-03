# The classify wedge rebuilds the nominal core in every probe (#404)

**Date:** 2026-11-03 · **Status:** diagnosed, **NOT fixed** (parked; the fix is a design decision)
**Severity:** wall/CPU only, no answer is lost. `ore_ont_9540` is 2× slower with wedge nominals at the default `--pair-timeout-ms`, and ~36× slower (831 s vs 23 s wall) at `--pair-timeout-ms 1000`. Rows are identical either way.

## Symptom

On `ore_ont_9540` with `RUSTDL_CLASSIFY_WEDGE_NOMINALS` on (the default), 40 of 50 label probes return `NoVerdict`. With it off, 3 do.

| `--pair-timeout-ms` | nominals off | nominals on |
|---|---|---|
| 5 (default) | 21.6 s wall / 22.4 CPU-s | 27.0 s / 41.7 CPU-s |
| 1000 | 23.1 s / 23.9 CPU-s | 831 s / 2,855 CPU-s |

At 1000 ms each label probe runs into the 30 s label-cache clamp, and the ~1,077 pairs that then fall through to per-pair search burn ~1 s each.

## Mechanism (measured with report-only counters)

The counters are on the archived branch `refs/archive/heads/fix/404-nominal-stall`: `RUSTDL_LABEL_STATS` prints one line per label probe, and `RUSTDL_HYPER_DECISION_TRACE` prints one line per `⊔` decision.

- **Not branching.** In all 40 stalled probes, at label deadlines of 250 ms, 2 s and 30 s: 0 `≠`-merge clashes, 0 `Unsat` children, 0 backjumps.
- **Not a blocking failure.** Blocking fires normally, and the #376 nominal guard never refuses a block. The peak graph is ~200–900 nodes.
- **A long, clash-free descent.** Each decision costs ~10 ms on one core, growing from ~1k to ~20k match attempts. Depth reaches ~40 at 250 ms, ~62 at 2 s and ~118 at 30 s. One class hits the 256-decision depth cap (`HYPER_WEDGE_DEPTH`), so a bigger budget cannot finish it.
- **Why it is long.** `Object_type ≡ OneOf(13 objects)`, and `Object_type` carries `∃is_touching.Object_type`, `∃has_shape.Shape_type` and similar. Shapes need `≥3 has_point.Point_type`, and points carry `∃has_fixed_orientation.Object_type`.
  - Every `Object_type` witness takes the 13-way `{oᵢ}` decision and NN-merges into `oᵢ`'s nominal node.
  - Nominal nodes are never blocked, so all 13 objects are expanded in full.
  - That nominal core does not depend on the probe, yet every label probe and every pair probe rebuilds it from scratch.
- **Nominals off.** `{oᵢ}` is an ordinary class and the witness is blocked: 9 decisions, ~1 ms.

## Options (not started)

- **(A) Build the nominal core once per classify** and seed each probe from it. This is the real fix. It is soundness-critical: the seed must be a model of the nominal part that every probe extends, with backtracking into it still possible, so it needs its own design and review.
- (B) Cheaper decisions (`role_atom_holds` is 54% of the profile). A constant factor only; it does not get past the depth cap.
- (C) An early `NoVerdict` when a probe's nominal core cannot finish in budget. This buys back time only at non-default timeouts, and it would need a corpus check showing it never cuts a probe that would have finished. Declined for now.

## Ruled out

- **ABox-edge hubs:** the #279 OPA edge clauses are off by default (`RUSTDL_CLASSIFY_NOMINAL_OPA`).
- **Precise `≠`-merge clash deps:** there are no clashes for them to act on.
- **A nominals-off label as a non-subsumption pruner:** this is #393's unsoundness by construction.
