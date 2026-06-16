# CB engine re-architecture onto the Sequoia ordered calculus (ALCHOIQ) — DESIGN

**Date:** 2026-06-16
**Crate:** `crates/owl-dl-cb` (consequence-based classifier, default-OFF)
**Branch:** `feat/cb-b1-integration` stack
**Status:** DESIGN ONLY — implementation-ready spec, no code written.
**Scope:** Replace the current **unordered** B1/B2 CB engine
(`engine.rs`/`model.rs`) with the **Sequoia ordered calculus** (Tena-Cucala,
Cuenca-Grau, Horrocks; arXiv:1805.01396, AIJ 298:103518, 2021), which is sound,
complete, **and terminating** for **ALCHOIQ** (disjunction + role hierarchy +
inverse roles + qualified number restrictions + nominals).

**Why:** the current unordered host is FP-sound but does NOT reliably terminate
— 21/42 fuzzed cyclic-cardinality ALCHQ ontologies hang (`/tmp/cbfuzz/`). The
structural cause is `apply_hyper`'s `combos = ∏ supports`: a cyclic `≥n` makes a
context accumulate an undisambiguated set of positive disjunctions whose
cartesian product detonates. The Sequoia order + redundancy bounds the
per-context clause set and is the published fix.

---

## 0. THE CENTRAL CRUX — RESOLVED (with citations + primary-source verification)

This gates everything below. The deliverable is the *correct* reconciliation of
"Slice-0 proved ordering broke rustdl's read-off" with "Sequoia is an ordered
calculus that reads positive subsumptions completely." **The mechanism below was
extracted verbatim from the decompressed full-text PDFs (arXiv:1805.01396 and
1602.04498) — Theorem 1/2, the Hyper rule, Condition C2, the algorithm — not
reconstructed.** An earlier draft of this section misattributed Sequoia's
mechanism to SKH 2011's negative-context device; §0.5 documents that correction
so the distinction is not re-lost.

### 0.1 What EXACTLY is Sequoia's read-off — DIRECT, POSITIVE, per-query context

**Sequoia decides entailment of a *query clause*** (AIJ Def. 1):

> "A **query clause** has only atoms of the form `B(x)`, with `B ∈ O_B`. Given an
> ontology `O` and a query clause `Γ → Δ`, our calculus decides whether
> `O ⊨ Γ → Δ` holds."

Concept subsumption `A ⊑ B` is the query clause **`A(x) → B(x)`**. The decision
procedure (AIJ, Theorem 2 + the stated algorithm, verbatim):

> "To test whether `O` entails a query clause `Γ_Q → Δ_Q`: **Step 1** create an
> empty context structure `D`, fix an expansion strategy. **Step 2** introduce a
> context `q` into `D`, **set its core to `Γ_Q`**, and **initialise the order
> `≻_q` in a way that is consistent with Condition C2**. **Step 3** saturate `D`
> and check whether `Γ_Q → Δ_Q` is contained **up to redundancy** in `S_q`. Such
> an algorithm generalises to check in a single run a set of input query clauses
> by initialising in Step 2 a context `q` for each query clause."

**Theorem 2 (Completeness), verbatim:**

> "Let `D` be a context structure which is sound for `O` and such that no rule of
> Table 2 or Table 3 can be applied to it. Then, for each query clause
> `Γ_Q → Δ_Q` and each context `q ∈ V` such that all of the following hold, we
> have that `Γ_Q → Δ_Q ∈̂ S_q`:
> **C1.** `O ⊨ Γ_Q → Δ_Q`.
> **C2.** For each context atom `A ∈ Γ_Q` and each `A'` of the form `B(x)` such
> that `A ≻_q A'`, we have `A' ∈ Γ_Q`.
> **C3.** For each `A ∈ Γ_Q`, we have `Γ_Q → A ∈̂ S_q`."

So the read-off is **direct and positive, with NO negation, NO `A ⊓ ¬B`, NO
refutation**: create a context `q` cored at the query body `{A(x)}`, saturate,
and `A ⊑ B` holds **iff `A(x) → B(x) ∈̂ S_q`** (up to redundancy — a derived unit
`A → B`, or a clause subsuming it, or `A → ⊥`). This is what "they derive in a
single run all consequences … and hence are **not just refutationally complete**"
(AIJ §3) means: positives are read **directly**. The earlier-draft
negative-context stratum (SKH `R⁻_A`) is **NOT a Sequoia rule** — Sequoia's Table
2 is exactly {Core, Hyper, Eq, Ineq, Fact, Pred, Succ, Elim} (+ Nom/Join/r-Succ/
r-Pred for nominals), with **no negative-context introduction rule** (§1.3,
quoted from the decompressed rule table).

### 0.2 Condition C2 IS the mechanism — and it is what rustdl B1 violated

**C2 is the order constraint that makes the positive read-off complete, and it is
per-context (per-query).** For a classification query `A(x) → B(x)` the body is
the single atom `A(x)`, so C2 says:

> **the body atom `A(x)` must be `≻_q`-MINIMAL among all `B(x)`-form concept
> atoms** (nothing of the form `B(x)` may sit strictly `≻_q`-below `A(x)` unless
> it *is* `A(x)`).

This is the decisive fact the task brief was circling. The brief's hypothesis —
"a maximal consequence atom never surfaces as a unit under a single total order"
— is exactly right as a diagnosis, and **C2 is Sequoia's answer: the order is
chosen per-context so the queried body is minimal**, and the candidate-model
construction (Theorem 2 proof; the rewrite system `R*_t`, generative clauses,
Lemmas 7–13) then guarantees the entailed head surfaces in `S_q` up to
redundancy.

