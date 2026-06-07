# Performance attribution — post-v0.3.6 (2026-06-07)

Measured + flamegraph-attributed the corpus perf picture after the
nominal-completeness work (wine 57→0). **Conclusion: the remaining perf frontier
is hard — every candidate lever is already-optimized, a known dead-end, or
soundness-nuanced. No clean low-risk high-value win is sitting here.** Banked as a
scoping deliverable rather than rushed into a build.

## Current classify walls (`classify --pair-timeout-ms 200`, single run)

| ontology | wall | vs 06-04 | note |
|---|---:|---|---|
| **wine** | **412.7 s** | (was n/a) | the corpus's dominant cost |
| ore-15672 | 29.1 s | flat | SROIQ-N ratio (~16× Konclude) |
| sio-stripped | 30.4 s | ~flat | SROIQ ratio (~13×) |
| ore-10908 | 5.3 s | flat | inside the ≤5× target |
| pizza | 2.1 s | 3.5→2.1 | fine |

The completeness levers gave wine **MISSED=0 but did not help its wall** — the
saturator now *derives* all 622 subsumptions, yet the per-pair tableau still runs.

## wine 412 s — attributed: redundant tableau invocation (tableau contributes 0)

`classify` stats: `subsumption: saturation=622 tableau=0`; `tier_walk=409.6s`
(≈ the whole wall); **`timed-out pairs: 9165`** (× 200 ms cap); `hyper-proven: 21`.
So **the per-pair tableau finds ZERO subsumptions** — it burns 200 ms on each of
9165 wedge-stalled pairs to default to "not subsumed" (all correct, FP=0/MISSED=0).
Flamegraph (60 s of the real classify): the cost is the **full completion over
the ABox-seeded graph** — `apply_nominal_assignment`, `apply_min`, `apply_forall`,
`apply_and`, `first_clash`, `clash_deps_at`, ~25 % rayon idle/wait.
**`is_blocked` is NOT hot** (overturns the earlier counter-based guess taken on a
single `Fruit` decide — confirm on the real run, always).

**The finding is "these tableau calls shouldn't happen," not "make a rule
faster."** Only 21 of the non-trivial pairs are settled by the wedge; the other
9165 *stall* in the wedge and fall through to the slow tableau. Lever = the
**orchestrator**: settle the wedge-stalled non-subsumptions without the 200 ms
ABox-seeded tableau. Two sub-questions, both real:
- *Why does the wedge stall on 9165 wine pairs?* (it has its own nominal/
  cardinality handling). If it returned `NotSubsumed` it would short-circuit via
  `trust_sat`; it doesn't, so it must `Stall`.
- *The ABox-seed-skip is NOT sound-for-completeness* (advisor): a named
  individual CAN force a universal `C ⊑ D` via nominals (`C ⊑ ∃R.{a}`, `a:E` ⟹
  `C ⊑ ∃R.E`), so dropping the seed risks a silent MISSED regression on
  Horn-shortcircuited ontologies — and wine's own 57→0 may lean on seeded
  individuals. Must be gated on MISSED=0 across the ABox-bearing fixtures, not
  just FP=0.

Highest *value* (412 s → potentially seconds) but soundness-area orchestrator
work — a careful, reviewed, fresh undertaking, not a session-tail build.

## sio-stripped / ore-15672 (the roadmap SROIQ ratio) — `edge_satisfies` call volume

Flamegraph: `are_declared_inverses` ~11.5 % + `is_sub_role` ~8 % + `apply_role_chains`
6.4 % + `apply_deferred_or_residuals` 8.5 % + `first_clash`/`clash_deps_at` ~11 %.
Both `are_declared_inverses` and `is_sub_role` live inside **`edge_satisfies`**
(`lib.rs:602`), the per-edge role-match predicate. The lookups are already O(1)
(Phase 3b hashset); the cost is **call volume** (per-edge × per-role-atom during
rule application). Reducing the calls = edge-keyed role indexing = **Phase 3e,
which was reverted** (+2.34 % GALEN regression — workload-dependent break-even;
dead-end ledger §16). So the obvious lever here is a *known dead-end*. A genuine
win needs a different structure (e.g. memoizing `edge_satisfies(s,w)` per role-pair
— but the lookup is already O(1), so the saving is call/branch overhead, likely
marginal) — verify redundancy (calls vs distinct `(s,w)` args) with a counter
before investing, or it repeats 3e.

## Shared frame — `apply_deferred_or_residuals` (wine 5.5 %, sio 8.5 %)

The one frame hot in both. **Already bloom-prefiltered** (Phase 3,
`needs_deferred_or` + `label_sig`, `rules.rs:589–609`). The residual is intrinsic
work over the deferred-OR rules per node; no obvious further win.

## wine root cause — MEASURED (corrects the earlier "stalls fast/structural" guess)

