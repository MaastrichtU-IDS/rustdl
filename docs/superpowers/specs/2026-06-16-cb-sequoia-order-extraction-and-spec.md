# Sequoia context-order: primary-source extraction + rustdl ALCH `seq_order.rs` spec

**Primary source:** Tena-Cucala, Cuenca-Grau, Horrocks, *Consequence-based
Reasoning for Description Logics with Disjunction, Inverse Roles, Number
Restrictions, and Nominals*, arXiv:1805.01396 (IJCAI'18 version; same calculus as
AIJ 298:103518, 2021). **The verbatim quotes below are from the LaTeX SOURCE of
the arXiv tarball** (`arxiv.org/src/1805.01396`, downloaded + extracted), not a
re-render — in particular `proof-order.tex` is **Appendix A, the order
construction the prior extraction MISSED**. Files cited: `calculus.tex`,
`proof-order.tex`, `proof-completeness.tex`.

---

## PART 1 — VERBATIM EXTRACTION

### 1.1 Definition 3 — the context order ≻ and the global a-term order ⋗

`calculus.tex` lines 102–113 (`\contextorder*` / `def:order`), verbatim:

> Let `⋗` be a total order on symbols of `Σ_f^O` and `Σ_o^O` such that for every
> `ρ ∈ Π`, if `ρ = ρ'·ρ''`, then `o_ρ ⋗ o_{ρ'}`. A (root) *context order* `≻`
> w.r.t. `⋗` is a strict order on (root) context atoms satisfying each of the
> following properties:
> 1. `A ≻ x ≻ y ≻ true` for each context p-term `A ≠ true`;
> 2. `n ≻ m` for each pair `n,m ∈ Σ_o^O` with `n ⋗ m`;
> 3. `f(x) ≻ g(x)`, for all `f,g ∈ Σ_f^O` with `f ⋗ g`;
> 4. `t[s₁]_p ≻ t[s₂]_p` for any context term `t`, position `p`, and context
>    terms `s₁,s₂` such that `s₁ ≻ s₂`;
> 5. `s ≻ s|_p` for each context term `s` and proper position `p` in `s`;
> 6. `A ⊁ s` for each atom `A ≈ true ∈ Pr` (`Pr^r`) and context term
>    `s ∉ {x,y,true} ∪ Σ_o^O`.

Preamble (`calculus.tex` 91–99), verbatim: *"clauses are ordered using a term
order `≻` based on a total order `⋗` on function symbols of sort a. The order
restricts the derived clauses since only `≻`-maximal literals can participate in
inferences … although each context can use a different `≻` order, a-terms are
compared in the same way across all contexts since `⋗` is globally defined. In
[Appendix A] we show how a context order can be constructed once `⋗` is fixed."*

**KEY READING (resolves the prior confusion):** `⋗` orders **a-terms only**
(function symbols `f,g` and nominals `n,m`). Def-3 says **nothing** about the
mutual order of two **predicate (concept) symbols** `B₁,B₂` — only `A ≻ x ≻ y ≻
true` (property 1, every p-term above the variables). Properties 2–5 are the
LPO/subterm machinery on a-terms; property 6 forbids predecessor-trigger atoms
from dominating non-trivial terms. **The concept-name precedence is FREE in
Def-3** — it is fixed (arbitrarily) by the construction, then disciplined per
query by Condition C2 (§1.3).

### 1.2 The order CONSTRUCTION (Appendix A — the piece the prior extraction missed)

`proof-order.tex`, **verbatim and complete** (it is short):

> **Context orders.** Given an order `⋗` verifying the properties in
> Definition 3, we can obtain a context term order `≻` as follows. Extend `⋗` on
> variables so that `x ⋗ y`. Then, extend it (arbitrarily) to symbols of sort p.
> Next, we let `≻` be the **lexicographic path order (LPO)** [BaN98] over context
> a- and p-terms induced by `⋗`. The well-known properties of LPOs ensure that
> `≻` is a **total simplification order** on all context terms that satisfies
> [properties 1–5] of Definition 3.
>
> To also satisfy [property 6], we relax `≽` by dropping all `A ≻ s` where
> `A ∈ Pr` and `s ∉ {x,y,true}`, as well as any of the form `y ≻ o`, `o ≻ y`,
> `x ≻ o`, `o ≻ x`. [Property 1] is satisfied after this step because we have not
> removed any of the orderings of that form … [properties 2,3] remain satisfied
> because the relaxation step does not eliminate any ordering involving only
> a-terms except `y ≻ o`, `o ≻ y`, `x ≻ o`, `o ≻ x` … [property 5: subterm] since
> we only eliminate orderings `A ≻ s` where `A ∈ Pr`, `A` contains no function
> symbols; … it can only be `x`, `y`, or in `Σ_o^O`. However, it is precisely
> these orderings that we do not remove …

