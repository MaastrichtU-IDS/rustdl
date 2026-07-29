# CB engine SP-A v2 — lazy-successor / backward-propagation: design seed

**Date:** 2026-07-28
**Status:** **PARKED 2026-07-29** — read "§ Park record (2026-07-29)" at the end of this
document BEFORE acting on anything above. Several load-bearing claims in this seed were
measured and did not survive, and the design axis it proposes was reviewed and needs a
different formulation. The seed text below is retained unedited as the historical record.
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

---

## § Park record (2026-07-29)

The brainstorming this seed asked for was run. It produced measurements that falsify parts
of the seed, an adversarial design review, and a decision to **park the CB arc** and
redirect to a measured, FP-safe lever
(`2026-07-29-negation-to-bot-gci-and-conjunctive-unsat-design.md`). Nothing on
`feat/cb-alch-taming` is deleted; the 92 tests stay green and the branch remains a valid
resumption point. What follows is what a future arc needs to know.

### Corrections to this seed (measured 2026-07-29)

1. **KM's result reproduces, and the family is polynomial for KM.** With a correctly
   encoded ontology (prefixed names + `owl:Thing`), `km classify` reports `:C`
   unsatisfiable in **83 ms at n=13** and **0.18 s at n=40**. So "a fast CB taming exists"
   holds. Caveat: the `adv13.ofn` left in the session scratchpad uses bare `Thing`/`C`
   names and KM returns `consistent, no subsumptions, no unsatisfiable` on it — that file
   is not the one behind the 88 ms figure. Any future measurement must re-verify the
   encoding first.
2. **"B1 *and* S1 both hang >30 s at n=13" is false in release.** Release build:
   S1 n=13 = **6.4 s** (n=12 = 2.3 s, ~2.7×/step); **B1 is the worse engine** —
   n=12 = 19 s, n=13 = timeout >30 s. The committed baseline
   (`crates/owl-dl-cb/tests/cb_blowup.rs:64`, `N_BLOWUP = 13`, and commit `2679fab`'s
   "S1 hangs >30s") is a **debug-build** measurement and would fail in release. The
   exponential is real; the constants and the "both hang" framing were not.
   Consequence: the `tamed ≡ B1` differential oracle only reaches n ≈ 11.
3. **The seed's mechanism claim is unconfirmed.** Source reading of KM does not support
   "backward propagation over a generic successor variable, without materialising the
   Skolem successor context". What KM's code shows is definer-based clausification of the
   union (heads carry no literal disjunction), same-term concept literals mutually
   *incomparable* (Hyper fires on all maximal), and `branch_ordered` — the disjunct-count
   cap — **off by default**. A `Pred` rule exists but was not shown to be what tames this
   instance. Treat the seed's §"The mechanism" as a hypothesis, not a finding.
4. **rustdl's shipped hybrid already solves the whole family, faster than KM.**
   `rustdl classify` reports `unsat http://t/C` correctly in **16 ms at n=13** and
   **66 ms at n=40**. `adversarial(n)` is therefore a gate on the experimental CB crate,
   **not** a rustdl capability gap — and the June 2026 CB retirement's actual
   recommendation ("route hard disjunctive to the hybrid") is corroborated, not refuted,
   by the very experiment used to reopen it.

### Root cause of the CB blowup (this is the durable finding)

The 2ⁿ is entirely in the **context space**, not the clause space. `RUSTDL_CB_DEBUG=1`
context counts: n=9 → 531, n=10 → 1045, n=11 → 2071, n=12 → 4121, with ~25 clauses **per
context** at every n.

Mechanism: `normalize` turns `∀R.(Aᵢ⊔Bᵢ)` into `∀R.Qᵢ` + `Qᵢ ⊑ Aᵢ⊔Bᵢ` (a definer — the
same device as KM). `apply_succ_and_forall` then fires R∀ **pairwise over (each existing
outgoing edge × each ∀ literal)**, interning a context keyed by `core(u) ∪ {Qᵢ}`
(`engine.rs:497-513`; identical in `seq_engine.rs:462-497`). Augmented edges are
themselves iterated in later rounds, so the reachable cores are the full **powerset** of
`{Q₁…Qₙ}`, and each subset is a separately saturated context.

The reason the engine has no choice: `DerivedClause.premise` is **always empty**
("core held implicitly", `model.rs:86-91`), and `intern_context` seeds every core atom as
an unconditional unit `⊤ → a` (`engine.rs:91-114`). The core *is* the hypothesis set, so
a hypothesis can only be recorded in the context key — and putting hypotheses in the key
is what makes the key space exponential.

### Design review outcome (if the arc resumes)

The reviewed design was: Stage 1 = batch the *unconditional* ∀s into one successor core;
Stage 2 = move hypotheses into the clause body (`premise`) with a Pred rule. Verdict:

