# Coupled-saturation SEED probe — RESULTS + VERDICT (2026-06-25)

**VERDICT: GO (probe-validated; classify-scale FP gate pending).** Seeding the wedge
root with the class's **named** all-model saturated subsumers soundly collapses wine's
disjunctive model search — the **first sound lever of the entire wine arc to make the
wall class terminate.** Controls confirm the *seed* (saturation knowledge) is the lever,
not an MRV-reorder artifact. This validates the coupled-saturation **seed** mechanism
(Steigmiller–Glimm JAIR 2015; the paper Konclude implements) — distinct from this
session's NO-GO'd guide/prune (SP-B) and failed-literal (SP-A) couplings.

## Mechanism

`seed_probe` (reasoner `lib.rs`): `sat(class)` with the wedge root optionally seeded with
`Q → D` for every **named** (non-synthetic) saturated subsumer `D` of `class`
(`owl_dl_saturation::saturate`). Synthetic ids (NomKey/ForallKey/DKey/Tseitin, ≥
`num_classes`) are **filtered** — forcing those cross-engine is a spurious-clash FP (the
first probe run hit it: Zinfandel→Unsat). Named subsumers are entailed in *both* engines,
so seeding them is sound (adding entailed labels is monotone — cannot make a satisfiable
class unsat). Seeded subsumers propagate through `horn_fixpoint` and cascade determinism
into the downstream disjunctions, collapsing the model search.

## Results (depth 256, adaptive budget OFF, 60 s deadline)

**MRV ON (default):**

| class | none | real-subsumers seed | factor |
|---|---|---|---|
| SweetWine | 12 366 br, Sat | 2 584 br, Sat | 4.8× |
| **Zinfandel** | **1.02 M br / 60 s / Stalled (DNF)** | **42 947 br / 2.6 s / Sat** | **~21×, DNF→terminates** |

**MRV OFF (control — isolates seed from MRV-reorder):**

| class | none | real-subsumers seed | factor |
|---|---|---|---|
| SweetWine | 1.24 M / Stalled | **78 k / 3.8 s / Sat** | **16×, DNF→terminates** |
| Zinfandel | 1.11 M / Stalled | 982 k / 54 s / **Sat** | 1.1× br, **DNF→terminates** |

**Garbage control** (same count of named NON-subsumers): both classes → **Unsat / 0 br**
— arbitrary labels over-constrain to a trivial (wrong) Unsat; they never produce a
correct fast Sat. Confirms only the *real* subsumers give the correct collapse.

## Interpretation (the controls)

1. **Seed is the lever, MRV-independent.** SweetWine collapses 16× *with MRV off* — a
   pure seed effect. The collapse is not "more root labels reorder MRV."
2. **It's saturation knowledge, not label count.** Garbage (same count) gives wrong-Unsat,
   never a correct collapse. Only the genuinely-entailed saturator subsumers work.
3. **Seed + MRV synergize.** Zinfandel's headline 21×→2.6 s needs both: the seed alone
   (MRV off) flips DNF→Sat (terminates) but only 1.1× branches; MRV amplifies the seeded
   knowledge into the 21× collapse.
4. **Sound on every correct-verdict case** (Sat preserved). The only Unsats were the
   synthetic-ID and garbage-over-constraint artifacts — both excluded by design.

This reverses the prior "all-model saturation can't resolve genuine value choices →
dead" prediction: the entailed subsumers, propagated, *do* cascade-resolve the downstream
choices even though they don't literally "pick Red."

## Honest caveats (what GO does and does NOT mean)

- **Not yet Konclude's 1 ms.** Zinfandel is 2.6 s (seed+MRV) with the *current* saturator's
  9 named subsumers. Richer saturation (the SP1 increments → more named subsumers seeded;
  SweetWine seeded 4→4.8×, Zinfandel 9→21×, so closure richness plausibly compounds) is now
  **evidence-backed**, but whether the compound reaches 1 ms or plateaus at "seconds, not
  DNF" is unmeasured. Both beat DNF; only one is the stated goal.
- **Classify-scale FP=0/MISSED=0 is unproven.** The probe tested 2 satisfiable classes.
  The coupling at classify scale (per-pair `¬sup` injection × seeded labels across 137²
  pairs) is the real soundness gate — full wine `konclude_closure_diff` with seeding wired
  into the per-pair path, byte-identical (FP=0 **and** MISSED=0). Until that runs this is a
  validated *probe*, not a validated *mechanism*.

## Next step (the committed build, now with a validated mechanism)

**SP2 minimal wiring**, NOT spec-increment-3: seed the named saturated subsumers into the
**classify per-pair path** behind a flag; gate on full wine `konclude_closure_diff`
(FP=0/MISSED=0 byte-identical) + classify wall-time. On clear: the first real GO at scale,
and increment-3 gets a measured payoff target. If the closure-diff shows any FP/MISSED, it
cost a day, not a sub-project. Probe code: `seed_probe` + `tests/seed_probe_gate.rs` on
`feat/nominal-rearch-sp0` (commit 4a506f4).
