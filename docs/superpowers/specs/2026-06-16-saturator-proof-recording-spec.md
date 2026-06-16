# Saturator proof recording — implementation spec

**Date:** 2026-06-16
**Status:** Draft — spec only (no .rs edits)
**Author:** rustdl (Michel Dumontier + Claude)
**Track:** B (proofs) from `docs/superpowers/plans/2026-06-16-konclude-parity-with-proofs.md`

---

## 0. Goal and scope

Add **opt-in, zero-cost-when-off** inference recording to the *production* EL
saturator (`crates/owl-dl-saturation/src/lib.rs`). When enabled, every derived
fact — subsumer edge, existential fact, unsatisfiability flag — is annotated
with the rule that produced it and the premises it consumed. The annotation is
a parallel side-table that never perturbs the existing bitset/Vec data
structures, so verdicts are byte-identical to the non-recording path.

After classification, a `prove(sub, sup)` API walks the side-table backward from
`sub ⊑ sup` to the ontology-axiom leaves and renders a DL proof tree in
Manchester syntax. For entailments that the saturator **did not** derive (SROIQ
fragment, tableau-only), the API falls back to the shipped black-box
justification (`owl_dl_reasoner::justify::find_one_justification`).

**In scope:**
- EL/Horn saturation fragment (told ⊑, ⊓, ∃, role hierarchy, chains,
  transitivity, domain, disjointness/⊥, functional witness-merge, nominal/DKey
  lowering).
- Step-level proofs for the saturation-derived portion.
- Justification fallback (axiom set, no step-proof) for out-of-saturation pairs.

**Out of scope (this spec):**
- Proof recording in the tableau (`owl-dl-tableau`) — different engine, later work.
- Changing any reasoning verdict or default behaviour.
- Track A perf work (racing; isolated in a separate worktree).

---

## 1. Opt-in flag and zero-cost-off design

### 1.1 Environment gate

```
RUSTDL_PROOF=1   # enable proof recording (default OFF)
```

Read once at startup via the same atomic-load pattern used by `hyper_enabled()`,
`trust_sat_enabled()`, etc. in `owl-dl-reasoner/src/lib.rs`:

```rust
fn proof_enabled() -> bool {
    std::env::var("RUSTDL_PROOF").as_deref() == Ok("1")
}
```

No `set_var` anywhere — respects the `unsafe_code` deny on the workspace.

### 1.2 Public API

Add a sibling entry point alongside `pub fn saturate`:

```rust
/// Saturate and, if `RUSTDL_PROOF=1` or `record_proofs` is set in
/// `SaturateConfig`, also record per-fact inference steps in a
/// `ProofTrace`. The trace is `None` when recording is off.
pub fn saturate_with_config(internal: &InternalOntology, cfg: &SaturateConfig)
    -> (Subsumers, Option<ProofTrace>)
```

```rust
pub struct SaturateConfig {
    /// Whether to record proof steps (also gated by RUSTDL_PROOF env).
    pub record_proofs: bool,
}

impl Default for SaturateConfig {
    fn default() -> Self { Self { record_proofs: proof_enabled() } }
}
```

The existing `pub fn saturate` becomes a one-liner:
```rust
pub fn saturate(internal: &InternalOntology) -> Subsumers {
    saturate_with_config(internal, &SaturateConfig::default()).0
}
```

The `rustdl prove` CLI subcommand (Track B.3) calls `saturate_with_config` with
`record_proofs: true` regardless of the env var.

### 1.3 Zero-cost guarantee

When `record_proofs` is `false`:
- `WorklistEngine` carries `record_proofs: bool` (one field, no box/alloc).
- Every recording call site is `if self.record_proofs { ... }` — a single
  predictable branch on a cached `bool`; modern branch predictors treat it as
  free on the off path.
- `proof_trace` is `Option<ProofTrace>` initialized to `None` when off — no
  allocation whatsoever.

**Gate:** a perf run with `RUSTDL_PROOF=0` (the default) must be wall-time
identical to the pre-recording baseline on galen and notgalen (within noise).

---

## 2. Derived-fact id scheme and the ProofTrace type

### 2.1 Derived-fact key

There are three categories of derived facts. Index them by content, not by
internal Vec index (which is not stable across different orderings and would
couple the proof table to allocator internals):

```rust
/// A derived fact produced by the EL saturator.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DerivedFact {
    /// A subsumer edge: `sub ⊑ sup`.
    Sub(ClassId, ClassId),
    /// An existential fact: `sub ⊑ ∃role.target`.
    Exist(ClassId, RoleId, ClassId),
    /// Unsatisfiability: `class ⊑ ⊥`.
    Unsat(ClassId),
}
```

`DerivedFact::Exist` is keyed identically to `WorklistEngine::seen_facts`
(the `(ClassId, RoleId, ClassId)` triple), so membership tests are O(1) and
the maps never diverge.

