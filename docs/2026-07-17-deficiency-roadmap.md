# rustdl deficiency roadmap (2026-07-17)

A prioritized, evidence-backed plan to address rustdl's deficiencies. Every item is anchored
to a measured finding from the 2026-07-16 ORE sweep + Konclude head-to-head, the dense-SROIQ
investigation, or the Sequoia (DL 2019) reading. **The FP=0 soundness contract is non-negotiable
in every item**; each carries a soundness gate and a measure-first go/no-go so we don't repeat
this session's dead-ends.

## Strategic frame (read first — prioritization is a choice, not a default)

rustdl's established positioning (`[[embeddability-resource-track]]`) is that its **niche is the
EL/Horn fragment** (cold-start ~2 ms/5 MB, competitive-to-winning vs Konclude — the ORE sweep's
EL 1.9× / 16 %-wins confirms it), and that the **expressive SROIQ tail is a domain Konclude
already owns** ("prefilter/cert framings die to 'just run Konclude'"). The active research thread
(`[[paper-iswc-eswc-plan]]`) is **anytime / calibrated-incompleteness, not a speed claim**.

This roadmap is deliberately ordered by that frame:
- **Coverage and EL/Horn-aligned work is elevated** (Tier 1 D1; Tier 3 Phase A) — it strengthens
  the niche rustdl actually competes in.
- **Directly chasing the disjunctive SROIQ tail is demoted** (Tier 3 Phase B) — it re-enters a
  ceded domain, and (per the Sequoia reading) even a worst-case-optimal CB engine does **not**
  make the hardest disjunctive+`≤n` cases fast. It is framed as opt-in / research, gated on a
  measurement, **not** as "the core deficiency to close."

## What rustdl is NOT deficient at (don't "fix" these)

- **Curated-corpus soundness + completeness** — FP=0/MISSED=0 across the curated fixtures vs the
  Konclude∩HermiT oracle (galen closed 2026-07-12).
- **EL / Horn performance** — the ORE sweep shows EL near-parity with Konclude (reasoning-core
  median **1.9×**, rustdl wins 16 % of EL onts), and 80 % of the reasoner-reachable corpus
  classifies < 60 s with median 50 ms.
- **Datatypes** — rustdl has broad SROIQ(D) datatype support; Sequoia and several others do not.
  A genuine differentiator; leave it.

## Deficiency catalog (measured)

| # | deficiency | evidence | class |
|---|---|---|---|
| **D1** | Anonymous individuals unsupported | 446/1920 ORE onts (23 %) rejected at conversion (EL 0, DL 136, pure-DL 310) | coverage / front-end |
| **D2** | Expressive-tail intractability (disjunctive-head + `≤n` explosion) | 294 DNF (20 % of reached); 20 pilot onts DNF that Konclude does in ~228 ms; reasoning core median **7.2×** Konclude, EL 1.9× → pure-DL **9.6×**; Sequoia confirms same failure mode | perf / effective-completeness |
| **D3** | Nominal / wine wall | wine ~640× gap; global-nominal merge-fold defeats backjumping; hard for Sequoia too (7/777 Nom-fires, hard for all) | perf (nominal fragment) |
| **D4** | Big-file scale / RSS tail | DNF rate 6 %(<1 MB) → 90 %(>50 MB); multi-GB RSS from ancestor-only pair-blocking | perf / memory |
| **D5** | Parse front-end overhead | horned-owl parse median 3.7 ms = ~20 % of the easy-ont wall | constant factor |
| **D6** | Orchestrator / wedge overhead on 1–10 s onts | Sequoia reports the analogous context-management overhead; rustdl has label-cache-build + per-pair orchestration | constant factor |

## Ruled OUT this session — do NOT resurrect without new evidence

- Whole-model / snapshot / status caching, CDCL clause-learning (reuse-trap — **FP-unsound** on
  non-Horn; `[[snapshot-cache-fp-soundness-fix]]`, `[[next-big-bet-reuse-trap-nominal-termination]]`).
- ∃-marker body absorption (redundant with the shipped card-disjunct-atoms fix).
- `DepSet` widening alone (inert — 0 branch-count change).
- `solve_at_most` exhaustion-dep narrowing (room is a mirage in the stalls — 0.2 %).
- ⊥-locality module extraction (the hard subsumptions are not separately modularizable).
- `--pair-timeout-ms` as a *tail* fix (the DNF tail is **scale-bound, not per-pair-search-bound**
  — bounding recovered only 24/1920).

