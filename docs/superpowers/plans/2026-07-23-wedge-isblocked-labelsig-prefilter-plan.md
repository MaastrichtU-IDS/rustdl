# Plan: label_sig bloom prefilter for the wedge's `HyperEngine::is_blocked`

**Status:** **CLOSED — P0 measured NO-GO on the `label_sig` prefilter (2026-07-23).** The clone-removal half shipped (v0.3.38). Do NOT implement the prefilter.

> **P0 result (the gate that decides this — §3/B4).** Profiled two *converging*,
> wedge-heavy classify runs (`ore_ont_1508` 12.9 s; `ore_ont_12698` 5.6 s).
> `HyperEngine::is_blocked` self-time = **0.05 % / 0.10 %** of total — vs the
> ≥15 % GO threshold (~150–300× below). The "is_blocked is the 7× hottest leaf"
> figure was, exactly as the advisor warned, a **diverging-`ore_ont_9899`-run
> artifact**; on converging workloads `is_blocked` is negligible. **NO-GO —
> abandoned.** The clone-removal (§4.3) shipped independently in v0.3.38.
>
> The profiles also located the *real* converging-wedge-classify hot spots (for
> the roadmap, not this plan): **`enumerate_matches` + `match_body` ~25 %**
> combined on 1508 (the maintainer's in-flight non-Horn fire-loop / trigger-index
> plans — confirms their targeting), and **`build_clause_indexes` ~11–15 % +
> `ClauseIndexes` drop + heavy malloc/free churn** (the clause-index structure is
> rebuilt+dropped per solve frame — a possible separate amortization target).
**Author:** Claude, 2026-07-23. Session: issue-#35 perf arc — the "remaining tableau frontier" after the abox-saturation wins (v0.3.36/v0.3.37).
**Branch (to create):** `perf/wedge-isblocked-labelsig`.

> **Advisor outcome (folded in below).**
> - **Strategic steer:** the `is_blocked` "7× hottest leaf" is measured on a
>   *diverging* 9899 run — it is hot *because* the search thrashes. On converging
>   wedge-heavy workloads (GALEN/sio classify, ~0.9 s total) the wedge is already
>   fast, so the sig prefilter is probably a sub-100 ms, corpus-invisible
>   micro-opt ("optimizing the hot leaf of the wrong workload"). **Best return is
>   Option 2 — port classify's shipped, default-ON `is_diverging` detector to the
>   consistency path** to cut the two back-to-back 10 s stalls on 9899 at their
>   real cause, verdict-preserving. That gets its own plan; do it next.
> - **B1 (blocking):** the label-insert audit found ONE bypass site — `from_snapshot`
>   (hyper.rs ~2150) does `labels.clone_from(...)` without touching the sig →
>   stale-`0` sig → **false prefilter reject → missed block → non-termination**
>   under `RUSTDL_SNAPSHOT_CAPTURE=1`. Must recompute `label_sig` there.
> - **B2 (blocking):** the §5.1 debug-assert gate must run with
>   `RUSTDL_SNAPSHOT_CAPTURE=1` (defaults OFF) + a `save`→`from_snapshot` round-trip
>   unit test asserting `label_sig == sig_of(labels)`, else B1's site is untested.
> - **B3 (blocking):** cannot reuse `graph.rs::label_sig_bit` — it takes
>   `ConceptId`; `HyperNode.labels` is `Vec<ClassId>` (distinct newtype) → type
>   error. Add a hyper-local `class_sig_bit(ClassId)` (same hash); soundness only
>   needs hyper to be internally consistent (it never compares vs a graph.rs sig).
> - **B4 (blocking):** re-gate P0 on **measured `is_blocked` self-time A/B on a
>   CONVERGING wedge-heavy run** (GALEN/sio classify), not the modeled reject-rate
>   and not 9899. If not material there → NO-GO on the sig (P0 does its job).
> - **Good news (simplification):** the wedge backtracks by whole-`Vec<HyperNode>`
>   clone/replace in `save`/`restore`, so a `label_sig` struct field is
>   saved/restored automatically — **no undo/recompute-on-backtrack path needed**
>   (no "can't un-OR a bloom bit" hazard). Only `add` (the incremental
>   choke-point at ~302, via `add_label` ~1479) and `from_snapshot` (B1) touch it.
> - **Clone-removal (§4.3) is the clean, independent win** — ship it regardless of
>   the sig: accumulate `block_compares`/`blocks_fired` in loop-locals (disjoint
>   immutable borrows of `block_index`/`nodes` coexist), write to `self.stats`
>   after the loop — removes the per-call `Vec` allocation, no borrow gymnastics.