**This is the whole construction.** It is **NOT** "told-subsumer depth" and
**NOT** "subsumer-respecting". It is:

1. Fix `⋗` on a-terms (function symbols + nominals), respecting the nominal-path
   constraint `o_ρ ⋗ o_{ρ'}` for `ρ = ρ'·ρ''`.
2. Extend `⋗` with `x ⋗ y` and **arbitrarily to predicate (concept) symbols**.
3. `≻` := the **LPO** over context terms induced by `⋗`.
4. Relax to satisfy property 6 (drop the listed `Pr`-domination orderings).

The concept-symbol precedence is literally *arbitrary* in the construction; the
query-specific discipline lives entirely in **Condition C2** of the completeness
theorem (next).

### 1.3 The Completeness Theorem with C1/C2/C3, and the algorithm (Steps 1–3)

`calculus.tex` lines 390–416, verbatim:

> **Theorem (Completeness).** Let `D` be a context structure which is sound for
> `O` and such that no rule of Table 2 or Table 3 can be applied to it. Then, for
> each query clause `Γ_Q → Δ_Q` and each context `q ∈ V` such that all of the
> following conditions hold, we have that `Γ_Q → Δ_Q ∈̂ S_q` also holds.
> - **C1.** `O ⊨ Γ_Q → Δ_Q`.
> - **C2.** For each context atom `A ∈ Δ_Q` and each `A'` of the form `B(x)` such
>   that `A ≻_q A'`, we have `A' ∈ Δ_Q`.
> - **C3.** For each `A ∈ Γ_Q`, we have `Γ_Q → A ∈̂ S_q`.
>
> To test whether `O` entails a query clause `Γ_Q → Δ_Q`, an algorithm can
> proceed as follows. In **Step 1**, create an empty context structure `D`, and
> fix an expansion strategy. In **Step 2**, introduce a context `q` into `D`, set
> its core to `Γ_Q`, and initialise the order `≻_q` in a way that is consistent
> with Condition C2 in [the Theorem]. Finally, in **Step 3**, saturate `D` over
> the inference rules of the calculus and check whether `Γ_Q → Δ_Q` is contained
> up to redundancy in `S_q`. Such algorithm generalises to check in a single run
> a set of input query clauses by initialising in Step 2 a context `q` for each
> query clause.

**C2 IS ABOUT THE HEAD `Δ_Q` — NOT THE BODY.** `Δ_Q = \InputQueryRHS` (the head
of the query clause). For a subsumption query `A ⊑ B`, the query clause is
`Γ_Q = {A(x)} → Δ_Q = {B(x)}`. C2 quantifies over `A ∈ Δ_Q`, i.e. over the head
atom **B**, and demands: every `B(x)`-form concept atom strictly `≻_q`-below the
head atom is itself in the head. Since `Δ_Q = {B}`, this forces **B (the
candidate subsumer) to be `≻_q`-MINIMAL among concept atoms.** The body/core
atom `A` is **not** the one C2 makes minimal.

This is **confirmed by the proof's use of C2.** `proof-completeness.tex` line 122:
at the query context `q`, condition `cond:fragment:groundorder:forbidden` (the
property that lets the candidate model refute a non-derived query) is discharged
"… for any `s₂ ∉ {x,y,true}∪Σ_o^O` such that `s₂ ≈ true ∉ Δ_Q`, we have
`s₁ ⊁_q s₂` **by C2 of the Theorem**." The head atoms must be `≻_q`-minimal so
the "forbidden" literal set (`Δ_Q`) is order-minimal; the model construction then
generates everything above and refutes only above the head — so any genuinely
entailed head atom is forced into `S_q`.

