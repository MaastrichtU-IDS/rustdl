# Negative certificates, Phase 1 — verified canonical model + independent axiom evaluator

**Date:** 2026-08-27 (v3, 2026-08-28 — rewritten after three adversarial reviews)
**Status:** design revised; plan pending
**Evidence:** `docs/benchmarks/2026-08-27-negative-certificates-scoping.md`,
`docs/benchmarks/2026-08-27-km-lean-proof-backing.md`

> **v3 is a REWRITE, not another patch.** v2 was applied as section-surgery and an audit found
> **nine** internal contradictions caused by that surgery — `still_holds_after` declared on the
> wrong type, a `VerifiedModel` referenced but never defined, a promised deadline parameter absent
> from both signatures. Rewriting is cheaper than patching a patched document.

## 1. Why

rustdl proves **positives** (`justify`, `prove`) and has **no artifact for a negative**, so every
completeness claim rests on an external oracle. The target is the **D10 class**: a fragment gate
certifying COMPLETE while the engine silently drops an axiom — a wrong answer *plus*
`incomplete: false`.

**The class is live, and designing this instrument found five more instances without running it.**
All measured on the current binary, all under
`# fragment: pure-EL (trust_sat sound by construction; saturator alone is complete)` with
`incomplete: false`:

| fixture | what is missed | adjudicated by |
|---|---|---|
| `chainpoison.ofn` | `C ⊑ ⊥` via `Chain(t,u) ⊑ r` + `Domain(r,⊥)` | Konclude (`C ≡ Nothing`) |
| `chain-range-bot.ofn` | `C ⊑ ⊥` via chain + `Range(r,⊥)` | hand-derived; range-side sibling |
| `cascade.ofn` | `A ⊑ FINAL` — rustdl emits **0 rows** | **Konclude** (only non-trivial row) + hand-derivation |
| `unsatnested.ofn` | `X ⊑ ⊥` | HermiT (`X ≡ Nothing`) |
| `chainrange.ofn` | `C ⊑ D` via chain + `Range(r,F)` | HermiT (Konclude also under-reports — a further under-report instance) |

`cascade.ofn` is the sharpest: eight classes, a range-driven cascade through **nested**
existentials, entailed by a seven-step derivation, and rustdl returns nothing while certifying
completeness. Discriminating healthy controls exist for the last three (`unsatconj.ofn`,
`chainrange_ctl.ofn`), which is what makes them tests rather than anecdotes.

**No existing gate catches any of these.** The FP=0 net is FP-shaped; the MISSED net is
oracle-shaped and drawn from *completers*.

## 2. Goals and non-goals

**Goals.** (1) An internal self-verification gate for D10 on the `is_pure_el` path, needing no peer
reasoner. (2) A **verified finite model** as a reusable artifact. (3) A thin incremental API
deciding whether a previously reported classification survives added axioms.

**Non-goals**, each with its successor phase: parse-tree evaluation (Phase 2 — Phase 1 reads the
lowered IR, so conversion-level drops are invisible, pinned by T9); serialized certificate and
external checker (Phase 3); **chain×range folding** (Phase 2, §4); functional/inverse-functional
roles and SROIQ (the canonical model is not functional — one element can acquire two
`r`-successors, giving spurious violations indistinguishable from real ones, which is why the scope
is `is_pure_el` and not `saturator_complete_fragment`); wiring into `classify` (**Phase 1 changes no
reasoning behaviour**); axiom removal.

## 3. Theory

**(a) The EL canonical model is universal** — one interpretation `M` with `C ⊑ D` iff `x_C ∈ D^I`,
so a single model witnesses every non-subsumption. No per-class shape is needed.

**(b) A model witnesses negatives soundly; a PRE-model does not.** rustdl's existing per-class
witness is a `Sat` completion — a pre-model, holding only what one branch was forced to derive.
That is why `RUSTDL_PSEUDO_MODEL` loses entailed types, and why an incremental check built on it
would be unsound. **Verifying that `M` is a model is what turns it into evidence**, so the D10 gate
is not a detour toward the incremental prize — it is its acceptance test.

