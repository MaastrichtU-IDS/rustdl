# Negative certificates for rustdl — scoping (2026-08-27)

**Purpose.** rustdl can prove positives (`justify`, `prove`) but has **no artifact for a
negative**. Every completeness claim therefore rests on an external oracle
(Konclude ∪ HermiT). A checked countermodel makes non-entailment *self-verifiable*.

**Phase 1 target is NOT the user-facing certificate.** It is an internal self-verification
gate for the **D10 bug class** — a fragment gate certifying COMPLETE while the engine
silently drops an axiom. Six recorded instances; found today only by hand-grepping the
engine per admitted construct.

## The target defect is live on the default path (measured)

Fixture behind the `#[ignore]`d `nested_existential_poisoned_role_via_chain`
(`conjunctive_unsat.rs:441`) — `SubObjectPropertyOf(ObjectPropertyChain(t,u), r)`,
`ObjectPropertyDomain(r, owl:Nothing)`, `SubClassOf(C, ∃t.∃u.A)`. Correct answer: `C` is
**unsatisfiable**. Today's binary:

```
consistent   : True
unsatisfiable: []          <- WRONG
incomplete   : False       <- claims completeness
# mode: pure EL (saturation-only)
# fragment: pure-EL (trust_sat sound by construction; saturator alone is complete)
```

A wrong answer with the gate asserting completeness by construction. **No existing gate
catches this** — the FP=0 net is FP-shaped, the MISSED net is oracle-shaped and drawn from
completers.

*(Note: that in-tree fixture writes `SubObjectPropertyChain`, which the OFN parser rejects
— the correct keyword is `ObjectPropertyChain`. Being `#[ignore]`d, it has never had to
parse. Fix when un-ignoring.)*

## Pre-checks: both PASS

**1. Are the ∃-facts reachable?** Yes — `owl_dl_saturation::saturate_with_exists_facts`
already returns `(Subsumers, Vec<(ClassId, RoleId, ClassId)>, HashMap<ClassId,
IndividualId>)`: the subsumer closure **plus every derived `X ⊑ ∃r.Y` edge**, sorted for
determinism, over the extended (Tseitin-inclusive) class space. No engine change needed to
read the model out.

**2. Does the violation reproduce by hand?** Yes, under the corrected construction below.

There is **no `Interpretation`/`Model` type anywhere in the workspace** — this is new
infrastructure, not a refactor.

## The construction — elements are LABEL SETS, not classes

A naive "one element per class" model is **wrong**, and Probe B proves it:

| probe | result | reading |
|---|---|---|
| `Domain(t,D)` ⇒ `C ⊑ D`? | **YES** | the `t`-edge from `x_C` is in the closure |
| `Range(u,F)` ⇒ `A ⊑ F`? | **NO** | rustdl is **correct**: `A ⊑ F` is not entailed |

Under a one-element-per-class model, `x_A` sits at the end of the `u`-edge, so `Range(u,F)`
reads as violated when nothing is wrong — a **false rejection**. `A`-as-a-class and
`A`-as-a-`u`-successor must be different elements.

Correct (standard EL) construction, built lazily from the ∃-facts:

* seed one element per named class `X`, label `subsumers(X)`;
* for a fact `X ⊑ ∃r.Y` at element `e`, the successor's label is
  `subsumers(Y) ∪ ⋃{Range(s) : r ⊑* s}`;
* **key elements by their label set** — dedupe by label. That distinguishes the two `A`s
  exactly when their labels differ, keeps the domain finite, and gives blocking for free.

The label rule is **local** (subsumers lookup + range union). Deliberately **not** a general
independent EL completion — that is the circularity-and-cost trap. Where a label needs
closure the local rule cannot supply, the element is `Unresolved` and the check declines.

The independence that carries the guarantee is in the **evaluator**, not the builder.

### Hand-verified calibration pair

| fixture | model | verdict |
|---|---|---|
| chain-poison | `x_C{C,⊤} -t-> e₁{M,⊤} -u-> e₂{A,⊤}`; RIA closure adds `(x_C,e₂) ∈ r`; `Domain(r,⊥)` forces `x_C ∈ ⊥^I = ∅` while `x_C` is in the domain | **REJECT** |
| `nested_existential_unpoisoned_role_stays_sat` (role `:s`, no chain axiom) | `r^I = ∅`, so `Domain(r,⊥)` holds vacuously; all axioms hold | **ACCEPT** |