**Why this is NOT a single global order (and why classification stays one-pass).**
For classification you seed **one context per atomic class `A`** (core `{A(x)}`),
each with its own `≻_q` making `A` minimal — different contexts use different
orders (AIJ Def. 3 preamble: "each context can use a different order [though]
a-terms are compared the same way across contexts since `⋗` is global"; SKH
Remark 5: "different orderings can be used for different left-hand sides `H`").
All subsumers `B` of `A` are read off `S_{q_A}` in that single saturation. So
classification is **one context per class (n contexts), not one per pair (n²)** —
the standard CB classification design (SKH §5.2: "classifies the ontology in one
pass"). Contexts are find-or-create / core-interned and shared across the run.

**Why rustdl B1 broke (the precise, now-verified answer).** B1 added an order to
`apply_hyper` but **kept a single shared order and a context cored at `{A}` whose
order did NOT enforce C2** — i.e. B1 conflated the *literal/Hyper resolution
order* with the *read-off order* and used one global order across all root
contexts. Under any single global total order, the queried subsumer `B` for some
class `A` is `≻`-maximal (it cannot be minimal for every `A` simultaneously), C2
is violated for that `(A,B)`, and the entailed unit is never produced — exactly
the `disjunctive_subsumption_by_cases` MISS. **The fix is per-context orders
satisfying C2 (each root context cored at `A` makes `A` minimal), NOT abandoning
ordering and NOT a negative/refutational stratum.** Slice-0's empirical break is
fully explained: it tested ordering *without* the C2 per-context-order discipline.

**Hand-trace (the acceptance test), under a C2-respecting order.** Query
`A(x) → D(x)`, context `q` cored at `{A}`. C2 for body `A` forces `A ≺_q` every
other `B(x)` atom (`A` minimal). Take `≻_q`: `D ≻ C ≻ B ≻ A`. (Note: C2 only
pins `A` minimal; the order among `B,C,D` is free — and completeness must hold for
*every* C2-respecting order, by Theorem 2.) Hyper side-condition is `Δᵢ ⊁_q Aᵢ`
(resolved atom eligible in its clause):

```
Core:                         A → A
Hyper(A⊑B⊔C) on A→A:          A → B⊔C          (resolved A; ok)
Hyper(C⊑D) on A→B⊔C, C resolved, needs {B} ⊁ C i.e. C⪰B ✓:   A → B⊔D
Hyper(B⊑D) on A→B⊔C, B resolved, needs {C} ⊁ B i.e. B⪰C — under C≻B, FALSE.
```

Under `D≻C≻B` the syntactic Hyper saturation alone halts at
`{A→A, A→B⊔C, A→B⊔D}` — `A→D` not a syntactic unit. **Theorem 2 nonetheless
guarantees `A→D ∈̂ S_q`** because completeness is proved via the candidate-model /
rewrite-system construction, not by syntactic unit-derivation under that
particular order: the saturated set, processed by the productive-clause model
(generative clauses generate their `≻`-maximal head literal), is shown to entail
the query up to redundancy. The operational consequence for the implementation:
**`∈̂` is "contained up to redundancy" — the read-off must check clause
*subsumption/redundancy* (Def. 4), not only literal identity, against `S_q`.** A
C2-respecting order under which the *direct* unit `A→D` is produced exists (e.g.
`B≻C≻D≻A` resolves `B⊑D` and `C⊑D` into `A→D⊔D = A→D`); but completeness does not
*require* picking that order — any C2 order yields `A→D ∈̂ S_q` by Theorem 2.

> **Implementation note (FP-soundness-critical and completeness-critical).** The
> `∈̂` (up-to-redundancy) read-off is subtler than B1's literal-unit check. Two
> safe options, in increasing fidelity: **(a)** pick, per root context `A`, a C2
> order that *also* makes the candidate units directly derivable (the EL/Horn and
> told-subsumer subsumers always surface as literal units; the residual
> maximal-disjunct ones are the rare case), and verify completeness empirically
> via the differential gate; **(b)** implement the `∈̂` check as Def-4 redundancy
> (a derived clause `Γ' → Δ'` with `Γ' ⊆ {A}` and `Δ' ⊆ {B}` witnesses `A→B`).
> The S1 crux gate (§6) is precisely *"`A⊑B⊔C,B⊑D,C⊑D ⊢ A⊑D` under an adversarial
> C2-respecting order"* — if (a) is chosen, the gate must run the adversarial
> order to prove the chosen-order heuristic is actually complete, else fall to
> (b). **This is the single highest-risk implementation point; opus-review it.**

### 0.3 Is goal-directed refutation required? — NO. Direct positive, one-pass.

This is the precise answer to crux question 3, now corrected:

- **Sequoia reads positives directly** (Theorem 2), one context per class, all
  subsumers off one saturation. **No per-pair `A ⊓ ¬B ⊑ ⊥` refutation, no
  negative contexts.** The user's "goal-directed" choice is therefore **NOT what
  Sequoia does, and the direct read-off is better** (no O(n²) refutation) — which
  the task explicitly says to recommend if true. **Recommend: build the direct
  positive read-off with per-context C2 orders.**
- The *cost* that replaces per-pair refutation is **per-context order
  initialisation (C2)** + the **`∈̂` redundancy read-off** (§0.2 note). That is
  the entire delta from B1's broken read-off.
- (Historical: SKH 2011 *did* use a negative-context device `A ⊓ ¬B ⊑ ⊥` —
  refutational, lazy, shared. Sequoia supersedes it with the direct query-clause
  read-off + C2. We follow **Sequoia**, not SKH, because it is the ALCHOIQ
  calculus and avoids the negative-context population entirely. §0.5.)

### 0.4 Why it terminates (kills the `combos = ∏` detonation)

Three bounded, monotone populations, each a *published* Sequoia bound
(AIJ Prop. 1/2 + Theorem 4 / the type-bound argument):

1. **Contexts** — find-or-create by `core`; cores are conjunctions over the finite
   `Su(O)` vocabulary ⟹ finitely many *if the expansion strategy introduces
   finitely many contexts* (Prop. 1; the cautious/eager strategies of §3.1 do).
2. **Clauses per context** — the **ordered-resolution + redundancy (Def. 4)**
   bound: Hyper resolves only on the `≻_q`-eligible (`Δᵢ ⊁_q Aᵢ`) premise atom, so
   a context's clause set is the bounded ordered saturation (≤ exponential in the
   signature per context), NOT the `2^|V|` powerset. **This is exactly what the
   unordered host lacks:** unordered `apply_hyper` builds `combos = ∏ supports`
   (arbitrary-union resolvents); ordered Hyper resolves the single eligible match.
3. **Successor/function/nominal terms** — bounded by `strat` (Def. 8) + the
   `Su`/`Pr` triggers; the global nominal-path order `⋗` (Def. 3 constraint)
   caps nominal-label length ⟹ no infinite nominal regress (B4).