**(c) The incremental claim.** `M` witnesses exactly the *reported* negatives, since `x_C`'s label
is `subsumers_of(C)` and a reported negative is `D ∉ subsumers(C)`. If every axiom of an added set
`Δ` holds in `M` then `M ⊨ KB ∪ Δ`, so every reported negative survives; reported positives survive
by monotonicity. **One model check proves the reported classification still valid.** No circularity:
it never assumes the closure was complete, only that `M` is a model.

Two preconditions, both load-bearing:

1. **`M` must have been verified.** Enforced by **type-state**: only `VerifiedModel` exposes
   `still_holds_after` (§6). `FiniteModel` does not.
2. **"Positives survive by monotonicity" presupposes the reported positives were correct.** This is
   a completeness instrument, not an FP net, so the claim is conditional on soundness, established
   separately.

**Scope of negatives covered.** `M` covers reported negatives over the named classes it seeds.
`owl:Thing` lowers to `ConceptExpr::Top`, not `Atomic`, so it is absent from
`Vocabulary::classes()`. This costs no soundness today — measured: adding `SubClassOf(owl:Thing, A)`
to `{D ⊑ A}` leaves `classify --json` **byte-identical**, because rustdl never reports `owl:Thing`.
Seed a ⊤-labeled element anyway so the contract does not silently depend on that convention.

**Direction of risk.** A **completeness** net, not an FP net: it checks that the reported closure
admits a model, not that it is minimal. A spurious subsumption makes `M` too full and is caught only
via a **disjointness, domain, or range** axiom.

## 4. The fragment exceeds the theorem — and the refusal

`is_el_axiom` admits `ObjectPropertyRange` and 2-leg `ObjectPropertyChain` **independently**, but
OWL 2 EL globally forbids a range on a chain-implied property (Baader–Brandt–Lutz 2008), precisely
because that breaks the canonical-model technique. Both the engine (see §1) and the construction
break there.

**Refusal predicate — exact wording, because a looser reading is UNSOUND.**

> `build_model` returns `Unresolved { ChainRangeOutOfProfile }` iff some admitted 2-leg chain
> `Chain(t,u) ⊑ v` has `eff_ranges(v) ⊄ eff_ranges(u)`, where `eff_ranges` is §5's
> **super-role-closed** set — *never* the declared ranges alone.

Three measured facts fix this wording:

* Of 1,920 pool files, 113 carry both a chain and an object range; the predicate fires on **61**,
  and **44 of those 61 fire only via the super-role path**. Reading "carrying a range" as
  *declared* range therefore misses the **majority** case — and that miss is a **false `Verified`**,
  not a false `Violated`, because the evaluator reads ranges per declared-role edge vector.
* **`TransitiveRole` is exempt by construction**: a materialised transitive edge's target was
  already an edge-target of the same or a sub-role, so it already carries `eff_ranges(r)`. Refusing
  it would be pure coverage loss over the dominant wild combination (495 transitive-only vs 4
  chain-only co-occurrences). The `⊆ eff_ranges(u)` clause also exempts the self-chain spelling
  `Chain(r r) ⊑ r`, which occurs in the wild.
* **Blast radius on the T3 population: 0 of 20.** None of the §8 inertness members declares an
  object *or* data range. T3 is not at risk.

**Why refuse rather than fold — corrected justification.** An earlier draft claimed folding risks
"the divergence the profile restriction exists to prevent." **That was wrong**: it conflates the
*engine's* completion-rule problem (real; the profile protects PTIME completion) with the
*builder's* fixpoint, which is bounded by construction — labels are interned points in a finite
lattice, folding is monotone, and `max_elements` turns any blow-up into an honest `BoundTripped`.
Refusal wins for Phase 1 on **sequencing** grounds: folding forces steps 5 and 6 into one
interleaved fixpoint (a folded chain target is a new element that can create new chain instances),
and the 61-file bucket would initially re-report a known defect, pricing in a noise floor before
finding anything new. **Phase 2 upgrade:** implement the fold; its acceptance test is exactly
today's refusal bucket flipping to `Verified`/`Violated`.

