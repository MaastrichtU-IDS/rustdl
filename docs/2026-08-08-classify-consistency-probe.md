# `classify` reports `consistent = true` on inconsistent ontologies (`RUSTDL_CLASSIFY_CONSISTENCY_PROBE`)

2026-08-08. Opt-in, default OFF. Found while scoping the `RUSTDL_INVERSE_PAIR_FUNC`
follow-up — the blocker there turned out to be downstream of this.

## The defect, sized

`classify`'s inconsistency detection consults the saturator's `⊤`-unsat signal and the
`ABox` pre-check, but **never the wedge-consistency route `is_consistent` uses**. Census
over all **1,920** ORE ontologies:

| | |
|---|---:|
| `is_consistent` says inconsistent | **43** |
| `classify` agrees | 41 |
| **`classify` says `consistent = true`** | **2** (`ore_ont_16372`, `ore_ont_7610`) |

Two wrong answers — the worst failure class, worse than a DNF, because the caller gets a
confident wrong verdict. Konclude independently reports both inconsistent.

## Why the obvious fix is a dead-end, and what makes this one affordable

Running a consistency check unconditionally on the classify path costs, on 60 sampled
**consistent** ontologies, a mean of **5.1 s** (16 over 1 s, max 30 s). That is the
already-recorded dead-end.

The gate is the whole idea:

> An inconsistent KB makes `⊤` unsatisfiable, hence **every** class unsatisfiable.
> Contrapositive: **zero unsatisfiable classes ⟹ consistent**, so no probe is needed.

Measured, that admits **1 of 60** sampled ontologies (**~1.6%**) — the probe runs on
roughly 31 of 1,920 rather than all of them. Both targets pass the gate (3 and 91
unsatisfiable classes).

**Soundness.** Skipping preserves today's behaviour exactly, so the gate can only fail to
fix, never break. It is a heuristic for *when to look*, not a claim — classify's own
per-class unsat detection is incomplete, so an inconsistent KB could in principle show
zero unsat classes and be skipped. A positive verdict is a wedge `Unsat`, which
`is_consistent` already trusts as a real inconsistency on the same justification.

## Result: 1 of 2 fixed

| | probe OFF | probe ON |
|---|---|---|
| `ore_ont_7610` | `consistent=true`, 0.08 s | **`consistent=false`**, 0.12 s ✓ |
| `ore_ont_16372` | `consistent=true`, 3.20 s | `consistent=true`, 3.79 s ✗ |

`ore_ont_7610` reports all **91 of its 91** classes unsatisfiable, i.e. `classify_inconsistent`
fires correctly.

**`ore_ont_16372` is NOT fixed, and not for want of budget** — 1,000 ms and 10,000 ms give
the same answer. `RUSTDL_TRACE` shows its wedge returns **`Stalled`**, with detection
happening in `is_consistent`'s *bounded main-tableau fall-through*, which this probe does
not reach. An earlier ablation predicted this: `RUSTDL_WEDGE_CONSISTENCY=0` still detected
it, so the wedge was never its detector. Reaching it means running the expensive route —
i.e. the dead-end above — and is deliberately not attempted here.

## Gates

- **0 flips on 40 sampled consistent ontologies**, ON vs OFF; total wall 93.6 s → 95.9 s
  (+2.5%, and that sample is deliberately *not* gate-filtered).
- Workspace **1,605 passed / 0 failed**; fmt and clippy clean.
- Canaries `crates/owl-dl-reasoner/tests/classify_consistency_probe.rs` (5).
  **Sabotage 1 of 2 caught.** Caught: a dead flag. **NOT caught: deleting the
  `unsat ≥ 1` gate** — the canaries pin env plumbing, and the gate is a *cost* property no
  unit test can observe. Its justification is the 1.6% measurement above, not a test. A
  future simplifier must not read these canaries as protecting it.

## Status

Default **OFF** pending a full-corpus two-arm sweep — the probe adds work on ~1.6% of
ontologies and its failure direction is a false *inconsistency*, so it needs the same
FP-shaped scrutiny as any soundness-adjacent change, on more than a 40-ontology sample.

Relation to `RUSTDL_INVERSE_PAIR_FUNC`: `ore_ont_16372` is that flag's only remaining
blocker, and it stays blocked. But the re-measurement in
`docs/known-limitations/inverse-pair-functionality-not-derived.md` shows its "regression"
is a comparison between two **wrong** answers on an inconsistent KB. If `16372`'s
inconsistency were detected, classify would return all-unsat in ~0.4 s and the flag's
53× slowdown there would be moot.
