# Negative certificates, Phase 1 — verified canonical model + independent axiom evaluator

**Date:** 2026-08-27
**Status:** design approved, plan pending
**Scoping evidence:** `docs/benchmarks/2026-08-27-negative-certificates-scoping.md`
**Origin:** `docs/benchmarks/2026-08-27-km-lean-proof-backing.md` § "The idea actually worth taking"

## 1. Why

rustdl can prove **positives** (`justify`, `prove`). It has **no artifact for a negative**. Every
completeness claim therefore rests on an external oracle (Konclude ∪ HermiT): the FP=0 net is
FP-shaped, and the MISSED net needs a 400-ontology oracle population at ~10 min per arm and is
drawn from *completers*, so it structurally cannot see the defect below.

**The target defect class is D10** — a fragment gate certifying COMPLETE while the engine silently
drops an axiom. Six recorded instances. It is the worst failure mode in the system: a wrong answer
*plus* `incomplete: false`. It is found today only by hand-grepping the engine per admitted
construct.

**It is live on the default path.** Measured 2026-08-27 on the fixture behind the `#[ignore]`d
`nested_existential_poisoned_role_via_chain` (`crates/owl-dl-reasoner/tests/conjunctive_unsat.rs:441`)
— `SubObjectPropertyOf(ObjectPropertyChain(t,u), r)`, `ObjectPropertyDomain(r, owl:Nothing)`,
`SubClassOf(C, ∃t.∃u.A)`, where `C` **is** unsatisfiable:

```
consistent   : True
unsatisfiable: []          <- WRONG
incomplete   : False       <- claims completeness
# mode: pure EL (saturation-only)
# fragment: pure-EL (trust_sat sound by construction; saturator alone is complete)
```

No existing gate catches this.

## 2. Goals and non-goals

**Goals.**

1. An internal self-verification gate detecting D10 defects on the `is_pure_el` path, needing **no
   peer reasoner**.
2. A **verified finite model** as a first-class, reusable artifact — the shared substrate for
   negative certificates and incremental re-checking.
3. A thin incremental API: given axioms added to the ontology, decide whether the previously
   reported classification remains valid.

**Non-goals (Phase 1).** Each is deferred deliberately, with its successor phase named.

* **Parse-tree axiom evaluation** → Phase 2. Phase 1 evaluates the lowered IR, so
  conversion-level drops are invisible. Blind spot pinned by a test (§9).
* **Serialized certificate / external checker** → Phase 3. Format is the hardest thing to change
  later; settle semantics first.
* **Functional / inverse-functional roles and SROIQ.** The canonical model is not functional; one
  element can acquire two `r`-successors, giving spurious violations indistinguishable from real
  ones. `saturator_complete_fragment` admits functional roles, which is why the scope is
  `is_pure_el` instead.
* **Wiring into `classify`.** Phase 1 changes **no reasoning behaviour**. It is diagnostic only.
* **Axiom removal.** Negatives survive removal free by monotonicity, but positives need the
  justification half; out of scope.

## 3. The theory this rests on

Three facts, in the order they matter.

**(a) For EL the canonical model is universal — but the admitted fragment EXCEEDS the theorem.**
One finite interpretation `M` with `C ⊑ D` iff
`x_C ∈ D^I`. So a single model witnesses *every* non-subsumption; there is no need for the
per-class shape the label cache uses.

**(b) A model witnesses negatives soundly; a pre-model does not.** Any model of the KB refutes any
subsumption it falsifies. rustdl's existing per-class witness is a `Sat` **completion** — a
pre-model, whose labels hold only what one branch was forced to derive. That is precisely why
`RUSTDL_PSEUDO_MODEL` loses entailed types, and why an incremental check built on it would be
unsound. **Verifying that `M` is a model is what upgrades it into evidence** — so the D10 gate is
not a detour toward the incremental prize, it is its acceptance test.

**(c) The incremental claim, stated exactly.** `M` witnesses exactly the *reported* negatives,
because `x_C`'s label is by construction `subsumers_of(C)`, and a reported negative is
`D ∉ subsumers(C)`. Therefore: if every axiom of an added set `Δ` holds in `M`, then
`M ⊨ KB ∪ Δ`, so every reported negative still holds; every reported positive still holds by
monotonicity. **One model check proves the entire reported classification still valid.** This never
assumes the closure was complete — only that `M` is a model — so there is no circularity.

