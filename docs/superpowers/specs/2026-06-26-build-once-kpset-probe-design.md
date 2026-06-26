# Stage-3 build-once / KPSet — Phase-1 viability probe (design)

**Status:** design. Phase-1 (gating probe) of build-once/KPSet classification — the lever
from wine 3.2 s (post ∃-seed + tight cap, sound) toward Konclude's ~114 ms. Throwaway-allowed
probe; the verdict doc is the durable deliverable. Phase-2 (production front-end) is a
separate spec, gated on this probe.

## Why — the measured cost

At `--pair-timeout-ms 1` with the ∃-seed, wine classify is 3.2 s, split:
**label_cache_build 1.66 s (137 per-class sat model builds) + tier_walk 1.41 s (2731 pairs,
~1 ms each)** — both driven by the **2470 NoVerdict misses** (classes whose per-class sat
times out → they neither prune their pairs nor finish their build). Build-once attacks both:
**one global model instead of 137**, and tighter/timeout-free pruning.

## Mechanism

Classify = **known** subsumers (the saturation closure — sound) + **possible** subsumers
(read off one model — sound *elimination*), testing only `possible ∖ known`. Today the
"possible" set is built per-class (137 sat models, the label cache). Build-once builds **one
global pseudo-model**: seed a single wedge completion graph with a probe individual `x_C : C`
for every named class C (∃-seeded so it terminates), run to a clash-free completion, and read
each `x_C`'s node labels → C's possible-subsumers (`D ∉ labels(x_C) ⟹ C ⋢ D`).

## Soundness — NOT the reuse-trap

The snapshot-cache FP came from inferring *subsumptions* from a model (`sup ∈ model ⇏
sub ⊑ sup` on non-Horn). Build-once KPSet does the **opposite, sound directions only**:
- **known** subsumers come from **saturation** (sound closure) — never from the model.
- the model is used only to **rule out** non-subsumers: `x_C` is a genuine C-instance in a
  clash-free model, so `D ∉ labels(x_C)` is a real counter-model ⟹ `C ⋢ D` (sound — exactly
  what the shipped Phase-7 per-class label heuristic does, now read off one shared model).

A class **not instantiated** in the global model (empty node) yields no pruning → its pairs
stay "possible" and are tested by the (sound) per-pair refutation. So coarseness costs
*speed*, never soundness. **The Phase-2 full-corpus FP=0/MISSED=0 gate is still the proof.**

## The open question (what the probe measures)

One global model makes *one* disjunctive choice, so its possible-set is sound but **coarser**
than 137 tailored per-class counter-models — it may prune fewer pairs (larger residual walk),
and the joint 137-probe model is a bigger disjunctive search than a single-class sat (the
∃-seed mitigates, but jointness may cost). The probe decides whether build-once is a net win.

**Phase-1 probe — build one ∃-seeded global model of wine, measure:**
1. **Build time** of the single global model vs the 1.66 s of 137 per-class builds.
2. **Prune rate / residual pair count** — how many pairs `possible ∖ known` leaves, vs the
   label cache's 11 286 pruned / 2731 walked. (Coarser ⇒ more residual.)
3. **Soundness spot-check** — the possible∖known classification reproduces wine 653=653
   (FP=0/MISSED=0) when the residual is refuted at a tight cap.
4. **Coverage** — how many of the 137 classes are instantiated (get pruning info) vs empty.

**Verdict (pre-committed):** GO iff the global model builds in ≪ 1.66 s **AND** prunes
comparably to the per-class cache (residual walk not much larger) **AND** is sound (653=653).
→ Phase-2 (wire as the classify front-end + full-corpus FP=0/MISSED=0 + net wall). NO-GO
(one model too coarse / too slow / unsound) → fall back to amortizing the per-class builds
(share the deterministic core — smaller, sound, lower ceiling) and lean on the Stage-2
∃-seed increments (fewer misses).

## Scope / non-goals

- Phase-1 **probe only** (a `build_once_kpset_probe` fn + an `#[ignore]` gate measuring the
  four numbers). Throwaway code; the verdict doc `docs/build-once-kpset-probe-results-2026-06-26.md`
  is durable. The global-model builder is the keep-on-GO piece.
- No production wiring, no default change in Phase-1. Phase-2 is a separate spec.
- Reuses: the ∃-seed (`exists_seed`/`saturate_with_exists_facts`), the seeded-wedge construction
  (`HyperEngine::new_seeded` / the ABox-seed path) for the 137-probe model, the saturation
  closure for `known`.
- Branch `feat/build-once-kpset-probe` off `feat/sat-precompletion-sp3-prod`. `main` untouched.

## Global constraints

- Soundness: Phase-1 = the wine 653=653 spot-check on the possible∖known split; full-corpus
  FP=0/MISSED=0 is Phase-2's gate.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D
  warnings` (pedantic) clean; `cargo test --workspace` green.
- Toolchain `RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo` + stable
  bin on PATH.
- Commit only when asked; trailers `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` +
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.
