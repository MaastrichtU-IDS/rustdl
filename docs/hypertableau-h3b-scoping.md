# Hypertableau H3b — ¬sup expansion + negative literals (scoping)

Drafted 2026-05-27. Scopes the next slice of H3, targeting the
**VegetarianPizza family** of pizza misses. See
[`hypertableau-scoping.md`](hypertableau-scoping.md) §H2c/§H3a for the
measurement context and the soundness asymmetry of the probe.

## The problem

After H3a, 77 pizza subsumptions remain missed (vs Konclude's
closure). Categorised by the superclass not derived, and by the
*mechanism* each needs:

| superclass(es) | misses | mechanism | phase |
|---|---|---|---|
| VegetarianPizza, NonVegetarianPizza, VegetarianPizzaEquivalent1/2, ThinAndCrispyPizza | **50** | antecedent `∀`/`¬` (negation in deriving sup) | **H3b (this doc)** |
| InterestingPizza | 20 | `≥3 hasTopping` min-cardinality | H3c |
| SpicyPizzaEquivalent | 5 | two-role-chain antecedent body | engine matching (separate) |
| RealItalianPizza | 2 | `hasValue` nominal | nominals (separate) |

The H3b family all share a shape that makes the **current
subsumption test the bottleneck, not the clausifier**:

- `VegetarianPizza ≡ Pizza ⊓ ¬∃hT.Fish ⊓ ¬∃hT.Meat`
- `NonVegetarianPizza ≡ Pizza ⊓ ¬VegetarianPizza`
- `VegetarianPizzaEquivalent1 ≡ Pizza ⊓ ∀hT.VegetarianTopping`
- `ThinAndCrispyPizza ≡ Pizza ⊓ ∀hasBase.ThinAndCrispyBase`

The probe (`hyper_subsumption_probe`) tests `sub ⊑ sup` by seeding a
fresh `Q` with `Q → sub` and **`Q ∧ sup → ⊥`** — which forces the
engine to *derive `sup` positively*. For these `sup`s that means
deriving a universal (`∀hT.¬Fish`) or a negation (`¬VegPizza`) as a
positive fact about the root — the genuinely hard direction, which an
open-world tableau cannot do cheaply.

## The approach — expand `¬sup`, do not derive `sup`

`sub ⊑ sup` iff `sub ⊓ ¬sup` is unsatisfiable. Instead of forcing
`sup` and clashing, **assert the NNF of `¬sup` at the root and let the
engine refute it** with machinery it already has (∃-generation,
disjunctive ∀-propagation, disjointness). Worked example, `Margherita
⊑ VegetarianPizza`:

```
¬VegetarianPizza  =  ¬Pizza  ⊔  ∃hT.Fish  ⊔  ∃hT.Meat   (NNF)
```

- `¬Pizza` branch: root is `Margherita ⊑ Pizza` (told) → clash with
  `¬Pizza` (needs negative literals — below).
- `∃hT.Fish` branch: generate an `hT`-successor labelled `Fish`.
  Margherita's `⊑ ∀hT.(Mozzarella ⊔ Tomato)` closure (a consequent-`∀`
  clause the clausifier already emits) fires on it → the successor
  branches `Mozz ⊔ Tomato`, and `Fish ⊓ Mozz → ⊥`, `Fish ⊓ Tomato → ⊥`
  (disjointness) → clash.
- `∃hT.Meat` branch: symmetric.

All branches clash ⇒ `sub ⊓ ¬sup` unsat ⇒ subsumption holds. **No new
engine search machinery** — only a new injection encoding and negative
literals.

## §1 — Q-gated `¬sup` encoding (with soundness sketch)

Replace `Q ∧ sup → ⊥` with a **single disjunctive-head clause gated on
`Q`**:

```
Q(x)  →  d1(x) ⊔ d2(x) ⊔ … ⊔ dk(x)
```

where `d1..dk` are the head atoms of `NNF(¬sup)`'s top-level
disjunction (§3). `Q → sub` is unchanged.

**Why gate on `Q`.** The `¬sup` constraint must hold *only at the root
individual being tested*, never at generated successors. If `¬sup`
fired at every node, a generated `Fish`-topping successor would also be
required to satisfy `¬sup` — wrong; successors are not `sub`. Because
`Q` is fresh and produced by no other clause, `Q(x)` holds only at the
root, so the body `Q(x)` confines the disjunction to it.

