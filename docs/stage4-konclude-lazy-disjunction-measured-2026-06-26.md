# Stage-4 Konclude path — component 1 (lazy disjunction) measured: NOT the lever

**Status:** measurement (durable, throwaway probe on feat/stage4-engine-characterization).
First increment of the committed Konclude path, run as a throwaway BEFORE any spec (the
advisor's measure-don't-spec discipline, after 3 prior inferences about these branches).

## Experiment

`RUSTDL_SKIP_NAMED_BINARY=137` in `find_open_disjunction`: skip branching on binary disjunctions
whose both disjuncts are named classes (id < 137 = wine's named-class count) — the `[4,25]`
(`ConsumableThing ⊔ NonConsumableThing`)-shaped excluded-middle/partition the recon flagged on
generated descriptor successors. Crude + UNsound (drops real obligations too); throwaway to measure
whether those branches drive Gamay's explosion.

## Result (sat(Gamay), adaptive-budget off)

- baseline (no skip): ~451k branches / 30s / **Stalled** (established this session).
- skip named-binary: **200,117 branches / 12.8s / Sat**.

Skipping helps — Gamay now *terminates* (Stalled→Sat), ~2.3× fewer branches — but **200k is still
huge**, far from the ~tens that would mean lazy-disjunction is the lever. This is the
*over-aggressive* skip (all named binaries); a SOUND tautology-only skip (exhaustive partitions
like `A ⊔ ¬A` only) would skip strictly fewer and reduce strictly less. So the ceiling of
lazy-disjunction is ≤200k — not the wall.

## Verdict

**Lazy disjunction (tautology/irrelevant-partition skip) is NOT the Konclude-path lever** — it is at
most a minor component (termination + ~2.3× on Gamay, FP-safe-by-construction if done soundly). The
451k explosion is dominated by the **interacting compound Tseitin wine-type-definition disjunctions**
(the synthetic `[308,318]` / `[193,268,192,257,288]`-shaped ⊔ points on the home node), exactly as
the linear argument predicted (irrelevant partitions cost ~linear; the product lives in the
definitions). Konclude collapses those via **saturation-driven deterministic expansion** of the
definition disjunctions — richer than told-disjointness (SP-B pruned=0) — i.e. the integrated
∀+nominal+≤n expansion. That is the hard, multi-component core of the Konclude path, with no cheap
entry: the cheap component (this one) is now measured out.

## Next (Konclude path)

The remaining 200k branches are on compound wine-type-definition disjunctions. The lever is their
deterministic resolution (integrated saturation/nominal expansion) — the large multi-component build.
Lazy-disjunction can be banked as a sound minor component (termination + 2.3×, FP-safe) but is not
the headline. main untouched (ee6904c+merge); this is a throwaway probe.
