# Conjunctive-unsat in the saturator + RHS-negation canonicalization

**Date:** 2026-07-29
**Status:** Design — approved for implementation planning
**Branch:** to be cut from `main` (the CB work stays parked on `feat/cb-alch-taming`)

## Why this exists

Two problems with one shared root: rustdl has several syntactic spellings of "X and Y
are disjoint", and the EL saturator supports only one of them, while the fragment gate
admits more than one.

**Problem 1 — a silent-incompleteness bug shipped on `main` (correctness).**
Lever 1b (commit `3e3a731`, 2026-07-20, default-ON) admitted the lowered `⊥` GCI
`X ⊓ Y ⊑ ⊥` to the fragment gate — `is_el_concept` accepts `Bot`, and
`is_saturator_axiom` grew an explicit `X ⊑ ⊥` arm gated on `disjoint_ok`. Its commit
message records that **54 of 359 rustdl-DNF-but-Konclude-OK ORE candidates had this as
their first fragment blocker**, so those ontologies were newly routed onto the
saturation-only fast path.

But the saturator's rule collector never learned the form. Its `And`-LHS arm derives
heads from `atomic_operands_on_right(sup)` plus an existential-RHS scan; with `sup = Bot`
both are empty, so **the axiom is silently dropped**. `rules.directly_unsat`
(`owl-dl-saturation/src/lib.rs:2273`, seeded at the `Atomic ⊑ Bot` check) covers only a
non-conjunctive LHS.

Reproduced (`target/release/rustdl`, 2026-07-29):

```
SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
SubClassOf(:C :A)
SubClassOf(:C :B)
```
`C` is unsatisfiable. rustdl reports **no unsatisfiable classes** and prints
`# fragment: pure-EL (trust_sat sound by construction; saturator alone is complete)`.
The same ontology written `DisjointClasses(:A :B)` correctly reports `unsat :C`.

This is the **D10 unsound-completeness class**: the gate certifies completeness while the
engine drops the axiom. It is a MISS, not an FP — but it is reported as complete, which
CLAUDE.md's soundness contract calls out as the most dangerous failure mode. **134 of
1920 ORE pool ontologies contain the one-line `SubClassOf(ObjectIntersectionOf(…)
owl:Nothing)` spelling.**

**Problem 2 — atomic negation kicks otherwise-EL ontologies onto the O(n²) path
(performance).** `is_el_concept` and `is_saturator_concept` reject `ConceptExpr::Not`
outright, so a single `A ⊑ ¬B` axiom is enough to route a large EL ontology onto the
per-pair hybrid path. Measured on `ore_ont_9318` (39 433 classes, **4** such axioms):

| form | wall | closure |
|---|---|---|
| as-is (`SubClassOf(A, ObjectComplementOf(B))` ×4) | **21.5 s** | 19 479 lines |
| those 4 axioms rewritten to `DisjointClasses` | **0.909 s** | 19 479 lines — **byte-identical** |

24×, from a semantics-preserving 4-axiom change. The strategy review that surfaced this
measured the same signature on ~13 ORE ontologies at 15×–>110×, including
`ore_ont_2397` (DNF >120 s → 1.07 s) and `ore_ont_10032` (DNF >120 s → 2.23 s).
**80 of 1920 ORE pool ontologies** contain a one-line `SubClassOf(… ObjectComplementOf …)`.

Both problems dissolve if "disjointness" has **one** internal form that the engine
actually reasons over, and if the negation spelling is canonicalized into it.

## Scope

**In scope.** Part A: a conjunctive-unsat rule in `owl-dl-saturation`. Part B: a pre-NNF
rewrite of `X ⊑ ¬Y` into `X ⊓ Y ⊑ ⊥` in `owl-dl-core`, plus preserving told-disjoint
coverage across the new form.

**Out of scope.** `EquivalentClasses(A, ¬B)` as a whole axiom — it carries a covering half
(`⊤ ⊑ A ⊔ B`) as well as a disjointness half, so *replacing* it with a disjointness
assertion would lose the covering and re-create a completeness gap. Note the rewrite is
defined on `Axiom::SubClassOf` only, so this is a statement about coverage, not a
soundness carve-out: if the pipeline has already decomposed the equivalence into two GCIs
by the insertion point, rewriting the `A ⊑ ¬B` direction is sound and loses nothing (the
`¬B ⊑ A` direction is retained separately, and that direction's `Not` is on the *left*,
which this rewrite does not touch). Either way the covering survives. The ontology as a
whole stays on the hybrid path because the un-rewritten direction still carries a `Not`.
`DisjointUnion` stays excluded from the gate for the reasons already recorded at
`classify.rs` (it entails a disjunctive covering the saturator has no rule for).
Widening `disjoint_ok` — the disjoint×functional-merge interaction remains unproven and
must keep forcing fallback.