**REVISED v2 — the fragment is out of the OWL 2 EL profile.** `is_el_axiom` admits
`ObjectPropertyRange` and 2-leg `ObjectPropertyChain` **independently, with no interaction
restriction**, but OWL 2 EL globally forbids a range on a property implied by a chain
(Baader-Brandt-Lutz 2008) — precisely because the unrestricted combination breaks the
canonical-model technique. The gap is live in BOTH directions: the **engine** is incomplete there
(two new D10 instances, scoping-doc appendix), and the **construction** yields a false `Violated` on
the *benign* combination, because a chain-materialised edge's target never receives `eff_ranges` of
the chain's super-role. `build_model` therefore **refuses**: if any role carrying a non-trivial
effective range is the super-role of an admitted chain, return
`Unresolved { ChainRangeOutOfProfile }`. Mirroring the profile restriction is honest and cheap;
folding ranges into chain targets risks the divergence the restriction exists to prevent.

**Two preconditions on the incremental claim, both load-bearing.**

1. **`M` must have been verified.** The theorem needs `M ⊨ KB`; a `still_holds_after` on an
   unverified or `Violated` model returns a `Verified` carrying no guarantee. Enforced by
   **type-state**: only `VerifiedModel` — produced by `verify` returning `Verified` — exposes the
   method. `FiniteModel` alone does not.
2. **"Positives survive by monotonicity" presupposes the reported positives were correct.** This is
   a completeness instrument, not an FP net, so the claim is conditional on soundness, which rustdl
   establishes separately. §8's doc comment must carry that premise.

**Scope of the negatives covered.** `M` witnesses reported negatives **over the named classes it
seeds**. `owl:Thing` lowers to `ConceptExpr::Top`, not an `Atomic` class, so it is absent from
`Vocabulary::classes()` and no element carries the ⊤-subsumer label. This costs no soundness today —
measured: adding `SubClassOf(owl:Thing, A)` to `{D ⊑ A}` leaves `classify --json`
**byte-identical**, because rustdl's reported classification never mentions `owl:Thing`. Seed one
⊤-labeled element anyway (classes `X` with `⊤ ⊑ X`; normally empty) so the contract does not
silently depend on that reporting convention.

**Direction of risk.** The instrument is a **completeness** (MISSED) net, not an FP net. It checks
that the reported closure admits a model; it does not check minimality. A spurious subsumption
makes `M` "too full" and is caught only when it violates a **disjointness, domain, or range**
axiom — the §6 step-5 union rule turns a spurious subsumption into spurious *edges*, so the channels
are broader than disjointness alone, though still the noisy direction. Do not advertise it
as an FP gate — every existing rustdl gate is already FP-shaped, and this fills the other hole.

## 4. Architecture

```
crates/owl-dl-verify/
  src/interp.rs  — Interpretation trait, Element
  src/model.rs   — FiniteModel + builder     [MAY use owl-dl-saturation]
  src/eval.rs    — evaluator, GENERIC over Interpretation  [owl-dl-core IR ONLY]
  src/lib.rs     — Verdict, public API, still_holds_after
```

`Cargo.toml` dependencies: `owl-dl-core`, `owl-dl-saturation`. **Not** `owl-dl-reasoner` — that
would cycle; the CLI wires them.

**The load-bearing architectural rule:** `eval.rs` is generic over the `Interpretation` trait and
resolves concepts only through `ConceptPool`, which is *data*, not saturation logic. It therefore
**structurally cannot reach the engine it checks**. An evaluator sharing code with the saturator
could hide the very bug it exists to find. Enforce with a test asserting `eval.rs` contains no
`owl_dl_saturation` path (see §8, T1).

## 5. Data structures

```rust
// interp.rs
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Element(u32);
impl Element { pub const fn new(i: u32) -> Self; pub const fn index(self) -> u32; }

pub trait Interpretation {
    fn domain_size(&self) -> usize;
    fn elements(&self) -> impl Iterator<Item = Element> + '_;
    /// Is `e` in the extension of atomic class `c`?
    fn in_concept(&self, e: Element, c: ClassId) -> bool;
    /// Successors of `e` under `r`, INCLUDING those held by any sub-role of `r`.
    fn successors(&self, e: Element, r: RoleId) -> Vec<Element>;
    fn has_edge(&self, from: Element, r: RoleId, to: Element) -> bool;
    /// Every edge of `r` (incl. sub-role edges), for whole-extension axioms.
    fn edges(&self, r: RoleId) -> Vec<(Element, Element)>;
    fn num_roles(&self) -> usize;
}
```

**Why `successors` and `edges` return owned `Vec`s rather than slices or borrowed iterators.**
Both must **union over sub-roles** — a sub-role edge is also a super-role edge — and that union is
not a stored contiguous slice. Returning `&[Element]` would be a false promise satisfiable only by
materialising the sub-role closure, which §6 deliberately avoids. Allocation is acceptable here:
this is a diagnostic instrument, not the reasoner hot loop. `has_edge` stays allocation-free
(it short-circuits over `sub_roles`).

