# Sparse `Classification.entailed` matrix — results (2026-07-21)

Implements
`docs/superpowers/specs/2026-07-21-sparse-classification-entailed-matrix-spec.md`
on branch `feat/sparse-classification-entailed`.

## What changed

`crates/owl-dl-reasoner/src/classify.rs` only:

- `Classification.entailed` is now a private adaptive `EntailmentMatrix`:
  - `Dense(Vec<FixedBitSet>)` for `n <= dense_max()` (60 000; test-only env
    override `RUSTDL_CLASSIFY_DENSE_MAX`) — every curated fixture stays on
    the historical dense path.
  - `Sparse(Vec<Vec<u32>>)` for the ORE giants — each row an
    ascending-sorted `Vec<u32>` of subsumer ids (`ore_ont_868`: 112 GB
    dense → a few hundred MB sparse).
- Unsatisfiable classes' rows are ELIDED in both arms. The trivial
  "⊥ ⊑ everything" fill (previously a per-row `insert_range(..n)`) is
  reintroduced solely by the new private `entails(i, j)` choke-point.
  Invariant: a satisfiable class's row contains only satisfiable supers.
- The three accessors (`is_subclass`, `equivalent_classes`,
  `direct_subsumers`) route every row read through `entails` and iterate
  the row O(k) instead of scanning `0..n` O(n) — public signatures and
  ascending-id output order unchanged. The unsat-subject arm of
  `direct_subsumers` keeps the `0..n` scan (rare, degenerate).
- All four builder sites (`classify` n²-pairwise, `classify_pure_el`,
  `classify_inconsistent`, `classify_top_down_internal`) write through
  `EntailmentMatrix::insert`. The n²-path per-pair `work` loop is
  unchanged per spec §2 (no giant enters it).

## Payoff: `ore_ont_868` real `classify` (`gate868.sh`, 150 GB / 20 min watchdog)

`ore_ont_868` = 981 151 classes, pure-EL, routes to `classify_pure_el`
(Sparse arm, since 981 151 > 60 000).

| | before (2026-07-21 baseline) | after |
|---|---|---|
| status | **TIMEOUT at 20 min, unfinished** | **ok, exit 0** |
| wall | ≥ 1200 s (killed) | **67 s** |
| peak RSS | **116 GB** | **3.3 GB** |
| output | 372 948 lines, incomplete | **981 153 lines, complete** |
| direct edges | — (never finished) | **981 144** |

Both walls collapse. The 112 GB dense `entailed` matrix is gone — the
sparse rep holds the real closure (14 809 043 subsumption entries ≈ tens
of MB of `Vec<u32>`), and the O(k)-per-class accessors let the CLI print
all 981 144 direct edges in 67 s total instead of not finishing the O(n²)
scan in 20 min.

The residual 3.3 GB peak is the EL **saturator's** own closure build
(hit at t≈10 s, before any hierarchy print), NOT the `entailed` matrix —
i.e. the separate D4 dense-saturator-matrix concern
(`docs/known-limitations` / MEMORY `d4-saturator-dense-matrix-memory`),
out of scope for this arc. The spec's "sub-GB" was an estimate of the
matrix contribution alone; the matrix contribution did drop from 112 GB
to ~tens of MB, and total peak fell 116 GB → 3.3 GB (≈ 35×).

## Gates

- **Gate 2 — dense-vs-sparse semantic identity unit test**
  (`crates/owl-dl-reasoner/tests/sparse_classification_identity.rs`; TDD,
  committed failing first): builds one `Classification` twice (default
  Dense vs `RUSTDL_CLASSIFY_DENSE_MAX=0` forced Sparse) over a fixture
  with an unsatisfiable class, an equivalence pair, and a 3-level chain;
  asserts all three accessors agree as ORDERED outputs for every
  pair/class (incl. unsat subjects) and that each arm actually engaged.
  **PASS.**
- **Gate 1 — dense-vs-sparse self-diff on real fixtures**: `rustdl
  classify` at default threshold vs `RUSTDL_CLASSIFY_DENSE_MAX=0`
  (all-Sparse), non-`#` hierarchy lines compared:
  - galen (2748 classes, pure-EL path): **rawdiff=0** (byte-identical,
    unsorted).
  - sio (1585 classes, hybrid top-down path): **rawdiff=0**.
  Because the dense path is Konclude-validated (FP=0/MISSED=0
  corpus-wide), byte-identical sparse output is transitively
  Konclude-validated. **PASS.**
- **Gate 3 — default-path perf non-regression**: galen classify wall
  0.31 s before → 0.25–0.35 s after (3 repeats) — unchanged within
  noise (galen 2748 ≪ 60k stays Dense). **PASS.**
- **Gate 4 (approximated locally) — before-vs-after byte-identity at the
  default threshold**: hierarchy output (non-`#` lines) of the pre-change
  HEAD binary vs the post-change binary is **diff=0** on galen, notgalen,
  sio, pizza, alehif-test, ro, bibtex, wine — the closures are
  byte-identical, so the corpus FP=0/MISSED=0 status is preserved by
  identity. **PASS.**
- **Gate 5 — workspace health**: `cargo test --workspace --all-targets
  --exclude owl-dl-py` + `--doc` (the CI invocations; `owl-dl-py` is
  CI-excluded because its pyo3 `extension-module` tests cannot link
  outside Python) — 85 result groups, 0 failures. `cargo clippy
  --workspace --all-targets --all-features -- -D warnings` clean.
  `cargo fmt --all -- --check` clean. **PASS.**

## Notes / deviations

- `EntailmentMatrix::row_ascending` returns a `Vec<usize>` rather than
  the spec-sketched `impl Iterator` — both enum arms stay trivial; sparse
  rows are tiny and the dense arm's accessors already paid an O(n) scan
  per call. Semantics identical.
- `equivalent_classes` on an unsatisfiable subject reads
  `unsatisfiable_idxs` (sorted ascending) instead of scanning rows, per
  spec §5 — identical to the old scan's output.
