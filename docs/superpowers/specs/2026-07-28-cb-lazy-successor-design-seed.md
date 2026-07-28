# CB engine SP-A v2 — lazy-successor / backward-propagation: design seed

**Date:** 2026-07-28
**Status:** DESIGN SEED (not a full spec) — hand-off for the next arc's brainstorming.
**Branch:** `feat/cb-alch-taming`. Supersedes the SP-A "taming as an eligibility/cap
tweak" framing (both candidates ruled out this session — see
`2026-07-28-cb-alch-second-maximal-taming-design.md` + ledger + memory
`cb-engine-pursuit-2026-07-28`).

## Why this seed exists

SP-A's empirical spike this session established: (1) the June CB-SROIQ NO-GO is
overturned — a fast+complete+sound taming exists (KM does `adversarial(13)` in 88 ms,
`C⊑⊥` correct; rustdl's B1 *and* S1 both hang >30 s); (2) neither quick candidate
(second-maximal eligibility / KM_SPLIT cap) is the mechanism; (3) the real mechanism,
reverse-engineered from KM's clausification, is **backward propagation with lazy
successors** — which is exactly the "fact-based engine needs Sequoia-style contexts"
architecture the repo scoped in `2026-07-17-saturator-backward-propagation-scoping.md`.
This seed captures that mechanism concretely so the next arc designs from it.

## The mechanism (reverse-engineered from KM, concrete)

KM's clausification of `adversarial(n)` (`C⊑∃R.⊤`, `⊤⊑∀R.(Aᵢ⊔Bᵢ)`, `Aᵢ,Bᵢ` pairwise
disjoint), verified from KM's own clause JSON:

- `∃R.⊤`  →  `Q(x) → R(x, f(x))`   (Skolem successor term `f(x)`; a definer `Q`)
- `∀R.(A⊔B)`  →  **forward** `Qₖ(x) ∧ R(x,y) → Qₘ(y)`  *and* **backward**
  `Qⱼ(y) ∧ R(x,y) → Qₖ(x)`   — role atom over a **universal `y` variable**, with
  definers `Q_*` standing for the union/filler concepts.
- disjointness `Aᵢ⊓Aⱼ⊑⊥`  →  `Aᵢ(t) ∧ Aⱼ(t) → ⊥`.

KM derives the ∃-owner's ⊥ by resolving the **backward** role-clauses over the *generic*
successor variable `y` together with the forward ∀-clauses and disjointness — deriving
that any R-successor of a `Q`-element is unsatisfiable, then back-propagating that to
`C⊑⊥` — **without ever materialising the Skolem successor context** (`succs=0`,
16 hyper-calls). The disjunctive clash needs only *two* of the `n` ∀-disjunctions
(any two picks are disjoint), so it is bounded, not 2ⁿ.

**rustdl `owl-dl-cb` B1 gap (the eager-successor baseline):** `apply_succ_and_forall`
(engine.rs ~442) **mints a concrete successor term/context** for `∃R.B` and materialises
its disjunctive core there; the ∀ propagates in; ⊥ is reflected **only after full 2ⁿ
case-exhaustion** (engine.rs doc lines 23-24, 786: "bare ⊥ reflected only when the
residual is empty"). No lazy/generic-successor reasoning; no early back-propagated ⊥.

## The design shape for the next arc (to be brainstormed, not assumed)

Evolve `owl-dl-cb` from eager-successor toward KM's lazy/backward form. Candidate design
axes (the fresh brainstorming picks among them, grounded in KM + the 2026-07-17 scoping):

1. **Backward role-clauses (the Pred rule).** Represent `∀R.C` as backward clauses
   `filler(y) ∧ R(x,y) → owner(x)` and resolve over the generic successor variable,
   so a successor's ⊥ back-propagates to the ∃-owner without full expansion. This is the
   CB **Pred rule** (Simančík et al.; Sequoia). It is the core addition.
2. **Lazy successors + context-merging/blocking** (the 2026-07-17 architectural finding):
   per-witness context synthetics with merge-when-same-core (the CB analogue of
   double-blocking) for termination. The termination crux.
3. **Eager ⊥ from a clashing subset:** derive a successor's ⊥ from two disjoint-forced
   disjunctions rather than the full product — complementary to (1), possibly a
   cheaper partial win to prototype first.
4. Sequoia's **second-maximal atom** + **two-phase (Horn-first) saturation**
   (`[[sequoia-cb-sroiq-paper]]`) as *additional* antichain levers layered on (1) — NOT
   substitutes (second-maximal alone under-tames, measured this session).

## Non-negotiable gate (unchanged)

- Tames `adversarial(13)` (crates/owl-dl-cb/tests/cb_blowup.rs; scratch OFN
  `adv_13.ofn`): terminates fast + reports `C⊑⊥`.
- **Completeness:** `tamed ≡ B1` (unordered, directly complete) on all B1-terminating
  onts (extend `cb_sequoia_diff.rs`).
- **FP=0** vs the Konclude∩HermiT oracle on the ALCH corpus. Crown jewel; a CB engine
  feeding classify is where KM's datatype FPs arose.

## Open questions for the brainstorming

- Vehicle: extend the resurrected `owl-dl-cb` (eager baseline) in place, vs a fresh
  lazy-successor engine reusing its normalize/model? (Leaning: extend in place —
  B1 stays the completeness oracle.)
- Does (3) eager-subset-⊥ alone tame the corpus tail, deferring the full lazy-successor
  (1)+(2)? Prototype (3) behind a flag first as the cheapest probe.
- Termination proof obligation for (2) (context-merging/blocking) — the hardest part;
  Sequoia's argument is the reference.

## Sequencing

This seed → next-session **brainstorming** (approaches among axes 1–4, pick + design) →
spec → plan → subagent-driven build behind a default-OFF flag → the gate → then SP-B (+Q),
SP-C (+nominals), SP-D (race). The resurrected `owl-dl-cb` + `adversarial(n)` + the
`B1`-differential are the ready-made scaffolding.