```rust
// model.rs
pub struct FiniteModel {
    labels:   Vec<Box<[ClassId]>>,              // per element, sorted ascending
    label_ix: HashMap<Box<[ClassId]>, Element>, // interning key
    edges:    Vec<Vec<(Element, Element)>>,     // indexed by RoleId, DECLARED role only
    hierarchy: RoleHierarchy,                   // resolves sub-role queries on demand
    class_of: HashMap<ClassId, Element>,        // seed element per satisfiable class
    bounds:   Bounds,
}
```

`labels` sorted ascending makes `in_concept` a binary search and makes the interning key canonical.
`edges` stores only declared-role edges; sub-role inclusion is answered on demand, so that closure
is never materialised.

```rust
// lib.rs
pub struct Bounds { pub max_elements: usize, pub max_edges: usize, pub deadline: Option<Instant> }
impl Default for Bounds { /* max_elements: 50_000, max_edges: 2_000_000, deadline: None */ }

pub struct Violation { pub axiom_index: usize, pub axiom: Axiom, pub witness: Vec<Element>, pub note: String }
pub enum UnresolvedReason {
    UnhandledAxiom { axiom_index: usize, variant: &'static str },
    UnhandledConcept { axiom_index: usize, variant: &'static str },
    BoundTripped { bound: &'static str, limit: Option<usize> }, // None = deadline trip
    ChainRangeOutOfProfile { chain_super: RoleId },              // §3a
    LabelNotClosed { class: ClassId, role: RoleId },             // §6 step 5 fallback
    GuardedRoleHasEdges { role: RoleId },   // see §7, guarded variants
}
pub enum Verdict {
    Verified   { axioms_checked: usize, domain_size: usize },
    Violated   (Vec<Violation>),
    Unresolved (Vec<UnresolvedReason>),
}
```

**`Unresolved` is never `Verified`.** Three outcomes, three exit codes. A tripped bound names the
bound. This is the KM discipline: their checker *rejects* `unresolved` rather than assuming it.

## 6. The construction

Inputs: `&InternalOntology`. Steps, in order:

1. `let (subs, facts, _nom) = owl_dl_saturation::saturate_with_exists_facts(internal);`
   (`crates/owl-dl-saturation/src/lib.rs:235`) → `(Subsumers, Vec<(ClassId, RoleId, ClassId)>,
   HashMap<ClassId, IndividualId>)`. Facts range over the **extended** class space (Tseitin markers
   included) and are sorted by `(sub, role, target)`.

2. **Role hierarchy.** Scan `internal.axioms`: `SubObjectPropertyOf { sub: SubRolePath::Role(r),
   sup }` with both non-inverse → `add_sub_role(r, sup)`; `EquivalentObjectProperties(roles)` →
   both directions pairwise. Feed `RoleHierarchyBuilder` (`crates/owl-dl-core/src/role_hierarchy.rs:19`),
   `build()` closes reflexive-transitively.
   *Do not* replicate the reasoner's inverse canonicalization: `is_pure_el` admits no inverse-role
   use, and any inverse occurrence puts the ontology out of fragment.

3. **Effective ranges.** For each `ObjectPropertyRange { role: Role::Named(r), range }` where
   `internal.concepts.get(range)` is `Atomic(c) | Top` (skip `Bot` — a `Bot` range means the role
   has no edges; that is checked as an axiom, not baked into labels), record `c`. Then
   `eff_ranges(r) = ⋃ { ranges(s) : s ∈ hierarchy.super_roles(r) }`. `super_roles` is reflexive, so
   `r`'s own ranges are included. **Direction check:** if `r ⊑ s` then an `r`-edge is also an
   `s`-edge, so `Range(s)` constrains `r`-successors — hence super-roles, not sub-roles.

4. **Seed elements.** For every class `C` in the extended space that is **not**
   `subs.is_unsatisfiable(C)`: `intern(subs.subsumers_of(C))` and record in `class_of`.
   Unsatisfiable classes get **no element** — this is what catches `RUSTDL_EL_BOT_FILLER` (§8, T5).
   **Seeding population, stated exactly** (two engineers would otherwise differ): the union of
   `Vocabulary::classes()` and every id appearing in `facts` **in either the source or the target
   position**. Do *not* seed `0..max_fact_id+1` — ids in no fact are unreachable and their
   `subsumers_of` rows are not meaningful here. `Vocabulary::classes()` includes DKey and other
   synthetic interned classes: harmless in the model, but reporting must filter them. Also seed the
   one ⊤-labeled element per §3c.

