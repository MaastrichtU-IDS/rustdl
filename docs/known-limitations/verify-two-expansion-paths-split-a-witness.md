# The model builder can report a false `Violated` (F1/F2/F3, plus the original split-witness risk)

**Found:** 2026-08-28 (Task 14, `owl-dl-verify`); F1/F2/F3 found 2026-08-28 during the final
whole-branch review · **Status:** PARTIALLY RESOLVED. **F2 is FIXED (2026-09-04)** — see the
note in its section below; F1 and F3 remain OPEN and reproducible, and each exits the CLI's
normal success path (`(0 unresolved)`, exit 2). The original split-witness risk below is still
hypothetical/untested ·
**Severity:** this crate's central claim ("a `Violated` verdict is a real engine defect") does
not hold as stated. See the corrected wording in `CLAUDE.md`'s `crates/owl-dl-verify` entry,
`README.md`, and `rustdl verify-el --help`.

## Direction of risk: read this before the mechanism sections below

Every mechanism on this page is a **false `Violated`** — the model builder itself builds an
incomplete or mislabelled witness, and the checker then reports that the very axiom which should
have closed the gap has failed. None of them is a false `Verified` (a real defect that the
checker misses). That asymmetry matters for how to read this instrument: **a `Violated` verdict
is a strong lead requiring adjudication against a real classification disagreement, not a
proof.** It does not, on its own, mean the reasoner dropped an entailment.