### 2.2 Axiom reference

An **axiom leaf** is a reference into `InternalOntology::axioms`. To keep
the proof trace self-contained without cloning axioms:

```rust
/// A reference to an ontology axiom by index into
/// `InternalOntology::axioms`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AxiomRef(pub usize);
```

One axiom may lower to many rules; a `DerivedFact` that is a direct
consequence of one ontology axiom (reflexivity is the only "no-axiom" case)
uses a `Vec<AxiomRef>` with the relevant axiom indices.

### 2.3 Inference node

```rust
/// One step in a proof: how a derived fact was produced.
#[derive(Debug, Clone)]
pub struct Inference {
    /// Which EL rule fired.
    pub rule: ElRule,
    /// Premises: prior derived facts this step consumed.
    /// Empty for direct-axiom leaves (reflexivity / told-subsumption /
    /// told-existential). All premises are `DerivedFact::Sub` or
    /// `DerivedFact::Exist`; `DerivedFact::Unsat` never appears as a
    /// premise (it is always a conclusion of this type or of the
    /// unsat-from-disjointness rule).
    pub premise_facts: Vec<DerivedFact>,
    /// The axiom(s) that justify this step (may be empty for rules
    /// that derive purely from previously derived facts, e.g.
    /// transitivity).
    pub axiom_refs: Vec<AxiomRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElRule {
    /// `C ⊑ C` seeded at start.
    Reflexivity,
    /// Direct `A ⊑ B` from `SubClassOf(A,B)` or `EquivalentClasses`.
    ToldSubsumer,
    /// Direct `A ⊑ ∃R.B` from `SubClassOf(A, ∃R.B)` or equivalent.
    ToldFact,
    /// Phase D4: `A ⊑ ⊥` from data-axiom preprocessing.
    ToldUnsat,
    /// Transitivity of ⊑: `A ⊑ B`, `B ⊑ C` ⟹ `A ⊑ C`.
    SubsumerTransitivity,
    /// Conjunctive trigger: all `Bᵢ ∈ supers(C)` ⟹ `C ⊑ head`.
    ConjunctiveTrigger,
    /// CR5 existential propagation: `sub ⊑ ∃r.T`, `T ⊑ body`, `∃r.body ⊑ head`
    /// ⟹ `sub ⊑ head`.
    ExistentialTrigger,
    /// Domain axiom: `sub ⊑ ∃r.T`, `Domain(r,D)` ⟹ `sub ⊑ D`.
    Domain,
    /// Role hierarchy (CR9): `(sub, r, T)`, `r ⊑ s` ⟹ `(sub, s, T)`.
    RoleHierarchy,
    /// Role chain: `(A, r1, X)`, `(X, r2, T)`, `r1∘r2 ⊑ s` ⟹ `(A, s, T)`.
    RoleChain,
    /// Phase 2d fact inheritance: `X ⊑ D`, `(D, r, T)` ⟹ `(X, r, T)`.
    FactInheritance,
    /// Disjointness → unsat: `C ⊑ A`, `C ⊑ B`, `Disjoint(A,B)` ⟹ `C ⊑ ⊥`.
    DisjointnessClash,
    /// Unsat propagation from subsumer: `C ⊑ D`, `D ⊑ ⊥` ⟹ `C ⊑ ⊥`.
    UnsatSubsumer,
    /// Unsat propagation from existential target: `C ⊑ ∃r.T`, `T ⊑ ⊥` ⟹ `C ⊑ ⊥`.
    UnsatTarget,
    /// Tseitin decomposition: `F ≡ B₁⊓…⊓Bₙ` ⟹ `F ⊑ Bᵢ`.
    TseitinDecomp,
    /// Tseitin conjunction: all bodies `Bᵢ ∈ supers(C)` ⟹ `C ⊑ F`.
    TseitinConj,
    /// Phase 2a functional witness-merge: multiple sub-role facts →
    /// merged synthetic. (High-risk; see §5.)
    FunctionalMerge,
    /// Phase 2c-redux: merged synthetic back-propagated to sub-role.
    FunctionalMergeSubRole,
    /// ABox nominal transitive propagation: `X ⊑ ∃R.{a}`, `a R⁺ b`
    /// (R transitive) ⟹ `X ⊑ ∃R.{b}`. (High-risk; see §5.)
    NominalTransitiveProp,
    /// Cluster-B ForallKey: functional R + `∃R.{a}`, a∈S ⟹ `C ⊑ ForallKey(R,S)`.
    ForallKeyDerived,
    /// Cluster-B MaxKey: `C ⊑ ≤1 R` + `∃R.{a}`, a∈S ⟹ `C ⊑ ForallKey(R,S)`.
    MaxKeyDerived,
}
```