## Plan — three tiers

### Tier 1 — cheap, sound, high-value coverage: **D1 anonymous individuals**

**Goal:** parse + reason over the 23 % of ORE ontologies currently rejected. The single largest
coverage gap, front-end-scoped, and — because anonymous individuals are just existentially-bound
domain elements — expressible in machinery rustdl already has (Skolem/blank successors).

**Approach (sub-project, its own spec→plan):**
1. `convert.rs`: stop rejecting `AnonymousIndividual`; map each anonymous individual to a fresh
   internal Skolem constant / blank node, threaded through `ClassAssertion` / `ObjectPropertyAssertion`
   / `SameIndividual` / `DifferentIndividuals` the same way named individuals are.
2. ABox saturation + tableau already handle unnamed successors; the work is the conversion mapping
   + the reporting surface (anonymous individuals are not reportable class members).
3. Datatype/`HasKey` interaction: anonymous individuals in data assertions route through the
   existing DKey lowering.

**Soundness gate — the sharp edge is IDENTITY, not existence.** Anonymous individuals are not
plain existentials: under OWL's no-unique-name assumption an anonymous individual may be
*entailed equal to a named one* (or to another anonymous one), and it carries scope. Skolemising
to a fresh constant is the standard sound treatment, but the soundness-critical obligations are
(a) the fresh constant participates in `SameAs`/`≤n`-merge and `DifferentFrom` exactly as a named
individual would, and (b) it is never reported as a named class member. Gate: curated oracle net
FP=0 UNCHANGED **plus** a new anonymous-individual oracle fixture (HermiT/Konclude) exercising the
SameAs/DifferentFrom/`≤n` identity interactions the feature enables. Coverage measured by the
ERR1-count drop on the ORE sweep (target: 446 → near-0 for the anon-individual subset).

**Go/no-go:** none needed to start — it is coverage, and the identity semantics are well-understood
and testable. Bounded scope. **This is the recommended first move.**

### Tier 2 — moderate, known levers

**D4 — anywhere-blocking for the memory/scale tail.** `[[tableau-memory-fanout]]` already
diagnosed the multi-GB RSS as ancestor-only pair-blocking → huge completion graphs; the fix is
**anywhere-blocking** (block against any earlier node with a compatible label, not just
ancestors), which HermiT uses. **Soundness gate is the sharp edge**: loosening blocking under
inverses + `≤n` is exactly where a false `Sat` (→ false "not-subsumed" → MISS, or worse a false
model) hides — the block condition must carry the double-blocking inverse/cardinality guards.
**Measure-first go/no-go:** on the >50 MB DNF onts, does anywhere-blocking bound the graph (RSS +
completion) enough to finish within the cap? Prototype behind a flag; gate on FP=0 curated
byte-identity + the RSS/DNF delta on the big-file tail. If it doesn't bound them → the tail is
generative-depth, not blocking-scope; stop.

**D2b — two-phase Horn-first saturation in the wedge.** rustdl already has
`RUSTDL_HORN_SHORTCIRCUIT` as a *dispatch* gate; Sequoia (`[[sequoia-cb-sroiq-paper]]`) does it
*inside* saturation — derive Horn context clauses to fixpoint first (they subsume many
disjunctive clauses), then the non-Horn phase — which cuts many-disjunct-head generation. Probe
whether an in-search Horn-first ordering trims the wedge's disjunctive-head generation on the
disjunctive DNF onts. **Measure-first go/no-go:** prototype behind a flag; gate on the
branch-count delta on `KetoneGroup ⊑ AcylGroup` / `PathOfLength4` AND FP=0/MISSED=0 curated. If it
doesn't drop, fold into D2a.

> Note: Sequoia's other headline trick — the **second-maximal-atom ordering** (2ⁿ → ~n³ on
> disjunctive-head derivation) — is defined over CB *context clauses* with a maximal-literal
> ordering; it presupposes the calculus structure D2a Phase B would build. It is **not** a
> standalone wedge probe (category error) — it belongs inside D2a, adopted from the outset there.

### Tier 3 — consequence-based engine extension (Phase A aligned; Phase B opt-in/research)

rustdl's expressive-tail weakness is structural: the tableau/wedge builds models and re-explores,
and every cheap lever to make that tractable is a soundness NO-GO (reuse-trap) or inert. rustdl
already *has* a consequence-based engine — `owl-dl-saturation` — but only for **EL**. Sequoia
(`[[sequoia-cb-sroiq-paper]]`) shows CB extends to full SROIQ, is worst-case optimal, one-pass,
and — critically — **sidesteps the model-construction / search-reuse trap** (it derives
consequences instead of caching models). That makes CB-extension the theoretically-grounded
direction. Two phases, deliberately split by strategic alignment:

- **Phase A — Horn-SROIQ closure (ALIGNED — this extends the EL/Horn niche; the recommended
  Tier-3 increment).** Extend the saturation/Horn-fixpoint to the full Horn-SROIQ consequence
  closure (Kazakov IJCAI 2009; Ortiz et al. KR 2010). rustdl already fires functional/`≤1` merges
  in `horn_fixpoint`; this generalizes it to the complete Horn-SROIQ rule set (∀-propagation, role
  chains, inverse-aware). **Sound-by-construction** (Horn = deterministic, no don't-know
  nondeterminism). Moves the Horn portion of the DNF tail off the tableau onto a complete,
  terminating CB closure — directly strengthening rustdl's actual niche.

- **Phase B — disjunctive contexts (ALCH → ALCHIQ⁺ → ALCHOIQ⁺) (OPT-IN / RESEARCH — chases the
  ceded SROIQ domain; do not treat as "the core deficiency").** Adopt the context-structure
  calculus (Simancík et al. AIJ 2014; Bate et al. JAIR 2018; Tena Cucala et al. IJCAI 2018 — the
  Sequoia calculus): contexts with cores, context clauses, `Hyper`/`Pred`/paramodulation rules,
  incorporating the **second-maximal-atom** (2ⁿ → ~n³) and **two-phase Horn** optimizations from
  the outset.
  **Honest ceiling (per the Sequoia reading — do not overclaim):** CB is worst-case *optimal*,
  which is **not** the same as fast. Sequoia's *own* timeouts are on exactly the hardest
  non-Horn + at-most cases (exponential clauses with quadratically many head-equalities). So
  Phase B would classify **more** of the disjunctive tail than the tableau, give one-pass
  classification, and avoid the reuse-trap — but it does **not** "close" the frontier; the hardest
  disjunctive+`≤n` instances stay exponential for CB too. This is a research-grade re-architecture
  whose payoff is bounded by that ceiling, in a domain rustdl has positioned away from — hence
  opt-in/research, entered only if the tier-gate below justifies it.