### 1.4 The Hyper rule and its eligibility side-condition

`calculus.tex` lines 231–234 (Table 2, **Hyper**), verbatim:

> If 1. `⋀_{i=1}^n A_i → Δ ∈ O` and `σ(x)=x` if `v ≠ v_r` or `σ(x) ∈ Σ_o^O` o.w.
> 2. and `Γ_i → Δ_i ∨ A_iσ ∈ S_v` **with `Δ_i ⊁_v A_iσ`**, for `1 ≤ i ≤ n`,
> then add `⋀_{i=1}^n Γ_i → ⋁_{i=1}^n Δ_i ∨ Δσ` to `S_v`.

The side-condition **`Δ_i ⊁_v A_iσ`** = "no literal of the residual `Δ_i` is
strictly `≻_v` the resolved atom `A_iσ`" = `A_iσ` is `≻_v`-**maximal** (eligible)
in its clause `Δ_i ∨ A_iσ`. **This is the operative requirement on the order**:
to resolve an ontology premise atom `A_i` against a context clause, the matching
atom must be `≻_v`-maximal in that clause. The same side-condition `Δ ⊁_u A`
gates **Succ** (line 267), **Pred** (262), **Join/r-Succ/r-Pred/Nom** (Table 3),
and `t ⊁ s`/`Δ ⊁ ...` gate **Eq/Fact**. So the order governs **all** rules, not
just Hyper.

### 1.5 Redundancy (Definition 4) and `∈̂`

`calculus.tex` lines 123–131, verbatim:

> A set of clauses `U` contains a clause `Γ → Δ` *up to redundancy*, written
> `Γ → Δ ∈̂ U`, if
> 1. `t ≈ t ∈ Δ` or `{t ≈ s, t ≉ s} ⊆ Δ` for some a-term `t,s`, or
> 2. `Γ' → Δ' ∈ U` for some `Γ' ⊆ Γ` and `Δ' ⊆ Δ`.

`Elim` (Table 2): drop `Γ → Δ ∈ S_v` if `Γ → Δ ∈̂ S_v ∖ {Γ → Δ}`. Note
(`calculus.tex` 134): clauses `A → A` are **not** tautological here.

---

## PART 2 — THE CENTRAL FINDING (what the prior extraction got wrong)

The design doc §0.45 concluded "completeness needs a **subsumer-respecting**
order (told `X⊑Y ⟹ X≻Y`); any C2 order MISSES; the construction is the
load-bearing thing the design did not extract." **Two of those three claims are
wrong, and the third (the construction) is now extracted (§1.2):**

1. **WRONG: C2 makes the body atom minimal.** Both the design doc (§0.2) and the
   current `seq_order.rs` (`per_context(query)` with `query` = the class being
   classified → `query_minimal` = core/body atom A) force the **body** minimal.
   C2 (§1.3, verbatim + proof line 122) forces the **head** (candidate subsumer)
   minimal. **The current engine never implements C2; it implements its inverse.**

2. **WRONG: "any C2 order misses; need subsumer-respecting depth."** The §0.45
   counterexample (query `A⊑D`, order `D≻C≻B≻A`) is **not a C2 order at all**:
   C2 makes the HEAD `D` minimal, so `D≻C≻B` (D maximal) violates C2. Under the
   correct C2 (D minimal) the by-cases derives `A→D` directly (§3.2). The "no
   static order works / inherit only B1-empirically" pessimism was an artifact of
   the inverted C2.

3. **The construction IS extracted (§1.2): LPO induced by `⋗`, concept-symbol
   precedence arbitrary, then C2 per query.** It is neither "told-subsumer depth"
   nor "subsumer-respecting" — those were a reconstruction that happened to
   approximate the right thing in the unit case and fail off it (§4).

