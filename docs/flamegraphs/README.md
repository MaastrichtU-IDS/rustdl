# Flamegraphs

CPU flamegraphs of rustdl reasoning runs, used to back the Phase A
findings in [`outperform-hermit-plan.md`](../outperform-hermit-plan.md)
and [`perf-2026-05-24-new-server.md`](../perf-2026-05-24-new-server.md).

## How to generate

`owl-dl-bench` carries a `profile` feature that wraps a run in
[`pprof-rs`](https://crates.io/crates/pprof). Sampling is signal-based
(SIGPROF), so no kernel privileges or `perf_event_paranoid` changes are
required.

```sh
cargo build -p owl-dl-bench --release --features profile

RUSTDL_PROFILE=docs/flamegraphs/<name>.svg \
RUSTDL_PROFILE_SECONDS=45 \
    ./target/release/owl-dl-bench classify <ontology.ofn>
```

`RUSTDL_PROFILE_SECONDS` (default 60) bounds the wall-clock budget. The
classify call runs in a worker thread; when the timer fires the
flamegraph is written and the process exits cleanly (exit 0), so
non-terminating workloads like `pizza.ofn` still produce an artifact.
If the body finishes before the timer, its exit status is propagated.

## Inventory

| File | Workload | Sampler | Window |
|---|---|---|---|
| [`pizza-2026-05-24.svg`](pizza-2026-05-24.svg) | `ontologies/real/pizza.ofn` classify, no per-pair timeout, **pre-B.1** (DepSet = `Vec<u32>`) | pprof, 199 Hz | 45 s wall (~12,385 samples across 32 workers) |
| [`pizza-2026-05-24-post-b1.svg`](pizza-2026-05-24-post-b1.svg) | same workload, **post-B.1** (DepSet = `SmallVec<[u32; 1]>`) | pprof, 199 Hz | 45 s wall (~12,325 samples) |
| [`pizza-2026-05-24-post-b4.svg`](pizza-2026-05-24-post-b4.svg) | same workload, **post-B.4 surface** (label_sig bloom prefilter on `Node`) | pprof, 199 Hz | 45 s wall (~12,497 samples) |
| [`pizza-2026-05-24-post-soa-fixed.svg`](pizza-2026-05-24-post-soa-fixed.svg) | same workload, **post-B.4 final** (label_sig + SoA `BlockingSummary` mirror, with merge-path fix) | pprof, 199 Hz | 45 s wall (~14,232 samples — 15 % more iterations per second) |

## Findings — pizza.ofn (2026-05-24)

Hot frames, ≥1 % of samples, deduplicated:

| Function | Samples | % of total |
|---|---:|---:|
| `apply_role_chains` ([rules.rs:736](../../crates/owl-dl-tableau/src/rules.rs#L736)) | 12,361 | **99.81 %** |
| `parent` (graph ancestor walk) | 7,480 | 60.40 % |
| `is_subset_sorted` ([lib.rs:926](../../crates/owl-dl-tableau/src/lib.rs#L926)) | 3,410 | 27.53 % |
| `cmp` (ConceptId comparison inside subset check) | 2,272 | 18.34 % |

These nest: `apply_role_chains` calls `is_blocked`, which walks
ancestors via `parent` and pair-blocks each one via `is_subset_sorted`
on the label sets. Pizza's `partOf`/`hasIngredient` chains drive a
combinatorial blow-up in this region.

### Why `apply_role_chains` burns

Reading the body at [rules.rs:736–855](../../crates/owl-dl-tableau/src/rules.rs#L736-L855),
the hot loop has several avoidable allocations per call, all in the
inner hot path:

1. `let chains: Vec<(Role, Role, Role)> = ctx.chains().to_vec();` —
   clones the chains list on every invocation per node.
2. `let outgoing: Vec<...>` and `let incoming: Vec<...>` snapshot the
   node's edge lists with **per-edge `DepSet::clone()`**. `DepSet` is
   `Vec<u32>`, which means an allocation per edge per call.
3. The `mids` and `tails` `filter_map(...).collect()` calls **clone
   each matching `DepSet` again** to land in a fresh `Vec`.
4. `pending.iter_mut().find(...)` does an O(n) linear scan to dedup
   the `(sup, tail_res)` pair before appending.
5. `chain_edge_already_present` does another O(n) scan over
   `node.edges()` per candidate tail.

[`outperform-hermit-plan.md`](../outperform-hermit-plan.md) §"Phase B"
already names the matching mitigations:

- **B.1 DepSet representation tuning** — `SmallVec<[u32; 1]>` or
  `u128` bitmask removes the per-edge allocation. This addresses
  points 2 and 3.
- **B.2 lazy unfolding** — fewer labels per node ⇒ smaller
  `is_subset_sorted` work in pair blocking. Addresses the 27.53 %
  in `is_subset_sorted` and 18.34 % in `cmp`.
- **B.4 anywhere blocking / subset blocking combined** — replaces the
  `parent`-walk linear ancestor scan that's currently 60.40 %.

### Caveat — sample distribution is by inclusive frame

The 99.81 % attached to `apply_role_chains` is **inclusive** time
(it's an ancestor frame of nearly every leaf sample). The
exclusive-time picture is the rows below it: `parent` (60.40 %) and
`is_subset_sorted` (27.53 %) are the actual hot leaves, both inside
the per-rule `is_blocked` check. Anywhere-blocking + DepSet tuning
hits both directly.
