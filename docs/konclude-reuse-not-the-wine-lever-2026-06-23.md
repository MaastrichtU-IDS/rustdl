# Konclude's model-reuse cache is NOT the wine lever — measured refutation — 2026-06-23

Follows the user's correct push: "if Konclude uses these saturation/reuse techniques
soundly, our FP is an implementation bug, not the technique." That reopened the
model-reuse lever (which rustdl's snapshot cache implemented unsoundly). We set out to
extract Konclude's sound-reuse conditions and scope adopting them. **A direct
measurement refutes reuse as the wine lever before any build.**

## What Konclude's sound reuse actually is (source-mapped)

`Source/Reasoner/Kernel/Cache/CReuseCompletionGraphCache*`. Unlike rustdl's snapshot
cache (which trusted ONE cached model as a subsumption *oracle* — `sup ∈ model ⟹
subsumed`, FP-unsound on non-Horn, the measured reuse-trap), Konclude:

- Caches per-entry `mEntailedValues` (deterministic consequences), `mIncompatibleValues`
  (clash markers), `mMinimalValues` (deterministic-only subset).
- Reuses an entry only when `mEntailedCount ≥ 1 && mIncompatibleCount == 0` (voting),
  and tags each reuse with a **connection level**: 0 deterministic (AND/SOME), 1
  universal (ALL/≥n), 2 domain/range, 3 disjunctive.
- `deterministicConnection = (minConnectionLevel ≤ 1)`. **Deterministic-connection
  reuse is sound by construction** (deterministic consequences hold in every model);
  **non-deterministic (level-3) reuse is flagged and the caller re-verifies** — i.e.
  reuse-*through-construction*, never an oracle verdict.
- Separate config flags `CompletionGraphDeterministicReuse` /
  `CompletionGraphNonDeterministicReuse`; full-graph build gated by
  `ForceFullCompletionGraphConstruction` + `MaximumIndividualLimit`.

So the user is right: the technique is sound, and rustdl's snapshot-cache FP was an
implementation choice (oracle reuse), not the technique.

## The decisive measurement: disable Konclude's caching, time wine

Native Konclude v0.7.0-1138, single-thread (`-w 1`), `ontologies/real/wine.ofn`:

| config | precompute | classify |
|---|---:|---:|
| default (all caching ON) | 44 ms | **119 ms** |
| completion-graph reuse OFF | 66 ms | **225 ms** |
| **ALL caching OFF** (completion-graph + unsat + satisfiable-expansion + saturation-sat caches) | 63 ms | **233 ms** |

**Konclude classifies wine in ~230 ms with every cache disabled.** The reuse/caching
machinery is a ~2× constant factor, not the 1700×-vs-rustdl gap. **Reuse is not the
wine lever.** (This corrects `docs/konclude-vs-rustdl-wine-2026-06-23.md` §4, which
attributed wine's speed primarily to "derive once + reuse.")

## What that leaves as the actual lever

With caching off, wine's speed comes from (a) the 63 ms approximated-saturation
precompute and (b) **per-test tableau efficiency**. rustdl already has equivalents of
(a) — its EL saturator + the Phase-7 label oracle (KPSet-equivalent pruning, 96–100%).
The 8251 wine pairs that time out are co-satisfiable (`sup ∈ label-model(sub) ⟹ C⊓D
SAT`), so they are unprunable by any possible-subsumer method (the SP3 result) — they
must be *tested*. Konclude tests them in aggregate ~230 ms; rustdl's wedge explodes to
168k+ branches **per** such pair (`docs/reuse-trap-nominal-termination-scoping…`,
bjgap≈1, disjunction+≤n-merge dominated).

So the gap is **per-test search efficiency on the nominal+cardinality+disjunction
fragment**, exactly what `[[wine-wall-bjgap1-genuine]]` concluded: rustdl's
`merge_with_cause` folds causation into `birth_deps`, making every clash depend on the
full ancestor context → backjumping is defeated (bjgap≈1), and lemma-learning / sound
caching all collapse together. Konclude's nominal architecture doesn't create those
dense dependency chains. **This measurement independently confirms that diagnosis by
elimination: with caching ruled out, only the per-test architecture remains.**

## Verdict

- **Model-reuse / completion-graph caching: NOT worth building for wine.** Refuted by
  direct measurement (Konclude wine = 230 ms cache-free). At best a ~2× constant
  factor; it does not address rustdl's per-test explosion. (Konclude's reuse IS sound —
  the user's point stands — it's just not the wine lever.)
- **The one real wine lever remains the nominal-architecture change**: stop folding
  merge causation into per-node dependencies so backjumping survives on the
  nominal+cardinality fragment. Its cheaper proxies (1-UIP, MOMS, snapshot reuse) are
  all measured out; the architectural change itself is the deferred multi-month effort,
  unchanged by this investigation.
- Net: the whole saturation+reuse arc (SP0–SP3 + reuse) is closed by measurement. The
  user's soundness correction was right and sharpened the picture — the FP was our bug,
  the reuse is sound in Konclude — but neither saturation completeness, deterministic
  seeding, possible-subsumer pruning, nor model reuse is rustdl's wine lever. Only the
  per-test nominal architecture is.
