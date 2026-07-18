# D4 memory tail — root cause (2026-07-18)

Systematic-debugging investigation of the 166 GB RSS on `ore_ont_3914` (the
`{disjoint,symmetric}` Horn giant the wedge-reuse measurement flagged as a
**scale/memory**, not calculus, problem — see
`2026-07-17-wedge-reuse-measurement-findings.md` Finding 4).

## Reproduction

`./target/release/rustdl classify ore_ont_3914.ofn` — completes in ~120–169 s at
**peak RSS 158 GB** (12,437 named classes, fully Horn, `tableau=0`).

## Root cause (confirmed by instrumentation, not code-reading alone)

Two independent facts, both from direct measurement:

1. **It is the EL saturator, not the classify orchestrator.**
   `classify --saturation-only` (skips the label-cache build and the tier-walk
   tableau probing) reproduces the explosion **identically** (same ~10 GB/3 s
   growth). So it is `owl_dl_saturation::saturate()`, not the tier-walk /
   entailment matrix / label cache / tableau. (The `tier_walk=113 s` wall bucket
   is misleading — it is `total − label_cache − snapshot`, which silently absorbs
   `saturate()`'s time.)

2. **It is NOT thread fan-out** (contrast `[[tableau-memory-fanout]]`, which was
   alehif tableau pair-graphs): `RAYON_NUM_THREADS=1` grows at the **same** rate.
   `/proc/PID/smaps` shows one contiguous main-arena heap region (~34 GB of 35 GB
   RSS at sample) — a single eager structure, not per-thread graphs.

3. **The exact allocation** (phase-marker probe): the explosion is inside
   **`WorklistEngine::new`** (`crates/owl-dl-saturation/src/lib.rs`), which eagerly
   allocates **two dense `num_total_classes × num_total_classes` bit-matrices**:
   - `subsumed_by` — `lib.rs:410-413`: `num_total_classes` × `FixedBitSet(num_total_classes)`
   - `subsumers` (`Subsumers::with_capacity`, `lib.rs:2036-2045`): another identical dense matrix

   **`num_total_classes = 582,815`** for `ore_ont_3914` — a **47× blow-up** over
   the 12,437 named classes, from Tseitin / existential-marker synthetics for its
   dense `∃R.C ⊓ D` ENVO definitions (`collect_el_rules` reported
   `num_total_classes=582815`, `existential_facts=480`, `existential_triggers=109`).

   Each matrix = 582,815 × (582,815 bits / 8) ≈ 582,815 × 71 KB ≈ **41.5 GB**.
   Two of them ≈ **84 GB at construction alone** (the probe died mid-`new()` at
   75 GB, before `run()` was ever entered), growing further during saturation to
   ~158 GB.

**Mechanism in one line:** the saturator's subsumer storage is a **dense
O(num_total_classes²)-bit matrix**, and `num_total_classes` is the
synthetic-inflated universe — so memory is quadratic in the synthetic count, which
is ~47× the named-class count on dense-`∃`-definition partonomies.

## Why it's this ont and not galen/sio

Small onts: `num_total_classes` ~ thousands ⇒ matrix ~ MBs (fine). The tail is onts
whose dense compound-`∃` definitions generate hundreds of thousands of synthetics
⇒ quadratic dense matrix in the tens–hundreds of GB.

## Fix direction (NOT yet implemented — Phase 4, its own spec/TDD/FP-gate)

The dense `num_total_classes²`-bit representation is the target. Options:
1. **Sparse per-class subsumer sets** — most classes/synthetics have few subsumers;
   replace `Vec<FixedBitSet(N)>` with per-class `Vec<ClassId>` / a sparse set. The
   access pattern is `contains(sub,sup)` (hot, must stay ~O(1)) + `subsumers_of` /
   `subsumers_bitset` (iterate a class's subsumers). A sparse set + per-class
   `HashSet`/sorted-Vec keeps `contains` acceptable; `subsumers_bitset` callers
   (classify.rs) would need adaptation.
2. **Lazy / right-sized bitsets** — allocate each class's bitset on first write,
   sized to the max subsumer id actually seen, not `num_total_classes`.
3. **Reduce synthetic proliferation** — 47 synthetics/named-class is high; fewer
   Tseitin synthetics shrinks `num_total_classes` and thus the matrix
   quadratically. Secondary (the dense rep is the primary killer), and riskier
   (touches soundness of the `∃`-body lowering).

**Soundness/verification gate for any fix:** FP=0/MISSED=0 closure-diff must stay
byte-identical (the closure content is unchanged; only its storage changes), plus
a memory-delta measurement on `ore_ont_3914` + the giant tail. Recommend option 1
or 2 (representation-only, closure-preserving, low soundness risk).

## Refined root cause (2026-07-18, follow-up) — it's an inert ABox, not just the rep

Instrumenting the synthetic namespaces showed `num_total_classes=582,815` breaks
down as: **`nominal=570,269`** + named=12,437 + by_existential=109 (everything else
0). The 570K are **NomKeys for ABox individuals** (`ObjectPropertyAssertion:
793,467` over GAZ gazetteer individuals via a transitive role). **But the TBox has
0 `ObjectHasValue`** — so these nominals are **100 % inert for the class
hierarchy** (`abox_nominal_reach` is consulted only when a processed fact targets a
NomKey, which requires a TBox `∃R.{a}`). The dense matrix was ~84 GB of pure
ABox-nominal waste, and the run-phase growth to 158 GB was the same ABox nominals'
facts.

## FIX SHIPPED — Option A (targeted ABox skip, 2026-07-18, TDD)

`crates/owl-dl-saturation/src/lib.rs`: gate `build_abox_nominal_reach` on
`!tseitin.nominal_by_ind.is_empty()` — i.e. only build the ABox-nominal reach when
the TBox actually introduced a nominal (the only source of a NomKey fact-target).
**Verdict-identical** (when there is no TBox nominal, `abox_nominal_reach` is
provably never consulted).

**Result on `ore_ont_3914`:** peak RSS **158 GB → 1.8 GB (~84×)**, wall
**169 s → 8.3 s (~20×)**, hierarchy **byte-identical** (12,069 edges, `tableau=0`).
**Gates:** TDD RED→GREEN (`transitive_abox_without_tbox_nominals_allocates_no_nomkeys`);
all 74 saturation lib tests pass incl. the TBox-nominal canary
(`nominal_transitive_abox_fold_classifies`); **FP=0/MISSED=0 closure-diff 22/22**
(wine's TBox-nominal path unchanged); fmt + clippy clean.

## Option 1 (size-adaptive subsumer rep) — SHIPPED (2026-07-18, TDD)

Gate measurement first: large *pure-TBox* giants (no ABox, so Option A is inert)
still blow the dense O(named²) matrix — ore_ont_16586 (148k) 18 GB, ore_ont_1673
(186k) 25 GB, ore_ont_7646 (236k) 19 GB; the 398k/981k ones would be 100s of GB.
So Option 1 is warranted for these.

**Key constraint discovered:** the dense `Vec<FixedBitSet>` was a *deliberate* perf
choice (its doc noted it replaced a `HashSet<ClassId>` rep for O(1) `contains`) —
so a naive "swap to HashSet" would regress the EL/galen niche. The fix is
therefore **size-adaptive**, not unconditionally sparse.

New `IdMatrix` enum (`crates/owl-dl-saturation/src/lib.rs`) backs both
`Subsumers.subsumers` and the engine's `subsumed_by`:
- **`Dense(Vec<FixedBitSet>)`** when `n ≤ DENSE_MAX` (50,000) — the EL/Horn common
  case; single cache-friendly bit-test `contains`, byte-identical to before.
- **`Sparse(Vec<hashbrown::HashSet<u32>>)`** above — memory-bounded for the giants.
- Row iteration is **ascending in both** (`.ones()` / sort-on-read), so output is
  deterministic / byte-identical across reps. `subsumers_bitset` (returned a
  borrowed `&FixedBitSet`) replaced by `subsumers_count` (O(1)) + `subsumers_of`
  (sorted); the 3 classify.rs callers updated. Grow becomes a no-op-append for
  sparse (also removes the old grow-every-row-width cost).

**Results:** galen (EL niche, dense) **0.23 s / 27 MB — no regression**;
ore_ont_1673 (186k, sparse) **25 GB → 8 GB (~3×)** (the residual ~8 GB is facts /
per-class Vecs, not the matrix — a further step if needed). **Gates:** TDD RED→GREEN
+ dense-vs-sparse equivalence unit test + threshold test; 76/76 saturation lib
tests; **FP=0/MISSED=0 closure-diff 22/22** (all fixtures dense-path,
byte-identical); fmt + clippy clean.

Note: the corpus FP gate only exercises the DENSE path (all fixtures < 50k), so the
SPARSE path's correctness rests on the `id_matrix_dense_and_sparse_are_semantically_identical`
unit test (same insert/contains/row_ascending as dense).

## Status

Root cause found + confirmed; **Option A + Option 1 both shipped & verified.**
Remaining giant-tail memory (facts / per-class Vecs beyond the matrix, and the
biggest 398k–981k onts) is a possible future step, not required by this arc.
