# Incremental reasoning — design

**Date:** 2026-08-18
**Status:** revised 2026-08-19 after adversarial design review (verdict: RETHINK-REQUIRED on
§3 and §5) and the lowering-floor spike
(`docs/2026-08-19-incremental-lowering-floor-findings.md`). Pending user review → implementation plan.
**Motivation:** `docs/nesy-research-agenda-2026-07-11.md:69` — "Incremental reasoning …
Load-bearing for Patterns 1–3 … Biggest engine investment; currently each check is
from-scratch." Deferred at strategy time (`docs/owl-dl-reasoner-rust-strategy-v2.md` §11.5,
line 393) with the explicit constraint that Phase 4 data structures must not foreclose it.
**Prior art consulted:** Kazakov & Klinov, *Incremental Reasoning in OWL EL without
Bookkeeping* (ISWC 2013); KM (`bio-ontology-research-group/kobayashi-marust`)
`docs/INCREMENTAL-REASONING.md`; ELK's own incremental implementation notes
(`liveontologies/elk-reasoner` wiki, `IncrementalReasoning`).

## Revision history

- **v1 (2026-08-18)** — initial design.
- **v2 (2026-08-19)** — reworked after review. §3 gained the derived-axiom overlay (v1 was
  **unsound on delete**); §5's mark-closure gained three missing dependency channels and the
  global-state repair contract; §6 now enumerates through `entails()` rather than raw rows;
  new §4a (entity refcounts), §5a (rule→axiom provenance), §6a (id remapping on rebuild),
  §9a (what forces a full rebuild). Gate 1 is now budget-free. Measured speedup ceilings
  added. v1's claim that the reverse ∃-index was new was wrong — `facts_by_target` already
  exists.

## Problem

Every rustdl entry point is stateless: `classify`, `is_subclass_of`, `is_consistent` each
call `convert_ontology` and rebuild the whole pipeline. An ontology-editing session — the
target use case — re-pays that cost on every edit, even when the edit touches one leaf
class. The same cost structure blocks the NeSy candidate loops in the research agenda.

## Goal

A long-lived session that accepts symmetric axiom additions and removals and answers
classification / subsumption / consistency queries after each change, reusing prior work.
Non-negotiable: **the session's answers are IRI-identical to a from-scratch run on the same
axiom set, at every revision.** Speed is the feature; soundness and completeness are not
negotiable against it.

## Scope decisions (locked)

| Decision | Choice | Rationale |
|---|---|---|
| Use case | Long-lived editing session, symmetric add/remove | User-selected. Deletion is in scope from P2, not deferred. |
| Engine coverage | EL saturation incremental **+** monotonicity-filtered tableau retention | EL-only would degrade to full rebuild on most real ontologies (pizza, SIO, wine are all out-of-EL). |
| Id stability | Reserved headroom + amortized rebuild | Avoids the invasive tagged-`ClassId` refactor; non-session path stays byte-identical. |
| Deletion algorithm | ELK-style context invalidation, no bookkeeping | Keeps `ProofTrace` opt-in; no memory tax at SNOMED scale. DRed-over-`ProofTrace` documented below as the precision upgrade if measurement demands it. |
| Surfaces | Rust + CLI JSONL + Python + Protégé | All four, phased; Protégé last. |

## Two blocking findings

### F1 — `convert_ontology` sorts before interning

`crates/owl-dl-core/src/convert.rs:2095-2096`:

```rust
let mut components: Vec<&AnnotatedComponent<A>> = src.iter().collect();
components.sort();
```

Ids are assigned in sorted-component order, so they are a function of the *entire* axiom
set. Adding one axiom can permute every `ClassId`. **A session can therefore never re-run
`convert_ontology` on the union** — it needs an incremental lowering path that interns into
the existing vocabulary.

`Vocabulary::intern_class` (`crates/owl-dl-core/src/vocab.rs:26`) is already monotone-append,
so incremental interning is sound by construction; it is only the sort in `convert_ontology`
that destroys stability.

**Consequence:** session ids ≠ from-scratch ids for the same ontology. Ids are internal, so
this is not a correctness problem, but it has two hard implications:

1. `reportable_class_iris` (`crates/owl-dl-reasoner/src/classify.rs:43`) emits classes in
   **id order**. The session path must sort by IRI at the reporting boundary, or session and
   from-scratch outputs differ in ordering only.
2. Every correctness gate compares **IRI-level** sets. No gate may compare ids.

### F2 — synthetic id regions are based at `num_classes()`

`TseitinAllocator` (`crates/owl-dl-saturation/src/lib.rs:2377`) allocates from
`next_id` seeded at `num_classes()`, across **eight** keyed maps (`by_body`, `by_existential`,
`by_union_existential`, `nominal_by_ind`, `max_key_by_role`). The nominal region likewise
uses `ClassId::new(num_named + ind.index())` (`crates/owl-dl-saturation/src/lib.rs:119`).