## Part A — conjunctive-unsat rule (correctness)

Add a rule kind beside the existing `ConjunctiveTrigger`
(`owl-dl-saturation/src/lib.rs:2337`):

```rust
struct ConjunctiveTrigger { bodies: Vec<ClassId>, head: ClassId }   // existing
struct ConjunctiveUnsat   { bodies: Vec<ClassId> }                  // new
```

Semantics: when every `b ∈ bodies` is a subsumer of `c`, call the existing
`enqueue_unsat(c)` (`lib.rs:924`). `process_unsat` then propagates unsatisfiability to
subclasses and back through `∃`-facts exactly as it does for `DisjointnessClash` and
`directly_unsat` today — no new propagation machinery.

Conceptually `DisjointnessClash` is the two-atom instance of this rule, but it is
**left untouched** — it is indexed and tuned for the pairwise case. Part A *adds* the
general arm rather than re-routing the existing one, so no currently-working path
changes shape.

Sites to touch, following `ConjunctiveTrigger`'s existing wiring:
- rule collection: the `And`-LHS arm — emit `ConjunctiveUnsat { bodies }` when
  `sup` is `Bot`, instead of falling through to the empty-head drop
- `ElRules` storage (`lib.rs:2223` neighbourhood)
- the dense per-class trigger index (`lib.rs:388`, built at `:479`)
- the incremental rule-addition path (`lib.rs:620-670`)
- consumption (`lib.rs:1101` neighbourhood)
- **axiom provenance** — `directly_unsat` carries `directly_unsat_axiom`
  (`lib.rs:2668`, read at `:776`) so `justify`/`explain` can name the responsible
  axiom. `ConjunctiveUnsat` must carry the same, or newly-derived unsatisfiabilities
  become unexplainable.

Note the `bodies` collection already handles non-atomic `And` operands (an `∃R.C`
operand is lowered to a marker class by the existing arm), so
`∃R.C ⊓ D ⊑ ⊥` is covered by the same rule — not just atomic pairs.

## Part B — RHS-negation canonicalization (performance)

Rewrite every `Axiom::SubClassOf { sub, sup }` whose `sup` is `ConceptExpr::Not(y)` into
`Axiom::SubClassOf { sub: And([sub, y]), sup: Bot }`.

**Conjunctive right-hand sides recurse.** `X ⊑ A ⊓ ¬B` must yield `X ⊑ A` plus
`X ⊓ B ⊑ ⊥`; otherwise the negation survives inside the `And` and the axiom stays
out-of-fragment. So the rewrite either runs after RHS-conjunction splitting or descends
into a top-level `And` on the right itself. Which of the two depends on whether the
pipeline already splits RHS conjunctions at the chosen insertion point; the
implementation must check and pick, and a canary pins the outcome either way.

`X ⊑ ¬Y ≡ X ⊓ Y ⊑ ⊥` is an unconditional logical equivalence, so **no atomicity
restriction is needed**: the rewrite always applies, and the existing fragment gate
decides fragment membership downstream by asking whether `And([X, Y])` is a saturator
concept. Complex `Y` simply yields a GCI the gate rejects, exactly as today.

**Placement: before NNF.** This is the load-bearing and counter-intuitive part. NNF
pushes negation to atomic leaves, so post-NNF `X ⊑ ¬(A ⊓ B)` has *already* become
`X ⊑ ¬A ⊔ ¬B` — an `Or`, and the opportunity is gone. Pre-NNF the same axiom becomes
`X ⊓ A ⊓ B ⊑ ⊥`, fully EL-positive and in-fragment. Likewise `X ⊑ ¬∃R.C` becomes
`X ⊓ ∃R.C ⊑ ⊥` (in-fragment) rather than `X ⊑ ∀R.¬C` (out-of-fragment). So Part B is
strictly more general than rewriting only the atomic-pair case.

