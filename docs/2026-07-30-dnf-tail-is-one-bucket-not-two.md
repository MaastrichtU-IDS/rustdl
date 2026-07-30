# The DNF tail is ONE mechanism, not two buckets

**Date:** 2026-07-30
**Status:** Measurement finding. Corrects the Bucket A / Bucket B taxonomy recorded on 2026-06-22.
**Method:** re-measured on current `main` (post fod-restricted-scan, match-plan precompute,
incremental `horn_fixpoint`, and the 2026-07-30 DKey work). No new instrumentation needed — the
existing `RUSTDL_TRACE_RSS` phase probes and the `# wall breakdown ms:` banner sufficed.

## What the old taxonomy said

The 2026-06-22 characterisation split the 13-ontology DNF tail into:

- **Bucket A — per-pair-bound (8):** finish when each pair is capped at 5 ms, so the cost is hard
  individual subsumption pairs (disjunctive branching).
- **Bucket B — label-cache-build-bound (5):** `5438 5548 7499 7712 10080`. *"Still DNF even at
  pair=5ms ⟹ cost is OUTSIDE the per-pair loop."*

That framing drove five weeks of work and a standing recommendation to treat Bucket B as the better
DNF target because its blocker was undiagnosed.

## What is actually true

**1. `ore_ont_7499` is not Bucket B. It is per-pair-bound.**

| config | result |
|---|---|
| default | DNF @180 s |
| `--pair-timeout-ms 5` | **COMPLETE in 28.73 s**, 5,109 classes, `label_cache_build=1,317 ms` |
| `RUSTDL_LABEL_HEURISTIC=0` | DNF @180 s |

Its label-cache build is 1.3 s of a 28.7 s run — nowhere near dominant. Capping the per-pair budget
rescues it, which is the definition of Bucket A. Either it was misclassified or the intervening work
moved it.

**2. The other four do stall in the label-cache build — but removing the build does not help.**

`RUSTDL_TRACE_RSS` phase probes, default config, 100 s budget: `10080` and `5438` both emit `entry`,
`after_saturate`, `before_prepared`, `after_prepared` and **never reach `after_label_cache`**. So the
stall is inside the per-class label-cache build. RSS at that point is small — 0.07 GB and 0.19 GB —
so it is **compute-bound, not memory-bound**. Conversion is not implicated either: `tbox-stats`
completes in ≤1 s for all four.

With `RUSTDL_LABEL_HEURISTIC=0`, the build is replaced by `vec![NoVerdict; n]` and skipped instantly —
both then reach `after_label_cache` and stall in the **tier walk** instead:

| ontology | tier-walk progress in ~100 s |
|---|---|
| `ore_ont_5438` | reaches `pair=500` at 2.18 GB — roughly **5 pairs/sec** |
| `ore_ont_10080` | **zero** pair probes, i.e. fewer than 100 pairs — **>1 s per subsumption test** |

**3. Therefore the Bucket A / Bucket B distinction is not a mechanism difference.** Both buckets are
the same cost — per-class / per-pair wedge satisfiability on dense disjunctive SROIQ — and the label
cache merely relocates it. The 2026-06-22 note itself contained the answer ("the label cache only
MOVES the wedge-sat cost between build and pairs") but the taxonomy built on top of it treated the
two locations as two problems.

Concretely: "still DNF at pair=5 ms" does **not** show the cost is outside the per-pair loop. It shows
the *label-cache build* is unbounded by `--pair-timeout-ms`, which is by design (Phase 8 deliberately
decoupled the cache-build deadline from the per-pair budget). The bucket was an artefact of which
budget each phase honours.

## Consequences

**The DNF tail is a single target.** After today's DKey work the tail is 12: `5964 6485 8273 8666
13545` + `5438 5548 7499 7712 10080` + `4410 5368`. Every one of them reduces to *wedge satisfiability
per class/pair is too slow*, differing only in which phase exhausts its budget first.

**One hypothesis was eliminated today by measurement.** `ore_ont_5548` lost 54% of its disjointness
axioms and half its RSS to the collapse/broadcast split (541,575 → 250,546; 949 MB → 506 MB) and still
DNFs. So the cost is not axiom volume.

**Two candidate levers remain, and they should be judged as one target, not two:**

1. **Share class-independent work across per-class wedge calls — UNTESTED.** The build is
   `(0..n).into_par_iter().map(|i| prepared.classify_labels(class_id, deadline))`: every class
   constructs its own context and derives Horn consequences from scratch (~3,533 fresh contexts on
   `10080`). If the class-independent portion of that derivation dominates a single call, sharing it
   saves `(n−1)×` that portion — and it attacks the cost in *both* locations, since the tier walk pays
   the same per-pair wedge cost. Note `RUSTDL_HYPER_INCREMENTAL_FIXPOINT` (2026-07-14) already does
   this *within* one solve (across branches); doing it *across classes* is the untried step, and is
   the same idea as the parked build-once-classify-many architecture.
   **Caveat that must be measured first:** `10080` spends >1 s on a *single* subsumption test, so the
   per-call cost is already large. Sharing only wins if the shared prefix is a large fraction of one
   call — measure that fraction before building anything.
2. **Clash-driven search / CDCL** — repeatedly NO-GO'd, no cheap entry.

## Method notes

- The `# wall breakdown ms: label_cache_build=… tier_walk=…` banner only prints on a **completed**
  classify, so it is useless on a DNF. The `RUSTDL_TRACE_RSS` phase probes are the right tool there —
  the last marker emitted localises the stall.
- Two self-inflicted measurement errors while doing this, both worth remembering: piping probe output
  through `tail` lost everything when the outer timeout killed the loop (`tail` buffers to EOF), and
  a 4-ontology × 120 s loop under a 10-minute cap silently truncated. Write per-ontology output to
  files and read the files.
