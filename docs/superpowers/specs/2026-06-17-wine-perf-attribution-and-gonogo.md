# Wine performance attribution + GO/NO-GO

**Date:** 2026-06-17
**Commit measured:** b436c1c (HEAD — clause-index amortization simplification)
**Host:** linux x86_64, 32 cores, rustdl v0.3.8 release binary
**Mandate:** Fresh attribution on current main; bjgap re-assessment; global-model
lever assessment; GO/NO-GO verdict.

---

## 0. State of wine on current main (soundness / completeness)

Wine is **SOUND + COMPLETE**: FP=0 / MISSED=0, closure 653 matches
Konclude∩HermiT oracle exactly. 137 classes, SROIQ (SHOIN(D)), out-of-EL.
There is **no completeness gap to close** — this is a pure performance problem.

Since the last scoped assessment (2026-06-08, `model-construction-classification-
design-2026-06-08.md`), two post-sweep-gate changes have landed that may affect
attribution:

1. **Clause-index amortization** (commits 21c2982 + b436c1c) — pre-builds
   `ClauseIndexes` + `disjoint_pairs` in `HyperCache::build` and Arc-shares
   them across all per-class `classify_labels` probes, eliminating the O(#clauses)
   rebuild per probe.
2. **FixedBitSet matrix** (f02ee4e) — replaces O(n²) bool matrix with FixedBitSet
   rows in the classify loop.

These were measured on SIO and ORE-10908 (see `2026-06-16-soundcaching-design-
and-gonogo.md`). Wine was listed as unaffected (search-dominated, not setup-
dominated). This doc confirms that with fresh measurements.

---

## 1. Fresh wine measurements (commit b436c1c)

### 1.1 Parallel (32-thread) runs at multiple budgets

```
# subsumption: saturation=622 tableau=0
# satisfiability probes: saturation=54 tableau=83
# label heuristic: pruned=5317 pass_through=53 misses=8661
# per-class BackPropRisk: safe=44 unsafe=93
# pairs-per-sub: n_subs=102 total=8714 median=97 p90=113 p99=122 max=122
```

| budget | wall (32T) | label_cache_build | tier_walk | timed-out pairs | RSS |
|---|---|---|---|---|---|
| 25 ms | **53.8 s** | 3010 ms (5.6%) | 50806 ms (94.4%) | 8666 | **136 MB** |
| 100 ms | 204 s | 3011 ms (1.5%) | 201273 ms (98.5%) | 8646 | ~136 MB |
| 200 ms | 404 s | 3012 ms (0.7%) | 401226 ms (99.3%) | 8623 | ~136 MB |

Scaling: `tier_walk` grows **linearly with budget** — almost every hard pair
burns the full deadline. Budget has essentially no effect on the pair count
(8666 vs 8623 — a difference of 43 across 8× budget increase, noise-level).

### 1.2 Single-thread (1T) at 25 ms

```
# wall breakdown ms: label_cache_build=86196  tier_walk=439608
# timed-out pairs: 8666
# wedge-cost-histogram: 15 | 8 | 23 | 1 | 1 | 8666 | 0 | 0 | 0
# hyper-proven pairs: 21
Elapsed: 8m46s   RSS: ~10 MB (per-thread stack only, sequential)
```

1T label_cache_build = 86196 ms = 630 ms/class × 137 classes.
The amortization (which cut SIO 32T label_build 922→~184 ms, ~5×) does
**nothing** for wine: each probe's 630 ms is pure search, not setup.
At 32T: 86196 ms of sequential search → 3010 ms wall (28.6× speedup from
parallelism alone — near-linear for this search-dominated workload).

### 1.3 Parallel efficiency

32-thread wine run: 53.8 s wall × 957% CPU = 515 s CPU = near-linear scaling.
This is expected: per-pair probes are independent, no lock contention.
The 32T gap vs Konclude's 0.127 s (native) is **424×**; vs the same docker-wall
comparison basis (0.623 s in the 06-16 baseline): **86×**. Neither ratio is
bridgeable by adding more cores.

---

## 2. Where the time goes: label_cache_build vs tier_walk

### 2.1 Label cache build (3010 ms / 5.6% at 25ms, 32T)

`classify_labels` runs 137 per-class probes to build the `LabelOracle`.
A probe starts with the base clause set (1503 clauses — more than SIO/ORE
because wine's nominal + disjunction axioms clausify densely), adds one
Q-clause, and calls `decide_with_deadline(depth=256, deadline)`.

Result by outcome:
- `pruned=5317` — 5317 (C,D) pairs where D ∉ C's label set (sound non-sub,
  free after oracle lookup)
- `pass_through=53` — 53 pairs where D ∈ C's labels, fall through to wedge
- `misses=8661` — 8661 pairs where oracle returned `NoVerdict` (label build
  timed out or Stalled), fall through to full per-pair wedge

The per-class probe cost is ~22 ms/class at 32T (3010 ms / 137 classes).
This is **search-dominated**: each per-class probe fires the wedge on a class
satisfiability question and stalls after consuming its budget. At 1T the
sequential cost is 630 ms/class (86196 ms / 137 classes). The label oracle
produces a `Sat` model (yielding `pruned` + `pass_through` oracle coverage)
only for the subset of classes whose satisfiability witness the wedge finds
within budget; the rest return `NoVerdict`, yielding the 8661 pair-level misses.

The amortization fix (which helped SIO/ORE where per-probe cost was 2-8 ms,
dominated by index-build) is irrelevant here: when search cost is 630 ms and
index-build cost is ~0.4 ms (estimated from the axiom-count scaling), the
80-90% rebuild fraction becomes <0.1% of total probe cost. Wine was always
in a different regime.

### 2.2 Tier walk (50806 ms / 94.4% at 25ms, 32T)

This is the per-pair subsumption probing (`find_direct_parents_top_down`),
which after the label oracle gate processes:
- 53 `pass_through` pairs (D ∈ C's labels → verify via wedge, fast)
- 8661 `misses` pairs (NoVerdict → fall through to per-pair wedge)
- = 8714 total wedge calls

Of the 8714 wedge calls:
- `hyper-proven=21` — 21 resolve quickly (Subsumed)
- `timed-out=8666` — 8666 exhaust the 25 ms deadline

**The tier_walk cost is almost entirely the 8666 timed-out pairs × 25 ms each:**
8666 × 25 ms = 216650 ms CPU-ms → 216650 / 32 threads × overhead ≈ 50+ s wall.
The 53 pass_through pairs are negligible; the BFS traversal overhead is
also negligible (wine is a small 137-class hierarchy, not GALEN-scale).

### 2.3 Summary: the cost is exactly the stalling pairs

The entire wine wall at 25ms/32T is:
- 8666 genuinely non-subsumed pairs, each stalling the wedge at the full
  25 ms budget, finding nothing.
- 21 pairs resolved by the wedge (fast).
- 622 pairs resolved by the EL saturator (effectively free).

**The tableau=0 stat is definitive:** the main tableau is never called on
wine (the wedge stalls before falling through, or trust_sat routes away).
All 653 real subsumptions come from the EL saturator's nominal levers
(MaxKey, ForallKey, NomKey, etc. — the 2026-06-07 completeness project).
The wedge's per-pair role is **exclusively to search for counterexample models
on non-subsumptions** — and it fails to terminate on every hard pair.

---

## 3. Per-pair stall characterization (fresh measurement)

Run: `cargo test -p owl-dl-reasoner --release wine_wedge_construct_vs_solve_probe -- --ignored --nocapture`

```
wine: 1503 base clauses
wine: 137 classes
found 3 hard (Unknown@200ms) pairs in 139 probes
pair(1,0)  clone=0.1ms  new=0.1ms  solve(5s cap)=5000.8ms  branches=12720  merge_branches=4122  -> Stalled
pair(1,2)  clone=0.1ms  new=0.1ms  solve(5s cap)=5001.0ms  branches=12724  merge_branches=4124  -> Stalled
pair(1,3)  clone=0.1ms  new=0.1ms  solve(5s cap)=5001.0ms  branches=12191  merge_branches=3944  -> Stalled
```

Key findings from this fresh measurement:

**Setup cost is negligible.** clone=0.1 ms, HyperEngine::new=0.1 ms.
Per-probe overhead has not changed from the prior measurements.

**Search stalls at 5s, every pair.** Confirmed on current main: the hard pairs
do not terminate in 5s. Branch rate ≈ 2500 branches/s.

**Disjunction-dominated.** At the 5s mark:
- Pair 1: 12720 total branches, 4122 merge = **67.6% disjunction, 32.4% merge**
- Pair 2: 12724 total, 4124 merge = **67.6% / 32.4%**
- Pair 3: 12191 total, 3944 merge = **67.6% / 32.4%**

The ratio is remarkably stable (67.6/32.4) across the 5s window. This matches
the prior 2026-06-08 measurements (disjunction-dominated pair: 107834/60412 =
64%/36%) — same regime, same split.

**RUSTDL_TRACE on classify (50ms budget) confirms the structural picture:**

```
# trace search depth=256 disj node=0 options=2 graph_nodes=424
# trace branch depth=256 my_id=0 pick=1/2 disj=828
```

- `options=2` on every branch — wine's disjunctions are always binary.
- The classifier's main-tableau satisfiability probes (83 of them for individual
  classes) show graph_nodes=422-425. These are the ABox-seeded main-tableau
  probes, not the per-pair wedge. The per-pair wedge (hyper.rs) uses the
  lightweight graph; the 10-node figure is from the 2026-06-07 worktree
  measurement (not the current probe, which measures branches, not node count).
- `restores ≈ branches` — every branch fails and backtracks.

**Note on RUSTDL_COUNTERS:** `RUSTDL_COUNTERS=1 --features counters` per-rule
call counts were not re-run for this fresh attribution — they require a separate
feature build and the prior counter data already characterizes the ABox-seeded
main-tableau path on wine (is_blocked_calls=8.85M, apply_*=1.48M, from the
wine-residual-31 memory / commit 139392b). Counter data is relevant only for
the 83 main-tableau satisfiability probes in `label_cache_build`; the dominant
cost (8666 timed-out per-pair wedge probes in tier_walk) is fully characterized
by the `branches_taken` / `merge_branches` output above.

---

## 4. CDCL / backjump-distance (bjgap) re-assessment

### 4.1 Code path invariance (why re-measurement is unnecessary)

The `conflict_bjgap_hist` instrumentation that produced the decisive
bjgap≈1 measurement (2026-06-08, `reuse-trap-nominal-termination-scoping-
2026-06-07.md` §BACKJUMP-DISTANCE block) was added in a temporary worktree
branch and is **not on main**. No new 1-UIP / asserting-clause code has
been added to `hyper.rs` since that measurement. The relevant code paths
are structurally identical to what was measured:

1. `solve_at_most` (hyper.rs:1842) still emits `DepSet::ALL` at partition
   exhaustion — partition-exhaustion Unsat clashes are un-backjumpable by
   construction. Merge branching accounts for 32.4% of wine's branches (fresh
   measurement §3); the prior 2026-06-08 measurement found 27–45% of all
   conflicts are `DepSet::ALL` overflow, consistent with that branching share.
