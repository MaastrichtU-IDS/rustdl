# family / family-stripped inconsistency — minimal cause (pinned 2026-06-15)

Oracle-driven (Konclude `consistency -i`) delta-debugging + axiom ablation,
to decide whether a *deterministic ABox saturation* could detect the
inconsistency rustdl currently misses (returns `consistent`, a sound MISS).

## Minimal inconsistent ABox core — 4 assertions (of 1341)

Found by ddmin over the OWL/XML ABox with the full TBox fixed (76 Konclude runs):

```
isBrotherOf(peter_william_bright_1941, david_bright_1934)
isMalePartnerIn(peter_william_bright_1941, m134)
hasFather(robert_david_bright_1965, david_bright_1934)
isBrotherOf(robert_david_bright_1965, richard_john_bright_1962)
```

## Essential TBox machinery (confirmed by ablation on the 4-assertion ontology)

Dropping either of these makes the 4-assertion ontology **consistent**:

- `SubObjectPropertyOf(ObjectPropertyChain(isMalePartnerIn hasFemalePartner) hasWife)`
- `SubClassOf(Marriage, ObjectIntersectionOf(∃hasFemalePartner.Woman, ∃hasMalePartner.Man))`

And (from the corpus-level ablation) `FunctionalObjectProperty(hasSex)` is load-bearing,
while `Sex ≡ Female ⊔ Male` (disjunction) and `Person ⊑ ∃hasFather.Man ⊓ …` are NOT needed.

## Mechanism (the decisive finding)

The clash **requires generating an anonymous ∃-successor**: `m134` is a `Marriage`,
so `Marriage ⊑ ∃hasFemalePartner.Woman` forces an anonymous female partner `w`. The
chain `isMalePartnerIn∘hasFemalePartner ⊑ hasWife` then fires over that *generated*
node, and combined with the sibling/parent chains (`isBrotherOf∘isParentOf ⊑ isUncleOf`,
inverses, `hasFather⊑hasParent`, symmetric/transitive `isSiblingOf`) and functional
roles (`hasSex`, `hasFemalePartner`, `hasMalePartner`) it forces a sex contradiction
(`Male ⊓ Female` under functional `hasSex`, against `DisjointClasses(Female, Male)`).

## Why a "deterministic ABox saturation over named individuals" CANNOT catch it

A substantial hand-rolled deterministic closure over the 1341 named individuals —
2- and 3-leg role chains, role hierarchy, inverses, symmetric/transitive,
functional-role merge, domain/range typing, told class hierarchy, all four disjoint
pairs — produces **zero** clashes and **zero** individuals typed both `Man` and `Woman`.
The ablation explains why: the contradiction depends on the *generated* Marriage
female-partner successor, which named-individual datalog never materialises.

## Consequence for the engine design

Detecting family soundly requires a **deterministic materialization engine** with:
∃-successor generation (at least for the participating existentials), role-chain
application over generated nodes, functional-role merge, and blocking for termination.
That is essentially a deterministic (non-branching) tableau. It is *not* the cheap
ABox datalog the "deterministic saturation" option implied, and it faces the same
graph-scale challenge that makes the full tableau's `decide(Top)` hang on this 1848-
individual ABox (the non-branching property avoids disjunctive blow-up, but not the
graph-size blow-up; termination hinges on good — e.g. anywhere — blocking).

## Disposition

family / family-stripped remain a **sound MISS** (rustdl returns `consistent`;
HermiT/Konclude inconsistent). This is the SAFE direction (false-*consistent*, never
false-*inconsistent*); classification stays FP=0/MISSED=0. Closing it is a major
engine investment for 2 fixtures — deferred unless a broader workload justifies a
deterministic-materialization / better-blocking engine. The 4-assertion core above is
the validation target if/when that work is taken on.