### 2.4 ProofTrace

```rust
/// Side-table mapping every derived fact to the inference step that
/// first produced it. `first-writer-wins`: when multiple rules could
/// derive the same fact, only the first one is recorded. This is
/// sound — any valid derivation path is a valid proof — and guarantees
/// the DAG is acyclic (premises were derived earlier in the fixpoint,
/// so they are already in the table when the new entry is written).
pub struct ProofTrace {
    pub steps: HashMap<DerivedFact, Inference>,
    /// Synthetic-class definitions, for human-readable rendering.
    /// `F → SyntheticDef(...)`. Populated once after
    /// `collect_el_rules` from the TseitinAllocator's reverse maps.
    pub synthetic_defs: HashMap<ClassId, SyntheticDef>,
}

#[derive(Debug, Clone)]
pub enum SyntheticDef {
    /// Tseitin: `F ≡ B₁⊓…⊓Bₙ`; bodies are user ClassIds (or other
    /// synthetics, resolved recursively).
    TseitinConj(Vec<ClassId>),
    /// Existential marker (one-way): `∃R.B ⊑ M`.
    ExistMarkerOneWay { role: RoleId, body: ClassId },
    /// Existential marker (two-way): `M ≡ ∃R.B`.
    ExistMarkerEquiv { role: RoleId, body: ClassId },
    /// Nominal key: stands in for individual `{a}`.
    NominalKey(IndividualId),
    /// MaxKey: stands in for `≤n R`.
    MaxKey { n: u32, role: RoleId },
    /// ForallKey: stands in for `∀R.OneOf(S)`.
    ForallKey { role: RoleId, members: Vec<IndividualId> },
    /// DKey (datatype): stands in for a datatype range interval.
    DKey(String),  // the IRI suffix identifies bucket + bounds
}
```

The `ProofTrace` is built once, alongside the `Subsumers`, and handed to the
caller. It is read-only from the reasoner's perspective; extraction and
rendering are in `owl-dl-reasoner` (§4).

---

## 3. Per-rule chokepoint table

For each rule, the table identifies:
- The derivation **call site** (function + approximate line range in `lib.rs`)
- The `DerivedFact` produced
- The premises (as `DerivedFact` references)
- The axiom refs (which `internal.axioms` indices to tag)
- Recording notes and risks

### Axiom provenance recovery

In `collect_el_rules` and `lower_sub_class_of`, axiom lowering happens in a
loop over `internal.axioms`. For each axiom at index `ax_idx`:

```rust
let before_atomic = rules.atomic_subsumptions.len();
let before_conj   = rules.conjunctive_triggers.len();
let before_facts  = rules.existential_facts.len();
let before_trigs  = rules.existential_triggers.len();
let before_disjt  = rules.disjoint_pairs.len();
let before_chain  = rules.chain_axioms.len();
let before_dom    = rules.role_domains.len();  // per-key, needs different tracking

// ... lower the axiom ...

// Tag ranges [before_X..rules.X.len()] with ax_idx
// in parallel provenance side-tables (Vec<AxiomRef> parallel to each rule Vec).
```

This is exactly the `before_atomic`/`before_conjunctive` range-snapshotting
pattern already used in `introduce_runtime_synthetic` (lines 300–309). The
provenance side-tables are only built when `record_proofs` is true, so they
incur zero cost on the off path.

### Rule table