One rejects, one accepts, on nearly identical input. That pair is the anti-vacuity check in
its sharpest form.

## Two constraints that decide whether the instrument can see anything

1. **Evaluate axioms from the horned-owl parse tree, NOT `InternalOntology`.** Two of the
   candidate sabotage flags are conversion-level (`RUSTDL_EL_BOT_FILLER` is a lowering drop;
   `RUSTDL_DKEY_POST_NNF` lives in `convert.rs`). An axiom dropped *at conversion* is simply
   absent from the IR, so the check would pass **vacuously** — blind to exactly the class
   this is built for. `InternalOntology.dropped` being empty is necessary, not sufficient:
   D10 drops are *silent*.
2. **No `_ => {}` arm in the evaluator.** An unhandled concept or axiom form must yield
   `Unresolved`, never a skip — otherwise "accept" can mean "ignored every form it did not
   recognise". This is the KM discipline (their checker *rejects* `unresolved` rather than
   assuming it).

## Scope: `is_pure_el`, not `saturator_complete_fragment`

`saturator_complete_fragment` admits functional and inverse-functional roles, and the naive
canonical model is **not functional** — one element can acquire two `r`-successors, giving
spurious rejections indistinguishable from real ones. `is_pure_el` admits the 2-leg chains
and the `⊥` shapes, which is **precisely where the live gap is** (CLAUDE.md states that
`is_pure_el` certifies completeness on chain ontologies while the engine drops chain-induced
poison). Role extensions must be closed under RIAs and transitivity in the builder —
mechanical, and required for the target fixture. Functional roles are a later, harder step.

## Components, and where the risk actually sits

| component | size | risk |
|---|---|---|
| `Interpretation` type (domain, concept ext, role ext) | small | low |
| canonical-model builder from `saturate_with_exists_facts` | small | **construction bugs → false rejections** (Probe B class) |
| **axiom evaluator over the parse tree** | largest | **all of it** — exhaustiveness |
| CLI surface + gate | small | low |

The evaluator's exhaustiveness is where a bug hides, and it has a net in **both**
directions: a false reject is caught by the inertness check, a false accept by the sabotage
set.

## Validation plan — inertness FIRST

Ordered deliberately; a rejection carries no information until accepts are clean.

1. **Inertness before sabotage.** Run over the curated fixtures where rustdl is believed
   complete and drive spurious rejections to **zero** first. Every one found is a
   construction bug like Probe B — far cheaper to find via a fixture than to misread as a
   discovered defect.
2. **The crown jewel: `nested_existential_poisoned_role_via_chain` must REJECT today,
   unsabotaged.** A live, documented incompleteness no existing gate catches. Reject ⇒ the
   instrument fires on a real defect. Accept ⇒ broken or mis-scoped, learned before
   trusting any of it.
3. **The calibration pair** (table above): poisoned rejects, unpoisoned accepts.
4. **Anti-vacuity:** on a fixture the checker accepts, drop one derived subsumption from the
   closure and confirm rejection — proving the accept was load-bearing
   ([[test-your-guard-against-known-good]]).
5. **Count the usable sabotage set before promising five.** `RUSTDL_DKEY_*` are
   datatype-path and likely outside `is_pure_el`, so may not exercise Phase 1 at all. The
   real in-fragment subset could be **2**. Count, don't assume.
6. **The meta-check:** the instrument must **disagree** with `saturator_complete_fragment`
   somewhere. If accept ⟺ gate admits, it has learned nothing. The chain case is a known
   disagreement — measure it first.

## Bounds stated up front

Domain size is `|atomic + Tseitin atoms|`; checking is `O(|axioms| × |domain|)`; RIA and
transitive closure are worse. This is a gate for **fixtures and small-to-medium ORE**, not
galen-scale and certainly not the 981k-class ontologies. Bound it and emit `Unresolved`
when the bound trips — same idiom, and the bound must be *reported*, not silent.

## Explicitly NOT in Phase 1

* **`RUSTDL_HYPERTABLEAU_TRUST_SAT` is not made checkable.** `trust_sat` governs the wedge
  on non-EL input; the EL fast path short-circuits and never consults it. SROIQ is where
  finite countermodels may not exist — the boundary KM itself declines to cross.
* The user-facing per-pair certificate, and any output format or API commitment. Phase 1 is
  the internal gate; it delivers the D10 win, commits to no format, and is a strict
  prerequisite for the certificate anyway.

---