**Told-disjoint preservation (obligation, not optional).** `told.rs:128` recognizes
`SubClassOf(A, Not(B))` with both atomic as a told-disjoint pair via `as_not_atomic`
(`told.rs:230`). After Part B that arm stops matching, so told-disjoint coverage would
silently shrink — and the told tables feed the classify tier walk and the tableau.
Part B must extend `told.rs`'s `SubClassOf` arm to recognize `And([A, B]) ⊑ Bot` with
both operands atomic. This *also* picks up natively-written `A ⊓ B ⊑ ⊥`, a pair the
table misses today, so told-disjoint coverage strictly increases.

## Soundness and completeness

**Part B cannot change the entailment set.** It is a logical equivalence, so the only
thing that changes is which engine answers. FP-safety is therefore structural, and the
correctness gate is closure identity.

**Part A only adds genuinely-entailed unsatisfiabilities.** `enqueue_unsat(c)` fires only
when `c` is subsumed by every member of a conjunction the ontology asserts
unsatisfiable — i.e. `c ⊑ b₁ ⊓ … ⊓ bₙ ⊑ ⊥`. It can never produce a subsumption that does
not hold. Its risk is the *opposite* direction: it makes the fast path derive **more**
than before, so on the 54 Lever-1b ontologies output will legitimately change.

**The two parts compose to close the gate/engine mismatch.** After Part A, every form
the gate admits (`Atomic ⊑ ⊥`, `And ⊑ ⊥`, `DisjointClasses`) is a form the engine
reasons over completely; after Part B, the `Not` spelling is canonicalized into one of
them rather than forcing fallback. `disjoint_ok` continues to force hybrid fallback
whenever a functional or inverse-functional role is present, so the unproven
disjoint×functional-merge interaction is untouched.

## Gates and testing

Two different instruments, because the two parts have different expected effects.

**Part A — new entailments ⇒ external oracle, not self-comparison.** Comparing against
previous rustdl output is invalid here: the previous output is the bug.
- **Spelling differential (the direct bug gate):** the same ontology written
  `A ⊓ B ⊑ ⊥` and `DisjointClasses(A B)` must produce identical closures. The engine
  fails this today; it must pass after.
- **Konclude∩HermiT oracle** on the ontologies whose output changes — every newly
  derived `unsat` must be confirmed, FP=0.
- Curated-corpus closure diff: FP=0, and any new MISS is a stop-and-diagnose.

**Part B — routing only ⇒ byte-identity.** All Part B comparisons are run **with Part A
already landed**, so that Part A's new entailments are present on both sides and the only
variable is the rewrite.
- `RUSTDL_NEG_TO_BOT_GCI=0` vs `=1` byte-identical closures across the curated corpus
  and the ORE set. Any diff is a bug, not a tuning matter.
- `ore_ont_9318`: wall drops from ~21.5 s to ~0.9 s, closure identical between flag-OFF
  and flag-ON. (The 19 479-line figure measured on 2026-07-29 predates Part A and is
  recorded as provenance for the *speedup*, not as an expected line count to assert
  against — Part A may legitimately change the closure of any ontology that also carries
  a dropped `⊓ ⊑ ⊥` axiom.)
- Re-measure the ~13 ontologies the strategy review identified, and report how many
  ontologies newly reach the fast path (the Lever 1b commit's `is_pure_el` /
  `tbox_elig` counts are the precedent for how to report this).

**Regression canaries, negatives-first.**
- The exact 3-axiom reproducer above (`A ⊓ B ⊑ ⊥`, `C ⊑ A`, `C ⊑ B` ⟹ `unsat C`) —
  a regression test for a bug that shipped.
- Complex-body variant: `∃R.C ⊓ D ⊑ ⊥` with a class subsumed by both ⟹ unsat.
- `X ⊑ ¬∃R.C` becomes in-fragment and classifies correctly (the pre-NNF placement
  is what this pins; a post-NNF implementation fails it).
- `X ⊑ ¬(A ⊓ B)` becomes in-fragment (same pin, nested case).
- **Must stay green:** `saturator_fragment_rejects_conjunctive_bot_with_functional` —
  `disjoint_ok` must still force fallback when a functional role is present.