5. **Expand to fixpoint.** Worklist of elements. For element `e` with label `L(e)`, for every class
   `X ∈ L(e)` and every fact `(X, r, Y)`:
   `target_label = subs.subsumers_of(Y) ∪ eff_ranges(r)`; `t = intern(target_label)`; push edge
   `(e, t)` into `edges[r]`; enqueue `t` if new.
   The range term is the Probe B fix — without it the `u`-successor of `∃u.A` would be the same
   element as `A`-as-a-class, and `ObjectPropertyRange(u, F)` would read as violated when nothing is
   wrong. Two elements coincide exactly when their labels do.

   **REVISED v2 — the label must be TBox-CLOSED; a plain union is a false-`Violated` generator.**
   Measured: with `Range(u,F)` and `F ⊑ G`, the plain union gives `{A, F}`, missing `G`, so
   `SubClassOf(F, G)` reads violated on a **healthy** pure-EL ontology
   (`tests/fixtures/label-closure-range-sub.ofn`). Unioning subsumer *closures* fixes that case but
   still misses **conjunctive** triggers like `A ⊓ F ⊑ H`. So:

   ```text
   aug = eff_ranges(r) \ subs.subsumers_of(Y)
   aug empty  ->  target_label = subs.subsumers_of(Y)           // already closed
   otherwise  ->  inject  Q ≡ Y ⊓ ⨅aug  into a COPY of the InternalOntology,
                  re-saturate ONCE, use subs2.subsumers_of(Q)   // fully closed
   ```

   Collect every needed `(Y, aug)` in a first pass so exactly **two** saturation runs occur. A fresh
   defined class is a **conservative extension** — it cannot change entailments among original
   classes — and the check still runs against the **original** axioms, so independence holds: using
   saturation to *build* is fine, because the guarantee comes from the independent *check*. If a
   pair cannot be injected, emit `Unresolved { LabelNotClosed }` rather than guessing.

   `aug` is empty in the common case — the saturator already range-wraps outer RHS existentials — so
   this fires only for **nested** markers and the `∃r.⊤` top-witness.
   Termination: labels are interned and the label lattice is finite. Bound anyway
   (`max_elements`, `max_edges`) → `Unresolved`.

6. **Materialise chain and transitive closure.** To fixpoint:
   `SubObjectPropertyOf { sub: Chain([r, u]), sup: v }` — for `(a,r,b)` and `(b,u,c)`, add `(a,v,c)`;
   `TransitiveRole(Role::Named(r))` — for `(a,r,b)` and `(b,r,c)`, add `(a,r,c)`.
   Both use `has_edge` (sub-role aware) when *reading* and write to the declared role's vector.
   Sub-role inclusion itself is **not** materialised.

**Why materialise these two but not sub-role inclusion.** Sub-role inclusion is a pure lookup
(`∃ r ⊑ s`), cheap on demand. Chains and transitivity generate *new pairs*, so a demand-driven
answer would need its own search — and the chain case is exactly the live defect, so it must be
present in the model rather than computed inside the check.

## 7. The evaluator

Wildcard-free `match` over `Axiom` (`crates/owl-dl-core/src/ontology.rs:36`, **25** variants) and
`ConceptExpr` (`crates/owl-dl-core/src/ir.rs:165`, **12** variants).
`BareRoleDecls::analyze` (`crates/owl-dl-reasoner/src/classify.rs:1970`) is **private — copy it, do
not call it**. It is an existing
wildcard-free match over all 25 — use it as the template.

### Concepts — 5 checked, 7 → `Unresolved`

| variant | semantics |
|---|---|
| `Top` | `true` |
| `Bot` | `false` |
| `Atomic(c)` | `interp.in_concept(e, c)` |
| `And(ops)` | all operands hold (empty ⇒ `true`) |
| `Some(Role::Named(r), body)` | ∃ `t ∈ successors(e, r)` with `body` holding at `t` |
| `Some(Role::Inverse(_), _)` | `Unresolved` |
| `Nominal`, `SelfRestriction`, `Not`, `Or`, `All`, `Min`, `Max` | `Unresolved` |

### Axioms — 13 checked, 12 → `Unresolved`

