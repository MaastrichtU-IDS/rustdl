# CB blowup vs Konclude — investigation verdict (2026-06-16)

**Question:** rustdl's consequence-based (CB) engines (unordered B1/B2 + ordered
"sequoia" S1) hang on ~4–7 of 243 random in-fragment ALCH ontologies; rustdl's own
hybrid/tableau does them in <2.1 s. Is this a fixable CB deficiency (e.g. eager
`⊥`-pruning) or fundamental? Benchmarked native Konclude + studied its source.

## Calibration — the failing seeds are genuinely trivial
Konclude (docker `konclude/konclude:latest`) classifies all 7 timeout seeds in
**1–26 ms of reasoning** (≤2 s wall incl. startup). rustdl-hybrid: all 7 <2.1 s
(mostly fixed overhead). Both CB engines: HANG (>30 s; one barely finishes ~28 s).
Verdicts agree (seed 82 unsat = {K0,K1,K2,K3,K6} across Konclude/hybrid/CB). On 30
*other* ALCH fuzz seeds the CB engine is fast (sub-100 ms) — so the blowup is
**workload-specific (∀-rich + disjunctive), not CB-across-the-board.**

## Root cause (CB) — combinatorial cross-product in hyperresolution
`engine.rs::apply_hyper` builds `∏ᵢ |supports(pᵢ)|` over an ontology clause's premise
atoms, concatenating disjunctive residuals, recomputed each call. As disjunctive
clauses accumulate (fed by the `∀`-rule's augmented successors back-propagating
disjunctions), the cross-product explodes **inside a single `process(v)` call**. The
redundancy gate can't help: the clauses are an **antichain of incomparable
disjunctions** (`{∃R.A⊔B}`,`{∃R.A⊔C}`,…). The ordered "sequoia" engine prunes ~20 %
via eligibility but does not change the asymptotics.

## Verdict — FUNDAMENTAL, no cheap fix
- **Not unsat-count-driven** (so eager-`⊥`-pruning would not fix it): seed 9 has ONE
  unsat class yet CB takes 3–4 s. Cost tracks the disjunctive antichain, not `⊥`.
- The hang is **inside one `apply_hyper` call**, so a between-calls dead-context gate
  cannot interrupt it.
- CB's design is to **not branch** — it materializes the full disjunctive consequence
  closure as clauses. Konclude's speed comes from **clash → immediate branch unwind**
  (`CCalculationTableauCompletionTaskHandleAlgorithm.cpp` 1342–1348) + dependency
  backjumping + saturation **common-disjunct extraction**
  (`CSaturationCommonDisjunctConceptsExtractor.cpp`) — a *tableau* (one-model,
  prune-on-clash) mechanism with **no clean CB analog**.
- The one transferable idea — common-disjunct concept extraction (saturate each
  disjunct, propagate shared consequences) — rustdl already ships in baby form
  (`owl-dl-core/src/disjunction_existential.rs`). Generalizing it only helps when
  disjuncts share consequences; it does not rescue adversarial random ALCH.

## Consequence for the re-architecture
The ordered Sequoia re-architecture was undertaken to get **guaranteed termination**
where the unordered engine blows up. The evidence shows ordering does **not** deliver
that on ∀-rich disjunctive input (only ~20 %; the antichain is intrinsic) — and the
SAME cross-product mechanism drives the cardinality (`≥n`) blowup that originally
motivated it. Meanwhile S1's order-completeness fix solved a gap that *only existed
because ordering was introduced* (the unordered engine is directly complete).

**Recommendation: CONSOLIDATE.** CB-saturation (ordered or not) is structurally worse
than clash-driven tableau on unsat-heavy/∀-rich disjunction; the hybrid already
classifies these fast and soundly and the orchestrator already routes to it. The CB
engine's honest, durable role is the **EL/Horn + easy-ALCH accelerator + differential
oracle** (where it is fast and FP=0, Konclude-validated). Do NOT build S2/S3/S4 to
chase guaranteed SROIQ termination — it is a property CB-saturation cannot provide on
hard disjunctive input. Keep B1/B2/S1 committed as sound experiments + the documented
limitation. The opt-in inference-record (DL proofs, #184) remains worthwhile on the
CB engine's actual EL/easy-ALCH domain.