**Soundness sketch.** The root node carries `Q`, hence (via `Q→sub`)
everything `sub` entails, *and* (via the gated clause) one disjunct of
`¬sup`. That is exactly a model-candidate for `sub ⊓ ¬sup`. The engine
explores all disjuncts (H2 branching) and all forced consequences; it
reports `Unsat` only if *every* branch clashes — i.e. `sub ⊓ ¬sup` has
no completion. Since dropping deferred axioms only removes constraints
(`Models(full) ⊆ Models(fragment)`), `Unsat` over the fragment implies
`Unsat` over the full ontology, so the derived subsumption is sound
(the §H2c asymmetry, unchanged). The current `Q ∧ sup → ⊥` is the
special case `sup` atomic, where `NNF(¬sup)` is the single literal
`¬sup` and the gated clause is `Q → sup̄`, with `sup ⊓ sup̄ → ⊥`.

## §2 — Negative literals (minimal)

`NNF(¬sup)` and antecedent `¬atomic` produce negative literals
(`¬Pizza`, `¬VegetarianTopping`). Represent them with **complement
classes, introduced only for atoms that actually appear under a `Not`
in some emitted clause body or head** (not one per vocabulary class):

- For each such atomic `A`, allocate a fresh complement id `Ā` and emit
  one clash clause `A(x) ⊓ Ā(x) → ⊥`.
- `¬A` anywhere in the encoding becomes the atom `Ā`.
- The engine treats `Ā` as an ordinary class label — **no engine
  change**; the clash is just another ⊥-headed clause.

This is sound for *refutation* (the only direction the probe uses): we
*assert* `Ā` and clash if `A` is derived. We never derive `Ā` from the
absence of `A` (open-world), which would be unsound — and we don't need
to. Clause count stays proportional to negation use.

## §3 — `NNF(concept) → disjunctive head atoms`

A small function dual to H3a's `encode_antecedent`, on the head side.
Scope it to exactly the shapes `¬sup` produces on the corpus (verified
against the H3b family): top-level `Or` of disjuncts, each disjunct one
of `atomic` → `Class(A)`, `¬atomic` → `Class(Ā)`, `∃R.atomic` →
`Exists(R,A)`, `∃R.¬atomic` → `Exists(R,Ā)` (the
`VegetarianPizzaEquivalent` shape `∃hT.¬VegTopping`). Anything outside
this set (nested deeper, cardinality, nominal) **defers the pair**
(counted), exactly as elsewhere — never silently dropped.

## §4 — Validation plan & result (shipped)

Re-run `hyper-classify-probe pizza.ofn --dump-subsumptions`; diff vs
Konclude closure (the §H2c methodology).

**Result:** misses **77 → 29** (48 unlocked — the entire
antecedent-`∀`/`¬` family), **0 false positives**, completeness
**89 % → 95.8 %**. `pairs_via_expansion = 490` (the rest used the bare
complement fallback), 5 complement classes introduced.

One mid-flight correction (diagnosed before shipping, per the H3a
discipline): the first cut dropped only 37, not ~50. The shortfall was
`VegetarianPizzaEquivalent2 ≡ Pizza ⊓ ∀hT.(Cheese ⊔ … ⊔ Veg)`, whose
`¬` produces `∃hT.(¬Cheese ⊓ … ⊓ ¬Veg)` — an `∃` over a *conjunction of
negated literals*, outside the original §3 set (it fell back, soundly).
Adding a structural name `N ⊑ ⊓literals` for the `∃`-inner (§3
extension) unlocked its 11.

**Residual 29 (the genuinely hard set):**
- InterestingPizza (20) — min-cardinality `≥3 hasTopping` (H3c).
- SpicyPizzaEquivalent (5) — two-role-chain body, engine `match_body`
  single-role limit (§5, separate).
- RealItalianPizza (2) + the two pizzas reaching ThinAndCrispyPizza
  *transitively through* RealItalianPizza (Napoletana, Veneziana) —
  all blocked by the `hasValue` nominal (4 total, nominals phase).

## §5 — Explicitly out of scope

- **Deriving universals positively** (`∀hT.¬Fish(x)` as a fact). The
  whole point of §1 is to *avoid* this. Not attempted.
- **Two-role-chain body matching** (SpicyPizzaEquivalent, 5): the
  clausifier emits the clause but `match_body` (hyper.rs:413) rejects a
  second role atom. A separate engine-matching enhancement.
- **Nominals** (`hasValue`, RealItalianPizza, 2) and **cardinality**
  (InterestingPizza, 20): later phases.
- Heuristics, trail, backjumping (the Konclude-gap work) — orthogonal.

## §6 — Note: SpicyPizzaEquivalent is not antecedent-`∀`

It looked like it belonged to this family but its antecedent is
`Pizza ⊓ ∃hT.(PizzaTopping ⊓ ∃hasSpiciness.Hot)` — a **two-role chain**,
not a `∀`/`¬`. The clausifier produces a Horn clause with two role
atoms (`hT(x,y) ∧ hasSpiciness(y,z)`); the engine's single-role
`match_body` never fires it. Categorising its 5 misses under H3b would
have been wrong; they belong to the separate multi-role-matching fix.
