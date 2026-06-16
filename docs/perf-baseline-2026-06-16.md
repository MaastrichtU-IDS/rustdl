# rustdl perf baseline — 2026-06-16

## Task 0.1 — Standing benchmark + closure-diff (full corpus, 2026-06-16)

Re-measured via `scripts/bench-konclude-parity.sh` on this host (fsesrv-g1, 32-core Linux 5.15).
**Host:** 32-core Linux 5.15. **rustdl:** v0.3.8 (`main`). **Konclude:** docker `konclude/konclude:latest`
v0.7.0-1138 (Jun 18 2021). **ROBOT:** docker `obolibrary/robot:v1.9.6`.
**per-pair timeout:** 200 ms (wine: 25 ms). **wall-cap:** 300 s.

`ratio` = rustdl 1T / Konclude docker wall. Docker wall includes ~0.5 s container start;
use `reason-ms` for engine-vs-engine comparison. `1T` = `RAYON_NUM_THREADS=1`; `MT` = 32 cores.

### Benchmark table

| fixture | #cls | rustdl frag | rustdl 1T | rustdl MT | Konclude wall | Konclude reason-ms | ratio(1T/K) | timed-out | note |
|---|---:|---|---:|---:|---:|---:|---:|---:|---|
| shoiq-knowledge | — | — | SKIP | SKIP | SKIP | SKIP | — | — | fixture absent from repo |
| galen | 2748 | Horn | 0.58 s | 0.60 s | 0.685 s | 16 ms | **0.8×** | 0 | complete |
| notgalen | 3087 | Horn | 1.06 s | 1.06 s | 0.773 s | 20 ms | **1.3×** | 0 | complete |
| alehif | 167 | Horn | 2.43 s | 0.46 s | 0.623 s | 1 ms | 3.8× (1T) | 0 | complete |
| ore-10908 | 692 | out-of-EL | 21.1 s | 1.14 s | 0.547 s | 23 ms | 38.5× (1T) / **2.1×** (MT) | 0 | complete |
| ore-15672 | 82 | out-of-EL | 47.9 s | 28.7 s | 0.513 s | 4 ms | 93× (1T) | 109 | INCOMPLETE(109 t/o) — but MISSED=0 |
| sio | 1585 | out-of-EL | DNF | 26.6 s | 0.648 s | 56 ms | — | 0 | 1T DNF: pair loop sequential; MT complete |
| wine | 137 | out-of-EL | DNF | 54.3 s | 0.623 s | 71 ms | — | 0 | 1T DNF; MT complete; 25ms budget |
| ro | 58 | out-of-EL | 0.14 s | 0.12 s | 0.641 s | 1 ms | **0.2×** | 0 | complete |
| pizza | 99 | out-of-EL | 3.58 s | 1.61 s | 0.526 s | 18 ms | 6.7× (1T) / 3.1× (MT) | 4 | INCOMPLETE(4 t/o) — but MISSED=0 |
| bibtex | 15 | pure-EL | 0.01 s | 0.01 s | 0.487 s | 0 ms | **0×** | 0 | trivial |

**1T DNF notes:** `sio`/`wine` DNF at 300s on 1T because the pair loop is sequential — O(n²) at
200/25 ms each = 500–3000 s. MT column is correct for these fixtures. Horn/EL fixtures (galen,
notgalen, alehif, bibtex) use the saturation fast path and don't have this issue.

### Closure-diff results

| fixture | rustdl closure | oracle closure | FP | MISSED | budget |
|---|---:|---:|---:|---:|---|
| galen | 27997 | 27997 | **0** | **0** | 200 ms |
| notgalen | 32739 | 32739 | **0** | **0** | 200 ms |
| alehif | 247 | 247 | **0** | **0** | 200 ms |
| ore-10908 | 6001 | 6001 | **0** | **0** | 200 ms |
| ore-15672 | 142 | 142 | **0** | **0** | 200 ms |
| sio | 8904 | 8904 | **0** | **0** | 200 ms (thing-equiv: SIO_000000≡owl:Thing excl.) |
| wine | 653 | 653 | **0** | **0** | 25 ms |
| ro | 158 | 158 | **0** | **0** | 200 ms |
| pizza | 499 | 499 | **0** | **0** | 200 ms (unsat: 2 classes) |
| bibtex | 16 | 16 | **0** | **0** | 200 ms |
| shoiq-knowledge | SKIP | SKIP | — | — | fixture absent from repo |

**SOUNDNESS GATE: FP=0 on ALL available fixtures. Gate PASSED.**

**Completeness notes:** ore-15672 (109 t/o) and pizza (4 t/o) both show MISSED=0, confirming
the `INCOMPLETE` signal over-warns (timed-out pairs are non-subsumptions). Closure counts match
Konclude∩HermiT oracle on every fixture.

### Notable changes vs 06-08 baseline

| fixture | rustdl MT (06-08) | rustdl MT (06-16) | delta |
|---|---:|---:|---|
| galen | 0.59 s | 0.60 s | +1.7% (noise) |
| notgalen | 1.05 s | 1.06 s | +1.0% (noise) |
| sio | 32.0 s | 26.6 s | **−17%** |
| ore-10908 | 5.43 s | 1.14 s | **−79%** (Phase 7/8: label heuristic + label-cache deadline) |
| ore-15672 | 29.1 s | 28.7 s | flat |

