# M2 (DifferentIndividuals→NomKey disjoint) — gate results

**Status:** verdict (durable). The first concrete engine increment from the Konclude
representation study, built + gated. **Sound (FP=0) but inert-to-slightly-negative on wine →
NO-GO as the wall lever, per the pre-committed gate.** Default-OFF flag stays off; not shipped.

## What was built

`RUSTDL_NOMKEY_DIFF` (default OFF), commit `74ca39f` on `feat/nomkey-diff-disjoint`: feed
`Axiom::DifferentIndividuals` into the EL saturator's `disjoint_pairs` as
`DisjointClasses(NomKey(a), NomKey(b))` per distinct pair (lookup-only on the existing NomKey map).
3 negatives-first canaries green. Premise verified: wine asserts the needed distinctness
(`DifferentIndividuals(vin:Dry vin:OffDry vin:Sweet)`, `(vin:Delicate vin:Moderate vin:Strong)`).

## Gate results

1. **Wine soundness (FIRST) — PASS.** `konclude_closure_diff` wine, flag-ON, 1000 ms/pair:
   `rustdl_closure=653 konclude_closure=653 FP=0 MISSED=0 (unsat: 0)` — byte-identical. The
   increment is sound on wine.
2. **Wine wall (production cap `--pair-timeout-ms 1`, ∃-seed default-ON), flag-OFF vs ON:**
   - saturation subsumptions: **645 → 645 (unchanged)** — M2 derives NO new subsumptions.
   - timed-out pairs: **2716 → 2821 (worse)**; wall 3.02 s → 3.30 s.
   - M2 *is* firing (behavior changed) but unproductively: it adds `disjoint_pairs` that cost
     per-pair saturation/wedge time (pushing ~105 borderline pairs over the 1 ms cap) while
     producing zero new derivations.

Corpus FP sweep (step 2 of the gate) was **not run** — unnecessary once the wall step showed M2
inert (the gate says don't ship an inert default-on, so corpus validation for a GO is moot; wine
soundness already passed).

## Why M2 is inert on wine (the instructive finding)

The hypothesized cascade was: M2 unsat-combination → B2a forced-disjunct → value-choice determinism
→ ∃-seed enrichment. It breaks at the second arrow: **wine's value-disjunctions are
`∀hasColor.{Red,White,Rosé}` (ForallKey / ∀R.OneOf), not the atomic `C ⊑ D₁⊔…⊔Dₙ` disjunctions
that B2a consumes** (B2a is atomic-only, confirmed in the rustdl audit). So even where M2's
functional-merge-unsat fires, there is **no consumer** to convert the exclusion into deterministic
value resolution. M2 supplies an exclusion signal with nowhere to propagate.

## Implication — the gap is M1, not M2

M2's inertness sharpens the mechanism picture: the load-bearing wine determinism is the
**disjunction→deterministic-implication absorption** (Konclude's M1: `∀R.{v₁,…,vₙ}` value-choice
absorbed so that excluding n−1 values forces the last), **not** the cardinality-insufficiency side
(M2). In Konclude the two are complementary — M2-style cardinality+distinctness produces the
`¬vᵢ` exclusions that M1's absorbed implication consumes — but rustdl has no M1 analog for the
nominal-value `∀R.OneOf` disjunction, so M2 alone is a signal with no consumer.

This is genuinely new evidence and lowers confidence in the "rustdl is missing one isolated rule"
reading: the isolated, concrete candidate (M2) is inert. The remaining candidate (M1) is the more
**entangled** one (the study flagged it needs Konclude's trigger/priority machinery). Before
investing in the larger M1 build, the open question is whether Konclude actually resolves wine's 8
hard classes deterministically (case A — M1 worth building) or also branches on them (case B — the
dense wall, M1 won't help either). That is the Konclude-branch-count ground-truth the advisor
flagged as confirmation; M2's inertness makes it worth getting *before* the M1 build, not after.

## Disposition

- M2 NOT shipped (sound but inert/slightly-negative). Flag stays default-OFF. Branch
  `feat/nomkey-diff-disjoint` retained (unmerged) as the record; `main` pristine.
- Next decision: (a) build M1 (nominal-value disjunction absorption) — bigger, entangled; or
  (b) get Konclude branch-count on Gamay first to confirm case A vs B before the M1 investment.