| # | Rule name | Call site | DerivedFact produced | Premises | Axiom refs | Notes / Risk |
|---|-----------|-----------|----------------------|----------|------------|--------------|
| R0 | **Reflexivity** | `seed()` L379–391 | `Sub(C,C)` | none | none | No axiom; one entry per user class |
| R1 | **ToldSubsumer** | `seed()` L393–395 + `process_subsumer` entry from queue | `Sub(A,B)` | none | axiom index for `SubClassOf(A,B)` | Lowered in `lower_sub_class_of` → `atomic_subsumptions`; provenance tagged at collection |
| R2 | **ToldFact** | `seed()` L398–400 | `Exist(A,r,T)` | none | axiom for `SubClassOf(A,∃r.T)` | Lowered to `existential_facts`; provenance tagged |
| R3 | **ToldUnsat** (D4) | `seed()` L407–410 via `enqueue_unsat` | `Unsat(C)` | none | axiom for `SubClassOf(C,⊥)` | From `rules.directly_unsat`; provenance tagged |
| R4 | **SubsumerTransitivity (forward)** | `process_subsumer` L543–546 (`for e in supers_of_d`) | `Sub(C,E)` | `[Sub(C,D), Sub(D,E)]` | none | `D` just became a subsumer of `C`; `E` was already known |
| R5 | **SubsumerTransitivity (backward)** | `process_subsumer` L549–552 (`for x in subs_of_c`) | `Sub(X,D)` | `[Sub(X,C), Sub(C,D)]` | none | `C` just gained `D`; propagate to pre-existing subs of `C` |
| R6 | **UnsatSubsumer** | `process_subsumer` L554–556 | `Unsat(C)` | `[Sub(C,D), Unsat(D)]` | none | `D` is unsat; `C ⊑ D` ⟹ `C ⊑ ⊥` |
| R7 | **ConjunctiveTrigger** | `process_subsumer` L559–570 | `Sub(C,head)` | `[Sub(C,Bᵢ) for each body Bᵢ]` | axiom for the GCI trigger | All bodies present; `enqueue_subsumer(c, head)` |
| R8 | **DisjointnessClash** | `process_subsumer` L574–580 | `Unsat(C)` | `[Sub(C,A), Sub(C,B)]` + disjoint pair | axiom for `DisjointClasses(A,B)` | `A,B` disjoint; both subsumers of `C` |
| R9 | **ExistentialTrigger (target-side new subsumer)** | `process_subsumer` L583–606 | `Sub(Y,head)` | `[Exist(sub,r,C), Sub(C,body), Sub(sub,Y)]` (or `Sub(sub,head)` when Y=sub) | axiom for `∃r.body ⊑ head` | A new subsumer `D=body` on target `C`; every fact pointing to `C` via a compatible role fires |
| R10 | **ExistentialTrigger (sub-side new subsumer)** | `process_subsumer` L611–642 | `Sub(C,head)` | `[Sub(C,D), Exist(D,r,T), Sub(T,body)]` | axiom for `∃r.body ⊑ head` | `C` just gained `D`; fire triggers via `D`'s facts |
| R11 | **Domain** (sub-side subsumer) | `process_subsumer` L631–642 | `Sub(C,dom)` or `Sub(Y,dom)` | `[Sub(C,D), Exist(D,r,_)]` | axiom for `ObjectPropertyDomain(r,dom)` | `C` gains `D`; `D` has an `r`-fact; `r` or super-role has domain `dom` |
| R12 | **FactInheritance** (Phase 2d, subsumer-side) | `process_subsumer` L656–667 | `Exist(C,r,T)` | `[Sub(C,D), Exist(D,r,T)]` | none | `C ⊑ D`, `D ⊑ ∃r.T` ⟹ `C ⊑ ∃r.T`. Risk: see §5 |
| R13 | **RoleChain (subsumer-triggered)** | `process_subsumer` L668–700 | `Exist(A,sup,T)` | `[Exist(A,r1,C), Sub(C,D), Exist(D,r2,T)]` | axiom for `r1∘r2 ⊑ sup` | New subsumer `D` on chain middle node `C` |
| R14 | **MaxKey-driven ForallKey (process_subsumer)** | `process_subsumer` L530–540 | `Sub(C,ForallKey)` | `[Sub(C,MaxKey1R), Exist(C,R,{a})]` | axiom for `≤1 R` + `ForallKey` def | ≤1-driven path; symmetric to R22 |
| R15 | **NominalTransitiveProp** | `process_fact` L707–722 | `Exist(X,R,NomKey(b))` | `[Exist(X,R,NomKey(a))]` | none (ABox transitive closure pre-built) | Premises hard to recover; see §5 |
| R16 | **ForallKey (functional, process_fact)** | `process_fact` L723–752 | `Sub(C,ForallKey)` | `[Exist(C,R,NomKey(a))]` + functionality axiom | axiom for `FunctionalObjectProperty(R)` or functional super-role | functional ∃ + nominal ⟹ ForallKey |
| R17 | **Domain (process_fact)** | `process_fact` L771–789 | `Sub(sub,dom)` | `[Exist(sub,r,_)]` | axiom for `ObjectPropertyDomain(r,dom)` | Role super closure used |
| R18 | **UnsatTarget** | `process_fact` L793–795 | `Unsat(sub)` | `[Exist(sub,r,T), Unsat(T)]` | none | Target is unsat; source forced unsat |
| R19 | **ExistentialTrigger (fact-triggered)** | `process_fact` L800–818 | `Sub(sub,head)` or `Sub(Y,head)` | `[Exist(sub,r,T), Sub(T,body)]` | axiom for `∃r.body ⊑ head` | New fact arrives; check triggers on target's subsumers |
| R20 | **RoleChain (fact head / tail)** | `process_fact` L820–861 | `Exist(A,sup,T)` | `[Exist(A,r1,X), Exist(X,r2,T)]` (or sub-role variants) | axiom for `r1∘r2 ⊑ sup` | Both head and tail arcs exist |
| R21 | **FunctionalMerge (Phase 2a)** | `process_fact` L862–972 (manual insert ~908–914 **and** `push_fact` ~957–971) | `Exist(sub,rf,synthetic)` | accumulated prior sub-role facts `[Exist(sub,Rᵢ,Tᵢ)]` | axiom for `FunctionalObjectProperty(rf)` + sub-role axioms | **HIGHEST RISK** — see §5 |
| R22 | **FunctionalMerge sub-role back-prop (Phase 2c-redux / 2e)** | `process_fact` L957–972 via `push_fact` | `Exist(sub,other.role,synthetic)` | `[Exist(sub,other.role,_)]` + merge synthetic | functionality axiom | Companion to R21 |
| R23 | **UnsatPropagation (process_unsat, subclass)** | `process_unsat` L984–986 | `Unsat(D)` | `[Unsat(C), Sub(D,C)]` | none | `D ⊑ C`, `C ⊑ ⊥` ⟹ `D ⊑ ⊥` |
| R24 | **UnsatPropagation (process_unsat, fact-source)** | `process_unsat` L989–994 | `Unsat(fact.sub)` | `[Unsat(C), Exist(fact.sub,r,C)]` | none | Source of a fact pointing to an unsat class |
| R25 | **TseitinDecomp** | `collect_el_rules` / `TseitinAllocator::introduce` L1339–1343 | `Sub(F,Bᵢ)` | none | axiom that caused the body to be Tseitin-introduced | Synthetic axiom seeded from `introduce`; provenance = originating SubClassOf/Equiv |
| R26 | **TseitinConj** | via conjunctive trigger whose head is `F` (R7) | `Sub(C,F)` | `[Sub(C,Bᵢ) for each body Bᵢ]` | axiom that introduced the conjunctive trigger | Same machinery as R7; just the head is synthetic |
| R27 | **RoleHierarchy (CR9) — implicit via supers_of()** | Throughout (e.g. `process_fact` L753, `process_subsumer` L589) | `Exist(sub,s,T)` from `Exist(sub,r,T)` with `r⊑s` | `[Exist(sub,r,T)]` + role hierarchy path | `SubObjectPropertyOf` axiom(s) | Not a distinct call site — `supers_of()` is consulted inline; recording wrapper must add explicit role-hierarchy derivation steps. See §5. |

