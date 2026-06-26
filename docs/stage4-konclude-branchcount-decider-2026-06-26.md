# Stage-4 — Konclude branch-count decider (determinism is real → M1)

**Status:** verdict (durable). The advisor elevated the Konclude branch-count from "confirmation"
to **the decider** after M2 came back sound-but-inert: M2's inertness is the first empirical crack
in the "determinism is the lever" inference (a real Konclude mechanism, ported soundly, derived
nothing). The hole in the elimination argument: it ruled out reuse and backjumping as Konclude's
edge, but **not raw per-branch cost** — Konclude's 114ms is consistent with *few branches*
(determinism → M1 worth building) OR *the same many branches at far lower per-branch cost* (no
determinism → M1 also inert; the lever is per-branch/representation). This measurement decides it.

## Measurement (Konclude v0.7.0, docker `konclude/konclude:latest`)

Full wine classification, `classification -v -i wine.ofn`:
- parse 20 ms, preprocess 7–12 ms, **precompute (SHOIN) 46–63 ms**, **class classification 123–132 ms**,
  total ~190–242 ms (docker wall 0.74 s incl. startup). Reproduced 3×.
- Statistics config (`Konclude.Calculation.Classification.CollectProcessStatistics` etc.) collects
  counts internally but the `classification` command does not surface them on stdout (they return
  only via the OWLlink RetrieveKB-statistics response — not worth the flow, the bound below is
  decisive).

## The total-time bound (rules out the per-branch-cost escape)

- Konclude resolves **all 137 wine classes** (all candidate pairs, SHOIN) in **~132 ms** of
  class-classification.
- rustdl `sat(Gamay)` = **451,175 branches and did NOT finish in 30 s** (Stalled; measured this
  session, minimal-sound-key gate). Hard wine classes are 10⁵–10⁶ branches each; rustdl per-branch
  ≈ 43 µs (irreducible match/propagation ~30 µs even discounting the graph-clone).
- For Konclude to resolve Gamay by the **same** ~451k-branch search: even granting Gamay
  Konclude's *entire* 132 ms, that is 0.29 µs/branch; since Gamay shares the budget with 136 other
  classes it actually gets ≪132 ms, forcing **sub-100 ns/branch** — below the physical floor of a
  single tableau branch (a label insert + clash check + propagation step). No physically-plausible
  per-branch cost lets 10⁵–10⁶-branch searches on the hard classes fit in 190 ms total.

Crucially this is **not** the earlier circular "divide time by an assumed per-branch cost" estimate
(the advisor's warning): the conclusion holds for **any** per-branch cost above a physical floor of
tens of ns. The branch *count* on wine's hard classes must be orders of magnitude smaller in
Konclude than in rustdl.

## Verdict — determinism is real (case A) → build M1

Konclude resolves wine's hard classes in dramatically fewer branches than rustdl. The speed is
**determinism**, not many-cheap-branches. Combined with the representation study (Konclude has no
algebraic cardinality; its reuse side-steps nominals; M2/cardinality-exclusion is inert in rustdl
because its consumer is missing), the load-bearing mechanism is **M1: Konclude's GCI absorption of
the value-choice disjunction (`∀R.{v₁,…,vₙ}`) into a deterministic triggered implication**, so
excluding n−1 values forces the last *without branching*. M2 (cardinality+distinctness → `¬vᵢ`
exclusions) is the *supplier*; M1 is the *consumer* that turns exclusions into deterministic value
resolution. rustdl has no M1 analog for the nominal-value `∀R.OneOf` disjunction (its forced-
disjunct/B2a is atomic-only), which is exactly why M2 alone was inert.

**Caveat (honest):** this is a total-time *bound*, not a direct branch count (the OWLlink stats
interface would give the exact number; deferred as the bound is decisive). And the study flagged M1
as the *more entangled* mechanism — Konclude's absorption is tied to its preprocessing +
priority-queue machinery, so "identified" does not guarantee "cleanly portable." Budget for an M1
build that may need both the absorption rule AND a determinism-cache/priority mechanism to fire,
and gate it with the same wine-FP-first discipline (M1 is disjunction pruning on the nominal
fragment — the exact shape that has been unsound three times).

## Next

M1 is the lever. It is a larger, more entangled build than M2. Decision point for the user:
commit to the M1 build (nominal-value disjunction → deterministic absorption, FP-gated wine-first),
with eyes open that it may require Konclude's trigger/priority machinery to actually fire and is
the disjunction-pruning shape that demands the strictest FP gate.