Any delta introducing a new named class or individual shifts both regions and invalidates
every retained structure keyed on class id — the closure rows, the ∃-fact index,
`merged_atom_sets`, the unsat bitset.

## Design

### 1. Module layout

New `crates/owl-dl-reasoner/src/incremental.rs` holding `pub struct IncrementalSession`.
Not a separate crate: it needs `pub(crate)` reach into `PreparedOntology`
(`crates/owl-dl-reasoner/src/lib.rs:4190`) and the classify pair loop.

`owl-dl-saturation` grows a persistent `pub struct SaturationState` — the `WorklistEngine`
is currently built and dropped inside `saturate()`, and must instead survive across
revisions:

```rust
pub struct SaturationState { /* engine + Subsumers + reverse indices */ }
impl SaturationState {
    pub fn build(internal: &InternalOntology, slack: usize) -> Self;
    pub fn apply_delta(&mut self, delta: &InternalDelta) -> DeltaOutcome;
    pub fn subsumers(&self) -> &Subsumers;
}
```

`DeltaOutcome` reports `{ marked_contexts, rebuilt: bool }` so reuse rate is measurable from
the outside — this is a first-class output, not debug instrumentation, because the
evaluation depends on it.

### 2. Delta contract and axiom identity

At the horned-owl boundary:

```rust
pub struct AxiomDelta<A: ForIRI> {
    pub added:   Vec<AnnotatedComponent<A>>,
    pub removed: Vec<AnnotatedComponent<A>>,
}
```

The session owns a `SetOntology<A>` mirror. `SetOntology` is `HashSet`-backed over
`AnnotatedComponent` (`horned-owl/src/ontology/set.rs:13`, `AnnotatedComponent: Hash + Eq`),
so removal-by-value is O(1) and a caller who prefers to hand over a whole replacement
ontology gets a set-difference diff for free.

**Axiom tombstoning.** `InternalOntology.axioms: Vec<Axiom>`
(`crates/owl-dl-core/src/ontology.rs:105`) never shrinks. A parallel `live: FixedBitSet`
marks active axioms. This is required, not stylistic: `ProofTrace`'s provenance vectors
(`atomic_sub_axiom`, `existential_fact_axiom`, `conjunctive_trigger_axiom`, …,
`crates/owl-dl-saturation/src/proof.rs:144-163`) are indexed parallel to the axiom list, and
`justify` / `repair` key on axiom indices. Shifting indices would silently corrupt all three.

### 3. Incremental lowering

```rust
pub fn convert_delta<A: ForIRI>(
    internal: &mut InternalOntology,
    added: &[AnnotatedComponent<A>],
) -> Result<Vec<usize>, ConversionError>;
```

Interns into the existing vocabulary and appends axioms, returning the new indices. Does not
sort. Removals resolve components to existing axiom indices and clear their `live` bits.

**Derived-axiom overlay (required for soundness).** `convert_ontology` does not merely intern
and lower. After lowering it runs four **whole-ontology** derivation passes whose output is a
function of the entire axiom set — `derive_data_axioms` (`convert.rs:2124`),
`seed_dkey_subsumptions` (`:2346`), `derive_disjunction_existentials`, and
`derive_functional_max_cardinality` (`:2237`) — and then sorts the axiom list a **second**
time (`:2203`), so axiom *indices* are also a function of the whole set.

A `convert_delta` that only appends would leave revision-0's derived axioms live forever.
Concretely: delete `Functional(dp)` and the derived `C ⊑ ⊥` it produced stays live, so the
session reports a class unsatisfiable that a from-scratch run does not — a **false positive**,
the failure mode this project treats as unacceptable. Additions diverge too: from-scratch may
derive a tighter common subsumer than the retained one.

Therefore every commit: re-run all four derivation passes over the **live** axiom set, diff
the new derived set against the retained one, and feed that diff through the same
tombstone/delta machinery as user axioms. Derived axioms are tagged as derived so they are
never confused with user axioms on removal.

**This is affordable — measured, not assumed.** Per
`docs/2026-08-19-incremental-lowering-floor-findings.md`, re-running the whole lowering plus
derivation costs 7.6 % of a saturation-only classify on galen (5.8 ms vs 76.6 ms) and the
share *falls* with size (41.5 % at 101 classes → 7.6 % at 2748). The derivation passes alone
are only ~30 % of that floor, so incrementalizing them is explicitly **not** worth doing.