### Notes on trigger indexing

Rules R7, R9, R10, R19 all fire through `enqueue_subsumer(c, head)`. The
recording wrapper must capture the trigger index (`tidx`) at the call site to
look up which axiom caused the trigger, and record the correct set of premise
facts.

---

## 4. Extraction algorithm: prove(sub, sup) → proof tree

### 4.1 Interface

```rust
/// Backward DAG from `sub ⊑ sup` to ontology-axiom leaves.
/// Returns `None` if the saturator did not derive `sub ⊑ sup`
/// (out-of-fragment; caller should use the justification fallback).
pub fn prove_subsumption(
    trace: &ProofTrace,
    sub: ClassId,
    sup: ClassId,
) -> Option<ProofNode>

/// A node in the extracted proof DAG.
pub struct ProofNode {
    pub conclusion: DerivedFact,
    pub rule: ElRule,
    pub axiom_refs: Vec<AxiomRef>,
    pub premises: Vec<ProofNode>,
}
```

### 4.2 Algorithm

```
prove(fact: DerivedFact) -> Option<ProofNode>:
  if fact not in trace.steps: return None
  inf = trace.steps[fact]
  premise_nodes = inf.premise_facts.map(prove).collect()
  // all premises are Some (they were derived earlier — monotone)
  return ProofNode { conclusion: fact, rule: inf.rule,
                     axiom_refs: inf.axiom_refs, premises: premise_nodes }
```

**Acyclicity:** premises appear in `trace.steps` strictly before the conclusion
(first-writer-wins; the worklist is monotone). Therefore the backward walk
terminates. Reflexivity and ToldSubsumer/ToldFact/ToldUnsat have empty
`premise_facts` — they are the leaves.

**Shared sub-derivations (DAG, not tree):** a fact `Sub(X,C)` may be a premise
of multiple conclusions. Memoize `prove` by `DerivedFact` to avoid exponential
re-expansion of shared lemmas, returning a reference-counted node.

### 4.3 Manchester rendering

Each `ProofNode` renders as:

```
(rule) premises ⊢ conclusion
```

where:
- `conclusion` is rendered as a Manchester `SubClassOf` / `SubObjectPropertyOf`
  expression via `horned_owl::io::omn::AsManchester`.
- Synthetic class ids are expanded via `trace.synthetic_defs` before rendering:
  - `TseitinConj(bodies)` → `ObjectIntersectionOf(bodies…)` (inlined)
  - `ExistMarker{role,body}` → `ObjectSomeValuesFrom(role, body)` (inlined)
  - `NominalKey(a)` → `{a}` / `ObjectHasValue(role, a)` depending on context
  - `MaxKey{n,role}` → `ObjectMaxCardinality(n, role)`
  - `ForallKey{role,members}` → `ObjectAllValuesFrom(role, ObjectOneOf(members…))`
  - `DKey` → shown as the corresponding range notation
