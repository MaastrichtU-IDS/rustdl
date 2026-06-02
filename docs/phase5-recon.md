# Phase 5 recon — Phase 2d + 2c-redux GALEN regression

Source: fresh flamegraphs at HEAD `a38a1f0` (post-Phase-2d + 2c-redux +
4b/4c) on both SIO and GALEN, diffed against the closest available pre-2d
baselines (`docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg`
and `docs/flamegraphs/galen-classify-2026-06-01-post-phase3.svg`).
Workload-of-record: **GALEN regression 12.55 min → 13.36 min (+6.5% wall,
+49 s)** from `docs/phase2d-2c-redux-results.md`.

Both flames archived alongside this doc:
- `docs/flamegraphs/sio-classify-2026-06-02-post-phase2d.svg` (60 s window)
- `docs/flamegraphs/galen-classify-2026-06-02-post-phase2d.svg` (180 s window)

Profile sampler: pprof-rs @ 199 Hz, signal-based, started at program
startup so saturation is fully inside the window on both runs.

## Headline

**The Phase 2d propagation code is below the noise floor of the CPU
flamegraph on the workload that regressed.** `process_subsumer` is 0.12 %
of GALEN samples (49 of 39 971). `push_fact`, `subs_of_class`,
`process_fact` do not appear at all (0 %). The +6.5 % wall regression is
present, but the flame cannot localize where the cost lives.

This is the explicit DONE_WITH_CONCERNS path that T1's task spec authorized:
"if the regression isn't where the propagation lives — it's somewhere else."
It is somewhere else, and we cannot pin it from CPU sampling alone.

## Why CPU sampling cannot localize this regression

`owl_dl_saturation::saturate()` is **single-threaded** and runs ONCE per
classify, at the start (`crates/owl-dl-reasoner/src/classify.rs:411`).
The tableau (`search`/`branch`/`apply_*`) runs in `rayon` across all 8
worker threads — `docs/perf-2026-05-24-new-server.md` shows GALEN classify
saturates 8 cores during the tableau phase.

For a 13.36 min total wall, CPU-time during the tableau phase is roughly
13.36 × 8 ≈ 107 minutes of CPU. A +49 s wall regression that lived
entirely in single-threaded saturation would contribute only +49 s ≈ 0.8 %
of total CPU time — and would be split across all sample buckets along the
call stack inside `WorklistEngine::run`.

Empirically the share is even smaller: `process_subsumer` at 0.12 % of
samples = 0.22 s of single-thread CPU = the wall regression is **at most
∼1 s** if it lives there. The other 48 s must be elsewhere — either
in code that is fully inlined out of frames the sampler attributes by name,
or in tableau-side cascade (more facts in the EL closure → more tableau
follow-up work that's spread across all 8 cores and shows up as small
percentage-point shifts).

## Flamegraph diff — what we observe

### GALEN top frame deltas (pre-2d→post-2d, % of total samples)

| Frame | pre-2d (post-phase3 baseline) | post-2d+2c-redux+4b/4c | Δpp |
|---|---:|---:|---:|
| `saturate` (top-of-search tableau wrapper)  | 70.15 % | 70.98 % | +0.83 |
| `apply_max` | 19.58 % | 25.34 % | +5.75 |
| `first_clash` | 12.98 % | 17.82 % | +4.84 |
| `clash_deps_at` | 12.94 % | 17.78 % | +4.83 |
| `apply_deferred_concept_or_rules` | 22.28 % | 8.33 % | **−13.95** |
| `apply_concept_rules` | 3.59 % | 4.54 % | +0.95 |
| `apply_role_chains` | 1.04 % | 1.42 % | +0.38 |
| `apply_exists` | 3.43 % | 4.09 % | +0.65 |
| `apply_and` | 2.09 % | 2.97 % | +0.88 |
| `eq` (PartialEq<ConceptId> leaf) | 7.42 % | 2.05 % | −5.36 |
| `eq<ConceptRule>` | 6.18 % | 0.00 % | −6.18 |
| `is_sub_role` | 4.48 % | 2.60 % | −1.88 |
| **`process_subsumer`** | 0.04 % | **0.12 %** | +0.08 |
| **`process_fact`** | 0.00 % | **0.02 %** | +0.02 |
| **`push_fact`** | 0.00 % | **0.00 %** | 0.00 |
| **`subs_of_class`** | 0.00 % | **0.00 %** | 0.00 |
| **`supers_of_class`** | 0.00 % | **0.01 %** | +0.01 |

### SIO top frame deltas (60 s window)

Effectively unchanged: total samples 17 888 → 17 641 (−1.4 %, profile
noise). Top frames track to within 1–2 pp. SIO is tableau-dominated and
saturation barely registers either pre- or post-2d. Listed in this recon
only because the spec asks for SIO; it does not pin the regression.

## §16 risk analysis

The candidate fixes the spec enumerates (A: push_fact recursion overhead,
B: `facts_by_sub[d].clone()` alloc removal, C: `seen_facts` HashSet thrash,
D: `facts_snapshot.clone()`, E: `subs_of_class` owned-Vec return) are
**all premature** at this stage. Each would be a workload-neutral micro-
optimisation, so by §16 none would *regress* anything — but the recon
provides no evidence that any of them is the actual hotspot. Shipping
candidate A in particular as a fix would risk a §17-style outcome:
visible-on-paper micro-opt with no measured wall payoff.

