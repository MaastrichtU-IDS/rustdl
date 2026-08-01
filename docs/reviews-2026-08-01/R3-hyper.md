# R3 — review of `crates/owl-dl-tableau/src/hyper.rs` (hypertableau "wedge")

Date 2026-08-01. Binary: fresh `cargo build --release -p owl-dl-cli` (pinned 1.95.0 toolchain
via `PATH=/data/dumontier/.cargo/bin`; `stable` does NOT compile this tree — `owl-dl-saturation`
fails with E0599, so the CLAUDE.md "build with RUSTUP_TOOLCHAIN=stable" note is now WRONG on this
host). Profiler: `samply record --save-only -r 499`, symbolicated offline via `nm --print-size`
+ binary search on frame addresses, filtered to the `rustdl` lib by `resourceTable`.

> METHOD WARNING (cost me ~30 min): `addr2line` batch symbolication of the samply funcTable
> silently misaligns (a `.strip()` on stdout, plus libc/vdso addresses resolved against the
> rustdl ELF) and produced two mutually inconsistent, entirely plausible-looking profiles of the
> SAME file. The nm-based resolver is the one to trust; cross-check that `libc.so.6` / `[vdso]`
> frames are attributed to their own lib before believing any name.

All walls single-thread (`RAYON_NUM_THREADS=1`) unless noted, on the shared 32-core host.

---

## 1. CONFIRMED — missing — the v0.3.39 clause-index amortization is not wired to the per-CLASS path

`crates/owl-dl-reasoner/src/lib.rs:2958` (`HyperCache::classify_labels`), consuming
`hyper.rs:1241 HyperEngine::new`. The amortization API it should use already exists and is used
by the per-PAIR sibling: `hyper.rs:1349 new_with_prebuilt_extras` + `hyper.rs:1167
build_clause_index_delta` (used at `lib.rs:2749`).

Mechanism: `classify_labels` appends the SP2.1 `sat_seed` and SP3 `exists_seed` clauses to the
per-class clause vector. Those clauses are not in `base_indexes` (built before the seed), so the
code deliberately falls back to `HyperEngine::new(&clauses, ..)` — a **full O(#clauses)
`ClauseIndexes` rebuild, once per class**. `RUSTDL_SAT_SEED` defaults ON, so this is the always
-taken branch; `new_with_prebuilt` (the amortized branch) is dead on any default run.
The seed clauses are all `Q → D` / `Q → ∃R.D` singletons — exactly the shape
`build_clause_index_delta` handles for the per-pair extras.

Measured, `ore_ont_1508` (11,659 classes, ~35k clauses), 43,793 samples:

| frame | inclusive |
|---|---|
| `HyperEngine::new` | **31.00%** |
| ↳ `index_one_clause` | 30.39% |
| ↳ `build_clause_match_plan` | 11.60% |
| ↳ `push_dense` | 7.42% |
| `drop_in_place<ClauseIndexes>` | 6.31% |
| `HyperEngine::solve` (all real search) | 30.44% |

**Intervention proof** (not arithmetic): re-profiled with `RUSTDL_SAT_SEED=0`, 57,886 samples —
`HyperEngine::new`, `index_one_clause`, `build_clause_match_plan`, `push_dense` and
`drop_in_place<ClauseIndexes>` are **entirely absent**; `new_with_prebuilt` is 0.01%.

Wall A/B (`--pair-timeout-ms 20`, closures compared with `grep -v '^#' | sort | diff`):

| ontology | default | `RUSTDL_SAT_SEED=0` | closure |
|---|---|---|---|
| ore_ont_1508 | 209.61 s | 119.92 s (−43%) | **byte-identical** |
| ore_ont_12698 | 109.89 s | 52.82 s (−52%) | **byte-identical** |
| ore_ont_10019 | 6.71 s | 6.70 s | identical (few classes) |

Falsifiable prediction: build a `ClauseIndexDelta` over the per-class seed clauses and call
`new_with_prebuilt_extras` instead of `new`. `index_one_clause` should drop below 1% of samples
and ore_ont_1508 / ore_ont_12698 should land within ~5% of their `SAT_SEED=0` walls **while
keeping the seed** (so the classes the seed rescues on wine/nominal-heavy inputs are unaffected).
If the wall does not move, this finding is wrong.

Note the seed bought **zero** subsumptions on either ontology — but do not conclude "delete the
seed"; the seed's documented value is elsewhere (wine label-cache misses). The defect is the
un-amortized index, not the seed.

## 2. CONFIRMED — inefficient — per-class deep clone of the clause vector

`crates/owl-dl-reasoner/src/lib.rs:2895` `let mut clauses = self.clauses.clone();`, per class.
`Vec<DlClause>` is a deep clone (each `DlClause` owns `body: Vec<Atom>` + `head: Vec<Atom>`), so
this is ~3 allocations × #clauses × #classes.

Measured on ore_ont_1508 with `SAT_SEED=0` (i.e. after finding 1 is neutralised):
`<Vec<T,A> as Clone>::clone` **20.28% inclusive**, of which 11.23% of all samples are libc
(memcpy/malloc) directly beneath it. ≈24 s of a 120 s wall.

**This contradicts the prior recorded measurement of 0.55–6.3% "NOT a DNF lever."** I am not
disputing that earlier number on whatever fixture produced it; on ore_ont_1508 it is 20% and the
ontology is a 120 s DNF. Report it as ontology-dependent, not as a settled non-lever.

`HyperEngine` borrows `&'c [DlClause]`, and `new_with_prebuilt_extras` already takes base and
extras as **separate slices** — so the clone exists purely to concatenate, and the fix is the
same one as finding 1. Prediction: with extras passed separately, `Vec::clone` drops out of the
profile and RSS falls (measured 202 MB → 152 MB across the SEED A/B, consistent with this).

## 3. CONFIRMED — the "35% allocator churn in `match_body`/`enumerate_matches`" lever is DEAD

Recorded-but-unbuilt lever, verified against current code and **refuted**. On ore_ont_1508
(SEED0), of `match_body`'s 38.79% inclusive time only **0.54% of total samples (1.4% of
`match_body`)** have a libc leaf:

