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

## Option 1 (sparse subsumer rep) — still the general backstop

Option A eliminates the *ABox-nominal* inflation. The dense
O(num_total_classes²)-bit matrices remain the representation, so an ont with a
genuinely large *TBox-synthetic* (Tseitin) universe could still blow up. Option 1
(sparse per-class subsumer rows) is the general defense. **Gate before building
it:** measure whether any remaining giant still blows up post-Option-A — if Option
A clears the tail, Option 1 is lower-priority hardening (and carries the EL
hot-path `contains` regression risk).

## Status

Root cause found + confirmed; **Option A shipped & verified**; Option 1 pending a
measure-first gate (does any giant still blow up post-A?).