The original analysis on this page (kept below, in "Why this is not shown to be an unsound
checker") reasoned about the *other* direction — whether an under-labelled or duplicated element
could let a genuinely-violated axiom read as `Verified`/`Holds`. That is the wrong question for
this instrument. `Verdict::Violated` is not a background risk that only matters if it *hides*
something; reporting `Violated` **is** the output signal a user acts on. The old paragraph's own
observation — "an extra, edge-poor element can only make MORE axioms fail to be witnessed, never
fewer" — is not a safety argument here; "more axioms fail" is precisely a spurious `Violated`.
F1/F2/F3 below are that mechanism family, confirmed rather than hypothetical.

## `cascade.ofn` joined F1 on 2026-08-29 — and it moved WITHOUT a test failing

Recorded because the transition is the instructive part. Before issue #81, `cascade.ofn` was a
genuine DETECTION: rustdl emitted zero rows, disagreeing with its Konclude oracle, and the
instrument's `Violated` correctly refused to vouch for that. Issue #81's nested-existential range
fold made rustdl derive `A ⊑ FINAL` — the oracle's only non-trivial row, re-confirmed against
Konclude v0.7.0-1138. **rustdl is now right and the instrument still says `Violated`,** so cascade
is a false positive of F1 below: its `SubClassOf(ObjectIntersectionOf(∃v.W, G), Z)` is a GCI over
a conjunction, and `materialise_exists` labels that witness by plain union without closing it
under the GCI. The reported element is `Element(3)={G}`.

**The acceptance suite did not notice**, because
`instrument_never_verifies_a_classification_that_disagrees_with_the_oracle` asserts nothing at all
when rustdl agrees — so cascade degraded from a detection to a vacuous pass and the suite stayed
green. Two guards were added in the same change:
`the_detection_set_has_not_silently_gone_vacuous` (the detection set must be non-empty) and
`cascade_now_agrees_with_its_oracle_and_its_violated_is_a_known_f1_false_positive` (both halves
pinned, so either moving is loud). If F1 is ever fixed, that second test is the one that will
fail — update it and this page together.

## F4 — `SubClassOf(owl:Thing, C)` is never applied to model elements

**Found:** 2026-08-30, by the corpus-wide hunt (`docs/benchmarks/2026-08-30-corpus-wide-d10-hunt.md`).
**Confirmed** on `ore_ont_11522`, `ore_ont_14128`, `ore_ont_14826`, `ore_ont_7270`.

`⊤ ⊑ C` requires every element to be a `C`. The builder never propagates it, so the checker
reports the very axiom that should have closed the label. The violation count equals the
ontology's `owl:Thing`-LHS axiom count **exactly** (2/2, 2/2, 8/8, 19/19), and the reported
element is a Tseitin synthetic carrying only itself (`Element(79)={<synthetic#80>}`).

The engine is right: `rustdl subclass-expr <ont> "owl:Thing" "<C>"` returns `yes` on every probed
pair. rustdl merely omits `owl:Thing` from classification OUTPUT — a reporting convention.

## F5 — `BoundTripped` produces `Violated` instead of `Unresolved`

**Found:** 2026-08-30, same sweep. **Confirmed** on `ore_ont_12317`, `ore_ont_13220`,
`ore_ont_15249`, `ore_ont_283` — all 50k–60k classes, all exceeding
`Bounds::max_elements = 50_000`, all reporting violations on the order of the domain size
(25,369 / 20,292 / 54,304 / 92,573) with `BoundTripped` in their own reasons.

A truncated model cannot witness most axioms, so the violations are artifacts of the cut. This is
an asymmetry in `owl-dl-cli`'s `fold_build_reasons`: its `Verified` arm downgrades to
`Unresolved` whenever build reasons are non-empty (the arm that stops a false all-clear), while
its `Violated` arm appends the reasons and KEEPS the verdict. Correct for a complete model with a
minor residual; wrong for `BoundTripped`, where the model is known-incomplete by construction. A
truncated model should exit 3, not 2.

## F1 — conjunctive `∃`-body plus a GCI over the conjunction

**Reproducer:** `crates/owl-dl-verify/tests/known_limitations.rs::f1_conjunctive_exists_body_gci_is_a_false_violated`
(control: `f1_control_flat_exists_body_verifies_cleanly`).

```
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:A :B)))
SubClassOf(ObjectIntersectionOf(:A :B) :C)
```

`materialise_exists`'s `ConceptExpr::Some` arm (`model.rs`, the loop over `required_atoms`) labels
the witness as `subsumers_of(A) ∪ subsumers_of(B)` — a plain union, never closed under the GCI
`A ⊓ B ⊑ C`. The witness therefore satisfies `A ⊓ B` by its own label (both `A` and `B` are in
it) but does **not** contain `C`. The checker then evaluates `A ⊓ B ⊑ C` over the whole domain,
finds this element in `A ⊓ B` and not in `C`, and reports **that axiom** — the one the model
should have used to close its own label — as violated.

The flat control (`X ⊑ ∃r.A`, `A ⊑ C`) verifies cleanly: with a single atomic body, `target_label`
resolves through the saturator's own closure, which already folds `A ⊑ C` into `A`'s subsumer
set before the model is ever built. Only the conjunctive body bypasses that closure.

## F2 — nested `∃` plus an ordinary `SubClassOf(owl:Thing, C)` — **FIXED 2026-09-04**

> **RESOLVED.** `⊤ ⊑ C` is now applied at `FiniteModel::intern` — the single chokepoint all
> three label-construction sites route through — so every element carries it, Tseitin
> synthetics included. `expand_from_axioms` also lets a universal antecedent fire as a RULE,
> which is what `⊤ ⊑ ∃r.C` needs (a label floor cannot build an existential).
>
> **F2 and #87's F4 are ONE mechanism, and neither write-up said so.** F2 was filed here as a
> builder imprecision; F4 was filed on #87 as a corpus-scale instrument false positive
> affecting four ORE ontologies whose violation counts equal their `owl:Thing`-LHS axiom counts
> exactly. They are the same sentence — "the Tseitin marker's label is never closed under
> `⊤ ⊑ C`" — approached from a fixture and from the corpus.
>
> **What surfaced the connection:** `f2_nested_exists_plus_thing_subclass_is_a_false_violated`
> FAILED when the F4 fix landed, because it asserted the false `Violated`. It was written to
> trip on a fix ("F2 known limitation did not reproduce (fixed?)") rather than being
> `#[ignore]`d — which is the only reason this did not pass unnoticed. It is retargeted to
> assert the absence, keeping the name searchable.
>
> **The root cause was a conflation worth remembering:** `required_atoms` returns an empty
> antecedent both for `⊤` (genuinely universal) and for every shape it cannot decompose
> (`Some`/`Or`/`Not`/`Min`/`All`). The builder read empty as "no rule". Reading it instead as
> "universal" would have been worse — applying a consequent to every element can satisfy an
> axiom that should have been reported violated, i.e. a **false all-clear**. So the fix tests
> for `⊤` structurally, guarded by
> `an_unevaluable_antecedent_is_not_treated_as_universal`, which is sabotage-verified: a first
> version of that guard did NOT fail when the predicate was loosened, and was rewritten around
> a `DisjointClasses` that makes the over-broad labelling observable.
>
> **Residual:** because the floor applies to every element, the instrument no longer tests
> whether the CLOSURE derived `⊤ ⊑ C` for a named class. Acceptable — the axiom is asserted,
> not derived, and rustdl's own `subclass-expr owl:Thing <C>` confirms it on all four
> ontologies — but it is a real narrowing of what a `Verified` means there.

### Original F2 report

**Reproducer:** `crates/owl-dl-verify/tests/known_limitations.rs::f2_nested_exists_plus_thing_subclass_is_a_false_violated`
(control: `f2_control_flat_exists_plus_thing_subclass_verifies_cleanly`).

```
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :Y)))
SubClassOf(owl:Thing :C)
```

A nested `∃` gets an element seeded per Tseitin marker, and that element's label comes back as
`target_label`'s minimal `{Q}` row — the marker's own (nearly empty) subsumer set — never closed
under `⊤ ⊑ C`. The checker evaluates `⊤ ⊑ C` over the whole domain (every element must be in
`C`), finds the marker element is not, and reports a violation. **`SubClassOf(owl:Thing, …)` is an
ordinary axiom shape in real EL ontologies** (a global range/typing constraint), not an exotic
construct manufactured to trigger this.

The flat control (single `∃`, no nesting) verifies cleanly, because the fact-driven expansion
path resolves the witness directly from the saturator's closure, which already has `⊤ ⊑ C` folded
into every class's subsumer set — the loss is specific to the Tseitin-marker path nested `∃`s
take.

## F3 — nested `∃` plus `ObjectPropertyDomain` on the inner role

**Reproducer:** `crates/owl-dl-verify/tests/known_limitations.rs::f3_nested_exists_plus_inner_domain_is_a_false_violated`.

```
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :Y)))
ObjectPropertyDomain(:s :D)
```

`materialise_exists` mints the outer `∃r`'s witness element with a label taken from
`effective_ranges(r)` — empty here, since `r` has no declared domain/range — and then
**recurses into the inner `∃s.Y` body at that same element**, giving it an outgoing `s`-edge.
`ObjectPropertyDomain(s, D)` is checked against every edge source under `s`; this element is one,
and its label is empty, so it fails a domain constraint the model itself is responsible for
having attached an edge without ever attaching the label that edge's presence should imply.

## Why these three, and not the original split-witness risk, were caught first

All four defects live in the same code path (`materialise_exists`'s handling of an opaque `∃`
body) and share the same root cause: **the label attached to a witness minted on the axiom-driven
path is not always closed against the axioms/edges the model goes on to build around it.** The
original split-witness analysis below asked whether that under-labelling could let a *different*,
already-built element mask a real violation (untested, still open); F1/F2/F3 show the more direct
consequence — the under-labelled element's OWN presence in the domain trips a checker that has no
way to tell "genuinely absent from this class" apart from "labelled this way because of how it was
built."

## Original split-witness analysis (2026-08-28, Task 14) — kept, direction of risk corrected above

`owl-dl-verify`'s `FiniteModel::build_model` runs **two** expansion passes over the same
saturation closure — `expand` (the fact-driven path, over the saturator's own
`(ClassId, RoleId, ClassId)` fact triples) and `expand_from_axioms` (the axiom-driven path,
walking `InternalOntology.axioms` directly via `materialise_exists`). Both exist because the
saturator emits **no fact** for a nested existential body: `X ⊑ ∃r.∃s.C` gets a Tseitin marker
with an empty subsumer set, so the fact path alone has no element for the nested witness at all
(`expand_from_axioms`'s own doc comment, `crates/owl-dl-verify/src/model.rs:455-467`).

**The two paths can label that same nested witness differently.** In `materialise_exists`'s
`ConceptExpr::Some(role, body)` arm, when `body` is itself opaque (no `required_atoms`, e.g.
another nested `∃`) the witness's label is built from `eff.get(&r)` — the role's *effective
ranges* — which is frequently **empty**. The fact path (`expand`) instead resolves the same shape
via `target_label`, whose `Ok` arm ultimately bottoms out at `subs.subsumers_of(Tseitin Q)` — and
a Tseitin marker's subsumer set is never empty; it contains at least `{Q}` itself.

`intern` dedups **purely by label content** (`label_ix: HashMap<Box<[ClassId]>, Element>`), with
no notion that two labels might denote the same underlying existential witness. So when both
paths visit the same nested `∃`, they can produce two *different* label vectors for what is
logically one witness — and `intern` allocates **two separate `Element`s** for it, one
under-labelled (from `eff_ranges`, often `[]`) and one correctly labelled (from the Tseitin
marker's subsumers).

This directly contradicts the design spec's "one canonical interpretation" framing (§3/§5 of
`docs/superpowers/specs/2026-08-27-negative-certificates-phase1-design.md`): the model built is
**not canonical** when the same witness can appear twice under different labels. (`src/lib.rs`
and `src/model.rs`'s module docs were corrected on this point during the branch review that added
F1/F2/F3 — they no longer claim the model is canonical.)

### Why this specific risk is not (yet) shown to cause a false `Violated` on its own

The under-labelled element (from `eff_ranges`) carries no edges of its own beyond what
`materialise_exists` immediately builds under it. Whether an evaluator check keyed to iterating
"all successors of `x` under `r`" could visit only the under-labelled copy and therefore miss
something the correctly-labelled copy would have satisfied — which WOULD be a false `Violated`,
by the same direction-of-risk logic as F1/F2/F3 — is **untested**: `eval.rs` currently only
checks `SubClassOf` / `EquivalentClasses` / role-hierarchy shapes over the domain as a whole,
never a check keyed to one specific `Element` by identity, so nothing in the current evaluator
exercises this. Nothing in the code prevents a future evaluator addition from doing so.

## Fix sketch (not built)

The two paths would need to converge on the target label BEFORE calling `intern`, e.g. by having
`materialise_exists`'s opaque-body branch also union in `subs.subsumers_of` over the Tseitin
markers `expand`'s `target_label` would have reached, rather than falling back to
`eff.get(&r)` alone. That requires exposing the fact path's marker resolution to the axiom path,
which the two functions do not currently share (`model.rs:379` vs `model.rs:545` take disjoint
parameter sets built by different callers in `build_model`). Note this fix sketch targets the
split-witness risk and F3's edge-attachment mechanism; F1 (conjunctive body) and F2 (Tseitin
marker vs. `⊤`-closure) need the label-closure step applied more generally, not just at the
`intern` boundary — see each section above for the specific gap.

## Where this is recorded in code

`materialise_exists`'s opaque-body branch (`crates/owl-dl-verify/src/model.rs`, the
`if atoms.is_empty() { ... }` block inside the `ConceptExpr::Some` arm) carries a matching
inline comment pointing back at this file, corrected during the branch review to describe the
edge it attaches immediately below (F3) rather than claiming the element is edge-less.
