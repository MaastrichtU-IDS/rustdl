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

## Root cause in rustdl — VERIFIED (two-engine gap, not a missing calculus)

This is NOT "rustdl needs a new materialization engine" — rustdl already HAS a
complete materialization tableau (it generates ∃-successors, applies role chains,
does functional merge). The gap is that family falls between rustdl's two engines,
each missing one of the two things Konclude has (chains AND efficient blocking):

| engine | role chains (RIAs) | blocking | behaviour on family |
|---|---|---|---|
| **Wedge** (the consistency path, `hyper.rs`) | **DROPPED** at clausify (`clause.rs:317-321`, `_ => {}`, deferred "HF3") | **anywhere** | terminates ~20 s but returns **consistent** (misses the chain-dependent clash) — a sound under-approximation |
| **Main tableau** (`lib.rs:765 is_blocked`) | 2-leg via `apply_role_chains` (rules.rs:1142) | **ancestor-only** | would detect, but **hangs** (graph blow-up on the 1848-individual generative ABox) |
| **Konclude** | yes | anywhere/pairwise | **~1 s, correct** |

The wedge can't catch it because it never sees `isMalePartnerIn∘hasFemalePartner⊑hasWife`
(RIA dropped). The main tableau could catch it but doesn't terminate, because
ancestor-only blocking lets the ∃-generation explode the completion graph
(`tableau-memory-fanout`: ancestor-only → huge graphs).

## The two real levers (both core-engine projects, broad payoff, real FP risk)

1. **Role-chains in the wedge (deferred "HF3").** The wedge already has
   anywhere-blocking + ∃-generation + functional merge; it only lacks RIA support.
   Hard part: the hyperresolution engine must **derive role EDGES** via clauses
   (`R₁(x,y)∧R₂(y,z) → R₃(x,z)` — a role-atom HEAD, a clause shape the clausifier
   does not currently emit). Sound (additive role facts); broadly useful (chains
   are ubiquitous). Would let the fast engine detect family.
2. **Anywhere/pairwise blocking in the main tableau.** Soundness-DELICATE with
   inverse roles + qualified cardinality — which is exactly why rustdl uses the
   conservative ancestor-only blocking (anywhere blocking is not unconditionally
   sound in SROIQ). Touches the hot loop. Larger, higher-risk.

Either would close family; neither is a pre-check. Their real justification is
GENERAL performance/completeness (every chain-heavy or large-ABox workload), not
these 2 fixtures.

## Disposition

family / family-stripped remain a **sound MISS** (rustdl returns `consistent`;
HermiT/Konclude inconsistent). This is the SAFE direction (false-*consistent*, never
false-*inconsistent*); classification stays FP=0/MISSED=0. Closing it is a major
core-engine investment (lever 1 = role-chains-in-the-wedge / "HF3", or lever 2 =
anywhere/pairwise blocking in the main tableau) whose real justification is general
performance/completeness, not these 2 fixtures — **deferred**. The 4-assertion core
above is the validation target if/when either lever is taken on.