**For ALCH the read-off comparison collapses to a concept-name precedence.**
Function terms DO exist in ALCH-S1: `∃R.C` skolemizes to `f(x)`, and S1's **Succ**
rule creates `f(x)`-cored successor contexts — so Def-3 property 3 (`f(x)≻g(x)`)
is **not** vacuous and `⋗`-on-functions is an S1 concern, not deferred to S2.
What IS true: the **Hyper eligibility that drives the subsumption read-off** only
compares **`B(x)` concept atoms** (all sharing the root variable `x`), and an LPO
over same-root terms reduces to the precedence on their top symbols =
**a total precedence on concept names**, with `B(x) ≻ x ≻ y ≻ true` (property 1)
automatic. The function-symbol order (`f(x)≻g(x)`) governs only the structural
Succ/`∃`/`∀` handling, which S1 treats as never-blocking in `eligible()` — so it
does **not** reach the concept-subsumption read-off. Nominals (`o`) and the
nominal-path `⋗` constraint are the genuinely S4 piece. **So the operative ALCH
construction for the read-off = pick a total concept-name precedence; the ONLY
real constraint on it is C2 (head/candidate-subsumer minimal).**

---

## PART 3 — THE rustdl ALCH ORDER SPEC (`crates/owl-dl-cb/src/seq_order.rs`)

### 3.0 The classification subtlety: batched read-off vs. per-query C2

Theorem 2 is **per query clause** `(A,B)`: context cored at `{A}`, `≻_q` with the
**head `B` minimal**. For classification we want **one context per class A** and
to read **all** subsumers `B` of A off `S_{q_A}` in one saturation
(`calculus.tex` 415: "a single run … a context `q` for each query clause" — the
batched optimization). But you cannot make every candidate subsumer `B` minimal
simultaneously in one order. Two regimes, presented honestly:

- **(R1) Guaranteed-complete (Theorem-2-exact), the fallback.** One context per
  query clause `(A,B)`, `≻` with `B` minimal among concept names. This is
  literally Theorem 2 ⟹ inherits its completeness. Cost: O(n²) contexts.
  **Interning caveat:** the current `by_core` map keys contexts by core alone, so
  it would wrongly MERGE `(A,B1)` and `(A,B2)` (identical core `{A}`, different
  orders). R1 must key by `(core, head)` or exempt these root contexts from
  `by_core` (fresh context per pair). Use R1 as the correctness oracle / fallback
  if the fuzz gate (§5) finds a residual R2 cannot order.

- **(R2) Empirically-complete optimization, the S1 re-gate target.** One context
  per class A, read all subsumers. The order is the **"dead-maximal,
  subsumer-respecting"** construction below. Same epistemic status as B1: NOT
  inherited from Theorem 2 (no single head atom is minimal) — validated by the
  differential fuzz gate.

### 3.1 The R2 per-context order construction (input → `atom_gt`)

**Input:** the normalized clause set `clauses`, the pool, and the **core class
A** of the context (the class being classified).

**Output:** a total order `≻_A` on concept atoms (`atom_gt(b1,b2)`), used by
`eligible()` (`Δ ⊁_A a`) and `sort_head`.

Three rank tiers, high to low (higher = more eligible = resolved first):

1. **Contextually-DEAD atoms — MAXIMAL.** An atom `X` is *dead in context A* if
   `O ⊨ A ⊓ X ⊑ ⊥`. Two sound, statically-detectable sources:
   - **Global unsat:** `X ⊑ ⊥` — i.e. a clause `{X} → {}` (one-atom body, EMPTY
     head). [Closes minimal-gap, seeds 31/58/147/158.]
   - **Told-disjoint from the core:** `A ⊓ X ⊑ ⊥` — i.e. a clause `{A,X} → {}`
     (two-atom body, empty head), or `X` in `told_disjoint[A]`. [Closes seed166.]
   Dead atoms ranked above all live atoms (tie-break among them by `ConceptId`).
   *Rationale:* a dead disjunct must be `≻`-maximal so the empty-head clause
   `A⊓X⊑⊥` / `X⊑⊥` is Hyper-eligible to resolve it OUT of any disjunction
   `A → …∨X∨…`, leaving the live disjuncts (§3.2/§3.4 traces).

2. **Live atoms — by subsumer-respecting depth (descending), so subsumees rank
   above subsumers.** `depth(X) = 1 + max{depth(Y) : told X⊑Y}`. A told `X⊑Y`
   ⟹ `depth(X) > depth(Y)` ⟹ `X ≻ Y`: the subsumee is eligible/maximal so the
   chain `X⊑Y` resolves and propagates the subsumer down to the core. (This is
   what the current builder does — KEEP it for the live tier.)