- **Stage 1 is sound** (no counterexample found; the invariant "every stored clause head
  at `v` is entailed by `⊓core(v)`" holds on every path) and **complete** (nothing needs a
  *minimal* core), but the justification "the only consumer of a successor context is
  ⊥-back-prop" is **false** — `apply_at_most:649`, `merge_terms:765` and
  `apply_succ_and_forall:502` all read successor cores. Needs two guards: restrict
  batching to `count == 1` Succ requests (else `≥n` mints duplicate witnesses), and either
  gate on ALCH (`max_roles.is_empty()`) or add ALCHQ `≤n`-interaction canaries.
- **Stage 1 kills one route, not the powerset.** The conditional-∀ arm still walks it.
  Gate ontology for anything beyond Stage 1 (`∀unc = ∅` throughout, so Stage 1 is a
  no-op, and `C ⊑ ⊥` for n ≥ 3):
  `⊤ ⊑ Pᵢ ⊔ ∀R.(Aᵢ⊔Bᵢ)` for i<n, `Pᵢ` pairwise disjoint, all `Aᵢ/Bᵢ` pairwise disjoint,
  `C ⊑ ∃R.⊤`.
- **Stage 2 as specified is incomplete and a regression.** Two structural holes: nothing
  discharges a hypothesis when the successor derives it itself (B1 gets that free *because*
  hypotheses are core atoms), and reflection drops the ∀-clauses' own premises. The
  premise-collapse bound is wrong — the transient goes n⁴ then n⁸, i.e. **worse than 2ⁿ at
  n=13** — so smallest-premise-first selection is required, not optional. Thirteen sites
  become premise-critical and all still compile; four turn a dropped premise into a false
  `C ⊑ ⊥`.
- **Preferred formulation ("Stage 1′"):** batch **all** ∀s (conditional included) into one
  successor core, and add a `used`-provenance field that **only back-prop may read**.
  Hypotheses stay in the core, so the `⊓core` invariant — and hence `classify.rs` read-off,
  `record_at_most`, `apply_at_most`, `merge_terms`, and `add_clause` subsumption — stays
  correct *by construction*. One FP surface instead of thirteen, and a provenance bug is a
  MISS rather than an FP. It also tames the conditional gate above in O(n²). Pair it with a
  provenance-size cap degrading to an `ALL` sentinel (MISS-biased, the
  `RUSTDL_PRECISE_CARD_DEPS` discipline).
- **Vehicle must be chosen explicitly:** `seq_model.rs::SeqClause` has no
  premise/provenance field, so any of this is **B1-only** as designed — S1 would stay
  exponential.

### Why parked rather than continued

Measured addressable market: of **289 ORE DNF ontologies, 8 (2.8 %) are inside
`owl-dl-cb`'s fragment**, and only `ore_ont_12012` has genuine ALC content; **SP-B (+Q)
adds zero**. The two ontologies where KM beats rustdl 70–310× (`ore_ont_9053`,
`ore_ont_10197`) are rejected by `normalize.rs` on three counts each (ABox, datatypes,
transitive/inverse), so the ALCH engine cannot load the cases that motivate it.
`ore_ont_10019` is out of gate too (5 `SymmetricObjectProperty` axioms). Meanwhile the
repo's own 80-ont ORE sample has rustdl solving 64/80 vs KM 59/80, ~4× faster at the
median, with less RSS and better completeness.

The 2026-07-17 backward-propagation NO-GO is therefore **not** overturned — its cost
analysis and its "no FP safety net on the target ontologies" finding transfer unchanged;
only the *symmetric-completeness* payoff scope was ever settled there.

### If the arc resumes, do this first

Wire a `cb-classify` debug entry point and run the **existing** B1/S1 over the ~30
in-fragment slow/DNF ORE ontologies (DNF set: `12012, 10016, 10032, 2397, 9318, 15703,
6212, 3524`; plus the slow set `11906, 15108, 1734, 13723, 2232, 33, 16299, 6870, 7726,
7275, 14066, 11739` and the EL giants). GO criterion: byte-identical to the shipped
hierarchy wherever the shipped path completes, strictly faster on ≥ half, and completes at
least one ontology the shipped path DNFs. That is a measurement, not an engine project,
and it produces the market evidence the pursuit currently lacks — or closes it on evidence.

Two framing rules for any resumed spec: the FP posture must be **candidate enumerator /
race arm only, never trusted alone, never default-ON** (precedents: the
`RUSTDL_SNAPSHOT_CAPTURE` silent-FP incident; KM's own FPs on `ore_ont_9054`); and
payoff-vs-cost must remain an admissible stop — the SP-A spec's "the only honest stop is a
demonstrated genuine impossibility" is an unfalsifiability clause, and payoff-vs-cost is
what closed two of the last three engine arcs.