The cyclic-`≥n` detonation dies at (2): ordered Hyper never forms the `∏`-product
of disjunctive supports. **The 21 hanging fuzz ontologies (`/tmp/cbfuzz/`) are the
S2 termination gate.** (Worst-case ALCHOIQ is 3-NExpTime per the paper — but
worst-case-*optimal* ExpTime for every proper sub-fragment ALCHIQ/ALCHOQ/ALCHOI/
Horn and PTime for ELHO; "pay-as-you-go", AIJ Prop. 2.)

### 0.45 CORRECTION (independent review, 2026-06-16): "any C2 order" is WRONG — the order construction is load-bearing

§0.2's claim that **completeness holds for every C2-respecting order** is FALSE, and
the §0.2 hand-trace proves it against this doc's own `∈̂` (Def. 4, §1.2). Counter:
query `A(x)→D(x)`, ontology `{A⊑B⊔C, B⊑D, C⊑D}`, order **`D≻C≻B≻A`** (A minimal ⟹
C2 holds; Def-3 leaves `B,C,D` mutual order free, so this IS a valid C2 order).
Saturation (only Core/Hyper/Elim apply — no Eq/Pred/Succ):
```
Core:                    A→A
Hyper(A⊑B⊔C) on A→A:     A→B⊔C
Hyper(C⊑D) on A→B⊔C:     A→B⊔D     (C eligible: C≻B ✓)
Hyper(B⊑D): B never eligible (D≻B and C≻B) ⟹ blocked everywhere.
```
Saturated `S_q = {A→A, A→B⊔C, A→B⊔D}`. For `A→D ∈̂ S_q` (Def. 4) we need a clause
with head `⊆{D}` — NONE exists (`{B,C}⊄{D}`, `{B,D}⊄{D}`). So **`A→D ∉̂ S_q`: the
read-off MISSES `A⊑D` under this C2 order.** "Theorem 2 nonetheless guarantees it"
(§0.2) is the error — `∈̂ S_q` is the *syntactic* Def-4 containment in the saturated
set; the candidate-model construction is how Theorem 2 is *proved*, not an alternate
read-off, and ordered resolution simply does not produce `A→D` under `D≻C≻B`.

**The real requirement (and what the design did NOT extract):** completeness needs
the **specific Sequoia order CONSTRUCTION (Appendix A)**, not "any order satisfying
Def-3 + C2." The decisive extra property is that the order must let derivations
*flow to the subsumers* — for the chain `B⊑D`, `B` must be `≻`-above `D` so `B` is
eligible and `A→D` is actually derived. A **subsumer-respecting order** (told `X⊑Y`
⟹ `X≻Y`; query atom `A` minimal) makes the by-cases derive `A→D` directly (order
`B≻C≻D≻A`). So:

- **Implementation mandate:** build the per-context order via a principled
  subsumer-respecting construction (told-subsumer-depth: an atom ranks above its
  told-subsumers; query atom minimal), targeting Sequoia App. A. Do **NOT** use an
  arbitrary C2 order — it MISSES.
- **This is empirically gate-validated, not assumed.** The S1 make-or-break gate is
  the by-cases canary `A⊑B⊔C,B⊑D,C⊑D ⊢ A⊑D` PLUS full ALCH differential parity vs
  the sound+complete B1 engine. If the heuristic order achieves B1 parity on ALCH,
  it is empirically complete for ALCH; if not, extract Sequoia App. A verbatim. The
  read-off completeness reduces to "the order construction is right," validated by
  the differential gate — same epistemic status as B1, NOT a free inheritance of
  Theorem 2.
- §6's S1 stage is therefore the crux test: **if S1 cannot reach ALCH parity, the
  order-construction theory is the blocker and the re-architecture must pause for a
  verbatim Appendix-A extraction before S2+.**

### 0.46 GAP-FOUND (S1 adversarial validation, 2026-06-16): the heuristic order is INCOMPLETE

S1 built the subsumer-respecting heuristic order (told-subsumer-depth) and it
PASSED the by-cases canary + bibtex, but volume fuzzing (sequoia vs the
ALCH-complete B1) FALSIFIED ALCH parity: **5 MISSes / 243 in-fragment ALCH
ontologies, FP=0.** Minimal witness (`/tmp/seqfuzz/GAP/minimal-gap.ofn`):
`K1 ⊑ K3⊔K2 ; K3⊑⊥ ⟹ K1⊑K2` — B1 derives it, sequoia MISSES it when the dead
disjunct K3 ranks `≻`-below the live K2 (order-of-interning-dependent; ~half of
orderings mask it, which is why bibtex passed).

**Root cause** (`seq_order.rs::build`): depth is credited ONLY from told UNIT
clauses (`premise.len==1 ∧ head.len==1`). `K3⊑⊥` is `premise=[K3], head=[]` — a
non-unit, invisible to the depth builder ⟹ the order is NOT subsumer-respecting
for subsumptions arising from `⊥`-elimination OR disjunction-derived subsumers.
The eligible-atom Hyper then can't fire the `⊥` propagation. **This is the exact
§0.45 `D≻C≻B` blocking failure one step removed from told units.** Seed 166 MISSES
with ZERO unsat classes — a separate `∀`/back-prop manifestation — so the hole is
BROADER than the `⊥` case; a "credit ⊥-disjuncts" patch is whack-a-mole.