The candidate-list classification stands as:
- A (push_fact recursion → manual loop): workload-neutral but
  flame-unsupported.
- B / D (Vec::clone hoisting): workload-neutral but flame-unsupported.
- C (seen_facts probes): structurally similar to §17's irreducible-probe
  finding; **medium risk** for a no-op outcome.
- E (subs_of_class &[]): workload-neutral but flame-unsupported.

## Discarded interpretation: the +13.95 pp shift on `apply_deferred_*`

The most-eye-catching delta in the table — `apply_deferred_concept_or_rules`
22.28 → 8.33 % — is **not** Phase 2d. It is Phase 3d's `concept_rules_by_trigger`
hoist (commit 32aeda6), which landed *after* the pre-2d baseline was
recorded (`docs/flamegraphs/galen-classify-2026-06-01-post-phase3-findings.md`
was captured at commit 64bee92, predating 32aeda6). The matched +5.75 / +4.84
/ +4.83 pp on `apply_max` / `first_clash` / `clash_deps_at` is the renormali-
sation tail of that same Phase 3d landing. The available pre-2d baseline is
**confounded by intervening Phase 3 commits**, so frame-deltas on this pair
mix Phase 2d's effect with Phase 3d/3b/3c gains.

A clean baseline for isolating Phase 2d's cost would require profiling at
HEAD-minus-2d-only (e.g. commit `aab6d03`, called out in
`docs/phase2d-2c-redux-results.md` as the "T2 pre-2d" baseline that ran in
752.83 s).

## Code-trace evidence

The propagation paths the plan worries about are real and live at:

- `crates/owl-dl-saturation/src/lib.rs:469-496` — `push_fact()` with the
  Phase 2d recursive subclass walk (`subs_of_class()` snapshot allocates
  `bs.ones().collect::<Vec<_>>()` per recursion level, line 253-263).
- `crates/owl-dl-saturation/src/lib.rs:619-630` — `process_subsumer()`'s
  inherit loop that clones `facts_by_sub[d]` and re-pushes each fact.
- `crates/owl-dl-saturation/src/lib.rs:860-877` — Phase 2c-redux inner
  loop that snapshots `facts_by_sub[fact.sub]` per witness-merge emission.
- `crates/owl-dl-saturation/src/lib.rs:106,470` — `seen_facts` HashSet
  probe per `push_fact` call.

The Phase 2d counters `phase2d_facts_inherited` (line 163) and
`phase2c_sub_role_propagations` (line 169) confirm the rules fire; the
question is purely whether they cost +49 s on GALEN. The flame does not
answer that question — these symbols are inlined out of frames and the
single-threaded share of total CPU is below 1 %.

## Proposed next step (handoff to T3) — wall-time instrumentation, not a fix

CPU sampling at 199 Hz × single-thread saturation × 8-thread tableau
denominator gives a noise floor of order 1 % per saturation frame. To
attribute the +49 s GALEN regression to a code path, T3 needs **wall-time**
attribution, not CPU sampling. Suggested probe (research-only, would not
ship):

```rust
// crates/owl-dl-reasoner/src/classify.rs, around line 411
let _sat_start = std::time::Instant::now();
let closure = saturate(internal);
eprintln!("EL saturation wall: {:?}", _sat_start.elapsed());
```

Plus a parallel measurement at the pre-2d baseline commit (`aab6d03` per
the results doc). If the saturation wall delta accounts for the 49 s
regression, then T3 has localized the cost to a single function call and
can drill in with internal `Instant::now()` spans across `run()`'s three
worklist branches (`process_subsumer` vs `process_fact` vs `process_unsat`).
If it does NOT account for the regression, the cost is in tableau cascade
caused by the extra +17 closure entries, and the recon target shifts
entirely.

This handoff is deliberately a **diagnostic next step**, not a code fix.
The spec's "if the dominant candidate is workload-dependent (high) OR if
the regression is split across 3+ sub-paths with no clear leader, REPORT
DONE_WITH_CONCERNS" gate triggers here: we found no dominant candidate.

## Status

**DONE_WITH_CONCERNS.** The flame at HEAD `a38a1f0` does not localize the
Phase 2d+2c-redux GALEN regression to any frame T3 could surgically fix.
Confidence T3 can produce a single fix from this recon: **low**. Confidence
that a wall-time-instrumentation probe (1 hour of work) would localize the
regression: **high** — once the saturation wall delta is measured, either
T3 has a target or has eliminated saturation as the source.

## Cross-references

- Phase 2d design: `docs/phase2d-design.md`
- Phase 2d intermediate results: `docs/phase2d-intermediate-results.md`
- Phase 2d + 2c-redux ship results: `docs/phase2d-2c-redux-results.md`
- Pre-2d GALEN baseline (CONFOUNDED — predates Phase 3d):
  `docs/flamegraphs/galen-classify-2026-06-01-post-phase3.svg` +
  `docs/flamegraphs/galen-classify-2026-06-01-post-phase3-findings.md`
- Phase 3d landing (confounds the available baseline): commit 32aeda6
- Phase 3f / §17 dead-end (analogous "no shippable lever" outcome):
  `docs/phase3f-recon.md`, `docs/hypertableau-dead-ends.md §17`
- Recon precedents this doc mirrors: `docs/phase3{d,e,f}-recon.md`