- Told-disjoint coverage: `A ⊑ ¬B`, `A ⊓ B ⊑ ⊥`, and `DisjointClasses(A B)` must all
  yield the same told-disjoint pair.
- `justify` on a newly-derived `unsat` names the responsible axiom (the provenance
  obligation in Part A).

## Rollout

**Part A lands first and independently, unflagged.** It is a correctness fix on a
default-ON path; gating a bug fix behind an off-by-default flag would leave the silent
incompleteness shipped. Its own gate (spelling differential + oracle) stands alone.

**Part B follows, default-ON with `RUSTDL_NEG_TO_BOT_GCI=0` to revert** — matching the
`RUSTDL_CLASSIFY_TBOX_FRAGMENT` / Lever 1b pattern for gate-widening levers.

Part B depends on Part A: without the conjunctive-unsat rule, canonicalizing `A ⊑ ¬B`
into `A ⊓ B ⊑ ⊥` would route the axiom to a form the engine drops — converting a slow
but correct answer into a fast wrong one. **Do not land Part B before Part A is green.**

## What this does not claim

- It does not address the ORE DNF tail as a whole. The strategy review measured that
  tail as overwhelmingly out of any EL-adjacent fragment (nominal / cardinality /
  datatype / ABox heavy); this lever addresses ontologies that are *otherwise EL* and
  blocked by a negation or a dropped `⊥` GCI. Honest addressable set: ~13 ontologies
  measured at 15×–>110×, plus up to 134 (`⊓⊑⊥`) / 80 (`⊑¬`) candidates in the ORE pool
  to be enumerated by the gate probe, not by grep — grep ≠ gate, per the Lever 1
  precedent.
- Part A's blast radius is a *correctness* claim, not a perf claim: it may make some
  ontologies slower (more derived unsats) while making them right.
- It says nothing about the consequence-based engine pursuit, which is parked with its
  own findings recorded on `feat/cb-alch-taming`.

## Measured results (2026-07-29)

**Part A — corpus-inert, a correctness fix.**

The five new axiom shapes (`And⊑⊥`, `⊤⊑⊥`, `∃r.A⊑⊥`, `∃r.⊤⊑⊥` /
`ObjectPropertyDomain(r,⊥)` / `ObjectPropertyRange(r,⊥)`) do not occur in any
ontology under `ontologies/real` or in the test suite fixtures. The curated-corpus
closure diff (bibtex / pizza / ro / ro-stripped / sio / sulo / sulo-stripped /
go-basic — 57 803 rows total) is **byte-identical between this branch and `main`**.
Part A carries zero measured completeness delta and zero measured performance delta
on the current corpus. Its value is correctness: after this fix, the fragment gate
and the engine agree — every axiom shape the gate admits is a shape the engine
reasons over completely.

The one **known remaining gap** — role-chain-induced poison
(`SubObjectPropertyOf(Chain(t,u),r)` + `ObjectPropertyDomain(r,⊥)` + `C⊑∃t.∃u.A` ⟹
`C` unsat, still MISSED) — is tested and `#[ignore]`d with a rationale: marking
`u` poisoned would be unsound for a standalone `∃u.A`, so closing this needs a
chain-aware rule that Part A does not supply.

**Part B — measurable ORE wins.**

Three representative ontologies, measured on release binary:

| ontology | before | after | mode |
|---|---|---|---|
| `ore_ont_9318` (39 433 classes, 4 negation axioms) | 21.8 s hybrid | 0.93 s pure-EL | fast path gained |
| `ore_ont_2397` | DNF >200 s | 1.03 s | fast path gained |
| `ore_ont_10032` | DNF | 2.41 s | fast path gained |

Closure identity: `ore_ont_9318` closure byte-identical flag-ON vs flag-OFF. FP=0
confirmed against the independent KM reasoner on `ore_ont_2397` and `ore_ont_10032`:
183 414 and 78 974 rustdl direct subsumptions respectively are all contained in KM's
closures; unsat sets agree. Flag ON-vs-OFF byte-identical across the curated corpus
(bibtex / pizza / ro / sio / sulo / go-basic).

The measured win is entirely **Part B**. Part A enables Part B to be correct (without
the `ConjunctiveUnsat` rule, Part B would route `A⊑¬B` to a fast path that drops the
axiom — trading a slow correct answer for a fast wrong one), but Part A itself produces
no observable change on the current corpus.