## 5. Construction

Inputs: `&InternalOntology`. **The fragment gate is a precondition of `build_model`, not merely of
the CLI** — a library caller on out-of-fragment input would otherwise get `Violated` for
entailments the EL closure legitimately never had.

1. `saturate_with_exists_facts(internal)` → `(Subsumers, Vec<(ClassId, RoleId, ClassId)>,
   HashMap<ClassId, IndividualId>)` (`crates/owl-dl-saturation/src/lib.rs:235`). Facts range over
   the extended (Tseitin-inclusive) class space, sorted by `(sub, role, target)`.
2. **Role hierarchy** from `internal.axioms`: `SubObjectPropertyOf { sub: Role(r), sup }` (both
   non-inverse) and `EquivalentObjectProperties` (both directions) → `RoleHierarchyBuilder`
   (`crates/owl-dl-core/src/role_hierarchy.rs:19`). Do not replicate the reasoner's inverse
   canonicalization: `is_pure_el` admits no inverse-role use.
3. **Effective ranges.** For `ObjectPropertyRange { role: Named(r), range }` where
   `internal.concepts.get(range)` is `Atomic(c)`, record `c`; **skip `Top`** (trivial, contributes
   nothing) and **skip `Bot`** (a label cannot carry `⊥`; the axiom check is its home — §7, and this
   is what makes T22 a detection rather than a refusal). Then
   `eff_ranges(r) = ⋃ { ranges(s) : s ∈ super_roles(r) }` — **super**-roles, because `r ⊑ s` makes an
   `r`-edge an `s`-edge, so `Range(s)` constrains `r`-successors. `super_roles` is reflexive.
   The §4 refusal is computed from **this** set.