The 2026-06-07 follow-up *measured* the per-pair wedge cost (CLI
`wedge-cost-histogram` + a construct-vs-solve probe + corpus closure-diffs at
swept timeouts). The earlier "stalls fast / structural depth-bound" reading was
**wrong**. The data:

- **wedge-cost histogram (wine, 200 ms budget):** bucket 7 (100-999 ms) = **9183**
  pairs; buckets 0-1 = 4272. So the wedge burns its **full ~200 ms deadline** on
  9183 pairs — a **slow deadline-stall**, not a cheap structural one.
- **construct-vs-solve probe** (`wine_wedge_construct_vs_solve_probe`, lib.rs):
  on 3 confirmed bucket-7 pairs, `clauses.clone()` = 0.1 ms, `HyperEngine::new` =
  0.0 ms, **`solve` (5 s cap) = 5000 ms → Stalled**. Construction is negligible
  (661 clauses); the **search genuinely does not terminate** on wine's
  nominal+cardinality fragment, even at 5 s. (Refutes the "build-once-reuse" and
  "raise the deadline to reach `Sat`" levers both.)
- Each hard wine pair therefore costs ~wedge(200) + ~tableau(200) ≈ 400 ms, and
  **the tableau finds zero** (`tableau=0`): both engines correctly fail to refute
  a genuine **non-subsumption**, defaulting to "not subsumed" (sound). wine's
  412 s is the engines *correctly* spending their full budget on 9165
  non-subsumptions that no amount of time refutes.

## The budget is workload-dependent at BOTH engine levels — no safe static lever

Swept `--pair-timeout-ms` / `RUSTDL_TEST_PAIR_MS` vs **ground truth** across the
non-Horn corpus:

| fixture | MISSED @25 ms | needs >25 ms? |
|---|---|---|
| wine, ore-10908, ore-15672, sio, sio-stripped, ro-stripped, sulo-stripped, shoiq-knowledge, alehif | **0** | no — identical to default |
| **pizza** | **4** | yes — `{AmericanHot,Cajun,PolloAdAstra,SloppyGiuseppe} ⊑ InterestingPizza` |

So `--pair-timeout-ms 25` gives **wine 7.5×** (412 s → **54.8 s**) with the
hierarchy **byte-identical** to the 200 ms baseline (MISSED=0), and is MISSED=0 on
every other non-Horn fixture — **except pizza**, which needs ~1000 ms.

**A wedge-only cap was built, tested, and reverted.** Hypothesis: cap the wedge
(useless on hard pairs everywhere) but keep the tableau's full budget, so pizza's
deep subsumptions still resolve. **Falsified by direct test:** at wedge-50 ms /
tableau-1000 ms, pizza recovered only 1 of 4 — `Cajun`/`PolloAdAstra`/
`SloppyGiuseppe ⊑ InterestingPizza` are **wedge-proved** (need 50-1000 ms of
*wedge*), not tableau-proved as assumed; the tableau alone gets only `AmericanHot`
in 1000 ms. So pizza needs **both** engines' full budget; wine wastes **both**.
There is no ontology that wants tableau-full + wedge-capped → **the wedge-only cap
serves no workload and was reverted** (kept: the `RUSTDL_TEST_PAIR_MS` harness
generalization + the construct-vs-solve probe).

## Recommendation

- **The right lever already exists: `--pair-timeout-ms` (caps both engines).**
  Document it for nominal-heavy / non-terminating-wedge ontologies: `25` gives
  wine 7.5× at MISSED=0; pizza-class needs the 1000 ms default. The default stays
  1000 ms (commit 6b06b1d's pizza-knee rationale holds).
- **No safe code change improves wine's wall** without regressing pizza-class —
  it's an intrinsic budget/workload tradeoff, surfaced (correctly) via the
  existing knob + the ⚠ loud-incompleteness signal.
- The only remaining engine-level lever would be **adaptive** (detect mid-search
  that the wedge/tableau is exploring an unbounded model vs progressing toward a
  clash, and back off only the former) — convergence-risky, a fresh-session
  undertaking, explicitly out of scope here.

- **Don't** open the editor on a lever now: the SROIQ one is the reverted-3e
  dead-end (verify redundancy first), the shared frame is already optimized, and
  the wine one is soundness-area orchestrator work.
- **Highest value = wine's redundant-tableau-invocation** (412 s → seconds): a
  fresh, reviewed undertaking — understand *why the wedge stalls on 9165 pairs*
  (the real root) and/or make the ABox-seed-skip with a MISSED=0 gate across the
  ABox-bearing fixtures. Soundness-critical; do it with full discipline.
- Cheap pre-work for next time: a redundancy counter on `edge_satisfies`/
  `are_declared_inverses` (calls vs distinct args) settles the SROIQ lever with
  evidence instead of a flamegraph %.
