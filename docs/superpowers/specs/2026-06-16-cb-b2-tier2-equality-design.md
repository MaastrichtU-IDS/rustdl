# CB engine B2 Tier 2 — equality reasoning for general `≤n R.C` (ALCHQ completeness)

**Date:** 2026-06-16
**Crate:** `crates/owl-dl-cb` (consequence-based classifier, default-OFF)
**Scope:** ONLY Tier 2 — the general qualified at-most `≤n R.C` (n ≥ 2) where m+1
(m ≥ n) `C`-witnesses exist but a pairwise merge is consistent, i.e. a genuine
*disjunction of equalities* `⋁_{i<j}(sᵢ≈sⱼ)` must be derived and discharged.
Tiers 0+1 (`≥n`→`∃`-Succ counting, `≤0`/`≥1` reductions, `≤n` disjoint-clique
clash, `≤1` functional-collapse) are the companion B2 design; this document layers
on top of them but — per §7 — recommends a **unified equality-first build** in which
Tier 1's `≤1`-collapse is the degenerate (forced-unit) case of the Tier-2 machinery.

**Status:** DESIGN ONLY. No code written. This is the implementation-ready spec.

---

## 0. The central finding (read this first)

**Tier-2 equality reasoning IS reconcilable with the engine's load-bearing
unordered concept resolution.** It does NOT require the ordering that Slice-0
falsified, because Tier-2 equality lives in a *disjoint syntactic stratum*:
equality literals range over **successor terms**, and the equality inferences
(paramodulation `Eq`, `Ineq`, `Fact`) are ordered by a **term ordering on
successor terms only** — provably separate from the concept-atom ordering that
the `Hyper` rule must NOT have.

This is exactly how Sequoia (Tena-Cucala, Cuenca-Grau, Horrocks; full calculus
arXiv:1805.01396 "…with Disjunction, Inverse Roles, Number Restrictions, and
Nominals"; AIJ 298:103518, 2021) and its ancestor Bate et al. (KR 2016,
arXiv:1602.04498) structure it: **two orderings governing two rule families.**
The reconciliation, the one genuine landmine, and the exact pass/fail condition
are in §3.