**Conclusion: the told-unit-depth heuristic is the wrong order construction.** A
correct order must credit ALL entailed subsumptions, not just told units — i.e.
the verbatim **Sequoia Appendix-A order construction** (which the design extracted
Theorem 2's *statement* + C2 but NOT the construction). S2+ is PAUSED per §0.45
until the order is correct. Open question for the next stage: whether Appendix-A's
order is even statically computable in rustdl's setting, or whether the
direct-positive read-off is fundamentally fragile and the engine should instead
keep B1's UNORDERED (directly-complete) resolution and solve termination a
different way (the tension this whole arc keeps surfacing). Artifacts:
`/tmp/seqfuzz/GAP/` (witnesses, VERIFY.sh, FINDINGS.md), `/tmp/seqfuzz/fuzz.py`.

### 0.47 RESOLVED via primary-source extraction (Appendix A) — the fix is a dead-maximal tier

Full extraction + spec: `docs/superpowers/specs/2026-06-16-cb-sequoia-order-extraction-and-spec.md`
(from the arXiv **LaTeX tarball** `proof-order.tex` — the Appendix-A construction ar5iv omits).
It CORRECTS two things in §0.45/§0.46 above:

1. **§0.45's C2 reading was INVERTED.** Theorem-2 C2 (verbatim) quantifies over the
   query **head** `Δ_Q`: for `A⊑B` it forces the **subsumer `B` ≻-MINIMAL**, not the
   body `A`. So §0.45's "counterexample" order `D≻C≻B≻A` (with `D` the head, ≻-maximal)
   is **NOT a C2 order** — it violates C2. Under a correct order the by-cases derives
   `A→D` directly. My §0.45 "any C2 order misses" is therefore wrong as stated; the real
   defect was narrower (below).
2. **The actual construction** is an **LPO induced by the a-term order `⋗`, with
   concept-symbol precedence ARBITRARY** — NOT "told-subsumer-depth," NOT "subsumer-
   respecting." For the ALCH read-off it reduces to a total concept-name precedence;
   the binding constraint is C2 (per-query) or the empirical three-tier order (per-class).

**The narrow, real defect (and the fix).** All 5 fuzz misses (§0.46) are ONE order gap,
NOT an engine gap (seed-166's "∀/back-prop" hypothesis is FALSIFIED — it's pure
propositional + disjointness). The hole: a **contextually-dead atom** `X` (where
`O⊨A⊓X⊑⊥`) must be `≻`-MAXIMAL so the empty-head clause (`{X}→{}` global-unsat, or
`{A,X}→{}` told-disjoint) is Hyper-eligible to resolve `X` OUT of a disjunction
`A→…⊔X⊔…`. The told-UNIT-depth heuristic gave dead atoms no rank ⟹ they could tie below
a live disjunct ⟹ MISS (interning-order-dependent). **Fix = three-tier per-context order:
dead-MAXIMAL (from empty-head clauses + told-disjoint-from-core) > live (subsumer-
respecting depth — my heuristic, correct for the unit-chain) > core-MINIMAL.** Three
traces (by-cases, minimal-gap, seed-166) confirm derivability (extraction §3.2–3.4).

**Regimes:** R2 = one context per class + the three-tier order = empirically complete
(B1-parity status, fuzz-validated — NOT Theorem-2-inherited, since no single head atom is
minimal). R1 = one context per query clause `(A,B)` with head `B` minimal (C2-exact),
keyed by `(core,head)` = literally Theorem-2-complete = the sound FALLBACK for any
residual R2 miss. The `∈̂` read-off stays syntactic (Def-4); a candidate-model read-off is
NOT the answer. Build R2, gate it on the fuzz suite, fall to R1 only on a measured residual.

### 0.5 Correction log (so the SKH-vs-Sequoia distinction is not re-lost)

An earlier draft of §0 claimed Sequoia restores completeness via a **negative-
literal stratum** (introduce `A ⊓ ¬B`, derive `⊥`, read `A⊑B` off it — SKH 2011's
`R⁻_A`). **That is wrong for Sequoia.** Verified against the decompressed rule
table: Sequoia's Table 2 has **no negative-context rule**; the read-off is the
**direct positive Theorem-2 query-clause check with the per-context C2 order**.
The SKH negative-context device is a *different, older* lineage (refutational,
ALCH-only). Consequences of the correction, propagated below:
- §1.3 lists exactly the eight Sequoia rules; there is **no "R⁻_A / §2.3" rule**.
- §2 data structures have **no `Neg` context kind**; instead they carry the
  **per-context order id** + the C2-init at root-context creation.
- §4 FP-soundness rests on **Sequoia's own Theorem 1 (per-rule soundness
  preservation)** — which DOES cover every rule we implement (no borrowed/uncovered
  rule). The order (incl. C2) gates *which* inferences fire ⟹ an order bug is
  MISS-biased, never FP (the rules are sound for any order; Theorem 1 is
  order-independent).
- §5/§6 "inherit Sequoia's published ALCHOIQ completeness" is now **literally
  true**: we implement Sequoia's rules + Sequoia's read-off, so Theorem 2 applies
  directly (the rustdl risk is implementation fidelity + the `∈̂` read-off, caught
  by the differential gate — same epistemic status as B1).

---

## 1. The calculus (precise rule set, ALCHOIQ)

Notation follows the Sequoia AIJ paper / Bate KR 2016. Contexts hold **DL-clauses
in many-sorted equational logic**: a clause `Γ → Δ` with body `Γ` a conjunction
of context atoms and head `Δ` a disjunction of context literals. **Context
a-terms**: `x` (central variable), `y` (predecessor variable), `f(x)` (function
successor, `f ∈ O_f`), `o` (nominal, `o ∈ O_o`). **Context p-terms** (atoms):
`B(x), B(y), B(f(x)), B(o)`, `S(x,y), S(y,x), S(x,x), S(x,f(x)), S(f(x),x),
S(x,o), S(o,x), S(o,o')`. A **(in)equality** is `s ≈ t` / `s ≉ t` between two
a-terms. An atom `B(t)` is written `B(t) ≈ true`.

**Context structure (Def. 5):** `D = ⟨V, E, S, core, ≻⟩`. `V` finite set of
contexts incl. **root context `v_r`**; `E ⊆ V × V × O_f` edges labelled by a
function symbol; `core_v` a conjunction of atoms over `Su(O)`; `S_v` a finite set
of context clauses; `≻_v` a per-context order (Def. 3).

**Triggers (Def. 2)** — these decide which atoms cross context boundaries
(load-bearing for completeness of Pred/Succ):
- `Su(O)` (successor triggers): smallest atom set s.t. for each ontology clause,
  if `B(x) ∈ Γ` then `B(y) ∈ Su`; if `S(x,zᵢ) ∈ Γ` then `S(x,y) ∈ Su`; etc.
- `Pr(O)` (predecessor triggers): `Pr = {A{x↦y,y↦x} | A ∈ Su} ∪ {B(y) | B ∈ O_B}
  ∪ {x≈y} ∪ {x≈o | o∈O_o} ∪ {y≈o | o∈O_o}`.

### 1.1 The context order (Definition 3)

Let `⋗` be a total order on the symbols of `O_f ∪ O_o` s.t. for every nominal
path `ρ = ρ'·ρ''`, `o_ρ ⋗ o_{ρ'}` (longer nominal paths dominate — *necessary for
both completeness and termination*, the one addition over prior CB orders). A
(root) context order `≻` w.r.t. `⋗` is a strict order on (root) context atoms s.t.:

1. `A ≻ x ≻ y ≻ true` for each context p-term `A ≠ true`;
2. `n ≻ m` for each `n,m ∈ O_o` with `n ⋗ m`;
3. `f(x) ≻ g(x)` for all `f,g ∈ O_f` with `f ⋗ g`;
4. `t[s₁]_p ≻ t[s₂]_p` for any context term `t`, position `p`, terms `s₁ ≻ s₂`;
5. `s ≻ s|_p` for each context term `s` and proper position `p` in `s`;
6. `A ⊁ s` for each atom `A ≈ true ∈ Pr (Pr_r)` and context term
   `s ∉ {x,y,true} ∪ O_o`.

**Each context picks its own `≻_v` extending these constraints; a-terms compare
identically across contexts (since `⋗` is global).** (Construction of a concrete
`≻_v` from a fixed `⋗`: Sequoia Appendix A.)

### 1.2 Redundancy / Elim (Definition 4)

A set `U` contains `Γ → Δ` *up to redundancy* (`Γ → Δ ∈̂ U`) iff:
1. `Δ` contains a tautology `t ≈ t` or both `{t≈s, t≉s}` for some a-terms, **or**
2. some `Γ' → Δ' ∈ U` with `Γ' ⊆ Γ` and `Δ' ⊆ Δ` (clause subsumption).

**Elim:** if `Γ → Δ ∈ S_v` and `Γ → Δ ∈̂ S_v ∖ {Γ → Δ}`, remove it. (Prop. 1
guarantees Elim is confluent — removing a redundant clause keeps the rest
redundant-closed.)

### 1.3 The inference rules (Table 2 + Table 3 of Sequoia)

Quoted shapes; `σ` is the context substitution (`σ(x)=x` for non-root, or
`σ(x)∈O_o` at the root). Each rule adds its conclusion to `S_v` (via Elim-gated
insert).

- **Core.** `A ∈ core_v ⟹ ⊤ → A ∈ S_v`. (Seeds the core as units.)

- **Hyper** (ordered hyperresolution — the disjunction engine). Premises:
  (1) an ontology clause `⋀ᵢ₌₁ⁿ Aᵢ → Δ ∈ O`;
  (2) for each `i`, a context clause `Γᵢ → Δᵢ ∨ Aᵢσ ∈ S_v` with **side-condition
  `Δᵢ ⊁_v Aᵢσ`** (resolved atom eligible/maximal).
  Conclusion: `⋀ᵢ Γᵢ → ⋁ᵢ Δᵢ ∨ Δσ`.
  *(This is the rule rustdl B1 deliberately ran UNORDERED. In Sequoia it is
  ORDERED via the side-condition; the per-context C2 read-off order — §0.2 — is
  what makes the ordered Hyper complete for the positive read-off.)*

- **Eq** (paramodulation). Premises:
  (1) `Γ₁ → Δ₁ ∨ s₁≈t₁ ∈ S_v` with `t₁ ⊁_v s₁`, `Δ₁ ⊁_v s₁≈t₁`;
  (2) `Γ₂ → Δ₂ ∨ s₂⋈t₂ ∈ S_v` (`⋈∈{≈,≉}`) with `t₂ ⊁_v s₂`, `Δ₂ ⊁_v s₂⋈t₂`,
  `s₂|_p` not a variable, and if `s₂|_p ∈ O_o` then `s₂` has no function symbols.
  Conclusion: `Γ₁ ∧ Γ₂ → Δ₁ ∨ Δ₂ ∨ s₂[t₁]_p ⋈ t₂`.

- **Ineq.** `Γ → Δ ∨ t≉t ∈ S_v ⟹ Γ → Δ`.

- **Fact** (equality factoring). `Γ → Δ ∨ s≈t ∨ s≈t' ∈ S_v`, with
  `Δ∪{s≈t} ⊁_v s≈t'` and `t' ⊁_v s` ⟹ `Γ → Δ ∨ t≉t' ∨ s≈t'`.

- **Succ** (create/reuse a successor context). From `Γ → Δ ∨ A ∈ S_u` with
  `Δ ⊁_u A` and `A` containing `f(x)`: call `strat(f, K₁, D)` to find-or-create
  the successor context `v` (core `core'`, order `≻'`), add edge `⟨u,v,f⟩`, and
  seed `A' → A'` in `S_v` for the carried triggers `A' ∈ K₂ ∖ core_v`.
  (`K₁ = {A' ∈ Su | ⊤ → A'σ ∈ S_u}`, `K₂ = {A' ∈ Su | Γ'→Δ'∨A' ∈ S_u, Δ' ⊁_u A'}`.)

- **Pred** (propagate consequences back to the predecessor). From a context
  clause at `v≠v_r` whose head literals are predicate-triggers `∈ Pr` and an edge
  `⟨u,v,f⟩` whose `S_u` supplies the body atoms (each with the maximality
  side-condition), derive the corresponding clause at `u` under
  `σ = {y↦x, x↦f(x)}`.

- **Nom** (nominal handling, root context). For an ontology clause with
  a-equality head literals and matching root-context support `Γᵢ → Δᵢ ∨ Aᵢσ ∈
  S_{v_r}` (`Δᵢ ⊁_{v_r} Aᵢσ`), derive the corresponding nominal clause in
  `S_{v_r}`. Companion root rules **r-Succ** / **r-Pred** / **Join** exchange
  ground (nominal-bearing) information between `v_r` and tree contexts.

- **Elim** (§1.2): redundancy deletion.

**That is the COMPLETE Sequoia rule set — there is NO negative-context /
refutational rule** (§0.5). The positive read-off (Theorem 2) handles the
disjunctive completeness via the per-context C2 order + the candidate-model
construction; negation enters only equationally (an ontology clause `A → ` /
`A → false` for a `⊥`-implying axiom resolves via Hyper to the empty clause —
that is how `A ⊑ ⊥` is detected, no special rule).

### 1.4 The read-off (classification)

Seed **one context `q_A` per atomic class `A`**, core `{A(x)}`, with `≻_{q_A}`
initialised so `A(x)` is `≻`-minimal among `B(x)` atoms (Condition C2 — §0.2).
Saturate once. Then, after saturation, for each candidate subsumer `B`:

- **`A ⊑ B`** iff `A(x) → B(x) ∈̂ S_{q_A}` — i.e. the clause `A → B` is contained
  **up to redundancy** (Def. 4) in `q_A`'s clause set: a derived literal unit
  `A → B`, or a derived clause subsuming it, or `A → false` (`A ⊑ ⊥` ⟹ `A`
  subsumes-everything / is unsatisfiable). **This is direct and positive; there is
  no `A ⊓ ¬B` context and no refutation step.**

