# Bound-the-tail (dense-SROIQ) — findings (2026-07-15)

**Context:** Fix #2 (semantic branching) was NO-GO on `ore_ont_10019`
(`docs/2026-07-15-semantic-branching-layerB-findings.md`). The spec's fallback is
**bound-the-tail**: make the `Stalled → NoVerdict → search.rs` fallthrough return
sound-incomplete fast so dense-SROIQ inputs degrade gracefully.

## Diagnostic (the valuable result)

On `ore_ont_10019` (`--pair-timeout-ms 250`, aggregate 120 s), tier_walk = **77.7 s**.
A measurement stub that skips the main-tableau fallthrough for **every** wedge
non-verdict drops it to **43.4 s (−44 %)**. So **~half the dense-SROIQ classify wall
is the main-tableau (search.rs) fallthrough re-thrashing pairs the wedge already
stalled on.** Mechanism: `subsumes_via_tableau` runs the wedge (`hyper_decide`,
recorded in the wedge-cost histogram — 1265 pairs at 100–999 ms), and on a `Stalled`
falls through to `prepared.decide_with_deadline(build)` (the main SROIQ tableau),
which `effective_deadline` hands a **fresh** per-pair budget — so a hard pair burns
the wedge budget *and* another full tableau budget, the second hidden from the
histogram.

## The sound-minimal fix is INERT on the target

The advisor-scoped sound move: skip the fallthrough only when the wedge stalled
**because `is_diverging` fired** (divergence-`Stalled`, not deadline-`Stalled`) —
completable onts don't trip `is_diverging`, so they stay untouched. Built behind
default-OFF `RUSTDL_BOUND_DIVERGED_TAIL` (a `diverged` bit threaded from the wedge
via `HyperVerdict::UnknownDiverged`).

**Measured inert:** `ore_ont_10019` is byte-identical + same wall (78.2 s vs 77.9 s)
OFF vs ON. The pairs hit the 250 ms per-pair **deadline before `is_diverging`'s
window/depth conditions are met** (they thrash at *unsaturated* depth, avg ~62 ms/pair;
`is_diverging` requires `depth_saturated` over a 500-branch window). So the wedge
returns a plain deadline-`Stalled` (`Unknown`), never `UnknownDiverged`. Curated is
byte-identical (sio/pizza/ore-10908), ore-15672 already fast (0.03 s). The flag is
sound (FP=0, curated MISSED=0) but exercises on nothing measurable here.

## Why the −44 % needs the completeness-RISKY path

The −44 % comes from skipping the fallthrough on **deadline**-`Stalled` pairs (the
stub skipped all). That is exactly the move the prior author deliberately rejected
(see the aggregate-deadline comment in `classify.rs`): a deadline-`Stalled` wedge may
just need the main tableau, which is **complete where the wedge is not** (defined-sup
/ functional-role + ≥n-with-disjointness patterns — the reason the fallthrough exists;
109 MISSED on GALEN traced to trusting a wedge `NotSubsumed` there). Skipping it (or
sharing one budget across wedge+tableau) risks MISSING real subsumptions on onts where
the tableau completes after a wedge stall — off-corpus completeness risk, curated
MISSED-gated at best.

## Decision

**Sound-minimal bound-the-tail is inert; the mover is completeness-risky.** Per the
spec/advisor exit condition, do not ship a risky knob to clear a menu item.

- Kept: the sound divergence-keyed skip (`RUSTDL_BOUND_DIVERGED_TAIL`, default OFF,
  curated MISSED=0) + the `diverged` / `UnknownDiverged` plumbing — a correct,
  reusable distinction (divergence-`Stalled` vs deadline-`Stalled`) and a foundation
  IF divergence detection is later made to fire pre-deadline (that is `DIV_WINDOW`
  tightening — corpus-MISSED-gated, previously flagged risky).
- **Deferred (options for the user):**
  1. **Budget-sharing / deadline-keyed skip** — the −44 % win, behind a default-OFF
     flag, validated by a **full-corpus MISSED=0** gate (the only thing that can bless
     it; it will MISS on any curated ont where the tableau rescues a wedge stall).
  2. **Earlier divergence detection** (lower `DIV_WINDOW` or an unsaturated-depth
     thrash signal) so the *sound* divergence-keyed skip actually fires on
     `ore_ont_10019` — corpus-MISSED-gated per the adaptive-budget precedent.
  3. **Leave as-is** — the aggregate deadline (`RUSTDL_AGGREGATE_DEADLINE_MS` /
     `--global-timeout-s`) already bounds total wall; the bare-path unboundedness is
     the prior author's deliberate choice to not cut completable-slow onts.

The genuine finding — half the dense-SROIQ wall is redundant fallthrough re-thrash —
is documented for whoever picks up option 1 or 2.
