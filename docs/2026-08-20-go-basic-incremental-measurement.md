# GO-basic incremental measurement — the reuse path works, and the ceiling is ~2.9× not ~13×

**Date:** 2026-08-20
**Purpose:** settle the open question left by the P1 exit criterion — `closure_answered = 0` on all
eight local ontologies meant either "the `is_pure_el` gate is too narrow to ever pay off" or "the
local corpus happens to be out of fragment." This answers it.

## Provenance (recorded this time)

| | |
|---|---|
| source | `http://purl.obolibrary.org/obo/go/go-basic.obo` (in `scripts/fetch-real-ontologies.sh`) |
| ontology IRI | `http://purl.obolibrary.org/obo/go.owl` |
| `.obo` | 32,227,785 bytes, sha256 `b08d45b268b8c24c…` |
| `.ofn` | 83,345,259 bytes, sha256 `786e9e6913fb01a8…` |
| converted by | `obolibrary/robot:v1.9.6` |
| classes | **51,986** (19× galen) |
| host | Apple M5 Max, 128 GB, `--release` |

## Result 1 — the gate has a real-world consumer

```
# mode: pure EL (saturation-only)
# fragment: pure-EL (trust_sat sound by construction; saturator alone is complete)
# subsumption: saturation=355356 tableau=0
```

GO-basic is genuinely in the fragment, at scale, with the tableau doing **zero** work. So the
`closure_answered = 0` result across galen/sio/ro/mie/sulo/paper5/pizza/family was a **property of
the local corpus, not of the gate.** The narrower reading of the P1 failure is refuted.

## Result 2 — the reuse path fires, and delivers ~2×

100 single-axiom class additions, anchor `GO_0000001`:

| metric | value |
|---|---|
| baseline classify (from scratch, median of 3) | 549.49 ms |
| baseline saturation-only (from scratch, median of 3) | 512.12 ms |
| session build | 570.19 ms |
| **per-revision p50** | **256.85 ms** |
| p95 / max / min | 276.93 / 445.97 / 247.62 ms |
| — of which `apply` | 177.37 ms |
| — of which `classify` | 79.40 ms |
| **`closure_answered`** | **101** ← first non-zero on any real ontology |
| `additions_reused` | 99 |
| `rebuilds` | 1, at revision 64 |
| **speedup vs from-scratch classify** | **2.14×** |
| speedup vs from-scratch saturation-only | 1.99× |

For calibration, KM publishes **4.90×** on its addition-only EL++ microbench. We are at 2.14×.

The single rebuild at revision 64 is `INITIAL_SLACK = 64` exhausting exactly as predicted, and it
is the 445.97 ms max. Slack then doubles to 128, so the next would land at revision 192 —
amortized cost is geometric and negligible, but it is the p-max outlier.

## Result 3 — CORRECTION: my floor-trend claim was wrong

`docs/2026-08-19-incremental-lowering-floor-findings.md` reported the lowering floor's *share* of a
saturation-only classify **falling** with size — 41.5 % at 101 classes, 30.5 % at 99, 11.5 % at
1,592, 7.6 % at 2,748 — and concluded "the share keeps decreasing as ontologies grow," extrapolating
a **~13× ceiling** on galen.

At 51,986 classes the share is **177.37 / 512.12 = 34.6 %.** The trend did not continue; it
inverted. Therefore:

- **The real ceiling at GO scale is ~2.9×** (`512.12 / 177.37`), not ~13×.
- Measured 1.99× against saturation-only is **≈69 % of that ceiling** — the design is capturing most
  of what is attainable. The remaining gap is `classify`'s 79 ms.
- **The ~13× galen figure must not be quoted as a target for large ontologies.** It was an
  extrapolation from a trend that reverses.

**Why it reverses:** `apply` re-lowers the whole union against a pre-seeded allocator and
multiset-diffs it (Task 6's deliberate choice — a delta-only compile silently drops range folding),
and `refresh_derived` re-runs all four whole-ontology derivation passes. Both are O(|ontology|). At
galen scale that is 4.63 ms against an 882 ms classify, so it vanishes; at GO scale it is 177 ms
against a 79 ms classify and **becomes the dominant cost.** The bottleneck has inverted.

## What this means for P2

- **The feature is real.** 2.14× end-to-end on a 52k-class ontology with 99/100 additions reused.
  P1 is worth keeping.
- **The exit criterion should be re-pointed here, not at galen** — and re-derived. A ≤12 ms bar was
  meaningless; a defensible bar is "within X % of the measured ceiling," which is ~2.9× here.
- **The optimisation target has moved.** Further speedup comes from making `apply` sub-linear, not
  from the closure reuse, which is already working. That means incrementalising the derivation
  passes — which my floor doc explicitly deprioritised on the grounds that they were only ~30 % of a
  5.8 ms floor. At GO scale that reasoning no longer holds.
- **Size `INITIAL_SLACK` against P0's edit-locality distribution.** 64 is demonstrably too small for
  a 52k-class ontology: it exhausts in 64 edits and costs a full rebuild.

## Reproduce

```sh
./scripts/fetch-real-ontologies.sh          # needs Docker + GNU stat; on macOS fetch/convert by hand
cargo run --release -p owl-dl-bench -- incremental-latency \
    ontologies/real/go-basic.ofn --revisions 100
```