## 0. Honest framing (read first)

This is **NOT** a fix for `ore_ont_9899`'s 21 s consistency wall. That wall is a
**convergence** problem, not a hot-scan: the wedge stalls its 10 s budget, the
tableau fall-through stalls another 10 s, and rustdl reports **"consistent
(incomplete)"** — which is the *correct* verdict (Konclude confirms 9899
consistent), just reached by timeout, not proof. No local micro-opt makes 9899
converge; that needs a better decision strategy (Konclude does it in 0.4 s via a
different algorithm) and is a separate, much larger project.

What this plan IS: a **throughput** optimization of the single hottest leaf in
the wedge search (`HyperEngine::is_blocked`), which is exercised by **every**
wedge query (classify pairs AND consistency), so a real per-call win compounds
corpus-wide. It will let 9899 do *more* search per 10 s budget (possibly
converging some currently-stalling inputs, more likely just cheaper stalls), and
should shave measurable wall off the many corpus ontologies where the wedge is
the bottleneck. **It is justified by a corpus wall delta, not by 9899.**

## 1. Problem (measured this session)

`sample`-profiling `RUSTDL_ABOX_SATURATION=0 consistent ore_ont_9899` (~21 s, the
tableau path) — **top self-time leaf is `HyperEngine::is_blocked` (6537 samples,
~7× the next frame** `clash_deps_at` 874). The wedge's blocking check
(`hyper.rs:1596`) is already bucket-indexed by parent-role (O(bucket), not O(n)),
but for each candidate it runs two `subset_sorted(labels)` calls with **no cheap
prefilter**:

```rust
for m_hnode in candidates {                       // same-parent-role bucket
    …
    if subset_sorted(&n.labels, &m.labels)        // O(|labels|) — no prefilter
        && subset_sorted(&np.labels, &mp.labels) { return true; }
}
```

Two per-call costs visible in the code:
1. **No `label_sig` bloom prefilter.** The **main** tableau's blocking
   (`graph.rs` `Node.label_sig`, `label_sig_bit`/`label_sig_of`;
   `lib.rs::is_blocked_ancestor`) skips `subset_sorted` when
   `blocked.sig & !cand.sig != 0` (a set bit missing from the candidate ⇒ can't
   be a superset). Phase 3 added exactly this to the main tableau (GALEN classify
   −14.6 %). **`HyperNode` has no `label_sig` field** — the wedge never got it.
2. **Per-call candidate-bucket clone.** `let candidates: Vec<HNode> = …cloned()`
   allocates every `is_blocked` call (the comment: cloned to release the borrow
   before mutating `stats`). In a loop called on every generated node, that is a
   per-call heap allocation.

## 2. Goal & non-goals

**Goal:** add a `label_sig: u64` bloom to `HyperNode`, maintained incrementally
on label insert, and gate the `subset_sorted` pair in `is_blocked` behind the
sig prefilter (both the blocked/candidate and parent/parent legs). Also remove
the per-call bucket clone. Target: reduce `is_blocked` self-time materially on
the wedge-bottlenecked corpus onts; **verdicts and block decisions byte-identical**.

**Non-goals:**
- NOT a 9899 convergence fix (§0). Do not touch the 10 s budgets or the
  wedge-Stalled→tableau fall-through policy (that is a separate budget-policy
  question — noted §8).
- Do not change the blocking *semantics* (subset pair-blocking stays); the
  prefilter is a sound necessary condition, so it only *skips* provable
  non-matches — the set of `is_blocked==true` decisions is unchanged.
- Do not restructure the hyperresolution loop, clausifier, or double-blocking
  candidate index (that overlaps the maintainer's in-flight `match_body` /
  `fire_clause` plans — §6 risk).

## 3. Phase 0 — measurement GATE (MANDATORY, do FIRST; this DECIDES the project)

Phase 3 (the main-tableau version of this) was gated on a real wall delta and
some variants were reverted at regressions — treat this the same.

- **P0a — prune-rate probe.** Instrument (behind an env flag, temporary) the
  counts already in `stats`: `block_compares` (candidate pairs examined) and a
  new `block_prefilter_rejects` (pairs the sig would skip). On `ore_ont_9899`
  and 2–3 wedge-bottlenecked corpus onts (candidates: `sio`, `ore_ont_6132`,
  and one high-`block_compares` ORE ont), report the fraction of `block_compares`
  the prefilter would reject BEFORE `subset_sorted`.
- **GO if:** prefilter reject-rate ≥ ~50 % on the probes AND `is_blocked` is
  ≥ ~15 % of wedge self-time (it is ~7× the next leaf on 9899). 
