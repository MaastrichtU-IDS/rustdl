# CB engine, SP-A: resurrect the ALCH consequence-based engine and add second-maximal-atom taming

**Date:** 2026-07-28
**Status:** Design — approved for implementation planning
**Branch:** `feat/cb-alch-taming` (off `main`)
**Fragment:** ALCH (first of a gated increment sequence — see Roadmap)

## Why this exists (the reconciliation)

The goal is a consequence-based (CB) reasoning engine as a **second general engine**
for rustdl's off-EL frontier — the theoretically-grounded fix for the DL-tail and
realize scale gaps (a per-pair-refutation weakness; a CB engine classifies in one
amortized saturation). KM (Kobayashi-MaRust) is a working existence proof: this
session measured its CB classify **70–310× faster than rustdl** on real ORE onts.

There is a recorded **prior NO-GO**: in June 2026 rustdl built and *retired* a CB
engine (`owl-dl-cb`, branch `feat/cb-b1-integration`: B1 unordered ALCH + B2 ALCHQ +
S1 ordered "sequoia" ALCH, all sound/FP=0/Konclude-validated). The retirement
(`docs/superpowers/specs/2026-06-16-cb-konclude-investigation-verdict.md`, on that
branch) found CB "materializes the disjunctive antichain" via an `∏ᵢ|supports(pᵢ)|`
cross-product and hangs (>30 s) on ∀-rich disjunctive input where the tableau is
<2.1 s — verdict "do NOT re-attempt as a SROIQ reasoner."

**Reconciliation (why we re-open, carefully):**
- The retirement was explicitly **workload-specific** ("not CB-across-the-board"),
  measured on **random-fuzz ALCH**, and — confirmed by reading the branch code — the
  retired S1 implements only **single-maximal-atom eligibility**. It has **no
  second-maximal-atom refinement**.
- The Sequoia note (2026-07-17, *after* the retirement) identifies that exact missing
  refinement — the **second-maximal-atom trick, ~2ⁿ→n³, completeness-preserving** —
  as the antichain-taming mechanism. KM's live engine carries an analogous
  `branch_ordered` disjunct-bounding guard and is measured fast on real onts.
- So the NO-GO is a verdict on a CB engine *lacking the key optimization*, on
  adversarial fuzz. It is plausibly overturnable. We do not assume it — we test it.

**Resurrection feasibility — measured 2026-07-28 (de-risked):** the retired B1+S1
(~4,700 LOC, single crate, only `owl-dl-core` dep) **builds against current
`owl-dl-core` with zero changes and all 92 of its tests pass**, despite 691 commits
of drift. "Port" is a no-op (un-stub the crate). The effort is the taming + the gate.

## Scope

**SP-A goal:** decide, cheaply and decisively, whether **second-maximal-atom taming
overturns the disjunctive-antichain blowup on ALCH** — the single result the whole
multi-sub-project pursuit hinges on.

**SP-A is NOT** "beat rustdl on the SROIQ ORE tail" — an ALCH-only engine cannot
classify that (nominal/cardinality-heavy) tail. That is the fruitful-end goal, reached
by later increments. SP-A's bar is the taming hypothesis on ALCH, at FP=0.