3. **The core class A itself — MINIMAL among live atoms.** Property 1 still holds
   (`A ≻ x`). Making A least among concept atoms is harmless (A is in the core,
   seeded as a unit `→A`, never needs to be resolved-out) and keeps determinism.

   **NOTE on C2:** in regime R2 there is no single head atom, so C2 is not
   literally enforceable; tier-1 (dead-maximal) is the *operational* substitute
   that makes the dead-disjunct resolutions fire. In regime R1 you instead set
   `query_minimal = B` (the head) — that is C2 verbatim and the only change R1
   needs over R2.

Comparison `atom_gt(a,b)`: compare keys `(tier, sortwithin)` where tier ∈
{dead=2, live=1, core=0}; within `dead` by ConceptId; within `live` by
`(depth, ConceptId)`; `core` is the unique minimum. `is_atomic` literals only;
`∃R.B`/`∀R.B` stay never-blocking in `eligible()` (discharged by Succ/All, not
Hyper) — sound, MISS-biased, unchanged from current.

### 3.2 Trace — by-cases `{A⊑B⊔C, B⊑D, C⊑D} ⊢ A⊑D` (§0.45's "counterexample")

Concept names B,C,D live; A core. depth: D=0 (no told sup), B=C=1 (told ⊑D), A=0.
Order (R2): live tier descending-depth ⟹ `B,C (depth1) ≻ D (depth0) ≻ A (core)`.
This is exactly the subsumer-respecting order. Saturate context cored {A}:
```
Core:                     → A
Hyper(A⊑B⊔C) on →A:       → B ⊔ C        (A resolved; A maximal-eligible: core, ok)
Hyper(B⊑D) on →B⊔C:       resolve B; residual {C}; need C ⊁ B. depth(C)=depth(B)=1,
                          tie by ConceptId — but BOTH are ≻ D regardless. Pick the
                          eligible one each round; say B eligible ⟹ → D ⊔ C
Hyper(C⊑D) on →D⊔C:       resolve C; residual {D}; need D ⊁ C. depth(D)=0<1=depth(C) ✓
                          ⟹ → D ⊔ D = → D
```
`→ D` derived; `A→D ∈̂ S_q` (head ⊆ {D}). **Derivable.** (Under the *inverted*
order `D≻C≻B` from §0.45, D would be maximal and the `B⊑D`/`C⊑D` resolutions
blocked — but that order is the WRONG one; the subsumer-respecting live tier
forbids it.) Note the by-cases never needed tier-1 (no dead atom); the live tier
alone suffices, vindicating the original depth heuristic *for the unit-chain
fragment*.

### 3.3 Trace — minimal-gap `{K1⊑K3⊔K2, K3⊑⊥} ⊢ K1⊑K2`

`K3⊑⊥` = clause `{K3}→{}` (empty head). Tier-1 (global unsat): **K3 dead ⟹
maximal.** K2 live (depth 0); K1 core. Order: `K3 (dead) ≻ K2 (live) ≻ K1 (core)`.
Saturate context cored {K1}:
```
Core:                     → K1
Hyper(K1⊑K3⊔K2) on →K1:   → K3 ⊔ K2
Hyper(K3⊑⊥ i.e. K3→·):    resolve K3; residual {K2}; need K2 ⊁ K3.
                          K3 dead-maximal ⟹ K2 ⊁ K3 ✓ ⟹ → K2  (empty Δσ)
```
`→ K2` derived; `K1→K2 ∈̂ S_q`. **Derivable.** The current depth-only builder
gives K3 depth 0 = K2 depth 0, so the ConceptId tie can put K2 ≻ K3 (when DEAD
interned first), blocking the K3-elimination — **exactly the documented MISS**
(`/tmp/seqfuzz/GAP/order-polarity-MISS.ofn`). Tier-1 fixes it deterministically.

### 3.4 Trace — seed166 `K7⊑K5`/`K7⊑K8` (the "∀/back-prop" suspect — IT IS NOT)