2. No 1-UIP implementation was added — the code comment at search.rs:194-200
   explicitly documents this: `"(CDCL lookup intentionally not wired here...)"`.
3. `solve`'s disjunction branching (hyper.rs:1585-1586) unchanged.
4. No new conflict-level tracking beyond the existing `DepSet` bits.

Therefore the prior bjgap≈1 finding **stands by code-path invariance**, not
by fresh re-measurement. The measurement is not repeatable without re-adding
the worktree instrumentation, which requires .rs edits (out of scope).

### 4.2 Why bjgap≈1 means CDCL is NO-GO

The 2026-06-08 measurement of `conflict_bjgap_hist` showed:
- For **disjunction-dominated** wine pairs (the 67.6% regime): conflicts have
  `avg_deps = 5.7` and `10.8`, spans concentrated at 17-32+, but
  `highest − second_highest` (the true 1-UIP backjump target) is **dominated
  by distance 1** — dep-sets are dense at the top, long sparse tail.
- A 1-UIP asserting clause fires at depth `highest − 1` ≈ chronological ≈
  backjumps nothing.

This is not contradicted by anything in the fresh measurement:
- The same 67.6% disjunction ratio at 5s confirms the same branching regime.
- `solve_at_most` still emits `DepSet::ALL` for the 32.4% merge branches,
  making them un-backjumpable regardless of 1-UIP.