**`ConceptPool` is safe** (verified 2026-08-18). `ConceptPool::intern_raw`
(`crates/owl-dl-core/src/ir.rs:303`) is monotone-append with dedup by expression and never
removes or re-interns — `ConceptId`s are stable under append, so `convert_delta` can intern
new concept expressions into an existing pool. `ConceptExpr::Atomic(ClassId)` embeds class
ids, which the headroom scheme (§4) keeps stable, so the two interning layers stay
consistent.

### 4. Id-space headroom

Session build sets `synth_base = num_user + slack`, with `slack = max(64, num_user / 16)`.
All eight `TseitinAllocator` maps (enumerated in §5) and the nominal region base off `synth_base` rather than
`num_classes()`. New named classes and individuals occupy slack without disturbing any
retained structure.

On slack exhaustion: one full rebuild with doubled slack. Amortized O(1) per edit.

**No-regression property.** The non-session path passes `slack = 0`, which reproduces
today's layout exactly. Every existing corpus gate must pass unchanged — this is the
cheapest possible proof that the refactor did not disturb the batch engine, and it is a
merge gate for P1.

### 4a. Entity refcounts — otherwise deletes leak phantom classes

`IriTable::intern` (`crates/owl-dl-core/src/vocab.rs:24-33`) is append-only with no removal,
and `reportable_class_iris` (`crates/owl-dl-reasoner/src/classify.rs:44`) iterates
`0..num_classes()`. Remove the last axiom mentioning class `C` and a from-scratch run never
interns `C`, while the session still reports it as `⊑ Thing` — the identity gate fails on the
**first** signature-shrinking delete, permanently.

Fix: per-entity live-axiom **refcounts** for classes, roles and individuals (declarations
count as references, which also handles punning correctly, since a declaration is what keeps
an otherwise-unused entity reportable). Report only entities with refcount > 0. Ids are never
recycled — only hidden — so retained state stays valid.

This also repairs §7's fail-closed claim, which v1 could not actually deliver: `convert_delta`
interns IRIs *before* a later component in the same delta can fail validation, so a rejected
delta would leave phantom vocabulary behind. Interning for a delta therefore goes into a
**staging overlay** that is committed atomically or discarded whole.

Memory cost of slack: the dense per-class `Vec<Vec<usize>>` indices in `WorklistEngine`
(`existential_triggers_by_body`, `disjoints_by_class`) grow by `slack` empty `Vec`s.
At `num_user / 16` that is a ~6 % overhead on those structures. Acceptable; measured in P1.

### 5. Deletion — ELK-style context invalidation

A *context* here is a class together with its subsumer row (`Subsumers` `IdMatrix` rows) and
its ∃-facts — the existing state is already organized this way, which is what makes this
approach cheap to graft on.

**Existing state, not new.** v1 claimed a reverse ∃-index was the one new structure required.
That was wrong: `facts_by_target` (`crates/owl-dl-saturation/src/lib.rs:371`) already exists
and is maintained alongside `facts_by_sub`. No new index is needed for the ∃ channel.

**`mark_affected(delta)` — five channels, not two.** v1 closed marks along the reverse-∃ and
conjunctive-trigger channels only, and asserted "unmarked contexts are never read or written."
Both were false against this engine. The closure must cover:

1. *Seed.* For each changed axiom, replay that axiom's rule trigger pattern against the
   current closure to find the contexts where it could fire; mark them.
2. *∃ channel.* Close backward through `facts_by_target` (`lib.rs:371`).
3. *Conjunctive-trigger channel.* Close through `existential_triggers_by_body` /
   `conjunctive_by_body`.
4. **Subsumption channel (Phase 2d).** `process_subsumer` copies `facts_by_sub[d]` into `c`
   (`lib.rs:1364-1390`) and `push_fact_impl` recursively pushes each new fact down to
   subclasses (`lib.rs:951-1017`). A context's facts therefore depend on **every** superclass's
   facts, and the rule writes into other contexts by design. Marking `D` requires marking
   every subsumee of `D` that holds an inherited copy.
5. **Unsat channel.** `process_unsat` (`lib.rs:1941`) flags every member of `subs_of_class(c)`
   unsat (`:1948`) *and* makes the source of every fact targeting `c` unsat (`:1966`) — two
   directions, both of which must be closed or unsat bits stay wrongly set after a delete.

**B2 disjunction forcing.** The `process_unsat` hook writes `class ⊑ survivor` or `class ⊑ ⊥`
into `class`'s context guarded by a one-shot `fired: bool` (`lib.rs:464`, set at `:2010`/`:2014`,
tested at `:1995`). Marking an `Sᵢ` synthetic without marking `class` **and clearing `fired`**
either retains a forced subsumption whose justification was deleted (FP) or permanently
suppresses re-forcing (MISS). Simplest correct treatment, and it also buys determinism (§B4
below): **clear every `fired` flag and recompute B2 forcing to fixpoint on every commit.**
It is cheap — `b2_disjunctions` is empty on the EL/Horn corpus — and it removes an
order-dependence that would otherwise make the identity gate non-deterministic.

