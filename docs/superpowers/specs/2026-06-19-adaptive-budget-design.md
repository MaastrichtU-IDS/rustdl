# Adaptive per-pair budget (divergence detection) — design (Lever #1)

**Date:** 2026-06-19
**Status:** approved (scoping → spec 2026-06-19)
**Context:** `docs/superpowers/specs/2026-06-19-perf-frontier-levers-scoping.md`,
memory `sp2-perf-attribution-2026-06-19`. The corpus perf gap is 3 disjunctive-
branch-count-bound SROIQ outliers (ore-15672 138s, wine, family); each hard pair
burns its full `--pair-timeout-ms` deadline before giving up. Konclude: ms.

---

## 1. Goal & non-goals

### Goal

Cut a **diverging** wedge search *early* — when it is provably making no progress
toward a model — instead of waiting out the full per-pair deadline. The pairs this
affects are the hard **non-subsumptions** that already time out (recorded "not
subsumed"); cutting them at ~100ms instead of 1000ms reclaims most of the wall
(ore-15672 target: 138s → ~15–30s).

### Non-goals

- **Reducing the branch count** (Lever #2 — gated research). Lever #1 stops futile
  searches *sooner*; it does not make them explore fewer branches.
- **Changing any verdict that the full deadline would have produced** (see §3).
- Non-wedge engines.

### Ship criterion

Ship **default-ON** iff the gate confirms: corpus closures **byte-identical** (FP=0
**and MISSED unchanged** — the cut pairs are exactly the ones that time out anyway)
AND ore-15672/wine wall drops materially. If any closure changes (a recovered MISS
turned into a cut, or vice versa) → flag-gate or retune. FP=0 is structural (see §3);
the real gate is **MISSED unchanged**.

## 2. Architecture

### Where

The wedge `HyperEngine::decide_with_deadline` loop (`hyper.rs`) already checks the
deadline periodically (`if let Some(dl) = self.deadline && Instant::now() >= dl {
return Stalled }`). Add a **divergence check** at the same cadence point: evaluate a
predicate over the live `SearchStats` + node count; if it fires, `return Stalled`
(the same value the deadline returns).

### The divergence predicate

A search is **diverging** (will not find a model) when, over a sampling window of `N`
branches, ALL of:
1. **No progress:** `Δrestores / Δbranches ≥ θ_r` (near 1.0 — essentially every
   branch in the window failed and was restored), AND
2. **Depth saturated:** `max_branch_depth` has reached the depth cap
   (`HYPER_WEDGE_DEPTH`), AND
3. **Model not stabilizing:** node count grew over the window (the ∃-generation keeps
   manufacturing successors instead of converging).

Implementation: track a checkpoint `(branches, restores, node_len)` snapshot every `N`
branches (a cheap counter in the decide loop). When a new checkpoint is taken, compare
to the previous; if the predicate holds across the window → `return Stalled` early.

Starting constants (tuned in the plan against the MISSED gate, loosened only while
MISSED stays 0): `N = 5_000`, `θ_r = 0.98`. Conservative by design — better to miss a
divergence (waste deadline, no harm) than to cut a converging search (a MISS).

### Flag

`RUSTDL_ADAPTIVE_BUDGET` — default decided by the gate. If the gate shows
MISSED-unchanged + wall-drop, **default ON** (strictly better: faster, identical
verdicts). If borderline, default OFF / opt-in. The flag also gives a clean A/B and a
revert path.

## 3. Soundness

**FP=0 is structural and trivial.** An early cut returns `Stalled` → the orchestrator
records the pair as **"not subsumed"** — identical to what the deadline already
produces. Early-cutting can therefore only **lose a subsumption (a MISS), never invent
one.** No subsumption is ever asserted by this change.

The *only* risk is **recall**: cutting a search that *would* have terminated with
`Unsat` (subsumed) → a new MISS. Two guards:
- The predicate fires only on **non-progressing** searches (`restores ≈ branches`,
  depth saturated, model growing) — the signature of a search that is NOT closing in
  on a clash. A search about to terminate `Unsat` has a clash imminent (a successful
  branch / shrinking obligations), which the predicate's "no progress" clause
  excludes.
- The **corpus MISSED gate** (byte-identical closures) is the empirical backstop: if
  any closure shrinks, the predicate cut a real subsumption → retune (raise `N`/`θ_r`)
  or flag-gate. This is the same discipline SP1/SP2 used.

## 4. Testing & gates

1. **Unit (predicate logic):** a synthetic that exercises the divergence counters —
   assert the predicate fires on a constructed non-progressing trace and does NOT fire
   on a progressing one (depth-not-saturated / restores<branches).
2. **Early-cut behavior:** a known-diverging pair (e.g. one of ore-15672's
   `e-interaction` pairs) returns "not subsumed" *faster* with the flag ON than the
   deadline alone — and the same verdict.
3. **Corpus closure-IDENTITY net (sacred):** FP=0 **and MISSED unchanged** (every
   closure byte-identical) across galen, notgalen, sio, wine, ore-10908, ore-15672,
   alehif, ro, pizza, bibtex. Any closure change → retune/flag.
4. **Wall measurement:** ore-15672 + wine classify wall, flag-ON vs OFF — material drop
   expected (ore-15672 ~5–10×). Fast fixtures (galen/sio) unchanged.
5. Workspace suite green; clippy `--all-features -D warnings`; fmt.

## 5. Decomposition

1. Divergence-counter plumbing in the decide loop + the predicate + flag (off);
   unit test (§4.1).
2. Wire the early-cut return; early-cut behavior test (§4.2).
3. Tune `N`/`θ_r` against the **MISSED gate** (§4.3) — loosen only while MISSED=0;
   measure wall (§4.4).
4. Flip default per the gate; CLAUDE.md + memory.

## 6. Open questions for implementation

- **Cadence hook:** confirm the decide loop has a natural per-branch (or per-window)
  point to snapshot the counters without per-branch overhead — piggyback on the
  existing deadline-check cadence if it's branch-driven, else add a cheap branch-count
  modulo.
- **`max_branch_depth == cap` detection:** the cap is `HYPER_WEDGE_DEPTH` (256, in
  reasoner `lib.rs`) passed into `decide_with_deadline` as `max_depth`; the engine
  knows it. Confirm `SearchStats.max_branch_depth` is live-updated (not only at the
  end) so the predicate can read it mid-search.
- **wine interaction:** wine's stalls are nominal+cardinality (`merge` branches), not
  pure `disj` like ore-15672 — confirm the predicate's clause (1) keys on total
  restores/branches (covers both `disj` and `merge`) so wine benefits too, or scope
  Lever #1 to `disj`-dominated stalls first and treat wine separately.
