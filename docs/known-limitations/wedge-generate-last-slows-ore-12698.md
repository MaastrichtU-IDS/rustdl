# Generate-last slows `ore_ont_12698` past the default per-pair budget (#483)

**Date:** 2026-11-19 · **Status:** diagnosed, **accepted** (no fix planned).
**Severity:** sound under-approximation only. At the default `--pair-timeout-ms` (5 ms),
`ore_ont_12698` loses 1–27 subsumptions per run (median 2), and since #483 it no longer reaches
the complete closure at that budget. `incomplete: true` marks budget truncation here; it is set on
every default-budget run, including the ones that reach the full closure, so it signals possible
loss, not loss. At `--pair-timeout-ms 0` the closure is complete (126,743 relations: strict
closure plus equivalence pairs) in every arm (34 runs); `--pair-timeout-ms 1000` was checked on
the default arm only (`incomplete: false`, complete).

## Symptom

Core 80, `RAYON_NUM_THREADS=1`, serial. The default-budget runs used the 0.4.43 release
candidate `c49a2f03` (12 runs) plus 3 runs of an instrumented `53b7d5a9` build with its
instrumentation off; the CPU column uses that instrumented build with instrumentation off.
`c49a2f03..53b7d5a9` changes no reasoner code.

| arm | reaches the complete 126,743 at 5 ms | CPU-s at `--pair-timeout-ms 0` (median, 5 reps) |
|---|---|---|
| `RUSTDL_WEDGE_GENERATE_LAST=1` (default) | 0 of 15 runs (lost 1–27, median 2) | 15.84 (13.06–17.37) |
| `RUSTDL_WEDGE_GENERATE_LAST=0` | 6 of 15 runs (lost 0–3) | 12.06 (10.27–13.44) |
| v0.4.42 | 11 of 15 runs | — |

Bracketing #483 directly (`b12698.log`): before it, `b6ed041d` reached the complete closure in 4
of 6 runs and `fe3f3854` in 3 of 3; after it, `3dc436e0`, `5e069d48`, `3f85ee41` and `c49a2f03`
reached it in 0 of 21. GL=0 (6 of 15) does not restore v0.4.42 (11 of 15); the remaining gap was
not attributed (#484's `RUSTDL_NOMINAL_MERGE_PRUNE` is ruled out below).

12698 was already a knife-edge ontology before #483: #266's sweep recorded 6 distinct answers in
6 serial default-budget runs (`docs/claude-md-history.md`, `RUSTDL_HYPER_EDGE_PIN` row).

## Mechanism (measured with temporary counters)

Generate-last (#465 B / #483) defers `∃`/`≥n` Horn clauses into a FIFO drained after the LIFO
worklist whenever the pool has a nominal. 12698 has only 2 `ObjectHasValue` axioms, but deferral
is pool-wide. Because the worklist is LIFO and the deferred queue FIFO, generation moves from
depth-first towards breadth-first. The counters are consistent with that reordering being the
cost, though it was not isolated:

| counter (GL=1 vs GL=0) | change |
|---|---:|
| events processed | **+27.5%** (4.30M vs 3.37M) |
| `fire_clause_with` calls | +30.5% |
| generating firings | +20% (731k drains vs 610k immediate) |
| decisions | +13.9% |
| edges created | −2.6% |
| nodes created | −2.2% |

The cheap explanations were measured out:

- **Unpinned re-drains (deferral drops the `RUSTDL_HYPER_EDGE_PIN` restriction).** No generating
  clause on 12698 ever fires pinned in either arm, every drain matches exactly one binding, and
  none comes up empty. Drain time is 0.42–0.51 s of a 3.78 s Δ. Keeping the pin through deferral
  would not help.
- **Queue overhead on firings that cannot match.** A prototype that rejects such firings before
  deferral rejected 0 on 12698. Queue plus deferral-check time is ≤1.16 s.
- **#484 `RUSTDL_NOMINAL_MERGE_PRUNE`.** No effect on CPU or on any wedge counter. #483 alone
  (`c06763b2`) reproduces the full counter signature.

The lost pairs are each probed directly by the classify wedge. 27 of the 28 seen are slower per
probe under GL=1, from 1.3× to 50× (e.g. 15.7 ms vs 0.3 ms), which pushes many of them past the
5 ms budget. Summed over all probes, the classify wedge accounts for only 0.9–1.8 s of the 3.78 s
CPU Δ; the rest is unattributed.

## Why accepted

The extra events come from the reordering #483 exists to make (it fixes #465, and the
4-axiom wedge loop from #473's case 00939). Undoing it on 12698 means undoing #483. The loss is budget truncation, it is
flagged `incomplete`, and a larger `--pair-timeout-ms` recovers it.

Possible future lever (not planned): scope `defers_generation` to the part of the pool the
nominals can reach, instead of pool-wide. That needs the full 1,920 sweep plus the MISSED net.

## Evidence

- Plan and raw outputs: `~/rustdl/sweeps/p12698/` on the coder host (`plan.md` v2.1,
  `step0-results.md`, `step0/raw/`, `step0/perpair-plain.md`).
- Before/after #483 bracketing: `~/rustdl/sweeps/rel443/b12698.log`.
- Instrumentation (temporary, never ships): `refs/archive/heads/diag/p12698-step0`
  (on `53b7d5a9`) and `refs/archive/heads/diag/p12698-step0-c067` (on `c06763b2`). Enable with
  `RUSTDL_P12698_CTR=1` / `RUSTDL_P12698_PAIR=1`.