**Repair must also reset global, non-context-keyed state.** "Drop the row and re-run" is
insufficient; three shared structures silently corrupt the result:

- `seen_facts: HashSet<(sub, role, target)>` (`lib.rs:365`) gates every derivation at
  `lib.rs:967` (`if !self.seen_facts.insert(...) { return }`). Dropping a context's facts
  without **evicting its triples** makes re-derivation a silent no-op — the fact simply
  vanishes from the closure with no error. This is the highest-risk site in the whole design.
- `facts: Vec<ExistentialFact>` (`lib.rs:364`) is global, indexed by both `facts_by_sub` and
  `facts_by_target`. "Drop its ∃-facts" requires an explicit fact-tombstone scheme; no
  per-context ownership exists.
- `merged_atom_sets` (documented as monotonically growing) and the `atomic_content_of` map
  must have marked-context entries cleared, or witness-merge state survives its justification.

Each of these gets its own structural canary, on the pattern the repo already uses for the
label cache and wedge levers.

**Repair.** For each marked context: evict its `seen_facts` triples, tombstone its facts, drop
its subsumer row, `merged_atom_sets` and `atomic_content_of` entries and its unsat bit; then
re-seed and re-run the worklist restricted to the marked set.

**The allocator has eight keyed maps, not five.** §4's headroom retarget must cover
`by_body`, `by_existential`, `by_union_existential`, `nominal_by_ind`, `max_key_by_role`,
`forall_key_by_role`, `forall_atomic_key_by_role` and `top_witness_by_role`
(`lib.rs:2377-2434`). v1 listed five; the three `forall`/`top_witness` maps were missed.

Also: `seed()` pushes reflexive `C ⊑ C` for every id in `num_user..num_total`
(`lib.rs:686-698`), which would give the slack gap phantom rows. Skip unallocated slack ids.

### 5a. Rule→axiom provenance, always on

`apply_delta` must remove a dead axiom's **compiled** rules from `ElRules` and the trigger
indices (`conjunctive_by_body`, `existential_triggers_by_body`, `disjoints_by_class`, chains,
domains, functional flags). Otherwise repair replays deleted axioms and re-derives their
consequences — unsound retention.

The only rule→axiom provenance in the tree today is `ProofTrace`'s parallel vectors
(`crates/owl-dl-saturation/src/proof.rs:146-163`), populated **only when `record_proofs` is
on** — the mode §5 rejects on memory grounds. The design therefore needs a cheap, always-on
rule→axiom index carrying **indices only**, no `Inference` records. This is a small structure
(one `usize` per compiled rule) and it is a prerequisite for deletion, not an optimization.

Note this also re-prices the DRed alternative: axiom-index-only provenance is a middle option
between no bookkeeping and full `ProofTrace` DRed, and it gets most of DRed's invalidation
precision at a fraction of the memory. If §5's context marking proves too coarse in P2, this
index — already built — is the upgrade path, and it is cheaper than the full-`ProofTrace` DRed
that v1 named as the fallback.

**Bail-out.** If `|marked| > RUSTDL_INC_REBUILD_FRACTION × num_classes` (default `0.30`),
discard the marking and full-rebuild. This is what bounds the worst case at roughly today's
cost plus one marking pass. On densely connected TBoxes (GALEN is the expected case) this
will fire often; the evaluation reports the firing rate per ontology as a headline number
rather than burying it.

**Documented alternative, not chosen.** `proof.rs` already ships a truth-maintenance table
(`steps: HashMap<DerivedFact, Inference>`, `crates/owl-dl-saturation/src/proof.rs:141`)
sufficient for DRed-style delete-and-rederive, and it records one derivation per fact, which
is exactly what DRed's over-delete phase tolerates. It was rejected for v1 because it forces
proof recording permanently on — currently opt-in and documented as zero-cost when off — and
its memory scales with derived facts, which is the binding constraint at GALEN/SNOMED scale.
If the evaluation shows context invalidation over-marking badly, this is the upgrade path.

### 6. Tableau side — monotonicity-filtered retention

`Classification.entailed` (`crates/owl-dl-reasoner/src/classify.rs:162`) is already the full
pairwise verdict matrix. Retain it across revisions — but **never by scanning raw rows.**

Unsatisfiable classes' rows are **elided** (`classify.rs:69-71`, invariant at `:411-419`), and
an inconsistent revision elides *every* row (`classify_inconsistent`, `classify.rs:1016-1042`).
The trivial "⊥ ⊑ everything" fill is reintroduced solely by the `Classification::entails`
choke-point (`classify.rs:418`). So for a previously-unsat class `i`, the true set of
previously-positive pairs is all of `(i, ·)` while the raw row reads *empty*: a row-scan
implementation of "pure delete ⇒ re-probe the positives" would silently retain those pairs as
negatives and return wrong verdicts.