## Two NEW live D10 instances, found by reviewing this spec (2026-08-27)

Both under the `# fragment: pure-EL (… saturator alone is complete)` banner with
`incomplete: false`. These are the **7th and 8th** recorded instances of the class, and they were
found by adversarial review of the design — before a line of the instrument was written.

**Root cause: the fragment gate is out of the OWL 2 EL profile.** `is_el_axiom` admits
`ObjectPropertyRange` and 2-leg `ObjectPropertyChain` **independently, with no interaction
restriction**. OWL 2 EL globally forbids a range on a property implied by a chain
(Baader–Brandt–Lutz 2008); the unrestricted combination is exactly what breaks the
canonical-model technique.

```
SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)
ObjectPropertyRange(:r owl:Nothing)              # or Range(:r :F) + SubClassOf(:F owl:Nothing)
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))
```

`C` **is** unsatisfiable: the chain forces an `r`-edge out of any `C`-instance, and the range
forces that edge's target into `⊥`. Measured on today's binary: `unsatisfiable: []`,
`incomplete: false`, `fragment: pure-EL`. (`A` alone is satisfiable — a standalone `A` need not be
an `r`-successor.)

This is the **range-side sibling** of the Domain crown jewel and is **not** covered by the
`#[ignore]`d `nested_existential_poisoned_role_via_chain`. Both forms belong in
`crates/owl-dl-verify/tests/fixtures/` as first-class gate fixtures.

**Consequence for the instrument itself:** the same chain×range combination breaks the §6
construction on the *benign* case (no `⊥`), because a chain-materialised edge's target never
receives `eff_ranges` of the chain's super-role — a **false `Violated`** on a correct ontology.
Spec v2 refuses that combination with `Unresolved` rather than guessing, mirroring the profile
restriction.

---

## Filed as issues #80, #81, #82 (2026-08-28) — and the headline was NOT what the reviews said

Five live pure-EL D10 defects, all found by **designing and reviewing** this instrument rather than
running it, grouped by root cause and filed:

* **#80 — nested existential monotonicity.** THE HEADLINE, and it emerged only from re-deriving a
  fixture the review had labelled a healthy control. **Three axioms, pure EL, no chain or range:**
  `C ⊑ ∃t.∃u.A`, `A ⊑ F`, `∃t.∃u.F ⊑ D` entails `C ⊑ D` by plain monotonicity. Konclude reports
  `(A,F)` and `(C,D)`; rustdl reports only `(A,F)`, `incomplete: false`, `fragment: pure-EL`.
  **The one-level form is caught correctly**, so nesting is exactly the trigger — a perfect
  discriminating control.
* **#81 — ranges not folded into nested existential witnesses.** `cascade.ofn`: 8 classes,
  `A ⊑ FINAL` entailed by a 7-step derivation, Konclude's only non-trivial row, **rustdl emits zero
  rows**. Plus the unsat-shaped `unsatnested.ofn`, with `unsatconj.ofn` (non-nested) passing as the
  control.
* **#82 — chain-implied roles.** Domain half Konclude-confirmed (the gap behind the `#[ignore]`d
  `nested_existential_poisoned_role_via_chain`, previously untracked); range half new, and **both
  reasoners miss it**, so it is adjudicated by derivation. Filed with the OWL 2 EL profile caveat
  stated: the profile forbids range-on-chain-implied-property, and `is_el_axiom` admits the two
  constructs independently, so the actionable defect may be the *gate* rather than the engine.

### Method notes worth keeping

**Konclude's OWX output is multi-line.** A one-line `grep 'SubClassOf(...)'` returns nothing on
every ontology — indistinguishable from "Konclude found nothing". Parse the
`<SubClassOf><Class IRI=.../><Class IRI=.../></SubClassOf>` element, and calibrate against a case
where the oracle is known to report something.

**A reviewer's "healthy control" label is a hypothesis, not a result.** `chainrange_ctl.ofn` was
handed over as a discriminating healthy control; measuring it showed Konclude derives `C ⊑ D` and
rustdl misses it, and re-deriving showed the entailment needs neither the chain nor the range. That
one re-derivation produced #80 — the most fundamental defect of the set. **Adjudicate every fixture
yourself, including the ones labelled as passing.**

**Konclude's silence is ambiguous, so say which claims rest on it.** It confirms #80, #81 and #82
Part 1, and misses #82 Part 2 — where the claim rests on derivation alone. Reporting that
distinction is the difference between an oracle result and an assertion.