| variant | check |
|---|---|
| `DeclareClass`, `DeclareObjectProperty`, `DeclareNamedIndividual` | vacuously true |
| `SubClassOf { sub, sup }` | ∀e. `eval(sub,e) ⟹ eval(sup,e)` |
| `EquivalentClasses(ms)` | ∀e. all members agree |
| `DisjointClasses(ms)` | ∀e. at most one member holds |
| `SubObjectPropertyOf { Role(r), sup }` | ∀`(a,b) ∈ edges(r)`. `has_edge(a, sup, b)` |
| `SubObjectPropertyOf { Chain([r,u]), v }` | ∀a,b,c. `(a,r,b) ∧ (b,u,c) ⟹ has_edge(a,v,c)` |
| `EquivalentObjectProperties(rs)` | pairwise, both directions |
| `TransitiveRole(Named(r))` | ∀a,b,c. `(a,r,b) ∧ (b,r,c) ⟹ has_edge(a,r,c)` |
| `ObjectPropertyDomain { Named(r), d }` | ∀`(a,_) ∈ edges(r)`. `eval(d, a)` |
| `ObjectPropertyRange { Named(r), rg }` | ∀`(_,b) ∈ edges(r)`. `eval(rg, b)` |
| `SymmetricRole(r)` | **verify `edges(r)` is empty**; if non-empty emit `GuardedRoleHasEdges` |
| `InverseObjectProperties(p,q)` | verify `edges(p)` **and** `edges(q)` are **both** empty — the gate's guard requires both roles unread |
| `SubObjectPropertyOf { Chain(c), _ }`, `c.len() != 2` | `Unresolved` — the gate admits only length 2; `Chain` is a `Vec`, so match `if let [r,u] = c.as_slice()` |
| `EquivalentObjectProperties` containing any `Role::Inverse` | `Unresolved` |
| any inverse-polarity role in the above | `Unresolved` |
| `DisjointUnion`, `DisjointObjectProperties`, `AsymmetricRole`, `ReflexiveRole`, `IrreflexiveRole`, `FunctionalRole`, `InverseFunctionalRole`, `ClassAssertion`, `ObjectPropertyAssertion`, `NegativeObjectPropertyAssertion`, `SameIndividual`, `DifferentIndividuals` | `Unresolved` |

**The two guarded variants deserve their rule spelled out.** `is_pure_el` admits `SymmetricRole` and
`InverseObjectProperties` only when `BareRoleDecls` proves the role **unread** — so it should have no
edges, and symmetry holds vacuously. We *verify* emptiness rather than assume it, because a
non-empty extension would mean the observability analysis is wrong, which is itself a finding worth
surfacing.

**Three distinct filler predicates exist in the gate** (`is_el_concept` for
`SubClassOf`/`EquivalentClasses`, `is_atomic_concept` for `DisjointClasses`,
`is_atomic_or_trivial_concept` for domain/range). The evaluator does not need to replicate them —
it evaluates whatever concept is present and reports `Unresolved` on a form it cannot handle. Do
**not** unify them or assume a filler's shape.

## 8. Public API and surfaces

```rust
pub fn build_model(internal: &InternalOntology, bounds: Bounds)
    -> Result<FiniteModel, UnresolvedReason>;

/// Check every axiom of `internal` against `model`.
pub fn verify(model: FiniteModel, internal: &InternalOntology)
    -> (Verdict, Option<VerifiedModel>);   // Some(..) iff Verdict::Verified

impl FiniteModel {
    /// Check ONLY `added` against this model.
    ///
    /// `Verified` ⇒ the classification previously reported for the ontology this
    /// model was built from remains valid in full: reported negatives are witnessed
    /// by this model, reported positives hold by monotonicity.
    ///
    /// ADDITIONS ONLY. Says nothing about removals.
    pub fn still_holds_after(&self, pool: &ConceptPool, added: &[Axiom]) -> Verdict;
}
```

**`added` is lowered IR, not horned-owl — and that boundary is the caller's job.** A caller holding
an edited `SetOntology` must run `convert_ontology` on the edited ontology and diff the resulting
`Vec<Axiom>` against the original. Two consequences to document at the call site:

* A conversion that **drops** an added axiom makes it invisible here, so the caller MUST check that
  `dropped` did not grow. A grown `dropped` invalidates the `Verified` answer.
* The added axioms must be interned against **the same `ConceptPool`** the model was built from, or
  `ClassId`s will not correspond. Re-converting the whole edited ontology yields a fresh pool; the
  caller must therefore re-intern, and the API takes `pool` explicitly so this cannot be forgotten
  silently.

This is a genuine sharp edge and the reason Phase 4, not Phase 1, owns the editing loop.

**CLI:** `rustdl verify-el <file> [--json]`. Exit **0** `Verified`, **2** `Violated`, **3**
`Unresolved`, **1** reserved for I/O and parse errors. Distinct codes so a corpus sweep buckets
without parsing stdout. `--json` emits `{verdict, axioms_checked, domain_size, violations[],
unresolved[]}`.