Retention therefore enumerates through `Classification::entails` plus `unsatisfiable_idxs`,
never through `EntailmentMatrix` directly. A delete from an inconsistent revision re-probes
everything. And on pure **add**, re-probing negative *pairs* is not sufficient on its own — a
satisfiable class can become unsatisfiable, which changes row elision, `unsatisfiable_idxs`
and every equivalence answer, so the per-class satisfiability pass re-runs on add and newly
unsat rows are re-elided.

The monotonicity *argument* below is sound; it is the data structure that does not encode raw
entailment.

OWL entailment is monotone in the axiom set, which gives two retentions:

- **Pure add.** Every previously-proven subsumption is still proven. Re-probe only the
  previously-*negative* pairs.
- **Pure delete.** Every previous non-subsumption is still a non-subsumption. Re-probe only
  the previously-*positive* pairs — typically a small minority, so deletion is the *cheap*
  direction on the tableau side, exactly opposite to the saturation side.

**Mixed transactions.** A `change` op with both removals `R` and additions `A` gets neither
retention directly. Split it: `S₀ → S₀∖R` is a pure delete (retain negatives), then
`S₀∖R → (S₀∖R)∪A` is a pure add (retain positives of the intermediate state). Both steps are
sound, so `change` is never worse than its two halves.

### 6a. Id remapping on every rebuild

Slack exhaustion (§4), the §5 bail-out, and P1's full-rebuild-on-delete all re-run
`convert_ontology`, which re-sorts (`convert.rs:2096` and again at `:2203`) and therefore
permutes ids and `Classification.classes` order. A retained `EntailmentMatrix` indexed by old
positions and read against new positions is garbage — and can manufacture spurious
**positives**, which is unsound, not merely wrong.

Contract: **on any rebuild, retained pair verdicts are either remapped by IRI or dropped
wholesale.** Dropping is the default; remapping is an optimization that must be justified by
measurement.

**Sticky incompleteness.** Retained negatives include pairs that merely hit the per-pair
budget — `timed_out_pairs` records timeouts as not-subsumed, a documented sound
under-approximation. A session must therefore keep `complete = false` for the remainder of
the session once any revision was incomplete, until a full rebuild clears it. Without this,
a session would launder an under-approximation into a claim of completeness across
revisions. This is a correctness requirement, not a nicety.

### 7. Error handling — fail-closed

Following KM's contract: the whole delta lowers into a staging area first and commits only
on success. An axiom the converter rejects leaves the previous revision **completely
unmutated** and returns an error; there is no half-applied state. The revision counter
advances only on commit.

Any detected internal invariant violation (marked-set inconsistency, slack accounting
mismatch) triggers a transparent full rebuild. The session degrades in speed, never in
soundness.

### 8. API surfaces

**Rust** (`owl-dl-reasoner`):

```rust
impl IncrementalSession {
    pub fn new<A: ForIRI>(onto: &SetOntology<A>) -> Result<Self, ReasonError>;
    pub fn apply<A: ForIRI>(&mut self, delta: &AxiomDelta<A>) -> Result<Revision, ReasonError>;
    pub fn classify(&mut self) -> Result<&Classification, ReasonError>;
    pub fn is_subclass_of(&mut self, sub: &str, sup: &str) -> Result<bool, ReasonError>;
    pub fn is_consistent(&mut self) -> Result<bool, ReasonError>;
    pub fn revision(&self) -> Revision;
    pub fn stats(&self) -> &SessionStats;
}
```

**CLI:** `rustdl incremental` — JSONL on stdin/stdout with ops `init` / `add` / `remove` /
`change` / `is_subsumed_by` / `classify` / `stats`. Every reply carries `revision` and
`rebuilt: bool`. Deliberately shaped after KM's protocol
(`kobayashi-marust/docs/INCREMENTAL-REASONING.md`) so the two are directly comparable in the
evaluation, with one deliberate divergence: KM's protocol takes *clauses* with
caller-assigned `clause_id`s; rustdl takes **axioms in OWL functional syntax** and resolves
identity structurally via the `SetOntology` mirror, so callers need not track ids.

**Python:** `rustdl.Session` wrapping the Rust type in-process — `s.add(axioms)`,
`s.remove(axioms)`, `s.classify()`, `s.is_subclass_of(a, b)`. This is the surface the NeSy
agenda's Patterns 1–3 need, and the only one with no serialization cost.