Relevant axioms (from `seed166.ofn`):
- Axiom 10 `SubClassOf(Union(Inter(K0,K8), K7), Union(K8,K6,K0))` — LHS-union
  disjunct ⟹ **`K7 ⊑ K8 ⊔ K6 ⊔ K0`**.
- `DisjointClasses(K7 K6)` ⟹ **`K7⊓K6⊑⊥`** (clause `{K7,K6}→{}`).
- `DisjointClasses(K3 K0 K7)` ⟹ **`K7⊓K0⊑⊥`** (clause `{K7,K0}→{}`).
- `EquivalentClasses(K8 K5)` ⟹ `K8⊑K5`, `K5⊑K8`.

The `∀r.K7` axioms (`K7⊑∀r.K7`, `∃r.K8⊑∀r.K7`) are **irrelevant** to these two
pairs. Context cored {K7}. Tier-1 dead-in-context-K7: **K6 dead** (`K7⊓K6⊑⊥`),
**K0 dead** (`K7⊓K0⊑⊥`); K8 live. Order: `K6,K0 (dead) ≻ K8 (live) ≻ K7 (core)`.
```
Core:                       → K7
Hyper(K7⊑K8⊔K6⊔K0) on →K7:  → K8 ⊔ K6 ⊔ K0
Hyper(K7⊓K6⊑⊥) on that:     resolve K6; need {K8,K0} ⊁ K6.  K6,K0 both dead-maximal;
   [the K7 body atom matches the core unit →K7]              K8 live < dead ⟹ K8 ⊁ K6 ✓;
                            K0 dead ties K6 — eligible by ConceptId ordering within
                            the dead tier; resolve K6 ⟹ → K8 ⊔ K0
Hyper(K7⊓K0⊑⊥) on →K8⊔K0:   resolve K0; need K8 ⊁ K0. K8 live < K0 dead ✓ ⟹ → K8
Hyper(K8⊑K5) on →K8:        → K5
```
`→K8` and `→K5` derived ⟹ `K7⊑K8`, `K7⊑K5`. **Derivable.** The miss happens
because K6/K0 get depth 0 = K8 depth 0, so K8 can tie-break ABOVE K6/K0 and block
the disjointness elimination — **the SAME mechanism as minimal-gap, one step
removed** (contextual disjointness `K7⊓K6⊑⊥` instead of global `K3⊑⊥`).

### 3.5 ORDER vs. ENGINE — the verdict

**All five witnesses (31, 58, 147, 158, 166) are ONE order gap, not an engine-rule
gap.** Two flavours of the same hole — an atom that is *contextually dead* and
must be `≻`-maximal to be Hyper-resolved out of a disjunction, but the
told-UNIT-depth builder gives it no rank:
- **global-unsat flavour** (`X⊑⊥`, empty-head 1-atom-body clause): minimal-gap,
  seeds 31/58/147/158.
- **told-disjoint-from-core flavour** (`A⊓X⊑⊥`, empty-head 2-atom-body clause):
  seed166.
FINDINGS.md's "seed166 may route through ∀/back-prop … the hole is BROADER" is
**falsified**: K7⊑K5/K7⊑K8 are pure propositional+disjointness. There is **no
∀-derived-subsumer order gap and no missing Pred/Succ/All interaction** in these
witnesses. The Hyper/Pred eligibility side-condition `Δ ⊁ A` *does* also govern
∀-derived subsumers in successor contexts, but none of the 5 witnesses exercises
that path for the missed pair. (If a future fuzz finds a miss that is *only*
explicable by a back-propagated atom getting no rank, THAT would be a genuinely
new manifestation — but the current evidence does not show one.)

**Precise statement of the prior heuristic's defect:** the told-unit-depth
builder credited rank only from `premise.len()==1 ∧ head.len()==1` clauses. It is
**right for the live unit-subsumer chain** (§3.2) but **gives zero credit to
every non-unit-derived deadness** — empty-head clauses (`X⊑⊥`, `A⊓X⊑⊥`) and, by
the same token, any disjunction-derived subsumer that should rank a disjunct.
The fix is **not** "credit ⊥-disjuncts" (whack-a-mole, misses the disjointness
half) — it is the **dead-maximal tier** (§3.1 tier 1) which subsumes both,
detected from the *empty-head clauses and the told-disjoint table*, plus
deadness being **per-context** (depends on the core A) — which is correct, since
orders are per-context anyway, and is exactly why a single GLOBAL depth map was
structurally wrong.

