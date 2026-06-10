# Scoping: tighter tableau blocking / smaller graphs (2026-06-10)

Scoping the fix for the alehif memory smell
(`docs/tableau-memory-investigation-2026-06-10.md`: 247 classes, 1.47 GB / 6.5 s
vs Konclude 60 MB / 0.18 s). **The premise was "add anywhere blocking" — but
scoping the code overturned it.**

## Premise correction: anywhere blocking is ALREADY implemented
The default per-pair satisfiability engine is the hyper **wedge**
(`hyper_subsumption_probe` → `HyperEngine`, lib.rs:478/574), NOT the main
tableau. Its `is_blocked` (hyper.rs:780) **already does anywhere blocking**, and
`with_double_blocking()` is **ON by default** (`RUSTDL_HYPER_DOUBLE_BLOCK`
defaults on). In that mode it does **anywhere subset-PAIRWISE** blocking: `m`
blocks `n` iff `m` is older, same parent-role, `L(n)⊆L(m)` AND
`L(parent(n))⊆L(parent(m))` — inverse-safe (the pair condition) and indexed by
parent-role (`block_index`, O(bucket)). (The main tableau's `is_blocked`
(lib.rs:726) is the weaker ancestor-only pair blocking, but alehif's 167 probes
run through the wedge, not it.)

So "implement anywhere blocking" is **done**. The residual blowup is NOT a
missing-blocking problem.

## Re-diagnosis: what actually drives the 1.47 GB
1. **Parallel fan-out** (confirmed): peak ≈ `#cores(32) × per-pair-wedge-model
   (~30 MB)`; single-thread is 42 MB. Not a leak.
2. **Per-pair model duplication**: classify builds an **independent wedge model
   per pair** (167 on alehif). Each `q ∧ ¬sup` model, even with anywhere
   subset-pairwise blocking, has many nodes on the inverse fragment (pair
   blocking is inherently conservative — the parent-label-subset condition is
   hard to satisfy, so blocking fires late). 167 independent ~30 MB models,
   none reused, × 32 parallel workers.

So the cost is **model size × duplication × parallelism**, not absent blocking.

## Levers (scoped)
- **L1 — mitigation, ~0 effort, ship now.** `RAYON_NUM_THREADS` already bounds
  the fan-out (8 → 258 MB / 21.6 s). Document it; optionally add a memory-aware
  default thread cap for the hybrid phase (leave EL/saturation at full width —
  it's lean). Zero soundness risk.
- **L2 — the deciding diagnostic (1st real step, small).** Aggregate
  `SearchStats.{is_blocked_calls, blocks_fired}` + max `node_count` across ALL
  classify wedge probes (currently per-probe, only "interesting" pairs
  retained — add sums to `HyperSubProbe`/`ClassificationStats`). Then on alehif:
  - `blocks_fired ≈ 0` with large `is_blocked_calls` ⟹ the wedge `block_index`
    is under-firing on alehif's structure → a **fixable wedge bug** (cheap win:
    candidate bucketing / prefilter). hyper.rs:325 explicitly calls this case
    out as the signal.
  - `blocks_fired > 0` but graphs still large ⟹ inherent pair-blocking
    conservativeness → only L3 helps.
- **L3 — the real fix, LARGE (architectural).** Eliminate the 167× model
  duplication: **global model construction** (build one model, read the
  hierarchy off it — the Konclude/HermiT approach) OR sound sub-model reuse
  across pairs. This is the "global model" rewrite raised earlier this session
  (deemed a rewrite) and overlaps the reuse-trap / snapshot-cache (FP-unsound on
  non-Horn, currently default-off). Attacks size + duplication at once; fixes
  memory AND the SROIQ wall. FP-critical; needs the full FP=0 gate + its own
  scoping spec.
- **L4 — tighter per-model blocking, limited headroom.** Cache blocking
  "cores", better candidate selection within `block_index`. Bounded upside;
  FP-sensitive. Lower priority than L2/L3.

## Recommendation
1. **L1 now** (document the `RAYON_NUM_THREADS` knob; it's the honest immediate
   answer for memory-constrained deployments).
2. **L2 next if pursuing** — it's small and decides whether a cheap wedge-bug
   win exists or it's L3-only. Do this before any L3 commitment.
3. **L3 only if SROIQ performance becomes a priority** — it's the previously
   identified global-model rewrite, large and FP-critical, and it sits OUTSIDE
   the EL/Horn embeddable niche (which is already lean: GALEN 30 MB). For the
   current Resource-track positioning, SROIQ perf is not on the critical path.

Verification for any change: env-flag gate (A/B) + the full FP=0/MISSED=0 corpus
gate + alehif/ore-10908/ore-15672 closure parity + memory re-measure.

## Correction logged
The prior `docs/tableau-memory-investigation-2026-06-10.md` named "anywhere
blocking" as the fix; that was based on the main-tableau `is_blocked` and missed
that the wedge (the actual per-pair engine) already implements it. Corrected
there and in memory.