The unified statement (AIJ Theorem 2): for the saturated structure, every entailed
query `A(x) → B(x)` satisfying C1–C3 is contained up to redundancy in `S_{q_A}`.
The implementation MUST check `∈̂` as Def-4 redundancy/subsumption, not literal
identity only (§0.2 implementation note — the highest-risk point).

---

## 2. Data structures (FRESH engine; reuse `normalize.rs` heavily)

**Recommendation: a FRESH engine module set, NOT a transform of `engine.rs`.**
The read-off (two-tier, refutational backstop), the per-context order, and the
ordered Hyper differ fundamentally from the current unordered positive-only host.
Bolting them onto `engine.rs` would mean rewriting `apply_hyper`, `add_clause`,
the read-off in `classify.rs`, and the term model simultaneously — higher risk
than a clean module with the unordered engine retained as a differential oracle
(§7). Concretely:

```
crates/owl-dl-cb/src/
  normalize.rs   REUSE — DL-clause normalization is shared. EXTEND: emit Sequoia
                 DL-clauses (central var x, function terms f(x), the DL4 ≤n/≥n
                 encoding, inverse-role atoms S(y,x), nominal atoms B(o)). The
                 ALCHOIQ fragment gate lives here.
  order.rs       NEW — the global symbol order ⋗ (Def 3 incl. nominal-path
                 constraint) + per-context ≻_v construction (Appendix A) + the
                 literal/multiset order extension + `eligible(lit, head, ≻_v)`
                 (the `Δ ⊁_v A` side-condition predicate).
  seq_model.rs   NEW — context structure: Context { core, clauses: OrderedClause
                 set, order: PerContextOrder, kind: Tree|Root, succ/pred edges },
                 the ContextGraph (find-or-create by core), the term/edge
                 representation. (Supersedes model.rs's Term/HeadLit; reuse the
                 ConceptId interning from owl-dl-core.) Root contexts for each
                 class A carry a C2-initialised order (A minimal). NO Neg kind.
  seq_engine.rs  NEW — the saturation loop + rules: core, hyper (ORDERED),
                 eq, ineq, fact, succ, pred, nom, join, r-succ, r-pred, elim.
  seq_classify.rs NEW — the two-tier read-off (§1.4).
  engine.rs / model.rs  KEEP as the unordered B1/B2 differential oracle (§7).
  lib.rs         dispatch: RUSTDL_CB_CALCULUS = unordered (default) | sequoia.
```

**Ordered clause representation.** A clause is `(body: Vec<Atom>, head:
Vec<Literal>)`, head kept **sorted by `≻_v`** so the maximal literal is `head
.last()` and the `Δ ⊁_v A` eligibility check is O(1)/O(log). `Atom` = an interned
`(Predicate, [a-term])`; `a-term` = `X | Y | F(fid) | O(oid)` (an enum, NOT a
`ConceptId`). `Literal` = `Atom | Eq(aterm,aterm) | Neq(aterm,aterm) | False`.
**Equalities/terms are a distinct literal kind.** Unlike the unordered B1/B2 host
(which had to keep `Eq`/`Neq` invisible to Hyper to avoid the term order leaking
into the concept read-off), Sequoia uses **one per-context order `≻_v` over all
literals** — this is sound and complete *because* the read-off is the Theorem-2
positive query check with C2, not the unordered direct read-off B2 had to
protect. The distinct literal kind remains useful for the Eq/Ineq/Fact rule
dispatch, but there is no stratification *constraint* to enforce here.

**Order representation.** `≻_v` is materialized as a precomputed rank map over the
finite per-context atom vocabulary (build once at context creation from `⋗` +
Def. 3). For a **root context `q_A`** the rank map is C2-initialised so `A(x)` is
minimal among `B(x)` atoms (§0.2). The multiset literal order is the standard
derived comparison.

**Context graph / interning.** Find-or-create by `core` (one root context per
class `A`; successor contexts shared by core via `strat`). There are **no
negative contexts** — the read-off is positive (§1.4). The one-pass,
n-contexts-not-n²-pairs property is the standard CB classification design (§0.2).

---

## 3. Fragment scope