- Axiom refs are rendered as the axiom text from the original ontology.

Recursion depth is bounded by the proof DAG depth, which equals the number of
saturation rounds — finite by the ELK termination argument.

### 4.4 Justification fallback

When `trace.steps` does not contain `Sub(sub, sup)`, the pair was not derived
by the saturator (SROIQ / tableau-only). Return:

```rust
pub enum ProveResult {
    /// Step-level proof from the EL saturator.
    SaturatorProof(ProofNode),
    /// Entailment is sound but not in the saturation fragment;
    /// an axiom-level justification is provided instead.
    JustificationFallback(Justification),
    /// The entailment is not held by the ontology.
    NotEntailed,
}
```

The fallback path calls `owl_dl_reasoner::justify::find_one_justification` with
the current `SetOntology`. This is the shipped `rustdl justify` backend, which
handles the full OWL DL fragment by black-box re-checking.

---

## 5. Recording-design risks (hard premises)

These are the rules where premises are **not directly available at the emission
site** and require additional bookkeeping or approximations.

### Risk 1 (HIGH): Functional witness-merge (R21, Phase 2a/2c/2d/2e, lines 862–972)

**Problem.** The emitted `Exist(sub, rf, synthetic)` is produced when a *second*
sub-role fact arrives and causes `merged_atom_sets[(sub,rf)]` to grow. But the
first sub-role fact that seeded the accumulation was processed earlier and is
not retained as a direct pointer. The merged `synthetic` encodes *all* prior
contributions, not just the triggering pair.

**Proposed solution.** When `record_proofs` is true, maintain a parallel
side-table:

```rust
/// For each (sub, rf) key, the list of existential fact DerivedFacts
/// that have contributed to the merged atom set so far.
merge_contributors: HashMap<(ClassId, RoleId), Vec<DerivedFact>>,
```

At each merge-triggering arrival, update `merge_contributors[(sub,rf)]` with
the current fact (`Exist(sub, original_role, T)`). When the synthetic is emitted,
record `premise_facts = merge_contributors[(sub,rf)].clone()` plus the
`FunctionalObjectProperty(rf)` axiom and any `SubObjectPropertyOf` axioms that
put sub-roles under `rf`. This is a **complete** premises list (all contributing
facts), though not minimal (a minimal proof would need only two of them to
exhibit the merge; full set is sound).

**Also note:** the functional witness-merge emits the synthetic fact via a
**manual insert** (lines ~908–914, `facts.push` + `todo_fact.push_back`
bypassing `push_fact`) in addition to the `push_fact` call at line 968 for
the back-propagation step. Recording must hook **both** sites separately:
- Manual insert (lines 908–914): record as R21 (`FunctionalMerge`)
- Back-prop `push_fact` (lines 963–971): record as R22 (`FunctionalMergeSubRole`)

### Risk 2 (MEDIUM): ABox nominal transitive propagation (R15, lines 707–722)

**Problem.** `abox_nominal_reach[(R, NomKey(a))]` is a pre-computed transitive
closure over the ABox; the individual step-proof `a R b` / `b R c` ... is
discarded. The `Exist(X,R,NomKey(b))` derivation's premise is recorded as
`Exist(X,R,NomKey(a))` (the triggering fact) but the intermediate path
`a R⁺ b` is not recoverable from the runtime state.

**Proposed solution.** In `build_abox_nominal_reach`, when `record_proofs` is
true, additionally build a parallel path table:

```rust
/// For each transitive-closure pair (role, NomKey(a), NomKey(b)),
/// the sequence of ABox ObjectPropertyAssertion indices witnessing a R⁺ b.
abox_path: HashMap<(RoleId, ClassId, ClassId), Vec<AxiomRef>>,
```

Store this alongside `abox_nominal_reach`. Then R15's recorded premise is
`[Exist(X,R,NomKey(a))]` and `axiom_refs = abox_path[(R,NomKey(a),NomKey(b))]`.
This is a slightly verbose proof but fully faithful: each `AxiomRef` in the path
is an `ObjectPropertyAssertion` in the original ontology.

If `abox_path` is too large to justify the implementation cost, a tolerable
approximation is to record the axiom_refs as the **set** of all
`ObjectPropertyAssertion(R,_,_)` in the ABox, annotating the inference as
"ABox transitive closure" — not step-precise but sound (the axioms suffice to
re-derive the entailment).

### Risk 3 (MEDIUM): Role-hierarchy (CR9) is implicit (R27)