**The honest caveat that bounds the win (§3.4).** Sequoia's *published* calculus
applies its term/literal order to the `Hyper` rule too (side-condition
`Δᵢ ⊁ᵥ Aᵢσ`, confirmed in the paper's Table 2). The rustdl B1 engine
*deliberately runs `Hyper` unordered* (Slice-0 proved the ordered form drops the
direct read-off of *maximal* consequence atoms). So this design does **not** port
Sequoia verbatim — it ports the **equality stratum only** onto an **unordered-Hyper
host**. The soundness of that combination is unconditional (every Tier-2 inference
is valid in all models — §4). The *completeness* of that combination on the full
ALCHQ fragment is argued in §3.3 to reduce to the same direct-read-off property
the unordered B1 engine already relies on, because the equality disjunction is
discharged by the *unordered* Hyper/back-prop reasoning-by-cases the engine
already does correctly (the `disjunctive_subsumption_by_cases` canary). **No
construct in ALCHQ ever paramodulates an equality INTO a concept-atom
disjunction** — that is the precise condition under which the two strata stay
decoupled, and it holds for ALCHQ (it stops holding only with nominals, B4 — §6).

---

## 1. Equality representation in this engine (the `model.rs` freeze-break)

### 1.1 Why `model.rs` must change: the term-decoupling constraint

B1 interns successor *contexts* by core (`by_core` is the termination key;
"find-or-create, never mutate a shared successor"). **That is structurally
insufficient for `≥n` / `≤n`.** Proof from bench fixture `47_min_max_conflict`
(`≥2 r.A ⊓ ≤1 r.A`):

- `≥2 r.A` requires **two distinct** `r`-successors, each with core `{A}`.
- B1's `link_successor` interns by core, so both collapse to **one** context.
  The "two distinct witnesses" the `≤1` must merge no longer exist as two
  things — there is nothing for an equality to range over.

So the first-class entity equality ranges over cannot be the core-interned
`ContextId`. It must be a **term** (a witness), one per existential/at-least
occurrence, *decoupled* from the context it is typed by. This mirrors Sequoia's
**function-term successors** `f(x)`: each `∃R.B` / `≥n R.B` literal mints a
distinct successor function symbol; equalities `s≈t` / `s≉t` range over these
terms, never over concept atoms.

### 1.2 The new types (minimal surface)

A `Term` is a successor witness owned by a *parent* context. It points at a
context (shareable by core, for type-reasoning) but has its own identity.

```rust
// model.rs — NEW. Breaks the B1 freeze (documented; re-sync parallel tasks).

/// A successor witness: identity is decoupled from the context that types it,
/// so `≥2 R.A` can mint two distinct terms both pointing at core {A}.
pub type TermId = usize;

#[derive(Clone, Debug)]
pub struct Term {
    /// The context whose core types this witness (find-or-create by core).
    pub ctx: ContextId,
    /// The role on the edge `parent —R→ this`.
    pub role: Role,
    /// The clause residual `N` from the spawning clause `N ⊔ ∃R.B` / the
    /// `≥n`-clause, carried for back-propagation (B1 edge-residual semantics).
    pub residual: Vec<Literal>,
    /// Set to `Some(other)` when this term has been merged INTO `other`
    /// (union-find parent). Merge never mutates a shared ctx — it repoints.
    pub merged_into: Option<TermId>,
}
```

The new head-literal kinds — **NOT `ConceptId`** (this is soundness-critical;
see §3.2):

```rust
/// A clause-head literal in the EXTENDED (B2) model. B1's `Literal = ConceptId`
/// (atomic / ∃R.B / ∀R.B) becomes the `Concept(ConceptId)` arm. Equalities are
/// a DISTINCT variant so the term-ordering can never leak into the concept
/// read-off (the Slice-0 hazard).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeadLit {
    /// B1 literal: atomic `B`, `∃R.B`, or `∀R.B` (interned `ConceptId`).
    Concept(ConceptId),
    /// `s ≈ t` — the two successor terms denote the same element.
    Eq(TermId, TermId),
    /// `s ≉ t` — the two successor terms are distinct.
    Neq(TermId, TermId),
}
```

`DerivedClause.head: Vec<Literal>` becomes `Vec<HeadLit>`. The empty head is
still `⊥`. Tautology = head contains a `Concept(Top)` **or** a `Neq(s,s)`-derived
reflexive (handled by the `Ineq` rule, §2.4) **or** an `Eq(s,s)` (reflexivity —
dropped on insert).

### 1.3 What ranges over what (the decomposition)

- **Concept atoms** (`Concept(_)`): range over the context variable `x` — exactly
  as in B1. The `Hyper` rule operates ONLY on `Concept(_)` literals and stays
  **unordered** (untouched).
- **Equality/inequality** (`Eq`/`Neq`): range over **`TermId`** — the successors
  of the *current context*. They participate ONLY in the new `Eq`/`Ineq`/`Fact`
  rules (§2), which are ordered by a **term ordering on `TermId`** (§3.1).

This is the stratification. Concept resolution and equality resolution never
share a literal kind, so no ordering crosses between them.

### 1.4 Per-context term tables

`Context` gains the witness bookkeeping (terms are *local to the parent*, since
they are that context's successors):

```rust
pub struct Context {
    pub core: BTreeSet<Atom>,
    pub clauses: Vec<DerivedClause>,
    pub seen: BTreeSet<DerivedClause>,
    // B1 `succ: Vec<(Role, ContextId)>` is REPLACED by terms:
    pub terms: Vec<Term>,                 // NEW: this context's successor witnesses
    pub at_most: Vec<(u32, Role, Atom)>,  // NEW: active `≤n R.C` constraints (Tier-2)
}
```

`ContextGraph` keeps `by_core` for context interning (unchanged termination key
for *contexts*); terms are a *separate* finite population (§5).

---

## 2. The Tier-2 inference rules

Cited to Sequoia (arXiv:1805.01396) Table 2 (rules `Core`, `Hyper`, `Eq`, `Ineq`,
`Fact`, `Elim`, `Pred`, `Succ`, `Nom`) and its DL→clause translation (their DL4
row encodes `≤nS.B`). Rule *shapes* are quoted from the paper; the *context-graph
realization* is this engine's adaptation.

### 2.1 Normalization admits `≤n`/`≥n` (normalize.rs)

The B1 fragment gate (`check_concept`) currently returns `Err("cardinality")` on
`Min`/`Max`. Tier 0/1/2 replace that with admission + lowering:

- `≥n R.C` (n ≥ 1): lower to **n distinct existential markers** plus pairwise
  inequalities. Concretely, emit a head literal that, at `Succ`-time, mints `n`
  distinct terms with core `{C}` (or `⊤` if `C=⊤`) **and** seeds
  `Neq(sᵢ,sⱼ)` for all `i<j`. (Tier 0: `≥1 R.C ≡ ∃R.C`, the existing `Some`.)
  `≥0` is `⊤`, dropped.
- `≤n R.C` (n ≥ 0): record `(n, R, C)` in the parent context's `at_most`. `≤0 R.C`
  is the Tier-0 reduction `∃R.C ⊑ ⊥`-style (head `∀R.¬C`); `≤1` and `≤n` (n ≥ 2)
  are handled by the `r≤` rule (§2.2).
- `=n R.C` (exact) = `≥n R.C ⊓ ≤n R.C` (fixture 49).
- Non-`⊤`, non-atomic `C` is named via the existing `def_atom_for` chokepoint so
  `C` is always atomic in the recorded constraint (reuses B1 structural transform).

The fragment gate now rejects only B3/B4 constructs (inverse, nominal, chains,
transitivity, functional/role-characteristics, ABox, datatypes) — `Min`/`Max`
become in-fragment.

### 2.2 The `r≤` / choose rule (generate the equality disjunction)

**Sequoia DL4 clause shape (quoted):**
`B₁(x) ∧ ⋀_{1≤i≤n+1} SB₂(x,zᵢ) → ⋁_{1≤i<j≤n+1} zᵢ≈zⱼ`

**Context-graph realization (`apply_at_most`):** for a context `v` with an active
constraint `(n, R, C) ∈ v.at_most`, collect the set `W` of `v`'s **live** terms
`s` (i.e. `s.merged_into == None`) such that `R' ⊑* R` for `s`'s edge role `R'`
**and** `C ∈ core(s.ctx)` (the witness is a `C`-witness). If `|W| ≥ n+1`, pick any
`n+1`-subset `{s₀,…,sₙ}` and derive the clause

```
v:  (residual disjunction)  ⊔  ⋁_{0≤i<j≤n}  Eq(sᵢ, sⱼ)
```

where *(residual disjunction)* is the union of the `n+1` chosen terms' edge
residuals `N` (so the equality disjunction is *conditioned on* every disjunct
that had to be true for these witnesses to exist — this is the FP guard, §4.2).

- This clause is a **real disjunction**: "given the core, at least one of these
  merges holds." It is NOT a commitment to any one merge — exactly like B1's
  `A ⊑ B ⊔ C` (`disjunction_alone_no_unit_subsumption`).
- **`r≤` does not merely store the clause — it spawns a *speculative merge* per
  disjunct** (§2.3). This is the load-bearing point: the disjunction is discharged
  by per-disjunct case analysis through the existing back-prop, NOT by waiting for
  a unit `Eq` to be derived some other way.
- The redundancy gate (forward/backward subsumption) prunes weaker `r≤` clauses
  automatically.

### 2.3 Discharging the equality disjunction: speculative merge with residual (find-or-create)

This is **B1 disjunctive back-propagation verbatim**, with the spawning literal an
`Eq`-disjunct instead of an `∃`. Compare the passing B1 unit test
`disjunctive_back_propagation` (`A ⊑ ∃R.C, C ⊑ D₁ ⊔ D₂, ∃R.D₁ ⊑ F, ∃R.D₂ ⊑ F ⟹ A ⊑ F`):
each disjunct spawns an edge whose residual is the *rest of the disjunction*, and
`⊥`/consequence back-propagates only when a disjunct's branch clashes; bare `⊥`
only when the residual empties.

For a derived `r≤` clause `R(v) ⊔ ⋁_{k} Eq(sᵢₖ,sⱼₖ)` at context `v` (where `R(v)`
is the spawning residual — possibly empty), spawn a **speculative merge for each
disjunct** `Eq(sᵢₖ,sⱼₖ)`, carrying edge-residual

```
res_k  =  R(v)  ⊔  ⋁_{l ≠ k} Eq(sᵢₗ,sⱼₗ)      // the spawning residual + the OTHER disjuncts
```

**`merge_terms(v, sᵢ, sⱼ, res_k)`** (the merge itself — find-or-create, never mutate):

1. Union-find representatives; WLOG merge `sⱼ` into `sᵢ` (`sⱼ.merged_into = sᵢ`).
   (Speculative: the union-find binding is *local to this merge edge's reasoning*;
   the disjunction is not committed — the other disjuncts remain live via `res_k`.)
2. `union_core = core(sᵢ.ctx) ∪ core(sⱼ.ctx)`.
3. **Find-or-create** the context with core `union_core` (`intern_context` —
   shared by core, never mutated). Call it `u'`.
4. Record a merge edge `v —(merge)→ u'` with residual `res_k` (same structure as a
   `Succ` edge residual; consumed by the existing `apply_back_prop`). The old
   contexts `sᵢ.ctx`, `sⱼ.ctx` are **untouched** — still shared by any other
   terms/predecessors (the B1 `R∀` "find-or-create, never mutate the shared
   successor" discipline, verbatim).
5. **Back-prop does the discharge** (existing `apply_back_prop`): if `u'` derives
   `⊥`, `v` derives `res_k`. So:
   - merge clashes AND `res_k` empty (last disjunct, no spawning residual) ⇒ `v ⊑ ⊥`.
   - merge clashes AND `res_k` non-empty ⇒ `v` derives `res_k` (the remaining merge
     options ⊔ spawning residual) — feeds the next disjunct's case.
   - merge's `u'` is satisfiable ⇒ nothing back-propagates from it ⇒ no `⊥`, no
     spurious read-off.
6. `Neq(sᵢ,sⱼ)` (e.g. from a `≥m` on the same role) co-present with the speculative
   `Eq(sᵢ,sⱼ)` → §2.4 same-pair clash → `u'`/`v` derives the `res_k` (or `⊥`).
7. The merged term may drop `v`'s live `C`-witness count below `n+1`, so
   `apply_at_most` re-evaluates (`enqueue(v)`); a satisfiable union-core simply
   retracts the obligation. Monotone (`merged_into` never unset) ⇒ no oscillation.

**Tier 1's `≤1`-collapse is the degenerate single-disjunct case:** `≤1 R.C` + two
`C`-witnesses ⇒ `r≤` emits the single pair `Eq(s₀,s₁)` (no disjunction; `res = R(v)`
only). The one speculative merge with empty `res` is a *forced* merge — identical to
B1's left-existential complement path collapsing to a unit. So Tier-1 is the n+1=2
instance of the same speculative-merge code path — confirming the §7 unified-build
recommendation. Fixtures 46 (satisfiable union-core ⇒ SAT) and 48 (`{A,B}⊑⊥` ⇒
`res` empty ⇒ UNSAT) are exactly this degenerate case.

### 2.4 The equality clash: same-pair `Eq ∧ Neq → ⊥` (the only equality rule B2 needs)

The representation choice in §2.3 — successor *types* live in **union-core
contexts**, NOT as parent-context literals `A(f(x))` — means there is almost
nothing for general positional paramodulation to act on in B2. The disjunction is
discharged by **speculative merge** (§2.3), not by rewriting equalities into each
other. So B2 needs only the degenerate slice of Sequoia's `Eq`/`Ineq`:

**Same-pair clash.** If a context (or a speculative merge edge's reasoning) has both
a forced `Eq(s,t)` and a `Neq(s,t)` on the **same pair** (canonicalized by
union-find so `s`,`t` are representatives), derive `⊥` for that branch. This is
the specialization of Sequoia's
```
Eq:   Γ₁ → Δ₁ ∨ s≈t,   Γ₂ → Δ₂ ∨ s≉t   ⟹   Γ₁∧Γ₂ → Δ₁ ∨ Δ₂ ∨ t≉t
Ineq: Γ → Δ ∨ t≉t                       ⟹   Γ → Δ
```
to flat, ground, depth-1 successor terms (`s₂[t₁]ₚ ⋈ t₂` with `p` the root position).
It is the only place `Neq` is consumed. Fixture 47 (`≥2 r.A` ⇒ `Neq(s,t)`; `≤1 r.A`
⇒ forced `Eq(s,t)`) is exactly this clash.

The "larger-to-smaller orientation" (`t ⊁ᵥ s`) is realized by **union-find
canonicalization** on `TermId`s — the term ordering, applied to `TermId`s only,
NEVER touching `Concept(_)` literals. This is the entirety of the equality
stratum's ordering in B2.

### 2.5 General `Eq`/`Fact` (positional paramodulation) — deferred to B3

Sequoia's full `Eq` (general subterm position `s₂[t₁]ₚ`) and `Fact` (equality
factoring `Γ → Δ ∨ s≈t ∨ s≈t′ ⟹ Γ → Δ ∨ t≉t′ ∨ s≈t′`) are needed only when terms
are **non-flat** (nested function terms) or relate to **constants** — i.e. when
inverse-role back-edges (B3) or nominals (B4) make a term's identity reachable
through multiple positions. **In pure ALCHQ they are not exercised:** every
equality is a root-position `Eq`/`Neq` between two depth-1 sibling successors of
one context, discharged by speculative merge (§2.3) + same-pair clash (§2.4).
Deferring them shrinks the B2 surface to exactly: `r≤` (choose) + speculative
merge + same-pair clash. The `HeadLit::Eq`/`Neq` representation is chosen so B3/B4
can add the positional rules without touching B2 (§6).

---

## 3. The ordering reconciliation (the crux)

### 3.1 Two orderings, two rule families (stratification)

| Stratum | Literals | Rules | Ordering | Source |
|---|---|---|---|---|
| **Concept** | `Concept(B)`, `Concept(∃R.B)`, `Concept(∀R.B)` | `Hyper`, `Succ`, `∀-prop`, back-prop | **UNORDERED** (Slice-0 mandate) | rustdl B1 |
| **Equality** | `Eq(s,t)`, `Neq(s,t)` | `r≤`, `Eq`, `Ineq`, `Fact` | **term order on `TermId`** (union-find canonicalization) | Sequoia Tbl 2 |

The paper's own `Eq`/`Fact` side-conditions (`t ⊁ᵥ s`, on **context a-terms** =
variables/nominals/function-terms) confirm the equality ordering is over *terms*,
not over concept atoms. The two orderings govern disjoint rule sets and disjoint
literal kinds. **`apply_hyper` stays byte-for-byte the B1 unordered version.** We
*add* `apply_at_most`, `apply_eq`, `apply_fact`, `merge_terms`.

### 3.2 Why the stratum cannot leak (the `HeadLit` enum is load-bearing)

If equalities were encoded as `ConceptId`s (e.g. a synthetic "sameAs" atom), the
term ordering would have to be expressed *as* a concept-atom ordering, and `Hyper`
would see ordered equality atoms — re-creating the Slice-0 break (a maximal
equality consequence would never surface as a unit). Keeping `Eq`/`Neq` a
**distinct `HeadLit` variant** that `Hyper` *does not index* is what physically
prevents the leak. `apply_hyper` filters to `HeadLit::Concept(_)` (it already
filters by `is_atomic`); `Eq`/`Neq` are invisible to it.

### 3.3 Why the direct concept read-off stays complete (no reasoning-by-cases regression)

The `r≤` rule emits a disjunctive head `⋁ Eq(·,·)` (plus residual). The *discharge*
of that disjunction into a concept-level consequence happens through TWO sound
paths, both already complete in the unordered B1 engine:

1. **UNSAT discharge (case-exhaustion via speculative merge).** Each `Eq(sᵢ,sⱼ)`
   disjunct spawns a *speculative merge* (§2.3) with residual = the other disjuncts.
   The merge's union-core context derives `⊥` (fixture 48: `A,B` disjoint ⇒ merged
   core `{A,B}` ⊑ ⊥), and `apply_back_prop` reflects the disjunct's residual to `v`.
   Iterating across disjuncts (each new residual feeds the next merge), the residual
   shrinks; when the last disjunct clashes with empty residual, `v ⊑ ⊥`.
   **This is structurally identical to the passing `disjunctive_back_propagation`
   test** (`A⊑∃R.C, C⊑D₁⊔D₂, ∃R.D₁⊑F, ∃R.D₂⊑F ⟹ A⊑F`) with the spawning literal an
   `Eq`-disjunct instead of `∃`, and `F = ⊥`. The equality stratum only *produces*
   the disjunction and *spawns the merges*; the case-combination is the existing
   **unordered** back-prop machinery — no ordered Hyper.
2. **SAT non-discharge.** If at least one speculative merge has a *satisfiable*
   union-core (fixture 46: merged core `{A,B}` satisfiable), that disjunct's branch
   back-propagates nothing, so the `⋁ Eq` residual never empties to `⊥`, and — exactly
   like `disjunction_alone_no_unit_subsumption` — **no spurious subsumption / no `⊥`
   is read off.** The class stays SAT.

So the only *new* completeness obligation is: "every individually-clashing merge
must be discoverable, and a fully-clashing disjunction must back-propagate `⊥`."
That obligation is met by the unordered speculative-merge + back-prop case-reasoning
the engine already has (it is `disjunctive_back_propagation` with `Eq`-disjuncts).
The union-find term ordering only affects *which* term is the representative (a
confluence/efficiency concern), never *whether* a concept consequence surfaces.

**Pass/fail condition (state explicitly).** The negative finding ("ALCHQ needs a
different core") would materialize **iff** some sound ALCHQ inference required
*paramodulating an equality INTO a concept-atom disjunction* — i.e. rewriting a
`Concept(_)` literal using an `Eq`, which would force the term order onto the
concept stratum. **This never happens in ALCHQ:** equalities relate successor
*terms*, and a term's *type* (its concept atoms) is reasoned about in its
union-core context by ordinary Hyper, not by paramodulation into the parent's
concept literals. Concept atoms in B2 are never indexed by a term. ⇒ **the strata
stay decoupled; reconciliation succeeds.** (This breaks with B4 nominals — §6 —
where an equality `x≈o` to a constant *does* rewrite the variable in concept atoms;
that is the front-loaded cost flagged there.)

### 3.4 The one residual risk and its reserve fallback

The risk: Sequoia's *completeness theorem* is proved for the *fully ordered*
calculus (Hyper ordered too). We are running **ordered-equality + unordered-Hyper**,
a combination the paper does not prove. The argument in §3.3 reduces our
completeness to the B1 direct-read-off property (empirically validated by the
differential gate + canaries), not to Sequoia's theorem. This is sound (§4 is
unconditional) and *believed* complete on ALCHQ, but it is a *composition*
argument, not a published guarantee — the same epistemic status as B1 itself
(see the B1 design's "empirical proof is the differential gate").

**Reserve fallback (do NOT build preemptively).** If a measured ALCHQ miss
surfaces that the differential gate attributes to incomplete equality discharge,
the bolt-on is a **per-query refutational mode for `≤n`-bearing classes only**:
seed the negated goal `sub ⊓ ¬sup` and saturate with the *fully ordered* calculus
(Hyper ordered) — Sequoia's published completeness then applies, at per-pair cost,
only for the cardinality-bearing minority. Trigger condition: a `cb-diff`
`only_in_current` (MISS) on a `≤n`-bearing fixture that is not explained by a
B3/B4 construct. This coexists with the ordered equality stratum and needs no
core rewrite of the concept stratum.

---

## 4. FP-soundness (every Tier-2 inference valid in all models)

The sacred invariant: never derive a subsumption/`⊥` that holds only in models
where a *chosen* identification happens (the residual-C false-positive).

### 4.1 `r≤` is a sound clause

`≤n R.C` semantically entails: in any model, an element with ≥ n+1 distinct
`R`-`C`-successors must have two of them equal. The clause
`⋁_{i<j} Eq(sᵢ,sⱼ)` (conditioned on the residual `N` = "these n+1 successors exist")
is therefore valid in **all** models. It is a disjunction, not a commitment — it
asserts *some* pair coincides, identical in logical force to B1's `A ⊑ B ⊔ C`.

### 4.2 The guard is the RESIDUAL, not "unit-only": `⊥` only when the residual empties

The soundness guard is **not** "merge only on a derived unit `Eq`" (that would make
the canonical n ≥ 2 UNSAT case a MISS — see the §8.1 hand-trace). The guard is the
**B1 residual principle**, applied to speculative merges (§2.3):

- A speculative merge for disjunct `Eq(sᵢ,sⱼ)` is spawned with edge-residual
  `res_k = (spawning residual) ⊔ (the OTHER Eq-disjuncts)`. The merge does NOT
  commit the disjunction — every other disjunct stays live inside `res_k`.
- If the merged union-core context derives `⊥`, `apply_back_prop` reflects `res_k`
  to `v` — a **disjunction**, not unconditional `⊥`. Bare `⊥` (true `v ⊑ ⊥`) is
  reflected **only when `res_k` is empty** — i.e. the spawning residual was empty
  AND every other disjunct was already discharged to `⊥` (full case-exhaustion).
  This is the identical guard as B1's residual back-prop landmine ("`⊥` itself only
  when `N` is empty") and B1's `disjunctive_back_propagation` test.
- A speculative merge into a **satisfiable** union-core reads off nothing (no `⊥`
  back-propagates) — so a consistent identification (fixture 46, the §8.1 SAT
  trace) yields no spurious `⊥`/subsumption.

So an FP `v ⊑ ⊥` (or `v ⊑ sup`) is impossible unless the consequence holds under
**every** resolution of **every** equality disjunct — i.e. in all models. The
residual-C false-positive ("holds only in the model where THIS pair is identified")
is structurally excluded: any single merge's `⊥` back-propagates a disjunction that
still offers the *other* identifications; only when ALL identifications clash does
the residual empty to a bare `⊥`. **This makes §4 strictly stronger than a
unit-only rule would: the speculative merge is sound precisely because it
back-propagates conditionally on its residual** (the standard CB disjunctive
back-prop), never unconditionally.

### 4.3 The shared-successor-corruption hazard under core-merge

Merging changes a *term's* `ctx` pointer; it **never mutates a context's `core`,
`clauses`, or another term**. `intern_context(union_core)` is find-or-create:

- If the union-core context already exists, we link to it (its derivations are
  shared, sound — any element with that core has those consequences).
- If new, it is seeded fresh (Init rule) and saturated independently.
- The pre-merge contexts `s.ctx`/`t.ctx` retain every other predecessor/term that
  pointed at them. No predecessor sees a changed core.

This is the B1 `R∀` "find-or-create, never mutate the shared successor" invariant,
reused verbatim. It is what makes the merge sound *and* terminating (§5).

### 4.4 `Eq`/`Ineq`/`Fact` are classical equality inferences

`Eq` (paramodulation), `Ineq` (`t≉t ⟹ delete`), `Fact` (equality factoring) are
the standard superposition equality calculus, sound by the congruence axioms of
`≈`. We use them only on ground depth-1 successor terms (congruence closure),
where soundness is immediate. Quoted shapes (§2.4–2.5) are applied unchanged.

---

## 5. Termination (post-equality)

Three finite populations, each bounded, each monotone:

1. **Contexts** — still interned by core; cores ⊆ `2^(finite atom vocabulary)`.
   Merges only ever produce *union* cores (within the same finite powerset). No
   new atoms are minted at saturation time. ⇒ finitely many contexts (the B1
   bound, unchanged).
2. **Terms** — bounded by `(#contexts) × (#existential/≥n literals in the
   normalized ontology)`: each context mints at most one term per `∃`/`≥n`
   occurrence in its clauses, and `≥n` mints a fixed `n` (n from the syntax).
   `n` is bounded by the largest cardinality constant in the input. Merges set
   `merged_into` (monotone — once set, never unset) and never mint new terms,
   so the live-term count only **decreases** under merge. ⇒ finitely many terms,
   decreasing under merge.
   - **Speculative-merge fan-out (new under §2.3).** One `r≤` obligation at a
     context with `w` live `C`-witnesses and bound `n` spawns up to
     `C(w, n+1) · C(n+1, 2)` speculative merge edges — finite, since `w ≤`
     the per-context term budget (point 2) and `n` is a fixed input constant.
     Each merge edge's target is a **union-core** context (core ⊆ the finite atom
     powerset), so by `by_core` interning only finitely many distinct merge-target
     contexts exist; an edge to an existing one is a no-op (`link`-deduped exactly
     as B1 `link_successor` dedups by `(pred, role, residual)`). So the fan-out is
     a finite, deduplicated set of edges into a finite context set — no regress.
3. **Clauses per context** — the B1 redundancy gate (forward/backward subsumption)
   keeps a subsumption-minimal antichain over a finite literal vocabulary
   (`Concept` literals are finite as in B1; `Eq`/`Neq` literals are over the
   finite live-term set of that context). ⇒ finitely many derivable clauses.

`apply_at_most` re-fires only when a context's live `C`-witness set grows past
`n+1`; merges shrink it; the worklist (`dirty`) drains because every rule either
adds a redundancy-minimal clause, links an existing-by-core context, or reduces
the live-term count — all well-founded. **Worst case stays ExpTime** (ALCHQ
classification is ExpTime-complete; matches Sequoia's "deterministic exponential
time").

The one new hazard vs B1: a speculative merge links to a *union-core* context,
which spawns its own successors → more terms. Bounded because union-core ⊆ finite
powerset (so only finitely many distinct union-core contexts exist), and each
contributes a bounded term budget (point 2). The fan-out per `r≤` is finite and
deduplicated (point 2 sub-bullet). No infinite regress.

---

## 6. Interaction with B3 (inverse roles) and B4 (nominals) — what this front-loads

Adding the equality stratum now is the right investment because B3/B4 are
*equality-centric* and reuse this machinery:

- **B4 nominals ARE equality-to-a-constant.** A nominal `{o}` is a term that is a
  *named constant*; `ObjectHasValue(R,o)` is `∃R.{o}`; nominal cardinality
  (`≤1`-style on a nominal) is forced-unit equality `x≈o`. B4 adds **constant
  terms** to the `Term` population and the `Nom` rule (Sequoia Table 2) — but the
  `Eq`/`Ineq`/`Fact`/merge machinery is *exactly* this design's. **Front-loaded:**
  the entire merge + equality-clash path. **B4-specific new risk (flagged in §3.3):
  the stratum-decoupling breaks** — an equality `x≈o` rewrites the *context
  variable* into concept atoms (a named individual's type is shared), so nominals
  reintroduce paramodulation-into-concept-literals. B4 will need either the
  "single root context per nominal" grouping (Sequoia's approach: ground atoms in
  derived clauses, named individuals share one root context) or the reserve
  refutational mode. **This design does not solve B4's leak; it isolates it** —
  by keeping `Eq`/`Neq` a distinct `HeadLit` variant, B4 can add `Eq(Term, Const)`
  without disturbing B2's term-only stratum, and the leak is localized to the
  nominal-constant arm.
- **B3 inverse roles** create **back-edges** (`R(f(x),x)` / `S(y,x)` atoms): a
  successor term's reasoning can flow back to its parent. Combined with `≤n` on
  `R⁻`, this needs `Pred`/`r-Pred` (Sequoia Table 2) and **nested term positions**
  (the `s₂[t₁]ₚ` general paramodulation position machinery §2.4 deferred in B2).
  **Front-loaded:** the `Term` decoupling (B3 `≤n R⁻.C` also needs distinct
  witnesses) and the equality-clash path. **New for B3:** non-flat terms ⇒ the
  full positional `Eq` rule, and predecessor-directed propagation.

Net: build the term layer + equality stratum once (B2), and B3/B4 add rules
(`Pred`, `Nom`, positional `Eq`) and term *kinds* (constants, back-edge terms)
rather than re-architecting. The `HeadLit` enum's distinct `Eq`/`Neq` variant is
the extension point.

---

## 7. Build sequencing (recommendation)

**Recommended: unified equality-first build.** Tier 1's `≤1`-collapse is the
degenerate forced-unit-equality merge (§2.3), so building an ad-hoc deterministic
`≤1` union *first* and retrofitting general equality *later* would duplicate the
merge path and risk two divergent merge implementations. Instead:

1. **Task 0 (serial, gating): the `model.rs` freeze-break.** Add `TermId`/`Term`,
   the `HeadLit` enum (migrate `DerivedClause.head` to `Vec<HeadLit>`), per-context
   `terms` + `at_most`, and the union-find `merged_into` skeleton. Re-sync the
   parallel-task note in `model.rs`. This is the contract every other task builds
   on; it must land first.
2. **Then fan out (parallel):**
   - **normalize:** admit `≥n`/`≤n` (§2.1), lower to term-minting markers +
     `Neq` seeds + `at_most` records; drop `Min`/`Max` from the gate's reject set.
   - **engine — term layer + merge:** replace `succ` with `terms`; `Succ` mints
     terms (and `≥n` mints n + pairwise `Neq`); implement `merge_terms`
     (find-or-create union-core, repoint, union-find). Delivers Tier 0/1
     deterministically (forced-unit merges).
   - **engine — Tier-2 rules:** `apply_at_most` (`r≤` choose), `apply_eq`
     (`Eq`+`Ineq` clash), `apply_fact`. Layered on the merge path.
   - **tests/canaries:** §8 (negatives-first FP guards).
   - **harness:** extend `cb-diff` to the ALCHQ fixtures (41-49 + new Tier-2 +
     stripped ALCHQ corpus subset).

The dependency edge is Task 0 → everything; engine-merge → engine-Tier2 (Tier-2
rules call `merge_terms`). normalize/tests/harness are independent of each other.

---

## 8. Validation (negatives-first canaries + differential gate)

### 8.1 The fixtures 41-49 do NOT cover genuine Tier-2 — add real ones

Audit of `crates/owl-dl-bench/fixtures`:
- `46_max_one_two_exists_merge_sat` = `≤1` + 2 witnesses → **forced unit** merge
  (Tier-1 degenerate, no undischarged disjunction).
- `48_max_merge_with_disjoint_unsat` = `≤1` + 2 disjoint witnesses → forced unit
  merge → clash (Tier-1 degenerate).
- `47_min_max_conflict_unsat` = `≥2 ⊓ ≤1` → `Neq` (from `≥2`) ∧ forced `Eq` (from
  `≤1`) → clash (Tier-1 + `Ineq`).
- `49_exact_cardinality_sat` = `=2` → `≥2`+`≤2`, no surplus witness → no `r≤`.

**None is `n ≥ 2` with an undischarged disjunction of equalities.** Add the two
THE-Tier-2 canaries (these are the FP guard and the case-exhaustion guard):

- **`tier2_at_most_two_three_witnesses_stays_sat` (THE Tier-2 FP guard).**
  `Test ⊑ ≤2 r.⊤ ⊓ ∃r.A ⊓ ∃r.B ⊓ ∃r.D` with `A,B,D` pairwise NON-disjoint and
  distinct cores. **Hand-trace:** `Succ` mints `sA{A}, sB{B}, sD{D}` (residual
  empty); `r≤` (3 witnesses, n=2) emits `Eq(sA,sB) ⊔ Eq(sA,sD) ⊔ Eq(sB,sD)`;
  spawns 3 speculative merges → union-cores `{A,B}, {A,D}, {B,D}`, all **satisfiable**
  (non-disjoint) ⇒ no `⊥` back-propagates ⇒ `Test` derives nothing new ⇒ **SAT, no
  spurious subsumption.** Mirrors `disjunction_alone_no_unit_subsumption`. *If this
  fails (FP `Test ⊑ ⊥`), STOP — a speculative merge back-propagated `⊥`
  unconditionally instead of conditioned on its residual (§4.2).*
- **`tier2_at_most_two_three_pairwise_disjoint_unsat` (case-exhaustion / coexistence
  proof — THE canonical genuine-Tier-2 UNSAT).** Same but `DisjointClasses(A,B,D)`
  pairwise. **Hand-trace** (this is the trace the unit-only restriction failed):
  `r≤` emits `Eq(sA,sB) ⊔ Eq(sA,sD) ⊔ Eq(sB,sD)`; spawn 3 speculative merges with
  residuals = the other two disjuncts:
  - merge `sA,sB` (res `Eq(sA,sD)⊔Eq(sB,sD)`) → core `{A,B}⊑⊥` → back-prop residual
    `Eq(sA,sD)⊔Eq(sB,sD)` to `Test`.
  - merge `sA,sD` (res `Eq(sB,sD)`) → core `{A,D}⊑⊥` → back-prop `Eq(sB,sD)`.
  - merge `sB,sD` (res empty) → core `{B,D}⊑⊥` → back-prop **empty = `⊥`** ⇒ `Test`
    UNSAT. ✓ (Equivalently: the back-propagated residuals resolve against each other
    via unordered Hyper/back-prop down to the empty clause — the
    `disjunctive_back_propagation` mechanism.) The equality analog of
    `disjunctive_subsumption_by_cases` with `D = ⊥`; **passing it empirically proves
    ordered-equality + unordered-Hyper coexist (§0/§3.3) and that the merge is NOT
    unit-only.**
- **`tier2_at_most_three_two_distinct_pairs_stays_sat`.** `≤3 r.⊤` + 3 witnesses
  (≤ n, no surplus) ⇒ no `r≤` obligation ⇒ SAT (boundary: obligation fires only at
  n+1 witnesses).
- **`tier2_merge_then_forall_clash_unsat`.** `≤1 r.⊤` + `∃r.A` + `∃r.B` +
  `A ⊓ B ⊑ ⊥` (disjoint via subclass, not `DisjointClasses`) ⇒ forced merge core
  `{A,B}` ⊑ ⊥ ⇒ UNSAT (exercises union-core derivation + back-prop, not just the
  clique clash).
- **`tier2_exact_cardinality_consistent_sat`** (fixture 49 as a unit test): `=2 r.A`
  satisfiable, no merge forced.

Plus the existing 41-49 as Tier-0/1 regression and `Ineq`/`Neq` unit tests:
`neq_meets_forced_eq_is_bot`, `at_most_retracts_when_witness_merges_away`.

### 8.2 Differential gate

- **bench fixtures 41-49** via `owl-dl-bench cb-diff`: each must match the
  hybrid-engine verdict (SAT/UNSAT per filename). FP=0 (`only_in_cb=[]`) is the
  hard gate; MISS (`only_in_current`) flags incomplete equality discharge → check
  §3.4 reserve trigger.
- **Stripped ALCHQ fixture**: a `≤n`/`≥n`-bearing subset of `shoiq-knowledge` or
  `wine` (the B2-target ontologies per the B1 design table) with inverse/nominal/
  datatype axioms stripped, so it is pure ALCHQ. `cb-diff identical:true` against
  the hybrid is the corpus-scale FP=0/MISSED=0 gate. (Wine's residual MISSes were
  nominal+cardinality; the stripped ALCHQ subset isolates the cardinality half.)
- **Synthetic 15-class ALCHQ gate** (extend the B1 synthetic gate with `≤n`/`≥n`):
  `cb-diff identical:true`.
- `cargo fmt --check` + `clippy -p owl-dl-cb -- -D warnings` clean.
- Independent opus review of §4 (FP-impossibility) and §3.3 (no reasoning-by-cases
  regression) before merge — the two soundness/completeness arguments.

**Stop conditions.** Any `only_in_cb` (FP) on any fixture ⇒ STOP, a speculative
merge back-propagated `⊥`/a consequence with a residual not carried (so it fired
unconditionally instead of conditioned on the other disjuncts — §4.2). Any
`only_in_current` (MISS) on a `≤n`-bearing pure-ALCHQ fixture not explained by
B3/B4 ⇒ the equality-discharge completeness argument (§3.3) has a gap → invoke the
§3.4 reserve refutational mode for that class, do NOT relax the residual guard (§4.2).

---

## Sources

- Tena-Cucala, Cuenca-Grau, Horrocks. *Consequence-based Reasoning for Description
  Logics with Disjunction, Inverse Roles, Number Restrictions, and Nominals* (Sequoia
  full calculus, ALCHOIQ). [arXiv:1805.01396](https://arxiv.org/abs/1805.01396);
  AIJ 298:103518, 2021 ([ScienceDirect](https://www.sciencedirect.com/science/article/pii/S0004370221000692)).
  Rule names + shapes (`Hyper`, `Eq`, `Ineq`, `Fact`, `Succ`, `Pred`, `Nom`; DL4
  at-most encoding; Definition 3 context order; `Hyper` side-condition `Δᵢ ⊁ᵥ Aᵢσ`)
  extracted via [ar5iv HTML render](https://ar5iv.labs.arxiv.org/abs/1805.01396).
- Bate, Motik, Cuenca-Grau, Simančík, Horrocks. *Extending Consequence-Based
  Reasoning to SRIQ.* KR 2016. [arXiv:1602.04498](https://arxiv.org/abs/1602.04498).
- Simančík, Kazakov, Horrocks. *Consequence-Based Reasoning beyond Horn Ontologies.*
  IJCAI 2011 (the unordered-Hyper / direct-read-off basis the B1 engine implements).
- rustdl internal: `docs/superpowers/specs/2026-06-16-cb-ordered-resolution-design.md`
  (Slice-0 falsification: ordered Hyper breaks the direct read-off — the constraint
  this design respects by keeping the concept stratum unordered);
  `docs/superpowers/specs/2026-06-15-cb-engine-b1-alch-design.md` (B1 + slice table).

---

# 9. REVISION — post independent adversarial review (2026-06-16) — AUTHORITATIVE

An independent adversarial review attacked this design on FP-soundness, termination,
stratification, and re-derived both headline traces. **Verdict: FP-soundness SOUND
(the sacred invariant holds, §4 confirmed against the real `apply_back_prop`); but a
COMPLETENESS-LEAK as-specified** — the discharge mechanism was under-specified, so the
canonical UNSAT canary is MISSED under a literal reading of §2.2/§2.3. This section
SUPERSEDES the affected parts of §2.2/§2.3 (discharge), §3.3-point-1 (wording),
§8.1 (the UNSAT trace), §5 (termination scope), and the §1.4 `succ`→`terms` interop.
**Soundness is unchanged; this is a discharge-rule specification fix, not a redesign.**

## 9.1 The bug (as-specified)

§2.2/§2.3 spawn speculative merges only from the *syntactic* `r≤` clause. The
*reflected* clauses that back-prop produces (`v` gains `res_k = the other Eq-disjuncts`)
are all-positive `Eq`-disjunctions. Nothing consumes them: `apply_hyper` filters to
`Concept(_)` (§3.2), `apply_at_most` fires on `(n,R,C)`+witnesses, `apply_back_prop`
only reflects on successor-⊥. So they sit **inert**; no empty clause is derived; the
class is wrongly reported SAT. (All-positive `Eq`-clauses are propositionally
satisfiable — set every `Eq` true — so no resolution path refutes them either.)
§8.1's "merge sB,sD (res empty)" and §3.3-point-1's "resolve via unordered Hyper" are
both wrong: the empty residual only arises after *recursive* re-spawning, and Hyper
never sees `Eq`.

## 9.2 The fix: the Eq-disjunction discharge rule (generalizes §2.3, `apply_eq_discharge`)

> **Rule (Eq-disjunction discharge).** For ANY derived clause stored at context `v`
> whose head `H` contains ≥1 `Eq` literal, let `E(H) = { Eq-literals of H }` and for
> each `e = Eq(s,t) ∈ E(H)` (with `s,t` union-find representatives, `s≠t`) spawn one
> speculative `merge_terms(v, s, t, res)` with **`res = H \ {e}`** (the clause minus
> that one `Eq` literal — keeps the spawning residual AND every other disjunct, `Eq`
> or `Concept`, live). Dedup merge edges by `(v, union_core, res)` (extends B1
> `link_successor`'s `(pred,role,residual)` dedup).

This fires uniformly on the `r≤` output (a clause with empty non-`Eq` part) **and** on
every reflected `Eq`-bearing clause — that recursion is the discharge. §2.3 is the
`n+1`-disjunct instance; §2.2 produces the seed clause; nothing else changes.
`merge_terms` and the §4.2 residual guard are unchanged. Reflexive `Eq(s,s)` (after
union-find canonicalization) is dropped on insert; `Eq(s,t)` co-present with `Neq(s,t)`
on a branch is the §2.4 same-pair clash.

## 9.3 Soundness of the recursive rule (still no FP)

Unchanged from §4.2, because every spawn is the *same* speculative-merge-with-residual:
- A merge for `e=Eq(s,t)` with residual `res = H\{e}` reflects `res` to `v` **only** if
  the union-core `core(s.ctx)∪core(t.ctx)` derives `⊥` (sound: that core is a subset of
  the true merged element's type, so its `⊥` is real). The reflection is the B1 residual
  back-prop — a disjunction, never an unconditional commitment.
- **Bare `⊥` at `v` ⟺ some derived clause has head `{Eq(s,t)}` (a *unit* Eq, residual
  empty) AND that merge clashes** ⟺ every `Eq` disjunct of the original `r≤` clause has
  been case-eliminated to `⊥` (full case-exhaustion) ⟺ the pigeonhole consequence holds
  in **all** models. The residual-C false-positive ("holds only where THIS pair merges")
  is structurally excluded: a single merge's `⊥` reflects a clause that still offers the
  other identifications.

So §4's FP-impossibility is preserved verbatim — the recursion only adds *more sound
disjunctive back-prop*, never an unconditional merge.

## 9.4 Corrected UNSAT trace (`tier2_at_most_two_three_pairwise_disjoint_unsat`)

`Test ⊑ ≤2 r.⊤ ⊓ ∃r.A ⊓ ∃r.B ⊓ ∃r.D`, `DisjointClasses(A,B,D)`:
- **R1:** `Succ` mints `sA{A},sB{B},sD{D}`; `apply_at_most` (3 witnesses, n=2) derives
  `r≤` clause `{Eq_AB, Eq_AD, Eq_BD}`. Discharge rule spawns 3 merges:
  `{A,B}⊑⊥`→reflect `{Eq_AD,Eq_BD}`; `{A,D}⊑⊥`→reflect `{Eq_AB,Eq_BD}`;
  `{B,D}⊑⊥`→reflect `{Eq_AB,Eq_AD}`.
- **R2:** discharge fires on each reflected 2-literal clause. E.g. `{Eq_AD,Eq_BD}`
  spawns merge(A,D) res `{Eq_BD}` (→reflect `{Eq_BD}`) and merge(B,D) res `{Eq_AD}`
  (→reflect `{Eq_AD}`). The three clauses jointly reflect the units `{Eq_AB}`,
  `{Eq_AD}`, `{Eq_BD}` (forward subsumption keeps the units, drops the 2-literal
  superset clauses).
- **R3:** discharge on unit `{Eq_BD}` spawns merge(B,D) with **res `{}` (empty)** →
  `{B,D}⊑⊥` → reflect **empty clause = `⊥`** ⇒ `Test` UNSAT. ✓

Well-founded: each round strictly decreases the `Eq`-disjunct count of the reflected
clause (3→2→1→0); the redundancy gate collapses superset clauses; merges dedup. This
is the equality analog of `disjunctive_subsumption_by_cases` with `D=⊥`, now via the
explicit discharge rule (not "unordered Hyper", which cannot see `Eq`). The SAT canary
(§8.1, non-disjoint cores) is unchanged: every union-core is satisfiable ⇒ no merge
reflects ⊥ ⇒ no clause ever reaches an empty residual ⇒ SAT, no FP.

## 9.5 §3.3 wording correction

The equality disjunction is discharged by the **§9.2 recursive Eq-disjunction discharge
rule** (speculative merge per `Eq`-disjunct, residual = the rest), NOT by `apply_hyper`
(which never indexes `Eq`, §3.2). The completeness reduction still holds: the discharge
is sound disjunctive back-prop over the equality stratum, structurally identical to the
B1 `disjunctive_back_propagation` mechanism but operating on `Eq` literals via
`merge_terms` rather than on `∃` literals via `link_successor`. The stratification claim
(§3.4 / point 4 of the review: no ALCHQ inference paramodulates an `Eq` into a
`Concept` disjunction) is CONFIRMED by the review and unchanged.

## 9.6 Termination (extends §5 to the recursive rule)

The recursive discharge terminates: (1) each spawned `res` has strictly fewer `Eq`
literals than the clause it came from (monotone decrease, base case = empty residual);
(2) reflected clauses live in the powerset of the finite live-`Eq`-pair set of `v`
(finite — `Eq` pairs range over `v`'s bounded live-term population, §5 point 2), and the
redundancy gate keeps a subsumption-minimal antichain; (3) merge edges dedup by
`(v, union_core, res)` over the finite (context × residual) space. No merge↔re-fire
oscillation (`merged_into` monotone; antichain gate). Worst case stays ExpTime.

## 9.7 `succ`→`terms` / merge-edge interop with the structural rules (was under-specified)

The B1 `Context.succ: Vec<(Role,ContextId)>` and `preds` carried Succ edges; B2 must
route BOTH term-edges and merge-edges through the same predecessor structure so the
existing structural rules see them uniformly:
- **`preds[u]`** holds `(parent, edge_kind, residual)` for every edge into `u`, where
  `edge_kind` is a `Succ` term-edge (role `R`) or a `Merge` edge (from §9.2). `Term`'s
  `(ctx, role, residual)` and each merge's `(union_core ctx, residual)` both register here.
- **`apply_back_prop`** is unchanged in logic: when `u` derives `⊥`, reflect each
  `preds[u]` entry's residual to its parent. Merge-edges participate identically (this is
  how §9.4's reflections reach `Test`).
- **`apply_succ_and_forall` (R∀)**: a `∀S.D` clause at `v` augments the core of `v`'s
  edge-targets where the edge role `R ⊑* S`. **Term-edges** carry role `R` (use it).
  **Merge-edges** have no single role — a merged term `u'` represents a witness reachable
  by (≥1 of) the merged terms' roles; R∀ augments `u'` for `∀S.D` iff *some* merged term's
  role `R ⊑* S` (sound: the merged element IS an `R`-successor). Conservative when unsure
  ⇒ MISS not FP. (B2's pure-ALCHQ merges are between same-role siblings, so the merged
  role is well-defined; the multi-role subtlety only bites at B3 inverse interplay.)
- **`preds_of_v_edges` / live-term iteration**: `v`'s outgoing edges = its live terms
  (`merged_into==None`) as `(role, ctx, residual)` + its merge-edges. `apply_at_most`'s
  witness set `W` iterates live terms only.

This interop is a Task-0 (freeze-break) deliverable: the `preds`/edge representation must
be designed to carry both edge kinds before normalize/engine fan out.

## 9.8 Build-gate consequence

The discharge rule (§9.2) and the corrected UNSAT trace (§9.4) are the authoritative
build target. The two THE-Tier-2 canaries (`..._stays_sat` FP guard, `..._pairwise_
disjoint_unsat` case-exhaustion) remain the headline acceptance tests — the UNSAT one
now passes via the recursive discharge. Independent opus re-review of §9.2–9.4 (the
recursive rule's soundness + the corrected trace) is folded into the pre-merge review
already required by §8.2.

---

# 10. CLOSURE — general `Eq/Neq` resolution (closes the cardinality pigeonhole) — AUTHORITATIVE

The first B2 build (commits `3dde6b8`..`89d4212`) shipped sound (FP=0) but with the
same-pair `Eq/Neq` clash implemented **unit-only**, so it MISSES `≥n R.A ⊓ ≤m R.A`
with `n>m≥2` (the cardinality **pigeonhole**, core ALCHQ) and the residual-conditioned
variant. The two MISSes are pinned by `#[ignore]`d canaries
`tier2_min3_max2_pigeonhole_unsat_missed` (cb_tier2.rs:57) and
`tier2_residual_conditioned_neq_eq_clash_missed` (cb_tier2.rs:208). This section is the
authoritative closure for full ALCHQ completeness.

## 10.1 The rule (`apply_eq_resolution`)

> **General `Eq/Neq` resolution.** For any two stored clauses at a context `v`,
> `C₁ = R ⊔ Eq(s,t)` and `C₂ = R′ ⊔ Neq(s,t)` where `(s,t)` are the **union-find
> representatives** of the same pair (`find(s),find(t)`, canonicalized `min/max` as in
> `add_clause`), derive `R ⊔ R′` via `add_clause(v, …)`. `R`, `R′` are the rest of each
> clause (any mix of `Concept`/`Eq`/`Neq` literals; possibly empty).

- The **unit case** `R=R′=∅` is the existing same-pair clash → `⊥`. This rule strictly
  generalizes it; keep the unit clash or subsume it under this rule.
- Lives **entirely in the equality stratum**: it consumes/produces `HeadLit` clauses but
  the *resolved literal* is the `Eq`/`Neq` pair — `apply_hyper` (concept resolution) is
  untouched and stays unordered. No new ordering on concept atoms.
- Route results through `add_clause` (tautology + forward/backward subsumption +
  union-find canonicalization) exactly like every other derived clause. Add
  `apply_eq_resolution(v)` to the per-context `process(v)` saturation step.

## 10.2 Soundness (FP-safe — standard binary resolution)

`Eq(s,t)` ≙ `s=t`, `Neq(s,t)` ≙ `s≠t` are complementary. From `core ⊑ R∨(s=t)` and
`core ⊑ R′∨(s≠t)`: in any model of `core`, `(s=t)` is true or false; if true the second
clause forces `R′`, if false the first forces `R`; either way `R∨R′` holds. So `R⊔R′` is
valid in **all** models ⇒ never an FP. This is plain resolution on the literal `(s=t)`;
no speculative state, no chosen identification (it resolves *derived sound clauses* — the
`r≤` clause §4.1 and the `≥n` `Neq` facts §2.1 are both sound). **It is cleaner than and
independent of the speculative-merge path (§9): that path handles distinctness arising
from a clashing union-core; this rule handles distinctness arising from an explicit
`Neq` (from `≥n`).** Both coexist.

## 10.3 Pigeonhole trace (`≥3 r.A ⊓ ≤2 r.A`)

`≥3` mints `sA1,sA2,sA3` (core `{A}`) + unit `Neq` clauses `{Neq12},{Neq13},{Neq23}`.
`≤2` (3 witnesses) ⇒ `r≤` clause `{Eq12,Eq13,Eq23}`. Resolve with `{Neq12}` → `{Eq13,Eq23}`;
with `{Neq13}` → `{Eq23}`; with `{Neq23}` → `{}` = `⊥`. UNSAT ✓. Residual-conditioned
`(≥2 r.A ⊔ E) ⊓ ≤1 r.A`: the `≥2` `Neq` and the `≤1` `Eq` both carry residual `E`;
`{Eq12 ⊔ E}` resolves with `{Neq12 ⊔ E}` → `{E}` ⇒ `C ⊑ E` ✓.

## 10.4 Termination

`Eq/Neq` literals range over the finite live-term-pair set of `v`; resolution produces
clauses over the same finite `HeadLit` vocabulary; the redundancy gate keeps a
subsumption-minimal antichain; `merged_into` monotone. Bounded, ExpTime worst case
unchanged.

## 10.5 Acceptance (closure gate)

- **Un-ignore** `tier2_min3_max2_pigeonhole_unsat_missed` + `tier2_residual_conditioned_
  neq_eq_clash_missed` → both PASS.
- **New FP-guard canaries:** `Eq`/`Neq` on DIFFERENT pairs must NOT resolve (no spurious
  `⊥`); `≥2 r.A ⊓ ≤2 r.A` (no pigeonhole, `n≤m`) stays SAT.
- All prior 64 active tests still green; the §8.1 FP-guard SAT canary still SAT.
- bibtex + fixtures 41–49 `cb-diff`: `only_in_cb=0` (FP=0 hard gate) preserved.
- clippy `-D warnings` + fmt clean. Independent adversarial re-check of §10.2 (no FP) +
  termination before merge.