### 3.6 Well-definedness (cycles, incomparability, C2/Def-3 compliance)

- **Told-subsumer cycles (equivalence classes).** `X⊑Y, Y⊑X` ⟹ depth fixpoint
  must terminate. Keep the current cap (`rounds = |atoms|`) OR, cleaner, **collapse
  told-subsumption SCCs** (Tarjan) to a DAG, assign depth on the condensation,
  give all members of an SCC equal depth, tie-break by `ConceptId`. Either way the
  result is a **total** order (ConceptId breaks all ties) ⟹ Def-3's "strict total
  order on context atoms" is satisfied. Equivalent classes ending at equal rank is
  correct (they are interchangeable subsumers).
- **Incomparable atoms** (no told relation): land in the live tier at depth 0,
  totally ordered by `ConceptId`. Fine — Def-3 leaves concept-name precedence
  free; any total extension is a valid context order.
- **Def-3 compliance (ALCH).** Property 1 (`B(x) ≻ x ≻ y ≻ true`) is enforced by
  treating only concept atoms (never `x,y,true`) in `atom_gt` for the read-off
  comparison. Property 3 (`f(x)≻g(x)`) IS live in S1 (skolem terms from `∃R.C`)
  and is the responsibility of the structural/Succ handling, not the concept
  `atom_gt` — keep them separate. Properties 2,6 (nominals, `Pr`-relaxation) are
  vacuous until S4. A debug `assert` that the order is a strict total
  order (irreflexive, transitive, total) on the concept-atom vocabulary is the
  guard. **An order bug is MISS-biased, never FP** (the rules are sound for ANY
  order; soundness Theorem is order-independent — `calculus.tex` 384–388).
- **Dead-tier soundness.** Tier-1 only promotes atoms `X` with `O ⊨ X⊑⊥` or
  `O ⊨ A⊓X⊑⊥`. Both are genuine entailments (empty-head clause / told-disjoint
  table). Mis-classifying a *live* atom as dead would only re-order ⟹ MISS-risk,
  never FP. So the dead-detection may be a sound under-approximation
  (told-disjoint, not full reasoning) without endangering FP=0.

### 3.7 Does a single static per-context order suffice, or is `∈̂` / candidate-model needed?

- The **read-off stays syntactic `∈̂` (Def-4)**: `A⊑B` iff some derived clause has
  body `⊆{A}` and head `⊆{B}` (a unit `→B`, or `→` empty = `A⊑⊥`). The
  candidate-model `R*_c` (`proof-completeness.tex`) is the **proof device** for
  Theorem 2, **NOT an alternate read-off** — do not implement a model-based
  read-off (that would reopen the correction-log conflation). Completeness comes
  from the **order making the resolutions fire**, then the syntactic witness
  exists in `S_q`.
- **For the unit-chain + direct-deadness fragment (all 5 witnesses + by-cases): a
  single static per-class order (R2) suffices** and is derivable, proven by the
  three traces (§3.2–3.4).