```
leaf breakdown inside match_body:
  21.45%  enumerate_matches
  10.45%  match_body
   6.18%  Map<I,F>::next
   0.54%  [libc.so.6]        <-- the entire allocator cost
   0.16%  SmallVec::reserve_one_unchecked
```

The `SmallVec<[HNode; 8]>` at `hyper.rs:4065` (with the in-code comment naming exactly this
profiling result) already harvested it. Do not re-scope SmallVec/scratch-buffer work here.

## 4. CONFIRMED (cost) / SUSPECTED (mechanism) — inefficient — unindexed edge scan in `enumerate_matches`

`hyper.rs:4065-4081`. For each role atom of each fired clause, the matcher linearly rescans
**every** out-edge in `src_data.edges` *and* **every** in-edge in `src_data.preds`, testing
`role_matches` (hierarchy-aware) on each, and routes every survivor through `resolve()` — a
union-find walk taken unconditionally because `inverse_func_merge` defaults ON. There is no
per-(node, role) edge index anywhere in `HyperNode`.

Confirmed cost (ore_ont_1508, SEED0): `enumerate_matches` self **21.45%**, `match_body` self
10.45%, the `.map(...)` iterator 6.18% — 38.79% inclusive, the single largest remaining wedge
cost once findings 1+2 are removed. The *mechanism* (scan-vs-match ratio) is not directly
instrumented, hence SUSPECTED.

Predictions: (a) bucketing `edges`/`preds` by `role_id` (or keeping them role-sorted) reduces
`enumerate_matches` self-time in proportion to scanned/matched; (b) a `merges_done == 0`
fast-path in `resolve` removes most of the 6.18% `Map::next`. If node fan-out on 1508 turns out
to be ~1–2 edges, (a) is wrong and the cost is the recursion/`other_classes` check instead.

## 5. CONFIRMED — the incumbent attribution ("disjunctive-branching blowup in the wedge") FAILS on both named hard cases