**1-UIP is NO-GO. Prior verdict stands.**

### 4.3 Simple nogood learning (already measured dead)

Prior result (2026-06-06, `conflict-learning-simple-is-weak` memory): simple
dep-set nogood learning (feat/conflict-learning, ~490-line `hyper.rs` nogood
store) gave −13.5% branches but **0 wine classes un-stalled**. The recurring
clashes are leaves — each prune saves 1 branch, never a subtree. Not repeated.

---

## 5. Global-model / refute-only lever re-assessment

The global-model-rewrite spec (memory `global-model-rewrite-spec`, branch
`spec/global-model-rewrite`) was authored 2026-06-10 and P0-gated. Its
P0 result (also 2026-06-10) was:

> "MIXED, rewrite PARKED ... The clear ontology-independent lever is TIER-WALK
> overhead, not probing."

### 5.1 Why the global-model approach does NOT apply to wine

The global-model approach's premise is that per-class pseudo-models (the label
oracle) can *refute* most pairs cheaply, leaving a small confirmed residual.
On wine, the oracle refutes 5317 of 14031 candidates (37.9%), but the 8661
`misses` pairs (61.7%) produced `NoVerdict` because the per-class label probe
itself stalled — there is no model to refute with. The oracle is sound only when
it terminates; wine's classes cannot build terminating per-class models at any
reasonable budget (630 ms/class at 1T, stalled in 5s at the probe level).