**Contract points that were ambiguous in v1 and are now decided.**

* **`is_pure_el` is `pub(crate)` and NOT re-exported**, so nothing outside `owl-dl-reasoner` can call
  it — v1 cited an unreachable symbol. The CLI instead calls the public, re-exported
  `owl_dl_reasoner::analyze_fragment(&internal) == FragmentClassification::PureEl`, whose first
  branch is `is_pure_el` verbatim, including the `RUSTDL_FRAGMENT_BARE_DECL` sensitivity the §7
  guarded variants depend on. No reasoner change; the verify crate never needs the symbol, so the §4
  layering stands.
* **`Violated` outranks `Unresolved`** when a run produces both: exit 2, and `--json` still lists the
  `unresolved[]` rows so coverage is never hidden by a violation.
* **`Bounds` govern construction only.** `verify` and `still_holds_after` take an explicit
  `Option<Instant>` deadline rather than reading a stale `Instant` off the model; `FiniteModel`
  retains `bounds` for provenance only.
* **Out-of-model ids are empty, never a panic.** A fresh `ClassId` appears in no label (binary search
  misses) — safe. A fresh `RoleId` would index past `edges` and `RoleHierarchy::{super,sub}_roles`
  **panics out of range**, and "the edit introduces a role" is the *normal* case for
  `still_holds_after`. The implementation MUST bounds-check and treat an unknown role as having an
  empty extension. Pin with a test.
* **The re-intern recipe, named so nobody re-derives it wrongly.** Convert added axioms against the
  **original** ontology's tables via the public
  `owl_dl_core::convert::convert_component(&component, &mut vocab, &mut pool)`
  (`crates/owl-dl-core/src/convert.rs:1889`). Re-converting the whole edited ontology yields a
  **fresh pool** and silently wrong `ClassId`s — the exact failure §8's warning describes.

The command **refuses** an ontology outside the pure-EL fragment, reporting `Unresolved` with the reason —
it must not silently produce a verdict on an out-of-fragment input.

Nothing is wired into `classify`, `consistent`, or `realize`.

## 9. Validation plan — inertness first

Ordered deliberately: **a violation carries no information until spurious violations are zero.**

**T3 WAS EMPTY AS SPECIFIED IN v1 — measured, and it invalidated the ordering.** Only **1 of 15**
curated files reports `pure-EL` (`go-basic.ofn`), and at **51,967 declared classes** it exceeds the
§5 default `max_elements: 50_000`, so it yields `BoundTripped`, not `Verified`. T3 would have
produced **zero `Verified` verdicts** and the inertness gate would have passed on an empty set.

**Respecified population, measured 2026-08-27:** banner-selected pure-EL members of the ORE pool
(`/data/dumontier/ore-run/pool_sample/files`). Two independent samples agree the population is
ample — **23/60 (38%)** at stride 23 offset 7, **11/50 (22%)** at stride 38 — i.e. roughly 420–730
corpus-wide. Take members **under** the bound, spanning small-and-hand-checkable to near-bound: from
the stride-23 sample, `ore_ont_13204` (35 classes), `3263` (170), `11274` (451), `4918` (594),
`2672` (813), `10742` (1315), `2022` (1861), `3102` (2849), `5115` (6584), `4570` (6995),
`3919` (7044), `13752` (7123), `12161` (9703), `4733` (15359), `16114` (18437), `5487` (19892),
`13902` (20336), `11739` (26454), `16687` (32324), `14879` (45462). Two members just **over** the
bound come free as `BoundTripped` cases: `ore_ont_1357` (60,973) and `ore_ont_283` (59,937).
`go-basic.ofn` is kept as a deliberate documented `BoundTripped` case, **not** raised past — §11
forbids raising a bound to make a sweep finish.

**Additional tests v1 omitted** (each closes a way the suite could pass while the instrument is
useless):