**TIER-3 GATE (measure first — split rustdl's own 294 DNF onts by fragment):** how much of the
DNF tail is Horn-ish (`disjunctive-clause count = 0` → Phase A helps, sound-by-construction) vs
genuinely disjunctive (Phase B territory, stays hard even for Sequoia)?

**RESULT (clause-stats over the 294 DNF onts, 2026-07-17):** 275 analyzable (19 clausify-timeout,
mean 12 MB — the big-file scale tail, orthogonal to fragment). Of the analyzable:
- **Horn-ish (0 disjunctive clauses): 150 (55 %)** — and these are Horn-*but-not-EL* (∀ / `≤n` /
  inverse), so they miss rustdl's current EL-only `HORN_SHORTCIRCUIT` fast path and DNF on the
  per-pair hybrid tableau. A complete **Horn-SROIQ CB closure (Phase A) would classify them
  one-pass, sound-by-construction** — this is the majority of the algorithmically-hard tail.
- **Disjunctive (>0): 125 (45 %, mean 607 disjunctive clauses)** — Phase B territory, ceiling-bound.

**Gate verdict:** the DNF tail is **majority Horn-ish (55 %)** → **Phase A is high-value,
well-aligned, and sound-by-construction — proceed** (it is the real content of the "expressive
tail" deficiency, and it strengthens the EL/Horn niche rather than chasing SROIQ). **Phase B stays
opt-in/research** — it addresses only the 45 % disjunctive slice, bounded by the worst-case-optimal
≠ fast ceiling, in a ceded domain; build it only if Phase A lands and a concrete workload demands
the disjunctive slice. The big-file scale/RSS tail (the 19 clausify-timeouts + the >50 MB DNFs) is
**D4's** problem (anywhere-blocking / memory), not the CB engine's.

**Accept/defer:** D3 (nominals — hard even for Sequoia; fold into D2a Phase B's root-context
nominal handling, don't chase separately), D5/D6 (constant factors — revisit only if a profile
shows them dominating after Tier 1/2).

## Recommended sequence

1. **D1 anonymous individuals** (coverage, sound, bounded) — the clear first move; recovers 23 %
   of ORE, identity-semantics gate is well-understood and testable.
2. **D2a Phase A — Horn-SROIQ CB closure** — **gate-validated as the real content of the
   expressive-tail deficiency** (55 % of the DNF tail is Horn-but-not-EL, one-pass-classifiable,
   sound-by-construction, and it strengthens the EL/Horn niche). The strategic centrepiece, entered
   incrementally and phase-gated (each rule-set extension: FP=0/MISSED=0 curated + DNF-recovery
   delta on the 150 Horn-ish DNF onts).
3. **D4 anywhere-blocking** (measure-first) — addresses the big-file scale/RSS tail (the >50 MB
   DNFs + the 19 clausify-timeouts), orthogonal to the CB work; flag-gated, hard FP + RSS/DNF gate.
   **D2b two-phase-Horn wedge probe** — cheap, opportunistic; only if it shows a branch-count win.
4. **D2a Phase B (disjunctive CB)** — **opt-in/research only.** Bounded by the worst-case-optimal ≠
   fast ceiling, addresses only the 45 % disjunctive slice in a ceded domain. Build only if Phase A
   lands and a concrete workload demands it — otherwise the aligned investment is the
   anytime/calibrated-incompleteness research thread (`[[paper-iswc-eswc-plan]]`), not this.

Each item becomes its own `brainstorming → spec → writing-plans` cycle; this roadmap is the
decomposition and the prioritisation, not an implementation plan.

## Execution status (2026-07-17)

- **D1 anonymous individuals — SHIPPED** (merged onto `feat/hard-antecedent-surrogate-absorption`,
  `7626c9d..3ae6588`). 23 % of ORE (446 onts, 100 % anon-reject) now readable; FP=0; final review
  caught + fixed a `realize()` output-leak. Real coverage win.
- **Phase A inc 1 — disjointness — SHIPPED as a FOUNDATION** (`a97cca0..787aa2b`). `DisjointClasses`
  admitted to the saturator complete fragment under the no-functional gate (allowlist change, reuses
  the existing `DisjointnessClash` + `process_unsat` back-prop). Sound/complete: reasoner 59/0,
  20/20 real ORE disjoint onts byte-identical fast-vs-hybrid, by-construction on giants; FP=0. Final
  review caught + fixed the `DisjointUnion` unsound-completeness gap (its disjunctive covering is
  out-of-fragment → `DisjointClasses`-only). **Standalone DNF-recovery ~0** (31/39 disjoint onts also
  need symmetric) — the foundation half of {disjoint, symmetric}.
- **Phase A inc 2 — symmetric — NOT BUILT (cheap gate proven unsound; the real fix is engine work).**
  A "symmetric is inert when used only in forward `∃`" gate was designed and **refuted by a
  4-axiom counterexample** (advisor + confirmed by rustdl `explain`):
  ```
  X ⊑ ∃R.C ;  ∃R.X ⊑ D ;  ∃R.D ⊑ E ;  Symmetric(R)   ⟹  X ⊑ E
  ```
  The forward-only saturator never builds the symmetric back-edge `c→x`, never fires the antecedent
  trigger `∃R.X ⊑ D`, and **misses `X ⊑ E`** — every role forward-`∃`, no ∀/range/card/inverse/chain.
  So a symmetric role is *inert* only when **write-only** (never an antecedent `∃R` trigger, no
  domain) — but a never-triggered role produces no subsumptions anyway, so the sound-inert case is
  the classification-irrelevant case. A write-only-only gate *might* be sound (an approximate grep
  hints the 31 {disjoint,symmetric} onts may be write-only), but it is **FP-critical with no
  empirical safety net** — the target onts are giants (58k–981k classes) that DNF on the hybrid path
  and have no oracle at that scale, so an unsound admission would be undetectable. **The real fix is
  backward / inverse propagation in the saturator (the CB "Pred rule"; Sequoia / Bate et al. JAIR
  2018) — a research-grade engine extension, entered deliberately, not built this session.** Until
  then the {disjoint, symmetric} giant-Horn tail (~29 onts) stays on the hybrid path (DNF).

**Session takeaway:** the aligned, safe wins (anon coverage; disjointness foundation) are banked;
Phase A's next real step is the backward-propagation engine, which is where lifting EL-CB toward
SRIQ-CB genuinely gets hard (Sequoia confirms even a mature CB reasoner has the same expressive
tail — `[[sequoia-cb-sroiq-paper]]`).