**Protégé:** the plugin currently shells out to the `rustdl` binary once per call
(`docs/protege-plugin.md`). Rework it to hold a long-lived subprocess speaking the JSONL
protocol, fed from `OWLOntologyChangeListener` deltas. Last phase; it is a Java-side project
gated on the CLI protocol being stable.

### 9. What forces a full rebuild (declared, not discovered)

- **Any object- or data-property axiom delta.** `role_super` is frozen at engine build
  (`freeze_role_super`), and a role-hierarchy change invalidates every context. This matches
  ELK's own documented limitation — `SubObjectPropertyOf`, `EquivalentObjectProperties`,
  `TransitiveObjectProperty`, `ReflexiveObjectProperty` and their data analogues trigger full
  re-classification there too, because such changes are non-local and incrementalizing them
  does not pay off. Declared up front rather than discovered in P2.
- Slack exhaustion (§4); bail-out threshold exceeded (§5); any detected invariant violation (§7).
- Dense↔sparse `EntailmentMatrix` arm flip: a session crossing `dense_max()` = 60k classes
  (`classify.rs:88`) rebuilds rather than migrating arms in place.

### 10. Deliberately out of scope for this spec

Named so they are not mistaken for oversights: **ABox / realize incrementality** (a session
query against `realize` / `materialize_*` forces a full from-scratch realize — the session
retains no ABox-derived state); **justification/proof invalidation** (`justify` / `repair`
against a session rebuild their trace on demand); **session persistence across process
restarts**; **ontology imports** (the `SetOntology` mirror holds a pre-resolved imports
closure; re-resolution is the caller's job).

Two cheap wins that ARE in scope and were missing from v1:

- **Consistency-verdict retention.** Consistency is monotone in both directions — `consistent`
  survives a delete, `inconsistent` survives an add — so one retained bit answers
  `is_consistent` for free in half of all transactions.
- **Logically-empty deltas.** `SetOntology` set-difference reports annotation-axiom changes,
  which the converter drops. The session detects a delta that lowers to zero logical axioms and
  commits a revision with **zero** invalidation. Annotation edits are high-frequency in real
  authoring, so this is a large practical win for near-zero effort.

### 11. Concurrency

`classify`'s pair loop is rayon-parallel inside what is now a `&mut self` session. P1 ships the
conservative contract: `IncrementalSession` is `Send` but not `Sync`, one transaction at a time,
no queries concurrent with `apply`. Anything better needs its own design.

## Evaluation

The user requirement is a comparison against other incremental reasoners across ontology
kinds, sizes, change sequences, and OWL profiles. This is a substantial sub-project and gets
its own implementation plan; the design fixes the axes, the baselines, the metrics and the
reporting rules.

### Harness

Extend `crates/owl-dl-bench/src/matrix/` rather than building a parallel harness. It already
provides: five-reasoner orchestration (`rustdl`, `konclude`, `hermit`, `elk`, `whelk-rs` —
`matrix/mod.rs:96`), `curated`/`ore`/`bioportal` tiers (`matrix/corpus.rs:88`), EL-vs-DL
fragment labeling with `na` status for EL-only reasoners (`matrix/mod.rs:169`), ROBOT format
staging, per-cell JSONL results with sha256 + host + budget provenance (`matrix/model.rs`),
and FP/MISSED scoring against the Konclude oracle (`matrix/correctness.rs`).

Add an `IncrementalCellResult` row type extending `CellResult` with: `edit_script`,
`revision`, `revision_wall_ms`, `rebuilt`, `marked_contexts`, `cumulative_rss_mb`.
One row per revision, not per ontology — the distribution is the result.

The KM head-to-head methodology in
`docs/superpowers/specs/2026-07-18-km-headtohead-and-rustdl-FP.md` (same box, ORE
`pool_sample`, IRI-fragment-normalized transitively-closed closure diff, ddmin on any
disagreement) is reused verbatim for correctness adjudication.

### Baselines

| Reasoner | Incremental support | Profile reach | Role |
|---|---|---|---|
| **ELK** | Yes — the canonical incremental EL reasoner (Kazakov & Klinov 2013), via OWLAPI change + `flush()` | EL | Primary EL baseline. Already wired in the matrix for batch. |
| **KM** | Yes — JSONL session, EL++/CB/HT backends | SROIQ (routed) | The reasoner named in the request. Direct protocol-level comparator. |
| **Openllet** | Yes — `IncrementalClassifier` | OWL DL | The DL-profile incremental baseline; ELK and whelk cannot cover this column. |
| **Snorocket** | Yes — load a classified reasoner, add axioms, reclassify incrementally | EL | The SNOMED-tooling reference point. Omitted from v1; a real gap for a paper-adjacent study. |
| **whelk** (Scala) | Addition-only (immutable state; no retraction) | EL | Add-only datapoint — **not** a symmetric baseline. **Verify** whether `whelk-rs` (already wired) exposes incremental state at all; if batch-only it is a control. |
| **RDFox** | Yes — DRed/FBF materialization maintenance | RL | The standard RL comparator. Without it the RL row is empty. |
| **Konclude** | No incremental classification | SROIQ | **Control**: from-scratch per revision. Also the correctness oracle. |
| **HermiT** | No incremental classification | SROIQ | Control. |
| **rustdl (batch)** | n/a | SROIQ | The control that matters most — the honest question is "does the session beat today's rustdl", and everything else is context. |

The from-scratch controls are not filler. The claim being tested is *reuse beats rebuild*,
and the rebuild number is the thing it must beat.

### Axes

**Kind.** Biomedical (GO, GALEN, FMA, an SNOMED-shaped substitute), upper ontologies
(BFO/SULO), lightweight web (pizza, wine, SIO), synthetic (`bench synthetic-el`, which gives
a controllable size/connectivity knob the real corpus cannot).

**Size.** Stratify by class count, already recorded per-cell: small (<1k), medium (1k–50k),
large (50k–500k), giant (>500k). The `dense_max()` boundary at 60k classes
(`classify.rs:88`) crosses the medium/large stratum, so the sparse-matrix arm must be
exercised on both sides of it.

**Change sequence.** This is the axis the existing harness has nothing for, and it is where
the interesting variance lives:

- `single-add` / `single-remove` — the interactive-latency floor.
- `batch-{add,remove}(k)` for k ∈ {1, 10, 100, 1000} — where does batching amortize?
- `interleaved` — randomized add/remove mix, seeded for reproducibility.
- `leaf-add` vs `root-edit` — adding a subclass at a leaf versus editing near ⊤. This
  directly tests the context-invalidation hypothesis: leaf edits should mark few contexts,
  root edits should trip the bail-out. If that stratification does not appear, the algorithm
  is not doing what the design claims.
- `undo` — add then remove the same axioms; must return to the exact prior classification.
- `authoring-trace` — **the realistic workload.** OBO ontologies (GO especially) are
  released on a versioned cadence; diffing consecutive releases yields real edit sequences of
  real shape and size. Synthetic scripts measure the algorithm; this measures the feature.
- `adversarial` — edits constructed to maximize the marked set, establishing the worst case
  empirically rather than by assertion.

**Axiom kind, within each change sequence.** ELK full-reclassifies on property-axiom changes,
and §9 commits rustdl to the same fallback. Scripts must therefore be stratified into
class-axiom-only and property-axiom-touching variants, or both the ELK column and rustdl's own
reuse rate are overclaimed by mixing the two.

**OWL profile.** Stratify by OWL 2 EL / QL / RL / DL, validated with `robot validate-profile`
rather than rustdl's own `analyze_fragment`, so the profile label is independent of the
system under test. Report `analyze_fragment`'s `PureEl` / `Horn` / `OutOfFragment` verdict
as a second, rustdl-internal column. The profile axis is what makes the baseline table
honest: ELK and whelk are `na` outside EL, and a table that hides that would overstate
rustdl's competition.

### Metrics

- **Per-revision wall time: p50, p95, max.** The tail is the metric that matters for
  interactive editing; a median-only claim would be misleading.
- **Speedup vs from-scratch control**, same host, same revision.
- **Reuse rate** — fraction of revisions served without a rebuild, plus mean
  `marked_contexts / num_classes`.
- **Memory drift** — peak RSS over a ≥1000-revision session. Session state that grows without
  bound is a real failure mode for the editing use case, and only a long session finds it.
- **Correctness at every revision** — FP/MISSED against a from-scratch oracle run at that
  revision, not only at the end of the script. Sessions drift; end-state-only checking would not
  catch it. Full per-revision oracle coverage is affordable only on the curated tier; on
  ORE/BioPortal, sample every k-th revision plus the final one, and **state k in the results**.
- **Measured speedup ceiling per ontology.** Every session pays the §3 lowering floor, so the
  achievable speedup is capped at `sat_only / convert` — measured as **~13× galen, ~9× sio,
  ~3.3× pizza, ~2.4× mie** (`docs/2026-08-19-incremental-lowering-floor-findings.md`). Report
  achieved speedup against this ceiling, not against 1×: "9.5× of a 13× ceiling" is
  interpretable, "9.5×" alone is not. For calibration KM reports 4.90× addition-only.
- **Cold vs warm, reported separately.** KM's published 4.90× EL++ figure explicitly excludes
  process startup and I/O. Any rustdl number compared against it must exclude the same costs
  *and* report the with-startup number alongside, or the comparison is not apples to apples.

### Reporting rules

Report the cases where incremental **loses** — bail-out firing rates, ontologies where the
session is slower than rebuild, profiles where reuse never materializes. Consistent with how
`docs/known-limitations/` and the KM head-to-head already handle negative results. A
per-revision histogram is published, not just aggregates: an editing session's felt
performance is its tail, not its mean.

## Correctness gates

1. **Identity gate (primary), run BUDGET-FREE.** For each curated ontology: from-scratch
   classification is the oracle; a session that reaches the same axiom set via a seeded random
   add/remove edit script must produce an IRI-identical hierarchy and unsat set. Compared at
   IRI level (see F1), and at **every** revision.

   The gate runs only with per-pair timeout and adaptive budget **off**. With budgets on,
   `timed_out_pairs` verdicts (`classify.rs:278-284`) and the per-class unsat probe's
   default-to-satisfiable on timeout (`classify.rs:828`) are host-speed dependent, so
   from-scratch is not reproducible against itself and the gate would flake by construction —
   the same failure mode as the previously-diagnosed prep-deadline flake. Byte-identity is a
   claim about the *algorithm*, and it is only testable where the algorithm is deterministic.
   Determinism additionally requires the per-commit B2 `fired` reset from §5.
2. **Round-trip gate.** Add axioms, then remove exactly those axioms; the result must equal
   the original classification. This is the gate that catches over-retention in deletion.
3. **No-regression gate.** All existing corpus tests pass unchanged on the `slack = 0`
   non-session path. Merge gate for P1.
4. **Bail-out agreement gate.** Force `RUSTDL_INC_REBUILD_FRACTION` to `0` and to `1`; both
   must produce identical results, proving the incremental path and the rebuild path agree.
   This is the cheap continuous proof that context invalidation is not silently wrong.
5. **Sticky-incompleteness gate.** A session that has had an incomplete revision must report
   `complete = false` until a full rebuild, verified by an explicit test.

## Phasing

Every phase is gated on the identity gate (gate 1) for the functionality it ships.

| Phase | Content | Ships |
|---|---|---|
| **P1** | Id headroom (8 allocator maps), `convert_delta` + derived-axiom overlay, entity refcounts, axiom tombstoning, always-on rule→axiom index, session skeleton, addition-only saturation; **full rebuild on any delete** | A working session with real speedup on additions. **Exit criterion:** a single-axiom addition on galen completes in ≤ 2× the measured 5.8 ms floor (≤ ~12 ms). If not, the retained-state design is not paying off and is re-examined before P2 builds deletion on it. |
| **P2** | ELK-style context invalidation for deletion + bail-out | Symmetric add/remove |
| **P3** | Tableau monotonicity retention + sticky incompleteness | Hybrid (non-EL) ontologies benefit |
| **P4** | CLI JSONL protocol + Python `Session` | External surfaces; makes the evaluation runnable |
| **P5** | Evaluation harness + the full comparison study | The results |
| **P6** | Protégé plugin wired to a long-lived session | User-visible payoff |

**Decomposition.** This spec drives **one** implementation plan covering P1–P4 — a coherent
engine-plus-API project with a single correctness story. **P5** (evaluation) and **P6**
(Protégé) each get their own spec and plan: P5 is a measurement study whose design is fixed
above but whose execution is independent, and P6 is a Java-side project. P5 depends on P4,
because the CLI protocol is how external reasoners get driven comparably.

## Open questions for review

1. **Slack default.** `max(64, num_user/16)` is a guess. P1 should measure rebuild frequency
   on the authoring-trace scripts and tune it with data.
2. **Bail-out threshold.** `0.30` is likewise a guess, and it is the single knob that decides
   whether the feature helps or hurts on GALEN. P2 should sweep it.
3. **Openllet availability.** Verified 2026-08-19: `IncrementalClassifier` exists and is used
   with the OWLAPI loader, but the last release is 2.6.5 (September 2019). Treat the DL
   incremental column as at-risk; if Openllet will not run, that column has only KM in it and
   the evaluation says so plainly rather than quietly dropping the axis.
4. **GO-scale pure-EL floor.** The lowering floor was measured on galen (2748 classes, 7.6 %)
   with the share falling as size grows, but no large pure-EL ontology (GO-basic ≈ 52k classes,
   the zero-tableau `classify_pure_el` path) was available locally. That is simultaneously the
   feature's best case and the floor's hardest test. Measure before fixing the evaluation's
   headline claims.
5. **Is the 0.30 bail-out a fig leaf?** With the §5 subsumption channel added, a delete above
   the leaves marks descendant cones, so on GALEN/SNOMED shapes deletion reuse may be near
   zero and P2's honest value proposition becomes *leaf-edit* deletion. The authoring-trace
   scripts decide whether that is enough; if not, §5a's axiom-index provenance is the answer.