- **NO-GO if:** reject-rate is low (labels are small / mostly-overlapping so the
  bloom rarely discriminates) — then the win is marginal and the `label_sig`
  maintenance cost on every label insert may not pay for itself.
- **P0b — sig maintenance cost.** Confirm maintaining `label_sig` on label
  insert (one `|=` per insert) does not itself regress; the main tableau already
  pays this, so expected negligible, but measure on GALEN classify (the
  wedge-heavy baseline).

## 4. Phase 1 — the prefilter (only if P0 = GO)

1. Add `label_sig: u64` to `HyperNode`; initialize 0; OR-in `label_sig_bit(c)`
   (reuse `graph.rs`'s function — make it `pub(crate)` if not already) at **every**
   label-insert site for a hyper node (single choke-point if one exists;
   otherwise audit all `.labels.push`/insert sites — this is the correctness-
   critical part: a missed insert site ⇒ a sig that under-represents the label
   set ⇒ a *false prefilter reject* ⇒ a MISSED block ⇒ non-termination /
   exponential blowup, NOT just slowness).
2. In `is_blocked`, before each `subset_sorted`:
   `if (n.sig & !m.sig) != 0 { continue; }` (blocked ⊄ candidate impossible) and
   likewise for the parent leg. Only survivors pay `subset_sorted`.
3. Remove the per-call bucket clone: restructure so `stats` is mutated without
   holding the `block_index` borrow across the loop (e.g. collect the decision
   first, or split the borrow) — avoid the `Vec` allocation per call.

## 5. Correctness gates (mandatory)

1. **Block-decision identity.** The prefilter is a sound necessary condition, so
   `is_blocked` must return the SAME bool for every call. Gate: a debug build
   asserting that whenever the sig prefilter would skip a candidate,
   `subset_sorted` would also have returned false (run under `RUSTDL_ABOX_...`-
   style flag on `sio` + 9899 + GALEN; zero violations).
2. **Verdict identity, corpus-wide.** classify closures byte-identical (FP=0/
   MISSED=0) on the full curated corpus (ro/sio/sulo/pizza/wine/galen +
   notgalen) and `is_consistent` verdicts unchanged on the 79 ORE ABox onts.
   This is the load-bearing gate — a missed block surfaces as a changed verdict
   or a new non-termination.
3. **Full suite** green; **fmt + clippy**.
4. **Perf:** GALEN classify wall and `ore_ont_9899` `is_blocked` self-time
   (re-`sample`) — both improved or flat, none regressed.

## 6. Risks

- **Missed label-insert site (HIGH).** The `label_sig` must OR-in at *every*
  hyper-node label mutation. A missed site makes the sig stale-low → the
  prefilter wrongly rejects a real superset → a MISSED block → the wedge fails
  to terminate on a cyclic input. Mitigation: a single `insert_label` choke-point
  + the §5.1 debug assertion (sig-skip ⟹ subset_sorted-false) which directly
  catches an under-representing sig. This is the same invariant the main tableau
  already upholds; port carefully.
- **Overlap with in-flight hyper.rs work (MEDIUM).** The maintainer has active
  `match_body` / `fire_clause` fire-loop plans in `hyper.rs`. Coordinate: this
  change touches `HyperNode` + `is_blocked` only (not the fire loop / clausifier),
  but rebasing may conflict. Keep the diff minimal and localized.
- **Marginal-win risk (MEDIUM).** If P0 shows low reject-rate, the sig
  maintenance cost may exceed the `subset_sorted` savings — hence the P0 GATE.
- **Does not help 9899 converge (KNOWN, §0).** Sets correct expectations.

## 7. Delegation notes (Fable)

- Files: `crates/owl-dl-tableau/src/hyper.rs` (HyperNode + is_blocked +
  label-insert sites); possibly `graph.rs` to `pub(crate)`-expose
  `label_sig_bit`. No public API change.
- Land P0 instrumentation first (bisectable), gate, then the prefilter.
- The label-insert audit is the whole ballgame — enumerate every site that
  pushes to a `HyperNode.labels` and route the sig update through it.

## 8. Follow-ups / separate frontiers (out of scope)

- **9899 convergence** — the real 21 s: the wedge + tableau don't decide
  Top-consistency in 2×10 s. Needs a stronger strategy (Konclude-style) or a
  divergence-aware budget policy (avoid the second 10 s stall when the first
  diverged — mirror classify's adaptive-budget divergence detector on the
  consistency path). Separate spec.
- Wedge fire-loop (`match_body`/`fire_clause`) — the maintainer's in-flight arc.