| # | test | expected |
|---|---|---|
| T10 | `still_holds_after`: `Δ` that holds in `M` | `Verified` |
| T11 | **`still_holds_after` NEGATIVE — essential:** `Δ = [SubClassOf(A,B)]` where some element has `A` but not `B` | `Violated` |
| T12 | `still_holds_after` with an unhandled form in `Δ` (e.g. `FunctionalRole`) | `Unresolved`, never `Verified` |
| T13 | `still_holds_after` with `Δ` naming a **fresh role** | no panic; empty extension |
| T14 | type-state: `still_holds_after` is unreachable on a model that did not verify | compile-time |
| T15 | `Bounds { max_elements: 1 }` on any fixture | `Unresolved { BoundTripped }` naming `max_elements` |
| T16 | deadline trip | `Unresolved { BoundTripped { limit: None } }` |
| T17 | determinism: `verify-el --json` twice on the T4 fixture | byte-identical |
| T18 | exit-code mapping 0/2/3 | as specified |
| T19 | out-of-fragment input (`pizza.ofn`) | `Unresolved`, exit 3 |
| T20 | an unhandled axiom variant yields `Unresolved` (guards the no-wildcard rule) | `Unresolved` |
| T21 | guarded variants: bare `SymmetricRole` with empty extension ⇒ `Verified`; a hand-built model where it HAS edges ⇒ `GuardedRoleHasEdges` | both |
| T22 | **`Range(r,⊥)`-via-chain** (`tests/fixtures/chain-range-bot.ofn`) | `Unresolved { ChainRangeOutOfProfile }` — the profile refusal, and a distinct engine defect from T4 |
| T23 | `label-closure-range-sub.ofn` (healthy `Range(u,F)`+`F ⊑ G`) | **`Verified`** — pins the v2 label-closure fix; the v1 plain union gives `Violated` |
| T24 | `subsumers_of` reflexivity, on which §3c and §6-step-5 both depend | pass |