**Problem.** Role hierarchy is not applied as a separate rule step; `supers_of(r)`
is called inline whenever a fact is processed, and the resulting super-roles feed
trigger lookups directly. There is no explicit "derive `Exist(sub,s,T)` from
`Exist(sub,r,T)` with `r⊑s`" step; instead, the rule fires *inside* trigger
evaluation.

**Proposed solution.** When `record_proofs` is true, at each `supers_of()` call
that feeds a trigger, if a super-role `s ≠ r` matches, explicitly record a
synthetic R27 step `Exist(sub,s,T)` with premise `Exist(sub,r,T)` and
`axiom_refs = [SubObjectPropertyOf(r,s)]` before proceeding to the trigger.
This expands the proof to include the role-lifting step, which is semantically
correct and more informative.

### Risk 4 (LOW): Phase 2d fact inheritance call-stack ambiguity (R12, push_fact recursion lines 499–516)

**Problem.** `push_fact` inherits a fact to all sub-classes of `fact.sub`
recursively. When called recursively (sub = some `C` ≠ the original fact's sub),
the "parent" fact is `Exist(D,r,T)` from which `C` inherited, but `push_fact`
only receives the new `Exist(C,r,T)` triple without context.

**Proposed solution.** Add an optional premise parameter to the inner inheritance
call (gated by `record_proofs`):

```rust
// Internal only (record path):
fn push_fact_with_parent(
    &mut self,
    fact: ExistentialFact,
    parent: Option<DerivedFact>,  // Some(Exist(D,r,T)) when inheriting
    subsumer_edge: Option<DerivedFact>,  // Some(Sub(C,D)) when inheriting
) -> Option<usize>
```

The public `push_fact` becomes a wrapper that calls `push_fact_with_parent` with
`None, None`. The R12 recording at `process_subsumer` L656–667 passes the parent
fact and the `Sub(C,D)` subsumer edge. This keeps the off-path signature identical.

### Verdict on low-risk rules

All other rules (R0–R13 excluding the above, R14, R16–R20, R23–R26) have
**directly available premises** at the call site: the triggering `(c,d)` or the
fact index `idx` carries the exact (sub,d,target) triples needed. Recording them
is a mechanical `if self.record_proofs { self.proof_trace.steps.insert(...) }`
guard at the relevant `enqueue_subsumer` / `enqueue_unsat` / `push_fact` call.

---

## 6. Faithfulness proof-checker design

A proof-checker validates that each recorded step is a correct rule instance:

```rust
pub fn check_proof(
    node: &ProofNode,
    trace: &ProofTrace,
    internal: &InternalOntology,
) -> Result<(), CheckError>
```

For each `ProofNode`:
1. **Re-derive:** apply the stated `rule` to the stated `premise_facts` and the
   stated `axiom_refs`, and check that the result matches `conclusion`.
2. **Recurse** on each premise node.
3. **Axiom validity:** verify each `AxiomRef` index is in bounds and the
   referenced axiom matches the rule's expected form.

Rule-specific checks (examples):
- `SubsumerTransitivity`: premises are `Sub(C,D)` and `Sub(D,E)`; conclusion is `Sub(C,E)`.
- `ConjunctiveTrigger`: all premise `Sub(C,Bᵢ)` match a `conjunctive_triggers` entry
  with those bodies and the stated head.
- `ExistentialTrigger`: premise `Exist(sub,r,T)` + `Sub(T,body)` match an
  `existential_triggers` entry.
- `RoleChain`: premises `Exist(A,r1,X)` + `Exist(X,r2,T)` match a `chain_axioms` entry.
- `FunctionalMerge`: stated premises are a subset of the facts in the
  `merge_contributors` entry and the emitted synthetic's atomic content is the
  union of their targets' atomic content.

The checker is cheap (one pass over the proof, no fixpoint) and runs in tests.
**It does NOT run on the hot classify path** — it is opt-in (`--verify-proof`
flag on the `prove` CLI subcommand, or explicit in the test suite).

---

## 7. Smoke tests

### 7.1 EL chain proof (saturation-derived)

Ontology: the `existential_propagation_pizza_food` test in `lib.rs` (line 2399):
```ofn
Pizza ⊑ ∃hasTopping.Topping
Topping ⊑ EdibleThing
∃hasTopping.EdibleThing ⊑ FoodItem
```

Expected proof steps (schematic):
```
(R1/ToldSubsumer)         ⊢  Topping ⊑ EdibleThing
(R2/ToldFact)             ⊢  Pizza ⊑ ∃hasTopping.Topping
(R4/SubsumerTransitivity) {Topping ⊑ EdibleThing} ⊢  Topping ∈ supers(Topping) → Topping ⊑ EdibleThing
(R19/ExistentialTrigger)  {Pizza ⊑ ∃hasTopping.Topping, Topping ⊑ EdibleThing} ⊢  Pizza ⊑ FoodItem
```

The smoke test asserts:
- `prove_subsumption(trace, Pizza, FoodItem)` returns `SaturatorProof(_)`.
- The returned `ProofNode` has rule `ExistentialTrigger` at the root.
- Its premises include nodes for `ToldFact(Pizza, hasTopping, Topping)` and
  `ToldSubsumer(Topping, EdibleThing)`.
- The proof-checker passes on the returned tree.

### 7.2 Role-chain proof

Ontology: `role_chain_propagates_through_two_existentials` (line 2446):
```ofn
Niece ⊑ ∃hasParent.Parent
Parent ⊑ ∃hasBrother.Man
SubObjectPropertyOf(ObjectPropertyChain(hasParent hasBrother) hasUncle)
∃hasUncle.Man ⊑ HasUncle
```

Expected proof: `Niece ⊑ HasUncle` via R20 (RoleChain) → R19 (ExistentialTrigger).

### 7.3 Out-of-fragment justification fallback

Ontology: disjunctive subsumption `A ⊑ B ⊔ C`, `B ⊑ D`, `C ⊑ D` ⟹ `A ⊑ D`.
The saturator misses this (EL doesn't have disjunction). Expected:
- `prove_subsumption(trace, A, D)` returns `JustificationFallback(j)`.
- `j` contains exactly the three axioms above.

### 7.4 Proof-checker passes on R21 (functional merge)

Synthetic: `functional_role_merge_body_on_sub_role` canary (Phase 2e). Record
the merge, extract the proof, check that the `FunctionalMerge` node lists all
contributing facts and that their union matches the synthetic's `atomic_content_of`.

### 7.5 Zero-cost off-path (perf gate)

Run `cargo bench` (or the corpus harness) on galen with `RUSTDL_PROOF=0`
(default) before and after the patch. Wall time must be within noise (< 2%).
This gate is enforced before the PR merges.

---

## 8. Integration with `rustdl prove` CLI (Track B.3)

The `prove` subcommand:

```
rustdl prove <file> <sub_iri> <sup_iri>
```

1. Parses the ontology.
2. Calls `saturate_with_config(internal, SaturateConfig { record_proofs: true })`.
3. Calls `prove_subsumption(trace, sub, sup)`.
4. Dispatches on `ProveResult`:
   - `SaturatorProof(node)`: renders the proof tree (depth-first, indented).
   - `JustificationFallback(j)`: prints "Step proof unavailable (out of EL
     saturation fragment). Axiom justification:" followed by the axiom set in
     Manchester.
   - `NotEntailed`: prints "NOT entailed".

The default `classify` path is unaffected (`SaturateConfig::default()` reads
`RUSTDL_PROOF`, which defaults to `0`).

---

## 9. Honest scope summary

| Fragment | Proof type | Completeness |
|----------|-----------|--------------|
| Pure EL (told ⊑, ⊓ left/right, ∃ both sides, role hierarchy, chains) | Step-level DL proof tree | Complete for the saturation-derived pairs |
| EL++ (functional witness-merge, nominal lowering, DKey) | Step-level proof tree with R21/R15 notes | Complete; R21 premises are the full contributing set (not minimal) |
| Horn (saturator complete, fast path) | Step-level proof tree | Complete |
| SROIQ (disjunction, ∀, cardinality, inverses, tableau-only) | Axiom-level justification fallback | Sound; not step-precise |

**Soundness invariant:** proof recording is purely observational. The side-table
is write-only with respect to the closure (never read back during saturation).
Verdicts are byte-identical to the non-recording path. The standing gate
`FP=0/MISSED=0` is re-verified with `RUSTDL_PROOF=1` on the full corpus
before the track B.1 PR merges.

---

## 10. Files to create / modify

| File | Change |
|------|--------|
| `crates/owl-dl-saturation/src/proof.rs` | New: `DerivedFact`, `Inference`, `ElRule`, `ProofTrace`, `SyntheticDef`, `ProofNode`, `prove_subsumption`, `check_proof` |
| `crates/owl-dl-saturation/src/lib.rs` | Add `SaturateConfig`, `saturate_with_config`; add `record_proofs: bool` + `proof_trace: Option<ProofTrace>` + `merge_contributors` to `WorklistEngine`; add `if self.record_proofs { ... }` guards at each rule site per §3 |
| `crates/owl-dl-reasoner/src/lib.rs` | Expose `ProveResult`, `saturate_with_proofs` (calls `saturate_with_config`), `prove` API |
| `crates/owl-dl-cli/src/main.rs` | Add `prove` subcommand (§8) |
| `crates/owl-dl-saturation/src/tests/proof_tests.rs` | Smoke tests §7 |

No other crates need changes for Track B.1/B.2. Track B.3 adds the CLI subcommand.

---

*Spec saved incrementally to `docs/superpowers/specs/2026-06-16-saturator-proof-recording-spec.md`.*