The `ore-10908` drop was attributed to Phase 8 (`30b641c`) — decoupling the label-cache
deadline from per-pair timeout. Already documented in `docs/phase8-results.md`.

---

## Task 0.2 Attribution — EL/Horn fast path

Re-measured on this host (linux x86_64, single-thread RAYON_NUM_THREADS=1).
Konclude comparison numbers from `docs/perf-2026-06-08-konclude-vs-rustdl.md`
(native binary, same host family).

### Wall-clock table (Task 0.2 runs)

| Ontology   | #cls  | frag     | rustdl 1T | Konclude | ratio | MISSED | FP |
|------------|-------|----------|-----------|----------|-------|--------|----|
| galen      | 2748  | Horn     | 0.574 s   | 0.272 s  | 2.1×  | 0      | 0  |
| notgalen   | 3087  | Horn     | 1.052 s   | 0.282 s  | 3.7×  | 0      | 0  |
| go-basic   | 51967 | pure-EL  | 19.4 s    | 4.1 s    | 4.7×  | 0      | 0  |

(galen/notgalen: saturation fast path, no tableau;
 go-basic: saturation fast path, pure-EL, no tableau)

## Task 0.2 Attribution (EL/Horn) — full report at /tmp/elhorn-attribution.md

### Top-3 constant-factor costs

| Rank | Bottleneck | File | galen | notgalen | Reducible? |
|------|-----------|------|-------|----------|-----------|
| 1 | `supers_of()` HashMap::get on role_super | `crates/owl-dl-saturation/src/lib.rs:1000` | **59.5%** | **61.0%** | Yes — dense Vec<Box<[RoleId]>> |
| 2 | `supers_of_class`/`subs_of_class` bitset→Vec | `lib.rs:238,253` | ~5% | ~4% | Yes — bulk bitset-OR |
| 3 | `classify_pure_el` n² matrix build | `crates/owl-dl-reasoner/src/classify.rs:715` | 3.6% | 2.8% | Yes (go-basic: 34%) |

All percentages are fraction of classify() wall on the pure-EL/Horn fast path.

### Galen phase split (0.574 s total, measured via locality-stats + bench)

| Phase | Wall | % |
|-------|------|---|
| horned-owl OFN parse | ~93 ms | 16% |
| convert_ontology | ~5 ms | 1% |
| saturate() | ~445 ms | **77%** |
| → process_subsumer (incl. supers_of) | ~290 ms | 51% |
| → process_fact | ~128 ms | 22% |
| → push_fact + misc | ~27 ms | 5% |
| classify_pure_el (2748² = 7.5M entries) | ~21 ms | 4% |
| output serialization (300 KB) | ~5 ms | 1% |

### Intra-saturation galen (from 60-repeat flamegraph, % of classify())

```
saturate(): 94.6%
  process_subsumer: 65.3%
    supers_of(): ≥59.5% (also called inside process_fact — see note)
      HashMap::get (find_inner + equivalent): 59.09% ← #1 hot leaf
      HashSet collect to Vec:                  0.42%  ← negligible
  process_fact: 28.8%
    supers_of() also fires here (lines 753/832/852): included in ≥59.5%
  push_fact: 5.9%
  introduce_runtime_synthetic: 4.1% (Phase 2a Tseitin alloc)
  supers_of_class (FixedBitSet→Vec): 3.0%  ← #2 hot leaf
  contains (FixedBitSet): 3.4%
classify_pure_el: 3.6%
```

Note: 59.5% is a **floor** on the supers_of() cost — the flamegraph extraction tracks the
largest single call-chain width; `process_fact` also calls `supers_of()` at lines 753/832/852
inside its 28.8% slice. Total role-super lookup cost is ≥59.5% of classify().

**RoleId density confirmed:** `build_role_super` iterates `0..num_roles` and assigns sequential
IDs (dense u32s). The fix-#1 `Vec<Box<[RoleId]>>[role.index()]` indexing is valid by construction.

### Parity verdict

- **galen → 0.27s**: reachable by fix #1 alone (dense Vec for role_super eliminates ≥59% →
  estimated ≤0.195 s; likely faster since fix #1 also kills the process_fact portion.
  Upper bound estimate 0.195 s; actual should be lower.)
- **notgalen → 0.28s**: fix #1 → ≤0.42 s (1.5×); full parity needs algorithmic work on Phase 2a
- **go-basic → 4.1s**: fix #3 (top-down walk) removes ~6 s from 19.4 s; horned-owl parse (~7.4 s)
  requires upstream crate work to address

### Flamegraph files (in docs/flamegraphs/)

- `galen-rich-2026-06-16.svg` — 60-repeat corpus loop, high-density, primary reference
- `notgalen-classify-2026-06-16.svg` — functional-role path, moderate samples
- `go-basic-classify-2026-06-16.svg` — 2486 samples, rich; parse + n² matrix dominant
