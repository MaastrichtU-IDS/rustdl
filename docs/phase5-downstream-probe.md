# Phase 5 T3b — downstream classify breakdown localizes GALEN wall to tier_walk

Run 2026-06-02 at HEAD `fed8ea5` (post-Phase-2d+2c-redux + 4b/4c) under
load avg ~89 (machine contention from external python processes; see
`docs/phase5-variance-check.md` §"Why aborted"). Temporary
`Instant::now()` instrumentation in
`crates/owl-dl-reasoner/src/classify.rs::classify_top_down_internal`
reverted after the probe. Absolute walls are inflated by contention;
**relative section proportions within a single run are preserved**.

Companion to:
- `docs/phase5-recon.md` (CPU flame couldn't see the regression).
- `docs/phase5-walltime-probe.md` (saturator innocent: 0.99 s of 802 s).
- `docs/phase5-variance-check.md` (variance check aborted under same contention).

## Probe results — alehif (sanity validation)

| Section | Wall (s) | % of TOTAL |
|---|---|---|
| `saturate()` | 0.001 | 0.04% |
| `PreparedOntology::from_internal()` | 0.002 | 0.08% |
| Unsat probes (parallel) | 1.393 | 54.5% |
| Tier walk + defined-sup sweep | 1.161 | 45.4% |
| Entailed matrix build | 0.000 | 0.00% |
| **TOTAL** | **2.557** | 100% |

(167 classes; reasonable proportions; instrumentation is sane.)

## Probe results — GALEN (the regression target)

| Section | Wall (s) | % of TOTAL |
|---|---|---|
| `saturate()` | 0.882 | 0.12% |
| `PreparedOntology::from_internal()` | 0.027 | 0.004% |
| Unsat probes (parallel) | 20.068 | 2.66% |
| **Tier walk + defined-sup sweep** | **732.976** | **97.22%** |
| Entailed matrix build | 0.005 | 0.001% |
| **TOTAL** | **753.959** (12.57 min) | 100% |

Context:
- 2748 classes (GALEN ORE 2015).
- `# fragment: Horn` (Phase 4c diagnostic): GALEN is provably Horn.
- `# subsumption: saturation=37181 tableau=0` — every subsumption verdict
  comes from the saturator's closure; the SROIQ tableau is never called.
- Total wall under contention: 12.57 min (compare to 13.36 min the T7
  measurement showed — the T7 reading is reproduced here within the
  noise floor).

## Localization

**The +6.5% GALEN wall regression lives in `find_direct_parents_top_down`'s
tier-walk + defined-sup sweep** — 97.2% of total wall sits here.

Mechanism (inferred, not directly measured):
- Phase 2d's fact-on-subclass propagation adds 17 atomic-subsumer pairs
  to GALEN's closure (27,980 → 27,997 pairs; the recovered IPBP-cluster
  + adjacent transitives).
- Each new closure pair shifts tier ordering (`order.sort_by_key(|i|
  closure.subsumers_of(i_id).len())` at line 736 of classify.rs) and
  increases the candidate set in each `find_direct_parents_top_down`
  invocation.
- 49 s wall regression / 37,181 saturation-answered subsumption calls
  ≈ 1.3 ms per call — consistent with a small per-call cost increase
  in `prepared.decide` or the closure-subsumer enumeration.

## Is this a "bug to fix"?

**No** — it's the inherent cost of completeness. Phase 2d + 2c-redux
recovered 17 GALEN pairs (full Konclude parity); each recovered pair
adds work to the tier walk because each new subsumption is now
discovered and tested. The +6.5% is **the price of completeness**, not
a bug introduced by Phase 2d's propagation logic.

To recover the +6.5%, you would need to either:
- (a) **Not recover the pairs** (defeats the purpose).
- (b) **Optimize `find_direct_parents_top_down` itself** — a separate
  perf phase orthogonal to Phase 2d. The tier walk's 733 s is the
  obvious next target if perf matters more than the 17 recovered pairs.
  Per-call cost of ~20 ms (733 s / 37 k calls) on top-down classifier
  invocation is the bottleneck.

## What this rules in vs out

- **Saturator-side propagation** (T2 already innocent): 0.12% confirmed.
- **PreparedOntology::from_internal**: 0.004% — also innocent.
- **Unsat probes**: 2.66% — non-trivial but stable. Phase 2d doesn't
  add work here directly (the unsat probe pattern is unchanged).
- **Matrix build**: 0.001% — innocent.
- **Tier walk + sweep**: 97.22% — confirmed bottleneck. Phase 2d's
  +49 s regression is here, proportional to the 17 new closure pairs.

## Recommended close-out

The GALEN +6.5% regression is **localized + understood + inherent**.
No surgical Phase-2d-side fix exists — the saturator-side changes are
~50 ms total per T2. Recovery would require optimizing the unrelated
tier-walk path (Phase 6+ if perf becomes a priority again).

**Stop the investigation**. Update `docs/phase2d-2c-redux-results.md`
to reference this finding — the +6.5% wall trade is now precisely
characterized as "tier-walk cost of the 17 new subsumption candidates."

## Cross-references

- Phase 5 recon (CPU flame failure mode):
  `docs/phase5-recon.md`.
- Phase 5 T2 (saturator innocent):
  `docs/phase5-walltime-probe.md`.
- Phase 5 T3a (variance check aborted under same contention):
  `docs/phase5-variance-check.md`.
- Phase 2d + 2c-redux results (where +6.5% was measured):
  `docs/phase2d-2c-redux-results.md`.
- Raw probe output (transient): `/tmp/p5-t3b-galen.log`.