**Commitment (2026-07-28, user directive "go the whole way").** This is a commitment
to the *full* pursuit — the complete ALCHOIQ CB engine through the SP-D race
integration that closes the real tail — not a validation gate that may stop at ALCH.
The gated increment sequence (ALCH → +Q → +nominals → race) is the **construction
method**, not an off-ramp: it is the only way to reach ALCHOIQ while holding FP=0.
Two invariants survive the commitment: (1) **FP=0 is absolute at every increment** —
"the whole way" means the whole way to a *sound* engine, and shipping an unsound CB
arm is exactly KM's datatype-FP failure we must not inherit; a red FP gate is
stop-and-fix, never ship. (2) If a fragment's taming proves *genuinely impossible*
(e.g. KM itself also blows up on it), that is real evidence to surface, not to paper
over. Absent that, a per-increment gate that goes red means **escalate** (deeper
Sequoia refinements, port KM's mechanism directly) — not abandon.

## Architecture

Resurrect `crates/owl-dl-cb` (replace main's 162-LOC stub with the branch's B1+S1):

- `engine.rs` (B1) — **unordered** ALCH CB engine. Directly complete. Role in SP-A:
  the **completeness oracle** (its subsumptions are the ground truth the tamed engine
  must match on B1-terminating onts).
- `seq_engine.rs` + `seq_order.rs` (S1) — **ordered** ALCH CB engine with
  single-maximal eligibility (`order.eligible`, `Δᵢ ⊁ᵥ Aᵢ`). This is where the taming
  is added.
- `normalize.rs`, `model.rs`, `classify.rs` — normalization, clause/context model,
  subsumption read-off (one saturation → all subsumptions).
- Depends only on `owl-dl-core` (IR + convert + nnf). Default-OFF, not wired into the
  production orchestrator in SP-A (it is validated stand-alone first).

## The taming mechanism — an empirical choice among candidates

**Correction (2026-07-28, from reading both engines):** the taming is *not* a single
known trick. There are (at least) two distinct, established mechanisms, and rustdl's
retired S1 uses neither:

- **rustdl retired S1** (`seq_order.rs`): a **total** order (`atom_key` breaks ties by
  `ConceptId`, so exactly ONE ≻ᵥ-maximal head atom) + `eligible()` firing only on that
  single maximal atom. Most restrictive; this is what blows up (the antichain of
  incomparable disjunctions is materialised because nothing bounds derived×derived).
- **Candidate 1 — Sequoia "second-maximal atom"** (Bate et al. SRIQ-CB, JAIR 63, 2018;
  Tena Cucala DL 2019): permit inferences on the maximal **and** second-maximal atom;
  completeness-preserving, bounds derived clauses ~2ⁿ→n³. In rustdl's total-order frame
  this is a local relaxation of `eligible()` (allow ≤1 atom of `delta` to exceed `a`).
- **Candidate 2 — KM's *measured-working* mechanism** (`engine.rs`, `clause.rs`): a
  **partial** order with an antichain of maximal atoms (`max_head_mask`, fire on all
  maximal), **plus the `branch_ordered` disjunct-count cap** (`engine.rs:3442-3453`:
  suppress resolvents combining ≥2 multi-literal-head premises; completeness recovered
  by splitting a disjunctive premise to a unit). This is the taming behind KM's
  measured 70–310× — but note it is a *cap + splitting*, not "second-maximal", and its
  completeness depends on the accompanying splitting.

**SP-A determines the taming empirically (per advisor: "KM's behaviour adjudicates;
don't out-think the calculus").** Implement the candidate(s) behind a flag in
`seq_engine.rs`/`seq_order.rs` (default-off = current S1), and let the gate pick: the
winning taming is the one that (a) terminates the blowup baseline fast AND (b) keeps
`tamed-S1 ≡ B1` (completeness) AND (c) is FP=0. Start with the cheaper local relaxation
(Candidate 1) and escalate to KM's cap+splitting (Candidate 2) if it under-tames. The
references are KM's live code + the Bate/Sequoia completeness arguments; correctness is
established by the differential + oracle gate, not an a-priori proof here.

## Soundness & completeness validation

The retired engine is its own oracle:

- **Completeness — differential vs B1.** B1 (unordered) is directly complete. The
  taming changes only *which inferences fire* (eligibility), never the set of entailed
  consequences, so **tamed-S1 must derive exactly B1's subsumptions on every ontology
  where B1 terminates.** Harness: extend the existing `cb_sequoia_diff.rs` (already a
  B1-vs-S1 differential) to cover tamed-S1.
- **Soundness — FP=0 vs external oracle.** Every CB inference is sound by
  construction; gate FP=0 vs the **Konclude∩HermiT oracle** on ALCH-fragment onts. A
  CB engine feeding classify is exactly where KM's datatype FPs arose — so this gate
  is non-negotiable. (Datatypes are out of ALCH scope, removing that FP surface here.)

## The go/no-go gate (SP-A's deliverable)

1. **Baseline.** The 243-random-ALCH failing-seed harness was not committed;
   reconstruct an adversarial ∀-rich-disjunctive ALCH generator matching the
   characterized failing pattern, and confirm B1 / current-S1 blow up (>30 s) on it —
   clean, current-hardware, isolated (no `-P`/contention artifacts).
2. **Tame.** Add second-maximal eligibility; the same adversarial onts terminate fast.
3. **Gate — all three required to declare GO:**
   - **Taming:** the blowup onts terminate in CB-fast time (seconds, not >30 s).
   - **Completeness:** `tamed-S1 ≡ B1` on all B1-terminating onts (differential).
   - **Soundness/value:** FP=0 vs Konclude oracle on ALCH-fragment real onts, and
     tamed-CB is competitive-or-faster than the tableau there.
4. **Verdict.** GREEN → SP-B (+Q). RED → **escalate, don't abandon** (per the
   Commitment): a taming that under-tames or breaks completeness routes to deeper
   Sequoia refinements / a direct port of KM's mechanism; an FP is stop-and-fix
   (never ship). The *only* honest stop is a demonstrated genuine impossibility (KM
   itself also blows up on the same input) — surfaced with evidence, not assumed.

## Testing

- `cb_sequoia_diff.rs` extended: `B1 ≡ tamed-S1` on the existing ALCH fixtures +
  new adversarial ∀-disjunctive fixtures.
- A new adversarial-blowup regression: the generator's failing pattern terminates
  under tamed-S1 within a wall bound (guards against silent asymptotic regression).
- Konclude-oracle FP=0 gate over the ALCH-fragment corpus.
- The retired engine's existing 92 tests stay green (un-stub is behaviour-preserving
  for B1/S1 as-is).

## Risks & cautions

- **Don't over-invest analytically in whether the trick works** — KM's behaviour and
  the differential/oracle gate adjudicate it empirically. If the gate is red, that is
  the answer; do not keep re-deriving the calculus to force a GO.
- **FP=0 is the crown jewel.** Any false subsumption at the gate is an immediate
  stop-and-diagnose, not a tuning problem.
- **ALCH ≠ the SROIQ tail.** A green SP-A validates the *mechanism*; the *payoff*
  (beating rustdl on the real tail) is only demonstrable after SP-B/C add
  cardinality/nominals. SP-A must not be over-sold as closing the frontier.

## Roadmap (each its own spec → plan → FP=0 gate)

- **SP-A (this):** ALCH — resurrect + tame + validate the taming hypothesis.
- **SP-B:** +Q (qualified cardinality; the retired B2 reached ALCHQ — extend the
  taming; the `≤n`-equality blow-up is the next antichain frontier).
- **SP-C:** +nominals (ALCHOIQ; hardest — nominals clash with CB's local focus, hard
  even for Sequoia; this is the wine-wall fragment).
- **SP-D:** integration — wire the tamed CB engine as a **CB-preference race arm**
  with the tableau as the FP=0 backstop (the originally-requested race), and extend to
  the realize path.