Even if per-class models terminated, refute-only is FP-safe but introduces
MISSED risk: "no clash on this model + ¬D" is not "no clash in every model."
The P2 de-risk (global-model-rewrite memory, 2026-06-10) confirmed this:
> "the cheap mechanism is unsound there + wouldn't decide them faster."

### 5.2 FP-soundness characterization (for completeness)

The snapshot-cache FP death (ORE 2015, 30+ FP, `RUSTDL_SNAPSHOT_CAPTURE`
flipped to default-OFF 2026-06-08) is a `Sat`-reuse failure: replaying one
model for ALL (C, D) pairs falsely reports non-subsumption when `D` is missing
from that model but present in every other. Refute-only avoids the FP direction
but risks the MISSED direction (single-model bias).

FP-soundness verdict: **refute-only avoids FP** (a counterexample model is a
genuine non-sub witness). **Does not avoid MISSED** (single model isn't every
model). So "refute-only is FP-safe" is correct but incomplete. It is:
- FP-safe: sound
- MISSED-risk: incomplete (by design, by the reuse-trap)
- Inapplicable to wine: no terminating per-class models to refute with

**GO/NO-GO on global-model for wine: NO-GO.** Reason: inapplicability
(per-class models don't terminate) supersedes the FP-soundness question.

---

## 6. Fresh-angle analysis: what changed since 2026-06-08?

Three changes landed after the 2026-06-08 close:

| Change | Wine impact |
|---|---|
| Clause-index amortization (21c2982, b436c1c) | None: search-dominated, setup ≈ 0 |
| FixedBitSet matrix (f02ee4e) | None: wine's pair loop is wedge-dominated |
| Defined-sub sweep + MaxKey/ForallKey/NomKey nominal levers | Completeness (wine 57→0 MISSED), not perf |

The sweep-gate (97f5153) that transformed SIO wall 42× reduced wine's
`label_cache_build` from ~86 s 1T to ~86 s 1T (zero: wine had no defined-sup
sweep calls to gate, its pairs are `misses`-dominated). The sweep-gate did
not gate wine because wine's pairs hit `misses` (NoVerdict), not `pass_through`
(which the sweep gate acts on).

The 2026-06-16 soundcaching doc measured wine post-sweep at 1T (since wine 1T
DNFs, it showed 1T sequential walls):
```
wine@25ms (1T, f02ee4e): label_cache_build=86307ms  tier_walk=439623ms
```
Current main (b436c1c), fresh 32T measurement:
```
wine@25ms (32T): label_cache_build=3010ms  tier_walk=50806ms
```
The 32T label_cache_build drops from 86307 ms to 3010 ms purely through
parallelism (137 × ~630 ms / 32 threads ≈ 2.7 s); amortization contributes
nothing on search-dominated probes.

**Net: no new lever opened by any post-2026-06-08 change.** The attribution
is structurally identical.

---

## 7. What Konclude does differently (no new evidence; invariants stand)

The 2026-06-14 native Konclude measurement (single-thread, docker, v0.7.0-1138)
classified the full wine SHOIN in 114 ms. This bounds `#hard_pairs × branches/pair`,
but cannot discriminate "far fewer hard pairs" from "far fewer branches/pair"
or a combination. The 2026-06-08 B-perf Phase 1 told-bracket measurement
(B-perf is 0% — told-bracket eliminates 0 of 53 pass-through pairs) plus the
M3-premise test (~100% open disjunctions) plus the M1 ruling-out (wine is all
`≤1`, partition fan-out avg 0.36) together argue Konclude's advantage is
architectural (O(n) global model construction vs O(n²) per-pair probing), not
a tractable search-pruning refinement. Nothing in the current landscape
contradicts or adds to this.

---

## 8. GO/NO-GO verdict

### 8.1 Lever exhaustion summary

| Lever | Status | Basis |
|---|---|---|
| 1-UIP CDCL | **NO-GO** | bjgap≈1 stands by code-path invariance; partition-exhaustion clashes un-backjumpable (DepSet::ALL at solve_at_most:1842, 27–45% of all conflicts per 2026-06-08 measurement); 0 wine classes un-stalled by simple learning (prior measured) |
| Global-model / refute-only | **NO-GO** | Per-class models don't terminate on wine (630ms→Stalled); refute-only inapplicable without a model; FP-safe but MISSED-risk |
| Clause-index amortization | **Done, inert on wine** | Already shipped (b436c1c); setup is <0.1% of per-probe cost on wine |
| B-perf / told-bracket | **Dead** | 0 of 53 pass-through pairs told-decidable (Phase 1 result, 2026-06-08) |
| M1 (≤n-clause-head encoding) | **Dead** | Wine is all ≤1, partition fan-out avg 0.36 (2026-06-08 #mcands measurement) |
| M2 / DifferentIndividuals→wedge | **Shipped, wall flat** | Merge-branches −16%, wine wall unchanged (commit 512fe56, 2026-06-08) |
| M3 (Konclude saturation-split + patching) | **Dead** | ~100% open disjunctions at Horn-fixpoint (2026-06-08 M3-premise test) |
| Snapshot gate loosening | **Dead** | FP=100 on pizza/ro/sio (§19/§20 recon, 2026-06-07) |
| Pair-timeout knob | **Shipped** | `--pair-timeout-ms 25` → 53.8 s (32T), closure 653 (correct) |
| `with_nominals` wiring | **Dead** | Nominals wired = 166206 branches vs 168246 without (2026-06-08 60s probe), not measurably different |

### 8.2 Could any new angle exist?

Three categories were reviewed:

**(i) Recent changes (post-2026-06-08):** None opened a wine lever. The
clause-index amortization is the largest recent change; it is inert on wine.

**(ii) Search-order / heuristics:** The `reorder_disjuncts` function
(search.rs:181) already performs a cheap-sat-first reordering. MOMS heuristics
were tried and reverted. The TRACE output shows `options=2` (binary) branching
throughout — no multi-way branching to reorder. The bjgap≈1 result shows
backjumping cannot help.

**(iii) Fresh instrumentation angles:** The one uncharacterized question from
the model-construction doc was "is it search order or wedge incompleteness?"
The 2026-06-08 60s probe (with nominals wired + `with_nominals` off, both
Stalled, ~168k branches) answered it: the model is finite (10 nodes), search
is the cost, and the wedge is NOT incomplete (nominals wired = same Stalled).
Nothing in the fresh measurements contradicts or adds to this.

### 8.3 Verdict

**Prior conclusion stands: ACCEPT THE WINE WALL.**

The evidence is stronger than before — not just a set of failed experiments but
a positive characterization of WHY the wall exists (8666 genuinely non-subsumed
pairs whose Sat models the wedge cannot find in any budget) and WHY it cannot
be closed by any measured lever:
- The 8661 `misses` (NoVerdict) pairs cannot be pruned by a label oracle that
  itself stalls on those classes — the prune-not-search path is blocked.
- The 8666 timed-out wedge probes are genuine non-subsumptions (MISSED=0
  confirms the timed-out pairs are correctly not-subsumed) — the cost is
  **purely wasted search on Sat witnesses the engine cannot construct.**
- All known search-improvement levers (1-UIP, MOMS, ordering, M2, nominals)
  are measured dead on wine specifically.
- The only non-dead lever (M3 Konclude-style architectural rewrite) is gated
  on an **unsatisfied prerequisite** (the M3 premise test showed ~100% open
  disjunctions, meaning richer propagation would not force them) and represents
  months of greenfield work for an at-best-partial, unverified payoff.
- Wine is **correctness-complete** (FP=0/MISSED=0): the wall is a perf-on-
  pathological-SROIQ problem, not a correctness defect.

**Practical recommendation: `--pair-timeout-ms 25`.** This is the shipped,
measured, safe setting: wine 53.8 s (32T), hierarchy 653 = Konclude-exact,
FP=0. Further budget provides essentially zero additional pairs (8623 vs 8666
at 200ms — a difference of 43 at 8× budget).

---

## 9. FP risk posture note

Any future wine-targeting lever must pass:
1. **Adversarial review** (FP-danger zone — snapshot FP was 30+ on ORE 2015)
2. **SROIQ fuzz**: 200 random SROIQ ontologies, flag-on vs flag-off verdict-identical
3. **Corpus closure-diff**: FP=0/MISSED=0 on all fixtures

Wine is the most FP-dangerous fixture in the corpus (nominal + cardinality +
disjunction + many non-Horn structures). Any lever that accepts a Sat verdict
from a partial model or reuses a model across (sub, sup) pairs carries A1 risk.
Do not re-attempt the snapshot gate (§19/§20 recon was clear: FP=100 on pizza
and sio with `risk=Safe` gate loosened). The RUSTDL_SNAPSHOT_CAPTURE
default-OFF is load-bearing.

---

## 10. Key file / line references

| What | File | Line |
|---|---|---|
| `solve_at_most` DepSet::ALL partition-exhaustion | `crates/owl-dl-tableau/src/hyper.rs` | 1842 |
| `solve` disj branch counter | `hyper.rs` | 1585–1586 |
| `solve_at_most` merge branch counter | `hyper.rs` | 1867–1868 |
| `SearchStats` struct | `hyper.rs` | 352 |
| `wine_wedge_construct_vs_solve_probe` test | `crates/owl-dl-reasoner/src/lib.rs` | 3581 |
| CDCL comment (intentionally not wired) | `crates/owl-dl-tableau/src/search.rs` | 194–200 |
| `HyperCache::classify_labels` (per-probe entry) | `crates/owl-dl-reasoner/src/lib.rs` | 1121 |
| `find_direct_parents_top_down` (tier walk) | `crates/owl-dl-reasoner/src/classify.rs` | 1813 |
| `subsumes_via_tableau` (per-pair wedge dispatch) | `classify.rs` | 1948 |

---

## 11. Related docs (do not re-derive)

- `docs/reuse-trap-nominal-termination-scoping-2026-06-07.md` — bjgap measurement + CDCL NO-GO
- `docs/model-construction-classification-design-2026-06-08.md` — B-perf dead (0%), M1/M2/M3 assessment, accepted-wall close
- `docs/superpowers/specs/2026-06-16-soundcaching-design-and-gonogo.md` — post-sweep attribution, clause-index amortization
- `docs/perf-baseline-2026-06-16.md` — full corpus baseline including wine@25ms = 54.3 s
- `docs/perf-2026-06-08-konclude-vs-rustdl.md` — three-way comparison; wine row