T15/T16 matter more than they look: a builder that silently truncates at a bound and returns
`Verified` on the truncated model passes every other test in the suite. T17 matters because rustdl
shipped exactly this bug in `justify`/`report` (#59: five different reports from one binary), and
`FiniteModel` holds two `HashMap`s.

**Determinism requirement on the guarded env-flag tests (T6, T9).** `RUSTDL_*` are process-global;
the reasoner crate uses a `test_env_lock`/`EnvGuard` convention for exactly this. The new crate must
either replicate that guard or shell out to the CLI binary. Also: **the model builder must reach the
saturator through the same env-flag-sensitive path**, or T6 tests nothing.

**Vacuity notes that change how tests are written.**
`T3` alone cannot detect an always-`Verified` implementation; it earns its keep only paired with
T6/T7, which change the closure itself and so would fail for a closure-echoing evaluator — that
pairing, not T1, is what actually enforces independence. `T2` must assert the successor
**exists** and carries `F` (a ∀-quantified phrasing passes vacuously on zero successors). `T4` must
assert **which** axiom is violated (`ObjectPropertyDomain(r, ⊥)`) and that the witness is `x_C`,
or any garbage violation counts. `T9` and `T5` are passed by an always-`Verified` implementation and
must not be counted as coverage — **the genuinely constraining sabotages are T6 and T7. Two.**

**The ordering is right for INTERPRETATION but is a hazard as a WORK order**, because "drive spurious
violations to zero" is a tuning loop in which *weakening the evaluator* also makes T3 green. Two
rules separate repair from suppression:

1. **`axioms_checked` must never decrease across a tuning step.** A builder change may legitimately
   create new `Verified`s; an evaluator change may only move an axiom from `Violated` to
   `Unresolved` — visible and counted — never to a silent pass.
2. **Run T4/T5/T6/T7 continuously DURING the inertness phase, not after it.** The signature of
   suppression is a tuning step flipping T4, T6 or T7 away from `Violated`. A calibration pair is
   only a calibration pair while it is armed.

| # | test | expected |
|---|---|---|
| T1 | `eval.rs` contains no `owl_dl_saturation` reference (source-scan test) | pass |
| T2 | label interning keeps Probe B's two `A` elements distinct: `C ⊑ ∃t.∃u.A` + `Range(u,F)` ⇒ the `u`-successor carries `F`, `x_A` does not | pass |
| T3 | **inertness** over banner-selected pure-EL fixtures (see the sizing note) | **`Verified`** specifically |
| T4 | **crown jewel:** chain-poison fixture, unsabotaged | `Violated` |
| T5 | calibration control: `nested_existential_unpoisoned_role_stays_sat` (role `:s`, no chain axiom) | `Verified` |
| T6 | sabotage `RUSTDL_EL_BOT_FILLER=0` | `Violated` |
| T7 | anti-vacuity: drop one derived subsumption from an accepted closure | `Violated` |
| T8 | meta-check: T4 disagrees with `is_pure_el`'s `# fragment: pure-EL` verdict | disagreement exists |
| T9 | **blind spot pinned:** sabotage `RUSTDL_DKEY_ONEOF_SEED=0` | `Verified` (documented blind spot; Phase 2 flips this) |

Notes that will otherwise cost a debugging cycle:

* **T3's fixture set must be selected empirically by the fragment banner**, not assumed.
  `is_pure_el` rejects `FunctionalRole`, so **galen does not qualify** despite being "EL". Run
  `rustdl classify <f> 2>&1 | grep '# fragment'` over the curated corpus and take the `pure-EL`
  rows.
* **T4/T5/T6/T9 fixtures must be committed under `crates/owl-dl-verify/tests/fixtures/`.** Working
  versions exist at `/tmp/km-cert-test/{chain-poison,bot_filler,dkey_oneof}.ofn` and `/tmp` is
  ephemeral.
* **OFN syntax:** the chain keyword is `ObjectPropertyChain`, **not** `SubObjectPropertyChain`. The
  in-tree `conjunctive_unsat.rs:441` fixture uses the latter and would fail to parse — being
  `#[ignore]`d, it has never had to. Fix when un-ignoring.
* **T9 is a test that asserts a known blind spot.** That is deliberate: it documents the Phase 1
  boundary in executable form and hands Phase 2 a failing target.
* Every guard must be **sabotage-verified**: break the guarded behaviour and confirm the test
  fails. A guard that passes under sabotage is not protecting anything.

## 10. Pre-existing API gaps this must write

Enumerated by inspection; all currently private or absent.

1. **Total class count including Tseitin** — `num_total_classes` is a local in the saturator and
   `Subsumers` exposes no row count or class iterator. Workaround for Phase 1: max id over `facts`
   plus one; synthetics appearing in no fact are unreachable in the model anyway. Note the
   workaround in code.
2. **Class iterator over synthetics** — none; named classes via `Vocabulary::classes()`
   (`crates/owl-dl-core/src/vocab.rs:134`).
3. **`RoleHierarchy` from `InternalOntology`** — the reasoner's `build_role_hierarchy`
   (`crates/owl-dl-reasoner/src/lib.rs:7503`) and the saturator's `build_role_super` are both
   private. Write our own per §6 step 2.
4. **Transitivity accessor** — none; scan `Axiom::TransitiveRole` (`ontology.rs:65`).
5. **Chain accessor** — `collect_chain_axioms` (`lib.rs:7610`) is private; scan
   `Axiom::SubObjectPropertyOf { sub: Chain(..) }`.
6. **Effective ranges of `r`** — computed twice as private locals inside the saturator; re-derive
   per §6 step 3.
7. **`SYNTHETIC_CLASS_IRI_PREFIX` is not root-re-exported** — use the full path
   `owl_dl_core::residual_absorbability::SYNTHETIC_CLASS_IRI_PREFIX` (the module is `pub`).
   `DKEY_IRI_PREFIX` *is* re-exported at the crate root.
8. **`reportable_class_iris` does not exist** as a function — only `ReportedClasses` (private,
   `classify.rs:73`). For reporting, filter with the public prefix constants
   `DKEY_IRI_PREFIX` (`convert.rs:51`) and `SYNTHETIC_CLASS_IRI_PREFIX`
   (`residual_absorbability.rs:44`). Note `Vocabulary::class_iri` **panics** on a Tseitin id — a
   `Violation` must render such elements by label, never by IRI.
9. **Doc drift to fix in passing:** `crates/owl-dl-saturation/src/lib.rs:103` documents
   `RUSTDL_EL_BOT_FILLER` as "Default OFF"; the predicate at line 149 and an empirical run both
   show default **ON**.

## 11. Bounds, and what they mean

Domain size is bounded by the number of distinct label sets; checking is
`O(|axioms| × |domain|)`; chain and transitive closure are worse. **This is a gate for fixtures and
small-to-medium ORE, not galen-scale and not the 981k-class ontologies.** A tripped bound yields
`Unresolved { BoundTripped }` naming the bound — reported, never silent. Do not raise a bound to
make a sweep finish; a bounded `Unresolved` is the correct answer.

## 12. Phase outlook

* **Phase 2 — parse-tree evaluation.** Evaluate horned-owl components instead of the lowered IR, so
  conversion-level drops become visible. Acceptance test: T9 flips from `Verified` to `Violated`.
  Requires datatype and data-range semantics, since data-property axioms have no `Axiom`
  representation — they are pre-lowered to DKey classes.
* **Phase 3 — serialized certificate + external checker.** KM's shape: emit the model and axioms,
  validate out of process. Only once the semantics are settled, because format is the hardest thing
  to change later.
* **Phase 4 — incremental classification surface.** rustdl has none today; the Protégé plugin spec
  (`docs/superpowers/specs/2026-07-24-protege-plugin-design.md` §2) names it deferred, with an edit
  marking the reasoner stale and forcing a full re-classify. `still_holds_after` is the negative
  half of the mechanism; `justify` is already the positive half.