- **ore_ont_10019** (120 s DNF, 17 MB RSS): `hyper::HyperEngine::solve` is only **15.33%**
  inclusive. **84.6%** is the MAIN tableau — `search::branch` 84.63% / `search::search` 72.21%,
  entered via `replay::replay_with_neg_sup` (81.62%). Co-occurrence split of all 29,939 samples:
  83.85% main-tableau-only, 11.50% hyper-only, 0.31% both.
- **ore_ont_1508** (120 s DNF): of 22,456 samples inside `match_body`, **22,454 are under Horn
  `fire_clause`** and **2** are under `find_open_disjunction`. `HyperEngine::save` — called
  exactly once per branch (`hyper.rs:3008`) — **does not appear in the profile at all**, i.e.
  essentially zero disjunctive branches were taken.

So on these two the residual is (a) main-tableau search reached through the snapshot-replay path,
and (b) Horn hyperresolution *matching* — not ⊔ branching. Prediction: `RUSTDL_HYPERTABLEAU=0`
or lowering `HYPER_WEDGE_DEPTH` will not move either wall.

(For contrast, 10019 *does* branch inside the wedge — `HyperEngine::save` 6.76% inclusive there,
i.e. ~44% of hyper's own 15.3%. Full-graph `Vec<HyperNode>` clone per branch, `hyper.rs:3122`:
8 owned `Vec`s per node, cloned wholesale. That is the wedge's own top cost *when it branches*,
and it is a candidate for trail-based undo like `trail.rs` — but it is 6.76%, not the DNF.)

## 6. CONFIRMED — inefficient (cross-subsystem, main tableau) — `Instant::now()` before every rule

`crates/owl-dl-tableau/src/saturate.rs:118` — the `step!` macro calls `ctx.check_deadline()`
(`lib.rs:535` → `Instant::now()`) before **each of 13 rules, per node, per saturation pass**.
On ore_ont_10019 the `[vdso]` clock_gettime frames are **11.28% self**. The in-code comment
("The check is a cheap Instant comparison, dwarfed by rule bodies") is refuted by measurement.

Explicitly NOT the hyper.rs deadline story: `hyper.rs:2826` checks once per `solve` frame and
does not show up — consistent with the prior "in-loop deadline check measured NO improvement"
NO-GO, which was about `FIXPOINT_ITERS`, a different site. Prediction: batching the saturate
check to once per node (or per 64 rule applications) removes ~10% of 10019's wall with
byte-identical verdicts.

## 7. REFUTED HYPOTHESIS — `find_open_at_most` has no `is_blocked` guard, but never fires on a blocked node

This was my best candidate for a second instance of the shipped one-line ⊔/blocking bug.

