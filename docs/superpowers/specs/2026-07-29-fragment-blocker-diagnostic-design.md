# Fragment-blocker diagnostic — NO-GO; and the gate/engine consistency fixes it turned up

**Date:** 2026-07-29
**Status:** **NO-GO on the diagnostic** (death certificate). Approved for implementation:
the gate/engine consistency fixes in Part 2.
**Motivation record:** `docs/2026-07-29-fragment-lever-selection-findings.md`

---

# Part 1 — Why the blocker diagnostic is a NO-GO

The proposal was: make `is_pure_el` / `saturator_complete_fragment` report *which* axiom and
construct disqualified an ontology, so the next gate lever could be chosen from a measured
histogram. Two independent reviews killed it. Recording the reasoning, because the idea is
attractive and will otherwise be re-proposed.

## 1.1 The premise was incomplete

The findings doc claimed lever payoff is decided by "is this construct the **last**
blocker". That is **necessary but not sufficient**. The real condition is
**liftable AND last AND the fast path is tractable for that ontology** — and a
construct-keyed histogram measures only the middle term.

**Liftability is a per-ontology *usage* property, not a construct property.** Proof from
our own data: `SubClassOf(X, ¬Y)` is liftable (13 flips, shipped); `EquivalentClasses(A, ¬B)`
appears in **114** ORE ontologies and is correctly *unliftable* (its covering half is real
DL expressivity). Same construct, opposite verdict. The next case is worse:
`InverseObjectProperties(p,q)` is inert-and-liftable when one side is never used in a
concept, and genuinely inverse-semantic when both are — **same axiom form, same construct,
same histogram bucket, opposite verdict.** The histogram would report "inverse role ×210"
and you would still have to read the axioms.

## 1.2 The unit overstates payoff ~2.6×

Levers have been reported in two different units. The negation lever supplies the
conversion: **13 fragment-eligibility flips → 5 user-visible DNF recoveries (38%)**.
Eligibility flips are exactly what a blocker histogram counts.

**Rule going forward: report gate levers in DNF recoveries, never in eligibility flips.**

## 1.3 The reachable population is ~11 ontologies, and it is memory-blocked

From `ore-run/work/fragments.tsv` (869 ontologies, gate-exact verdicts — not the regex
histogram):

| population | count | reachable by a gate lever? |
|---|---|---|
| out-of-fragment **and** slow (>10 s) | **28**, all 11.7k–186k classes | No — the fast path is the D4 dense-matrix problem (`ore_ont_3914`: 166 GB at 12.4k named classes) |
| `TIMEOUT` (no verdict) | **13**, of which `6212` and `15703` were already taken by the negation lever | ~11 — this is where the mechanism genuinely works |
| `ERROR(137)` (OOM-killed) | **11** | No — memory-bound before any verdict |

Ceiling: **low single digits of DNF recoveries**, contingent on the memory work landing
first. **Sequencing inversion: the dense-timeout memory work is a *precondition* for gate
levers paying on this tail, not an alternative to them.** Building a lever-ranking tool
first ranks levers whose destination is an OOM.

## 1.4 Inverse is closed by calculus, not by preference

210 of 289 DNF-tail ontologies fall in buckets containing inverse/symmetric. Admitting
inverse *use* into either gate would certify the saturator complete on a fragment where it
is provably incomplete — the 4-axiom counterexample
`X ⊑ ∃R.C; ∃R.X ⊑ D; ∃R.D ⊑ E; Symmetric(R) ⟹ X ⊑ E`. That is the D10 unsound-completeness
class, not a lever. It is unavailable until backward propagation exists, which carries its
own NO-GO. No histogram changes this.

## 1.5 The design was also wrong on its own terms

Recorded so a future attempt doesn't repeat them:

- **Outermost-first was the wrong per-axiom answer.** `SubClassOf(A, Or(B, Not(C)))` would
  report `{Or}`; lifting `Or` does not flip it because `Not` is behind it. That is the
  grep≠gate trap reproduced *inside* the gate-based tool, and worse for carrying gate
  authority. A correct version needs the full per-axiom blocker **set**, collected by
  descending past rejections.
- **Per-axiom maps cannot express whole-ontology conditions.** `disjoint_ok` is
  `!has_cardinality_role` — `DisjointClasses` is blocked by a `FunctionalRole` axiom
  *elsewhere*, which is itself accepted. A reader would "lift DisjointClasses" and
  reintroduce the D10 bug class. Same for `ontology_uses_nominals` gating Lever 1.
- **The surface could not observe its own target population.** The banner prints from
  `write_classification`, i.e. *after* classification returns; the DNF tail by definition
  never returns.
- **The `skip_abox` view would mislead on the largest off-EL fast-path population.** Lever 1
  ontologies would report `ClassAssertion ×90000` while actually being on the fast path.
- **The safety argument was wrong.** The spec claimed the bool must be *derived* from the
  reporter or the two drift. The actual guard is the agreement test, which works equally for
  a parallel reporter. Derivation buys tidiness, not safety — while putting a measurement
  tool's requirements into the gate predicates, whose bug class (D10) has a live open
  instance (role-chain-induced poison).

## 1.6 The one piece worth salvaging (not built here)