4. **Seed elements.** For every class not `is_unsatisfiable`, `intern(subsumers_of(C))`. Population,
   stated exactly: the union of `Vocabulary::classes()` and every id appearing in `facts` **in
   either source or target position** — *not* `0..max_fact_id+1`, whose unreferenced ids have no
   meaningful rows. `Vocabulary::classes()` includes DKey and other synthetic interned classes:
   harmless in the model, filtered in reporting. Also seed the ⊤-labeled element (§3).
   Unsatisfiable classes get **no element**; that is inertness hygiene, not a detection mechanism
   (§8 T6's catch happens in the evaluator).
5. **Expand to fixpoint.** For element `e`, each class `X ∈ L(e)`, each fact `(X, r, Y)`:
   `t = intern(target_label(Y, r))`; push `(e, t)` into `edges[r]`; enqueue `t` if new.

   **`target_label` must be TBox-CLOSED.** Measured: with `Range(u,F)` and `F ⊑ G`, a plain union
   gives `{A,F}`, missing `G`, so `SubClassOf(F,G)` reads `Violated` on a **healthy** pure-EL
   ontology (`tests/fixtures/label-closure-range-sub.ofn`). Unioning subsumer *closures* fixes that
   but still misses **conjunctive** triggers like `A ⊓ F ⊑ H`. So:

   ```text
   aug = eff_ranges(r) \ subsumers_of(Y)
   aug empty  ->  target_label = subsumers_of(Y)               // already closed
   otherwise  ->  mint fresh Q with an IRI carrying SYNTHETIC_CLASS_IRI_PREFIX,
                  add `EquivalentClasses(Q, And([Y] ++ aug))` to a CLONE of the
                  InternalOntology, re-saturate, use subsumers_of(Q) minus Q itself
   ```

   `aug` is empty in the common case — the saturator already range-wraps *outer* RHS existentials —
   so this fires only for **nested** markers and the `∃r.⊤` top-witness.

   **ITERATE TO A FIXPOINT.** **CORRECTED 2026-08-28 (measured during implementation):** this
   originally read "two runs are NOT enough — `cascade.ofn` needs three, and an `n`-deep nesting
   needs `n+1`". That analysis described the FACT-driven path only. Task 4b's axiom-driven expansion
   walks the axioms directly, removing the anchor-class limitation, and `cascade.ofn` measurably
   converges in **ONE** round at any `max_rounds >= 1` — it has no injectable gap. The fixpoint
   remains required in principle (an injection can expose new `(target, aug)` pairs) but **no fixture
   exercises more than one round**, so the loop past round 1 is untested machinery. The original
   reasoning is retained below because it still explains why the FACT path cannot stand alone. The pair `(M_{∃v.W}, {G})` is undiscoverable in pass 1 because its
   only incoming fact is the conclusion of a conjunctive `ConceptRule` whose trigger fires for no
   run-1 class; conditional `∃`-RHS have no anchor class, unlike told/Tseitin existentials (which
   are anchored because `WorklistEngine::seed` seeds every class reflexively — that reflexive
   seeding is why most cascades *are* covered in one pass, and it is exactly the argument two-run
   sufficiency needed and lacked). Rounds terminate: bounded by syntactic `∃`-nesting depth over a
   finite label lattice, and `max_rounds` trips `Unresolved { BoundTripped }`.

   **BUILD THE ENTIRE MODEL FROM THE FINAL AUGMENTED RUN.** This is not an optimisation; it closes
   two false-`Verified` channels:
   * **ClassId drift.** `TseitinAllocator::new(internal.vocabulary.num_classes())`
     (`crates/owl-dl-saturation/src/lib.rs:3398`) bases marker ids at the user-class count, so
     injecting `k` classes shifts **every** Tseitin id by `k`. Markers have no IRIs to remap by, so
     joining run-2 ids against run-1 facts mislabels elements arbitrarily.
   * **Authoritative-run confusion.** Taking seeds, facts or the unsat set from a later run lets the
     model verify an answer the user never received.

   So: seeds, facts and labels all come from the **final** run, and the **classification being
   verified** is the one the user actually got (run 1). **Any delta between run 1 and the final run
   on an ORIGINAL class is itself a `Violated`-grade defect signal** — direct evidence the shipped
   classification is incomplete. This is measured, not hypothetical: on `unsatnested.ofn`, injection
   flips the original class `X` from satisfiable to **unsatisfiable**, and HermiT agrees `X` is
   unsat.

   **Unsatisfiable `Q`.** If `is_unsatisfiable(Q)`, do **not** use the row as a label — it is
   truncated, not a model row. It also *means* the fact's source class is genuinely unsatisfiable,
   so if run 1 reported that source satisfiable, emit `Violated` (engine incompleteness).

   **Conservativity, stated correctly.** Injection is a **sound monotone extension**: the final run
   can only *add* entailed derivations among original classes, never remove or falsify one. The
   stronger claim "cannot change entailments among original classes" is **false at the level of
   derived output** (the `unsatnested.ofn` flip above), and the Hasse `direct_subsumptions` also
   restructures — the direct-vs-closure trap again.

6. **Materialise chain and transitive closure** to fixpoint: `Chain([r,u]) ⊑ v` adds `(a,v,c)` from
   `(a,r,b),(b,u,c)`; `TransitiveRole(Named(r))` closes `r`. Read via `has_edge` (sub-role aware),
   write to the **declared** role's vector. Sub-role inclusion is answered on demand and never
   materialised: it is a lookup, whereas chains and transitivity generate new pairs.

## 6. Data structures and API

```rust
// interp.rs
pub struct Element(u32);

pub trait Interpretation {
    fn domain_size(&self) -> usize;
    fn elements(&self) -> impl Iterator<Item = Element> + '_;
    fn in_concept(&self, e: Element, c: ClassId) -> bool;
    fn successors(&self, e: Element, r: RoleId) -> Vec<Element>;
    fn has_edge(&self, from: Element, r: RoleId, to: Element) -> bool;
    fn edges(&self, r: RoleId) -> Vec<(Element, Element)>;
    fn num_roles(&self) -> usize;
}
```

`successors`/`edges` return owned `Vec`s because both must **union over sub-roles**, which is not a
stored slice; `&[Element]` would be a promise satisfiable only by materialising the sub-role
closure. `has_edge` stays allocation-free. Note `impl Trait` in return position makes the trait
**not dyn-compatible** — fine for §9's generic evaluator; Phase 3 inherits the constraint.

```rust
// model.rs
pub struct FiniteModel { /* labels, label_ix, edges, hierarchy, class_of, provenance */ }

/// Type-state: ONLY this exposes `still_holds_after`. §3 precondition 1.
pub struct VerifiedModel(FiniteModel);

// lib.rs
pub struct Bounds { pub max_elements: usize, pub max_edges: usize, pub max_rounds: usize,
                    pub deadline: Option<Instant> }   // defaults 50_000 / 2_000_000 / 8 / None

pub enum UnresolvedReason {
    UnhandledAxiom   { axiom_index: usize, variant: &'static str },
    UnhandledConcept { axiom_index: usize, variant: &'static str },
    BoundTripped     { bound: &'static str, limit: Option<usize> },  // None = deadline
    GuardedRoleHasEdges     { role: RoleId },
    ChainRangeOutOfProfile  { chain_super: RoleId },
    LabelNotClosed          { class: ClassId, role: RoleId },
    RunDelta                { class: ClassId },   // §5 step 5 defect signal
}
pub enum Verdict {
    Verified   { axioms_checked: usize, domain_size: usize },
    Violated   { domain_size: usize, violations: Vec<Violation>,
                 unresolved: Vec<UnresolvedReason> },
    Unresolved { domain_size: usize, reasons: Vec<UnresolvedReason> },
}

pub fn build_model(internal: &InternalOntology, bounds: Bounds)
    -> Result<FiniteModel, UnresolvedReason>;

pub fn verify(model: FiniteModel, internal: &InternalOntology, deadline: Option<Instant>)
    -> (Verdict, Option<VerifiedModel>);          // Some(..) iff Verified

impl VerifiedModel {
    /// `Verified` ⇒ the classification reported for the ontology this model was built
    /// from remains valid: reported negatives are witnessed here, reported positives
    /// hold by monotonicity GIVEN they were correct (§3 precondition 2).
    /// ADDITIONS ONLY — says nothing about removals.
    pub fn still_holds_after(&self, pool: &ConceptPool, added: &[Axiom],
                             deadline: Option<Instant>) -> Verdict;
}
```

**Decisions that were ambiguous before, now fixed.**

* **`Violated` outranks `Unresolved`** — exit 2 — and still carries its `unresolved` rows so
  coverage is never hidden by a violation. `domain_size` is on **all three** variants, since
  `verify` consumes the model.
* **`Violation` must render witnesses inside `verify`**, while the model is alive:
  `Vocabulary::class_iri` **panics** on Tseitin ids, so synthetic elements render by label, and the
  caller cannot do this after the model is consumed.
* **`Bounds` govern construction; checking is bounded by the passed `deadline`.**
* **Out-of-model ids are empty extensions, never a panic.** A fresh `ClassId` is safe (binary search
  misses). A fresh `RoleId` would index past `edges` and `RoleHierarchy::{super,sub}_roles`
  **panics out of range** — and "the edit introduces a role" is the *normal* case for
  `still_holds_after`.
* **Re-intern recipe, named:** convert added axioms against the **original** tables via
  `owl_dl_core::convert::convert_component(&component, &mut vocab, &mut pool)`
  (`crates/owl-dl-core/src/convert.rs:1889`). Re-converting the whole edited ontology yields a
  **fresh pool** and silently wrong `ClassId`s.
* The caller must also check `dropped` did not grow; a grown `dropped` invalidates `Verified`.

**CLI:** `rustdl verify-el <file> [--json]`, exit **0** `Verified`, **2** `Violated`, **3**
`Unresolved`, **1** I/O and parse errors. The fragment check uses the public
`owl_dl_reasoner::analyze_fragment(&internal) == FragmentClassification::PureEl` — `is_pure_el`
itself is `pub(crate)` and unexported, so it is unreachable even from the CLI.

## 7. Architecture

```
crates/owl-dl-verify/
  src/interp.rs  — Interpretation, Element
  src/model.rs   — FiniteModel, VerifiedModel, builder   [MAY use owl-dl-saturation]
  src/eval.rs    — evaluator, GENERIC over Interpretation [owl-dl-core IR ONLY]
  src/lib.rs     — Verdict, build_model, verify
```

Deps: `owl-dl-core`, `owl-dl-saturation`; **not** `owl-dl-reasoner` (cycle; the CLI wires them).
Dev-deps: `horned-owl` for fixture parsing.

**The load-bearing rule:** `eval.rs` is generic over `Interpretation` and resolves concepts only via
`ConceptPool` — *data*, not saturation logic — so it **cannot reach the engine it checks**. Using
saturation to *build* is fine; the guarantee comes from the independent *check*.

Workspace: add to `members` **and** `default-members`; `[lints] workspace = true` is **mandatory**
or the crate escapes pedantic and `unwrap_used`; add to `[workspace.dependencies]` with the frozen
internal `version = "0.4.5"`. `deny.toml`, xtask and CI need no change.

## 8. The evaluator

Wildcard-free `match` over `Axiom` (`crates/owl-dl-core/src/ontology.rs:36`, **25** variants) and
`ConceptExpr` (`crates/owl-dl-core/src/ir.rs:165`, **12**). Counts verified exact.
`BareRoleDecls::analyze` (`crates/owl-dl-reasoner/src/classify.rs:1970`) is a wildcard-free match
over all 25 — **copy it; it is private, do not call it.** Alias `use ConceptExpr as CE;` so
`CE::Some` cannot capture `Option::Some`.

**Concepts — 5 checked:** `Top`→true; `Bot`→false; `Atomic(c)`→`in_concept`; `And`→all operands;
`Some(Named(r), body)`→∃ successor satisfying `body`. **`Some(Inverse(_), _)`** and `Nominal`,
`SelfRestriction`, `Not`, `Or`, `All`, `Min`, `Max` → `Unresolved`.

**Axioms — 13 checked:** declarations vacuous; `SubClassOf` ∀e; `EquivalentClasses` all members
agree ∀e; `DisjointClasses` at most one member ∀e; `SubObjectPropertyOf{Role(r),sup}` over
`edges(r)`; `SubObjectPropertyOf{Chain([r,u]),v}`; `EquivalentObjectProperties` pairwise;
`TransitiveRole`; `Domain` over edge sources; `Range` over edge targets; `SymmetricRole(r)` verifies
`edges(r)` empty; `InverseObjectProperties(p,q)` verifies **both** `edges(p)` and `edges(q)` empty
(the gate's guard requires both unread). `Chain` with `len != 2`, any inverse-polarity role, any
`EquivalentObjectProperties` containing an inverse, and the 12 unchecked variants → `Unresolved`.

The gate uses **three different filler predicates** (`is_el_concept`, `is_atomic_concept`,
`is_atomic_or_trivial_concept`). The evaluator does not replicate them — it evaluates whatever
concept is present and reports `Unresolved` on a form it cannot handle.

## 9. Validation

**A prior suite let all 24 tests pass while 10 of the 13 axiom evaluators were `true` stubs**, and
passed T3 *more* easily than an honest implementation, since stubs cannot fire spuriously. The
repair is a per-variant sabotage matrix.

### 9.1 Per-variant sabotage matrix (the core)

For **each of the 13 checked axiom variants**: one fixture where the axiom genuinely constrains the
model, plus one mutation — drop a closure fact, delete a materialised edge, or hand-mutate a label —
that MUST flip `Verified → Violated` **with the specific axiom index and witness element pinned**.
An unpinned expectation is satisfied by any garbage violation. Without this matrix a stub evaluator
is indistinguishable from a working one.

### 9.2 Acceptance tests — real, live, oracle-adjudicated defects

| # | fixture | expected |
|---|---|---|
| A1 | `chainpoison.ofn` | `Violated`, pinning `ObjectPropertyDomain(r,⊥)`, witness `x_C` |
| A2 | `chain-range-bot.ofn` | `Violated`, pinning `ObjectPropertyRange(r,⊥)`, chain-target witness |
| A3 | `cascade.ofn` | `Violated` — Konclude-confirmed `A ⊑ FINAL`, rustdl emits 0 rows |
| A4 | `unsatnested.ofn` | `Violated` or `RunDelta` — HermiT-confirmed `X ≡ ⊥` |
| A5 | `chainrange.ofn` | `Unresolved { ChainRangeOutOfProfile }` under §4; `Violated` once Phase 2 folds |
| A6 | `unsatconj.ofn`, `chainrange_ctl.ofn` | `Verified` — discriminating healthy controls |

**ACCEPTANCE TESTS MUST NOT ASSERT `Violated` DIRECTLY — they would break when the engine is
fixed.** A1–A5 are now filed as issues #80/#81/#82, so the engine defects they detect are expected
to be repaired, and a test asserting `Violated` would then fail *because the codebase improved* —
the `#[ignore]`d-sentinel trap in reverse (a test whose green depends on a bug persisting).

Phrase each acceptance test as the **stable invariant** instead:

> On fixture `F` with committed oracle verdict `O`, the instrument must **not** return `Verified`
> whenever rustdl's own classification disagrees with `O`.

That holds in both engine states: while the defect is live the instrument must report `Violated`
(or `Unresolved`, which is honest but weaker — log which); once the defect is fixed rustdl agrees
with `O`, the antecedent is false, and the test passes without modification. Commit each oracle
verdict beside its fixture (`<fixture>.oracle`) with its provenance — **Konclude-confirmed** for
#80, #81 and #82-domain, **derivation-only** for #82-range, where both reasoners miss it. A test
whose oracle is derivation-only must say so in its failure message, so nobody later mistakes it for
peer-confirmed.

Corollary for sequencing: **fixing #80/#81 removes two of this instrument's six live prey**, but the
tests survive the fix by construction, and the residual value is unchanged — the instrument exists
to catch the *next* instance, not these five.

A2 is the **second crown jewel**, not a refusal test: `Bot` is skipped from `eff_ranges` (§5 step 3)
so the refusal cannot fire, and the case has perfect discrimination — a complete engine leaves `C`
unsatisfiable, hence unseeded, hence no chain edge, hence the range passes vacuously (`Verified`),
while the D10 miss seeds `C` and yields `Violated`.

### 9.3 Inertness

**T3's population is ORE, not the curated corpus** — measured: only **1 of 15** curated files is
pure-EL (`go-basic.ofn`), and at **51,967 classes** it exceeds `max_elements: 50_000`, so v1's T3
would have produced **zero `Verified`** verdicts and passed on an empty set. Two independent samples
put ORE at **23/60** and **11/50** pure-EL (~420–730 corpus-wide). Members, spanning 35 → 45,462
classes: `ore_ont_13204, 3263, 11274, 4918, 2672, 10742, 2022, 3102, 5115, 4570, 3919, 13752,
12161, 4733, 16114, 5487, 13902, 11739, 16687, 14879`. **All 20 verified unaffected by the §4
refusal.** `ore_ont_1357` (60,973) and `ore_ont_283` (59,937) are free `BoundTripped` cases;
`go-basic.ofn` is a documented `BoundTripped` case and is **not** raised past — §10 forbids that.
Expectation is **`Verified` specifically**, never "zero `Violated`" (an always-`Unresolved`
implementation also has zero violations).

**Injection is corpus-rare: 0 injections across 6 real pure-EL ontologies** (0 nested/⊤ fillers
under range-bearing roles). So T3 will not exercise §5 step 5 at all — the injection path needs
**synthetic** fixtures, including one separating the full injection from the cheaper closure-union
(`A ⊓ F ⊑ H`), and a `LabelNotClosed` case.

### 9.4 Remaining tests

Independence (`eval.rs` source-scan, **sabotage-verified** by adding a saturation `use` and
confirming failure — a bad regex matches nothing and passes forever); Probe B distinctness asserting
the successor **exists** and carries `F`; anti-vacuity (drop a derived subsumption ⇒ `Violated`);
`still_holds_after` positive, **negative** (`Δ` that genuinely changes the classification ⇒
`Violated`), unhandled-form ⇒ `Unresolved`, fresh-role no-panic, and the compile-time type-state
check; `BoundTripped` for `max_elements`, `max_edges`, `max_rounds` and deadline; determinism
(`--json` twice byte-identical — rustdl shipped exactly this bug in `justify`/`report`, #59, and
`FiniteModel` holds hash maps); exit-code mapping; out-of-fragment refusal; unhandled variants as a
**loop over all 12+7**, including unhandled *concepts* in `Δ`; guard tests both ways;
`subsumers_of` reflexivity, on which §3 and §5 step 5 both depend; and the Phase-2 blind spot pinned
(`RUSTDL_DKEY_ONEOF_SEED=0` ⇒ `Verified` — its fixture is confirmed banner pure-EL).

`RUSTDL_*` flags are process-global: replicate the reasoner's `test_env_lock`/`EnvGuard` convention
or shell out to the CLI. **The builder must reach the saturator through the same env-flag-sensitive
path**, or the flag sabotages test nothing.

### 9.5 Ordering

Inertness first — a violation carries no information until spurious violations are zero. But as a
*work* order this is a hazard: "drive spurious violations to zero" is a tuning loop in which
weakening the evaluator also makes T3 green. Two rules separate repair from suppression:

1. **`axioms_checked` must never decrease across a tuning step.** A builder change may create new
   `Verified`s; an evaluator change may only move an axiom `Violated → Unresolved` — visible and
   counted — never to a silent pass.
2. **Run the acceptance tests (§9.2) continuously DURING the inertness phase**, not after. The
   signature of suppression is an acceptance test flipping away from `Violated`. A calibration pair
   is only a calibration pair while it is armed.

## 10. Bounds

Domain size is bounded by distinct label sets; checking is `O(|axioms| × |domain|)`; chain and
transitive closure are worse; injection adds one saturation run per round. **A gate for fixtures and
small-to-medium ORE**, not galen-scale. A tripped bound yields `Unresolved { BoundTripped }` naming
the bound — reported, never silent. **Do not raise a bound to make a sweep finish.**

## 11. APIs this must write

All verified private or absent: total class count including Tseitin (workaround: max fact id + 1);
class iterator over synthetics; `RoleHierarchy` from `InternalOntology` (the reasoner's
`build_role_hierarchy` and the saturator's `build_role_super` are private); transitivity accessor
(scan `Axiom::TransitiveRole`); chain accessor (`collect_chain_axioms` is private); effective ranges;
the §5 injection recipe — `InternalOntology` is `Clone` with all-`pub` fields and
`Vocabulary::intern_class` / `ConceptPool::{atomic, and}` are public, so injection needs no new API,
but `Q` must carry `SYNTHETIC_CLASS_IRI_PREFIX` (reachable only as
`owl_dl_core::residual_absorbability::SYNTHETIC_CLASS_IRI_PREFIX`; `DKEY_IRI_PREFIX` *is* root
re-exported) and note `class_iri(Q)` will **not** panic, so item 8's render-by-label rule does not
automatically shield `Q`-labeled elements. `reportable_class_iris` does not exist; `ReportedClasses`
is private.

**Fix in passing:** `crates/owl-dl-saturation/src/lib.rs:103` documents `RUSTDL_EL_BOT_FILLER` as
"Default OFF"; the predicate at `:149` is `is_none_or(|v| v != "0")` — **ON**.

**Fixture note:** the in-tree `#[ignore]`d `conjunctive_unsat.rs:441` writes
`SubObjectPropertyChain`; the OFN keyword is `ObjectPropertyChain`, so it would not parse. Being
`#[ignore]`d it never had to. Do not copy it.