`hyper.rs:3704 find_open_at_most` checks only `resolve(node) != node`. Its three sibling
rule-selection sites all additionally decline on a blocked node:
`hyper.rs:3242` (`find_open_disjunction`, the shipped fix), `hyper.rs:4326` (`fire_exists`),
`hyper.rs:4424` (`generate_at_least`). Under anywhere-blocking a node CAN become blocked after
generating successors (the blocker's label set grows), so the gap is reachable in principle.

I instrumented it (a read-only replica of both blocking predicates at the return site) and
**proved the instrument fires before believing it**: `strings rustdl-R3PROBE | grep -c
R3PROBE_ATMOST` = 1, and the probe emitted 96,001 events on ore_ont_10019.

| fixture | `≤n` branch points | on a blocked node |
|---|---|---|
| ore_ont_10019 | 96,001 | **0** |
| pizza.ofn | 1 | 0 |
| wine.ofn | 0 | — |
| ore_ont_13723 | 0 | — |

So: real guard inconsistency in code, zero occurrences in practice. Not a bug to fix now.
Falsifiable: any ontology showing `on_blocked_node > 0` reopens it. Instrumentation reverted;
`git diff crates/owl-dl-tableau/` is empty and the rebuilt `target/release/rustdl` contains 0
occurrences of the marker.

(Note also: even if reached, a ≤n merge on a blocked node is FP-safe — the merge is a genuine
semantic consequence, so a clash from it is real. The exposure would be termination/perf only.)

## 8. SUSPECTED — incorrect (completeness, MISS-direction — cannot break FP=0)

`hyper.rs:1942-1948`, `is_blocked` double-blocking arm, uses **subset** semantics:
`L(n) ⊆ L(m) ∧ L(parent(n)) ⊆ L(parent(m))`. Pairwise ("double") blocking for SHIQ/SROIQ with
inverse roles is standardly stated with **equality** of both label pairs (Motik/Shearer/Horrocks,
HermiT); subset pairwise blocking is sound only in the absence of inverse roles. The in-code
comment asserts subset is "sound with inverses" without citation.

Direction of the risk: over-blocking only *removes* rule applications ⇒ fewer clashes ⇒ Sat where
Unsat was correct ⇒ a MISS. It cannot manufacture an unentailed subsumption, so FP=0 is safe.
Falsifiable: construct an inverse-role fixture where a successor's label is a strict subset of an
earlier node's and the unblocked expansion clashes; the wedge should answer Sat while
HermiT/Konclude answer Unsat. Not exhibited — do not act on this without the fixture.

## 9. SUSPECTED — inefficient — linear scans in nominal / `≠` / blocking bookkeeping

Not hot on anything I profiled (the profiled fixtures are nominal-free; wine took 0 `≤n` branch
points), so all three are SUSPECTED and low priority.

- `hyper.rs:4480` `apply_nn_rule` does a full `0..self.nodes.len()` scan to find the other
  carrier of a nominal, on **every** nominal `Event::Label`. O(|nodes|) per label; a
  nominal→node side map is O(1).
- `hyper.rs:4373` `are_neq` linearly scans `self.neq`, calling `resolve` on **both** endpoints of
  **every** stored pair; it is called O(k²) times from `forced_distinct_exceeds`
  (`hyper.rs:3492`) and from `must_be_distinct`.
- `hyper.rs:1925` the `block_index` bucket is appended to at node creation and **never pruned on
  merge**, and `is_blocked` does not `resolve()` the candidate `m_hnode` — so a merged-away ghost
  is rescanned forever and can act as a blocker on stale labels (again MISS-direction, not FP).

## 10. SUSPECTED (inert by default) — inefficient — non-MRV `find_open_disjunction` ignores the `nonhorn` index

`hyper.rs:3245` iterates `0..self.num_clauses()` and filters with `is_horn()`, while the MRV arm
at `hyper.rs:3183` walks `self.indexes.nonhorn` with an X-anchor prefilter. `RUSTDL_MRV_ORDERING`
defaults ON (`lib.rs:1580`) so the slow arm is off the default path; it only bites A/B runs with
`RUSTDL_MRV_ORDERING=0`, which is exactly when someone is trying to measure MRV's value — the
comparison is therefore biased against the flag-off baseline.

Conversely the MRV arm cannot early-exit (it must score every candidate to take the minimum), so
it pays a full nodes × nonhorn-clauses × bindings scan per branch decision. On the ontologies I
profiled this is free (2 samples), because they barely branch — unmeasured on a genuinely
branch-heavy input.

---

## Things I checked and found clean

- `save`/`restore` worklist snapshotting under `RUSTDL_HYPER_INCREMENTAL_FIXPOINT`
  (`hyper.rs:3122/3138`): the worklist is snapshotted only in incremental mode and, at the save
  point, `horn_fixpoint` has just drained it, so the clone is of an (essentially) empty vec — no
  hidden O(graph) work there. The O(graph) work in `save` is `nodes.clone()`, which both modes
  pay.
- `merge_with_cause` (`hyper.rs:3759`) re-queues out-edges, in-edges and labels, and propagates
  `nn_tainted` / `at_most_tainted` / `at_most_dep` in the survivor direction; the one thing it
  drops is `excluded` (semantic-branching Layer B), which is documented as MISS-only and is
  default-OFF anyway.
- Both arms of `find_open_disjunction` carry the `is_blocked` guard (the shipped fix is not
  half-applied — the MRV arm at 3171 has it too).
