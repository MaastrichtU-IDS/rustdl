# `tier_walk` 400× gap — investigation plan

**Target:** `ore_ont_10460` — **601 classes**, 750 `SubClassOf`. Konclude **0.22 s** / 1,226
subsumptions; rustdl **88.59 s** / 588 rows. `tier_walk = 17,943 ms` of a 20 s budget,
`label heuristic: pruned=24538 pass_through=0 misses=1501`, `pairs-per-sub: n_subs=17
total=1501 median=95 p90=133 max=133`, 1,609 pairs hitting the 5 ms cap.

**Why this target and not the tail's biggest bucket:** it is *tiny* (so profiling is tractable and
a fix is unlikely to be an architecture rewrite), it is **definitively a gap** rather than
intrinsic hardness (a peer does it in 0.22 s), the cost is localised to one phase, and **13 tail
members share the bucket** with the same signature (11–18 s for 588–1,160 rows).

**Standing rule for this plan — no fixes before root cause.** Three causal stories died in the
2026-08-19/20 sessions for being plausible ahead of measurement. Every step below ends in a
measurement that can kill the next one, and each states what would falsify it.

---

## Phase 1 — root cause (no code changes)

### Step 1. Is `588` vs `1,226` a COMPLETENESS gap or a presentation difference? **[gates everything]**

rustdl emits `direct` rows; Konclude emits its own hierarchy. Compare **transitive closures**, not
row counts — this exact confusion produced a false "lost 3 rows" reading on 2026-08-19.

* Run: close rustdl's `direct`+`equiv` output and Konclude's `.owx` hierarchy, diff the sets.
* **If rustdl's closure is a strict subset** → this is a *completeness* bug, a different and
  higher-priority problem, and the perf framing is wrong. **Stop and re-scope.**
* **If the closures agree** → purely performance. Continue.
* Falsifier: any subsumption in Konclude's closure absent from rustdl's, confirmed by
  `justify` on that pair.

### Step 2. Attribute the 17.9 s *inside* `tier_walk`

The unbounded run is 88.59 s, so sampling is easy.

* Run: `perf record` on the unbounded classify, report top self-time frames.
* Deliverable: the function that owns the time, with a percentage.
* Falsifier for the whole plan's premise: if the time is spread with no frame above ~10%, there is
  no localised hot spot and the target is scale-shaped after all.

### Step 3. Why does the label cache MISS 1,501 pairs?

`pruned=24538` vs `misses=1501` — the cache works for 94% and then fails. The misses are what
reaches the expensive path.

* Run: `RUSTDL_DUMP_LABELS=1` (already in-tree) to dump per-class labels; check whether the misses
  concentrate on a few classes or are uniform.
* Decides: a per-class defect (fixable) vs a uniform property of the ontology (not).

### Step 4. What makes the 1,609 capped pairs hard?

* Run: identify several capped pairs, then `rustdl explain <sub> <sup>` on each — the in-tree tool
  for "which engine answered and why".
* Decides: one repeated shape (one fix) vs a heterogeneous set (no single fix).
* Note the constructs present: 32 `EquivalentClasses`, 10 `DisjointClasses`, **2 `DisjointUnion`**,
  2 `TransitiveObjectProperty`. `DisjointUnion` is a live suspect —
  covering-dependent *same-tier* subsumptions are a documented limitation needing
  `RUSTDL_CLASSIFY_SAME_TIER`, and the tier walk explicitly never compares same-tier classes.

## Phase 2 — pattern analysis

### Step 5. Do the other 12 `tier_walk` members share the mechanism?

* Run Steps 2–4's cheap checks on `ore_ont_2901`, `9890`, `10949` (comparable class counts).
* Decides whether this is **one** defect or 13. A shape census sizes a population; it does not
  predict a rescue — so this is confirmation, not selection.

### Step 6. What is Konclude doing differently?

Not to copy an implementation, but to identify which pruning rustdl lacks. 0.22 s for 1,226
subsumptions on 601 classes implies told-subsumer-driven traversal with far stronger pruning than
1,501 probes for 17 subsumptions.

## Phase 3 — hypothesis and minimal test

### Step 7. State ONE hypothesis, test it with the smallest possible change

* Written as "I think X is the root cause because Y", with the measurement that would refute it.
* Prefer an existing flag over new code: if `RUSTDL_CLASSIFY_SAME_TIER=1` (default OFF, known
  ~2× cost) closes the gap, the mechanism is confirmed *before* anything is built.
* One variable at a time. If the test fails, form a NEW hypothesis — do not stack fixes.

## Phase 4 — implementation, only if Phase 3 confirmed

### Step 8. Build behind a default-OFF flag, then gate

Required before any default flip, per `docs/releases/RELEASE-PROCESS.md`:

1. unit + lint — `cargo test`, `clippy -D warnings`, `fmt --check`, **chained on exit codes**
   (grepping a check into a display line pushed a clippy break on 2026-08-20).
2. FP=0 net — `./scripts/run-soundness-diff.sh`.
3. corpus report + **verdict gate** — catches consistency flips that a sweep structurally cannot.
4. two-arm sweep over the affected frame; verify a comparison is stable **within** one arm before
   comparing two arms (a self-inconsistent metric reported 859 fictional differences on
   2026-08-21).
5. MISSED net if completeness is traded.

## Kill criteria — when to stop rather than continue

* Step 1 shows a completeness gap → re-scope; this is not a perf task.
* Step 2 finds no frame above ~10% self-time → no localised hot spot; deprioritise.
* Step 5 shows 13 unrelated mechanisms → the bucket is not one defect; report and stop.
* Step 7's hypothesis fails twice → return to Phase 1 with the new information rather than
  attempting a third fix.