A `rustdl fragment <file>` subcommand that converts and prints the three gate verdicts
(`is_pure_el`, `saturator_complete_fragment`, `tbox_only_saturator_eligible`) **before**
classification, alongside the existing `tbox-stats` / `clause-stats` diagnostics. It fills
the one genuine hole — the 24 `TIMEOUT`/`OOM` rows have no verdict because `# fragment:` is
printed post-classification — and makes construct **ablation** gate-exact on the DNF tail,
so future lever questions are answerable with a shell loop and no Rust. `fragment` is also
absent from the Python surface entirely, a larger usability gap than the missing blocker
detail. Deliberately **without** the per-axiom blocker refactor.

Caveat measured while reviewing: a convert-only probe is not uniformly cheap on tail
members — `tbox-stats` on `ore_ont_9347` takes **118 s / 14.2 GB RSS** with no reasoning at
all. That is independently interesting for the memory work: it implicates a
**pre-classification** allocation site distinct from the saturator's dense matrices, which
the existing D4 root-cause note does not cover.

---

# Part 2 — Gate/engine consistency fixes (approved for implementation)

Both reviews independently found live instances of the branch's own bug class, by *reading*
the gate rather than instrumenting it. Verified on a fresh release build (2026-07-29).

## 2.1 Bug A — `DisjointClasses` members unchecked

`is_saturator_axiom`'s arm is `Axiom::DisjointClasses(_) => disjoint_ok` — members are never
inspected, in contrast to `is_el_axiom`, which does check them. The saturator filters
members to atomics and silently discards the rest.

```
DisjointClasses(:A ObjectUnionOf(:B :C))
SubClassOf(:X ObjectIntersectionOf(:A :B))
```
`X` is unsatisfiable (`A ⊓ B ⊑ A ⊓ (B ⊔ C) ⊑ ⊥`). Observed: `direct X A`, `direct X B`, no
`unsat`, under `# fragment: Horn (… hyper Horn fixpoint is complete)`. With
`RUSTDL_HORN_SHORTCIRCUIT=0` the hybrid path correctly reports `unsat X`.

Footprint: 7 ORE ontologies carry a non-atomic `DisjointClasses` member.

## 2.2 Bug B — non-atomic `Domain`/`Range` accepted by both gates

Both gates accept any `is_*_concept` domain/range filler, including `And` and `Some`.
`role_domains` / `role_ranges` are atomic-only.

```
ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))
SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing))
```
Entails `X ⊑ P` and `X ⊑ Q`. Observed: **zero subsumptions**, under
`# fragment: pure-EL (… saturator alone is complete)`. Control with two *atomic* domain
axioms — semantically identical — correctly emits both. This one sits in `is_pure_el`, so
`RUSTDL_HORN_SHORTCIRCUIT=0` does not rescue it.

Footprint: 2 ORE ontologies with an `And`-of-atomics filler, 2 with a `Some` filler.
`ro`/`ro-stripped` carry `Some`-filler domains but take the **hybrid** path, so the curated
corpus is unaffected — it is rescued by being off the fast path, not by the gate being
right.

## 2.3 The fix: tighten the gate, do not extend the engine

Make the gates reject exactly what the engine drops:
- `is_saturator_axiom`'s `DisjointClasses` arm must require every member to satisfy
  `is_saturator_concept` **and** be atomic (matching what `disjoint_pairs` keeps).
- Both gates' `Domain`/`Range` arms must require an **atomic** filler.

**Tightening is FP-safe by direction**: it can only route *more* ontologies to the
sound-and-complete hybrid path. It cannot introduce a false positive or a new miss. The
cost is that 2–7 ontologies get slower and correct.

Gate: the two repro ontologies above become correct; curated-corpus closure byte-identical
(no curated fixture is on the affected path); `cargo test --workspace` green.

## 2.4 Two candidate levers — each blocked on engine verification

Both widen a gate, and **widening a gate without verifying engine support is the bug class
in 2.1/2.2.** Neither may ship on the argument "the construct looks EL".

- **`Bot` arm in `is_saturator_concept`.** `is_el_concept` accepts `ConceptExpr::Bot`;
  `is_saturator_concept` does not, so `saturator_complete_fragment` still rejects
  `∃r.⊤ ⊑ ⊥`, `Domain(r,⊥)`, `Range(r,⊥)` — shapes Part A of the just-merged branch taught
  the engine to handle via `poisoned_roles`. **But a blanket `Bot` arm also admits
  `SubClassOf(A, ∃r.⊥)`, and there is no evidence the engine derives `A ⊑ ⊥` from that.**
  Required before widening: enumerate every position `Bot` can occupy under
  `is_saturator_concept`'s recursion and verify engine support for each, admitting only the
  verified positions. An unverified blanket arm would create a fresh D10 instance.
- **`Min(1,r,C) ≡ ∃r.C`.** `is_saturator_concept` excludes all `Min` conservatively, citing
  a `Min(≥2)`+functional interaction that does not apply at `n=1`. Required before widening:
  confirm the engine actually lowers `Min(1,r,C)` to an existential fact rather than
  dropping it.

Each is a few lines *if* verification passes, and a documented non-lever if it does not.
Verification is the work; the edit is not.