**IN (Sequoia ALCHOIQ, this re-architecture's target):**
disjunction (`⊔`), full negation/`∀`, role hierarchy (`⊑` on roles), **inverse
roles**, **qualified number restrictions** (`≤n R.C` / `≥n R.C`, the DL4
encoding), **nominals** (`{o}`, `ObjectHasValue`, nominal cardinality). This is
**B2 + B3 + B4 in ONE engine** — the whole reason to adopt Sequoia rather than
layering B3/B4 onto the unordered host.

**OUT (route to `OutOfFragment` → existing hybrid):**
- **Role chains / transitivity / complex RBox** (`R∘S ⊑ T`, `Trans(R)`,
  reflexive/irreflexive/(a)symmetric role characteristics). This is **Bate et al.
  SRIQ** (KR 2016) — a *later* extension that pre-processes chains away; scope it
  as a follow-on once ALCHOIQ is solid. (SROIQ = ALCHOIQ + this RBox via the
  standard chain-elimination transform.)
- **`Self`** restrictions.
- **Datatypes / concrete domains** — already handled by the existing
  preprocessing channel (Phases D4–D11); leave there.

**Fragment gate (`normalize.rs`).** Accept the ALCHOIQ constructs; reject (→
`OutOfFragment`) role chains, transitivity, role characteristics beyond hierarchy,
`Self`, datatype ranges not already lowered. This *widens* the current gate
(which rejects `≤n`/inverse/nominal) — the gate becomes "reject only SR-RBox +
Self + un-lowered datatypes."

---

## 4. FP-soundness (FP=0 is SACRED)

**Published soundness (Sequoia Def. 7 + the soundness theorem).** A context
structure is *sound for `O`* if for every model `I` of `O` there is an
`N`-compatible conservative extension `J` (adding nominals `N`) satisfying every
derived clause `core_v ∧ Γ → Δ` and every edge implication `core_u →
core_v{x↦f(x),y↦x}`. The paper proves **every inference rule preserves
soundness** (the `if and only if` proof block: "applying an inference rule … to a
sound `D` … produces a sound context structure"; Lemma 1 + the per-rule cases
incl. `r-Pred`). **Consequence: every derived clause is a logical consequence of
`O` ⟹ every read-off subsumption holds in all models ⟹ FP=0 by construction.**

**rustdl-specific adaptation risks (the FP surface to guard):**
1. **Order construction (`order.rs`).** A `≻_v` that violates Def. 3 (e.g. nominal
   constraint, or property 6) can break *completeness* (MISS) but **not
   soundness** — the inference rules are sound for *any* order; the order only
   gates *which* inferences fire. So an order bug is MISS-biased. *Guard:* assert
   the Def. 3 properties at context-build (debug); the differential gate catches
   MISSes.
2. **The `∈̂` (up-to-redundancy) read-off (§0.2 / §1.4).** The read-off checks
   `A(x) → B(x) ∈̂ S_{q_A}` via Def-4 subsumption. *Risk:* a too-loose redundancy
   check could report `A ⊑ B` from a clause that does not actually subsume `A→B`
   (FP), or a too-strict one MISSes. *Guard:* implement `∈̂` exactly as Def. 4
   (`Γ' ⊆ {A}, Δ' ⊆ {B}` plus the tautology cases) — a clause witnesses `A→B`
   only if its body is `⊆ {A}` and head `⊆ {B}`. This is conservative toward FP=0
   (a strictly-larger-head clause does NOT witness). The differential gate catches
   any MISS. **This is the highest-risk FP point** (opus-review at S1).
3. **DL4 `≤n` encoding + Eq/Fact paramodulation.** Soundness is the standard
   superposition equality calculus on ground depth-1 terms (congruence). The B2
   design's `Eq/Neq` soundness analysis (its §4, §10.2 — "plain binary resolution
   on the complementary literal, valid in all models") carries over verbatim and
   is *strengthened* here because the order is now the published one. *Guard:* the
   B2 negatives-first canaries (`tier2_*`) port directly.
4. **Inverse-role back-edges (Pred).** The `S(y,x)` atoms + `Pred` rule are sound
   per Def. 7's edge implication. *Guard:* B3 canaries (§6).
5. **Nominal identity (Nom/Join/r-*).** The `⋗` nominal-path bound is
   soundness-neutral (it bounds *creation*, a termination device); nominal
   equality `x ≈ o` is sound classical equality. *Guard:* B4 canaries.

**The single most important FP invariant:** *the read-off only reports `A ⊑ B`
when a derived clause genuinely subsumes `A(x) → B(x)` under Def. 4 (body `⊆{A}`,
head `⊆{B}`), or when `A → false` is derived.* Soundness is then Theorem 1
(every derived clause is an `O`-consequence) + this conservative read-off; no
larger-head or different-body clause may be (mis)counted as a subsumer.

---

## 5. Soundness/completeness contract

- **Sound:** unconditional, by construction (§4) — FP=0 on every measured
  ontology, the sacred invariant. Holds for *any* `≻_v` (order bugs are MISS, not
  FP).
- **Complete + terminating for ALCHOIQ:** Sequoia's published guarantee
  (the correctness + complexity theorems; deterministic ExpTime for ALCHOIQ,
  worst-case-optimal for every proper sub-fragment). The rustdl *composition*
  risk is the same epistemic status as B1: the differential gate (§6) is the
  empirical proof.
- **Default OFF**, opt-in (`RUSTDL_CB_CALCULUS=sequoia`), comparison-only. The
  production hybrid is unchanged.

---

## 6. Build decomposition (staged, each gated)

Frozen-interface discipline: **Stage 0 freezes `order.rs` + `seq_model.rs` types
+ the `seq_engine` rule-trait surface**, then later stages fan out. Each stage is
gated by **(a) differential-vs-hybrid `identical:true`** on its fragment and **(b)
termination on the 21 divergent fuzz onts** (`/tmp/cbfuzz/`, the
cyclic-cardinality hangs) once cardinality lands.

| Stage | Fragment | Delivers | Primary gate |
|---|---|---|---|
| **S0** (serial) | — | `order.rs` (⋗, ≻_v, eligibility), `seq_model.rs` (ordered clause, context graph, term enum), the engine skeleton + worklist + Elim. | builds; clippy/fmt clean; order-property debug asserts. |
| **S1** | **ALCH** | Core + **ordered** Hyper + Succ + Pred(∀) + Eq/Ineq/Fact-free + **per-context C2-order root contexts + the `∈̂` positive read-off** (§0.2/§1.4). | **THE crux gate:** `A⊑B⊔C,B⊑D,C⊑D ⊢ A⊑D` passes under an ADVERSARIAL C2-respecting order (`D≻C≻B`, `A` minimal) via the `∈̂` read-off, plus all B1 ALCH canaries; `cb-diff` `identical:true` on **alehif** (ALC) + bibtex + the synthetic 15-class ALCH gate. **FP=0 hard.** |
| **S2** | **ALCHQ** | DL4 `≤n`/`≥n` lowering + Eq/Ineq/Fact paramodulation + the cardinality pigeonhole. | the B2 `tier2_*` canaries (port verbatim); `cb-diff identical` on a `≤n`-bearing stripped-ALCHQ subset (shoiq-knowledge / wine, inverse+nominal stripped); **termination on the 21 fuzz onts** (the headline win — must converge, not hang). FP=0. |
| **S3** | **ALCHIQ** | inverse-role atoms `S(y,x)` + the full Pred/r-Pred predecessor propagation + non-flat term positional Eq. | `cb-diff identical` on ore-15672 / ore-10908 ALCHIQ subsets; inverse+cardinality canaries; termination preserved. FP=0. |
| **S4** | **ALCHOIQ** | nominals: root context `v_r`, Nom/Join/r-Succ/r-Pred, nominal `⋗`-path bound. | `cb-diff identical` on wine (full nominal+cardinality), the corpus parity target; FP=0/MISSED=0. |

**Parallel fan-out** applies *within* S1 (normalize-extension ∥ order.rs ∥
canaries ∥ harness) and again within each later stage once S0 is frozen. The
dependency spine is **S0 → S1 → S2 → S3 → S4** (each adds rules/term-kinds, not a
re-architecture — the Sequoia rule set is designed for exactly this layering).

**Termination instrumentation.** Port `RUSTDL_CB_DEBUG` (per-dequeue clause
counts) so each stage can *prove* the bounded-clause-set property on the fuzz
onts (max clauses/context plateaus) rather than just "didn't hang in 120s."

**Independent opus review** at the S1 gate (the crux: the per-context C2 order +
the `∈̂` redundancy read-off — the highest-risk point, §0.2/§4) and the S2 gate
(Eq/Fact + cardinality FP-impossibility), mirroring the B1/B2 review discipline.

---

## 7. Migration

- **Coexistence.** Keep the current unordered B1/B2 engine
  (`engine.rs`/`model.rs`) as a **differential oracle**: `owl-dl-bench cb-diff`
  runs *both* CB engines (and the hybrid) on the same ontology and reports
  hierarchy equality + wall + RSS. The unordered engine is the *known-FP=0*
  baseline its `identical:true` gate compares against on the ALCH/ALCHQ fragments
  it already handles — a third independent check beyond the hybrid.
- **Same public API.** `owl_dl_cb::classify(&InternalOntology) -> CbOutcome`
  unchanged; dispatch internally on `RUSTDL_CB_CALCULUS` (`unordered` default |
  `sequoia`). Both return the same `CbHierarchy`. `OutOfFragment` widens (Sequoia
  accepts more) but the *contract* (defer to hybrid) is identical.
- **Default OFF** throughout. The Sequoia engine becomes the CB default only when
  it reaches `cb-diff identical:true` on the full ALCHOIQ corpus subset **and**
  terminates on all 21 fuzz onts — i.e. after S4 passes its gate. Even then,
  CB-as-production-default is a separate decision (it is the existing
  hybrid/wedge that ships).
- **Retirement path.** Once Sequoia subsumes the unordered engine's fragment with
  identical verdicts, the unordered B1/B2 can be demoted to test-only (kept as the
  oracle) or removed — but only after S4, not before (it is the safety net for the
  whole build).

---

## 8. Sources (primary, verbatim-extracted)

- **Tena-Cucala, Cuenca-Grau, Horrocks.** *Consequence-Based Reasoning for
  Description Logics with Disjunction, Inverse Roles, Number Restrictions, and
  Nominals.* arXiv:1805.01396; AIJ 298:103518 (2021); IJCAI 2018. Def. 1
  (context terms / query clause), Def. 2 (Su/Pr triggers), Def. 3 (context
  order), Def. 4 (redundancy), Def. 5 (context structure), Def. 7 (soundness),
  Def. 8 (strat), Table 2/3 (rules), Lemma 9 + generative-clause R1–R4 / `R*_t`
  (candidate model), the "not just refutationally complete" §3 statement.
  *(Full PDF decompressed locally via zlib for the rule shapes, the order
  definition, the soundness proof block, and the "single run … subsumptions
  between atomic concepts, ⊤ and ⊥" read-off characterization.)*
- **Bate, Motik, Cuenca-Grau, Simančík, Horrocks.** *Extending Consequence-Based
  Reasoning to SRIQ.* KR 2016; arXiv:1602.04498. The **query-clause** read-off
  ("a query clause is a DL-clause in which all literals are `B(x)`; our calculus
  decides whether `O ⊨ Γ → Δ`"), Def. 5 (context structure), Prop. 1 (Elim
  confluence), the role-chain elimination that scopes SRIQ as the later extension.
  *(Decompressed locally.)*
- **Simančík, Kazakov, Horrocks.** *Consequence-Based Reasoning beyond Horn
  Ontologies.* IJCAI 2011. The **older ALCH** ordered calculus — the **HISTORICAL
  ALTERNATIVE** we do NOT adopt (it is refutational; Sequoia supersedes it with
  the direct positive read-off). Cited for: **Remark 5** *"different orderings
  can be used for different left-hand sides `H`"* (corroborates Sequoia's
  per-context order); the **candidate-model construction** (eqs. (4)–(8),
  Lemmas 2–3, Thm 4 — the same Bachmair–Ganzinger machinery Sequoia's Theorem 2
  uses); and §5.1–5.2's one-context-per-class classification design. Its read-off
  *"`O ⊨ A ⊑ B` iff `A ⊑ B` or `A ⊑ ⊥` derived"* via *"`A ⊓ ¬B ⊑ ⊥`"* is the SKH
  **negative-context** device — useful to know it exists, but **NOT Sequoia's
  mechanism** (§0.5). *(Decompressed locally.)*
- **rustdl internal:** `docs/superpowers/specs/2026-06-16-cb-ordered-resolution-
  design.md` (Slice-0 falsification — the failure this design explains and fixes);
  `2026-06-16-cb-b2-tier2-equality-design.md` (B2 equality stratum — its Eq/Neq
  soundness analysis + `tier2_*` canaries port to S2);
  `2026-06-15-cb-engine-b1-alch-design.md` (B1 fragment table).

---

## 9. One-paragraph executive answer to the crux

Sequoia's read-off is **direct and positive** (AIJ Theorem 2): classify by
seeding **one context `q_A` per atomic class `A`** (core `{A(x)}`) with a
**per-context order `≻_{q_A}` initialised so `A(x)` is `≻`-MINIMAL** (Condition
C2), saturating once, and reading `A ⊑ B` off `A(x) → B(x) ∈̂ S_{q_A}` (contained
**up to redundancy**, Def. 4). **There is NO negation, NO `A ⊓ ¬B` context, and NO
per-pair refutation** — the calculus reads positives directly in one pass over
`n` contexts (not `n²` pairs). Completeness for the disjunctive/maximal-disjunct
case is delivered by **C2 (the per-context query-body-minimal order) + the
Bachmair–Ganzinger candidate-model construction** in the Theorem-2 proof, NOT by
a refutational stratum. **rustdl B1 broke because it ordered Hyper but used a
single global order and a read-off that did not enforce C2** — no single global
order can make every queried subsumer minimal, so the Slice-0 MISS was inevitable
(and is reproducible inside Sequoia's own rules under a non-C2 order — §0.2
trace). The fix is **per-context C2-respecting orders + the `∈̂` (up-to-redundancy)
read-off**, which the ordering + Def-4 redundancy then make terminating, killing
the unordered host's `combos = ∏` detonation. The user's "goal-directed
refutation" framing is therefore NOT what Sequoia does, and the direct positive
read-off is strictly better (no O(n²)) — recommend building it. **(An earlier
draft of this doc mis-attributed an SKH-style negative-context stratum to Sequoia;
corrected and logged in §0.5 — the implementer must not re-introduce it.)**