- **The honest residual risk:** *chained/derived* deadness — an atom dead via a
  **derived** disjointness rather than a told one — could, in principle, force two
  candidate subsumers to impose conflicting "must-be-above" constraints that no
  single linear order satisfies. Direct deadness forms a DAG (told-disjoint +
  global-unsat edges don't cycle with told-subsumption), so a single linear
  extension exists; derived deadness has no such guarantee. **Therefore R2 is
  empirically-complete (B1-parity status), not Theorem-2-inherited.** If the fuzz
  gate (§5) exposes a residual that R2 cannot order, the sound resolution is **R1
  (per-query-clause contexts, C2-exact, B minimal)** — NOT a richer read-off.

---

## PART 4 — CONCRETE `seq_order.rs` CHANGES (spec, no code written)

1. `OrderBuilder::build` — additionally scan for **empty-head clauses**:
   - `premise.len()==1, head.is_empty()` ⟹ record `X` in a `global_unsat: HashSet`.
   - `premise.len()==2, head.is_empty()` ⟹ record the pair in `told_disjoint`.
   - Also ingest the IR's existing told-disjoint table if available (`told.rs`).
   Keep the existing live-tier `depth` fixpoint unchanged.
2. `per_context(core: ConceptId)` — change semantics: the argument is the **core
   class A** (already is), but build a `PerContextOrder` carrying (a) `global_unsat`,
   (b) `told_disjoint[A]` materialized into a per-context `dead: HashSet`, (c) the
   `depth` map, (d) `core = A`. **Remove `query_minimal`-on-body** as the C2
   mechanism; replace with the three-tier `atom_key`.
3. `atom_key(atom) -> (tier, within)`:
   `tier = 2 if atom∈dead else (0 if atom==core else 1)`;
   `within = ConceptId` for dead/core, `(depth, ConceptId)` for live;
   `atom_gt = atom_key(a) > atom_key(b)` (dead > live > core; live by depth desc).
4. `eligible` / `sort_head` — unchanged in shape; they consume `atom_gt`.
5. **R1 fallback hook** (for the gate / residuals): a `per_query(core, head)`
   variant that sets `query_minimal = head` (C2-exact) and seeds one context per
   `(A,B)` pair. Wire behind a flag (e.g. `RUSTDL_CB_ORDER=per_query|per_class`),
   default `per_class` (R2) once it passes the gate.

---

## PART 5 — VALIDATION PLAN (the S1 re-gate)

1. **The 5 witnesses must flip to derivable** under R2:
   `/tmp/seqfuzz/GAP/{minimal-gap,order-polarity-MISS,seed31,seed58,seed147,seed158,seed166}.ofn`
   — for each, `RUSTDL_CB_CALCULUS=sequoia owl-dl-bench cb-diff FILE` ⟹
   `identical: true` (currently `only_in_cb` / `only_in_cur` ≠ 0). The
   order-polarity pair must give the SAME verdict regardless of interning order
   (the determinism check — tier-1 removes the ConceptId-tie dependence).
2. **243-ontology ALCH fuzz, MISSED=0 vs B1, FP=0** (`/tmp/seqfuzz/fuzz.py`,
   re-run with the new order). This is the empirical-completeness proof for R2 on
   ALCH (same epistemic status as B1).
3. **The by-cases canary + bibtex + alehif(ALC) + synthetic 15-class ALCH gate**
   `cb-diff identical:true` (the original S1 gate, §6 of the design doc).
4. **If any residual survives R2:** switch the failing query to R1 (per-query,
   C2-exact) and re-confirm; that residual is then the documented evidence that a
   single per-class order is insufficient and per-query contexts are mandatory
   (the critical-finding branch). Current evidence (5/5 witnesses + by-cases all
   derivable under R2, §3) suggests **R2 will pass**, but this is gate-decided,
   not assumed.
5. **FP=0 is the hard invariant throughout** — the order is sound for ANY order
   (Soundness Theorem, order-independent), so steps 1–4 can only move MISSes,
   never introduce a false positive.

---

## SOURCES

- arXiv:1805.01396 LaTeX source (`arxiv.org/src/1805.01396`): `calculus.tex`
  (Def 1 context terms, Def 3 `def:order`, Def 4 redundancy, Hyper/Eq/Fact/Succ/
  Pred/Nom rule tables, Completeness Theorem C1/C2/C3, Steps 1–3),
  `proof-order.tex` (**Appendix A — the LPO construction**),
  `proof-completeness.tex` (candidate-model / generative-clause proof; line 122
  = C2's operative use).
- rustdl: `crates/owl-dl-cb/src/seq_order.rs` (current inverted-C2 + told-unit
  depth), `seq_engine.rs` (`intern_context`/`per_context`/`add_clause` `∈̂`),
  `/tmp/seqfuzz/GAP/FINDINGS.md` (the 5 witnesses; its seed166 "∀/back-prop"
  hypothesis falsified here), `docs/superpowers/specs/2026-06-16-cb-sequoia-
  rearchitecture-design.md` §0.45/§0.46 (the gap; its "subsumer-respecting / any
  C2 misses / construction not extracted" framing corrected here).
