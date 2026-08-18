# Incremental reasoning — design

**Date:** 2026-08-18
**Status:** design approved in chat; pending user review of this document → implementation plan
**Motivation:** `docs/nesy-research-agenda-2026-07-11.md:69` — "Incremental reasoning …
Load-bearing for Patterns 1–3 … Biggest engine investment; currently each check is
from-scratch." Deferred at strategy time (`docs/owl-dl-reasoner-rust-strategy-v2.md` §11.5,
line 393) with the explicit constraint that Phase 4 data structures must not foreclose it.
**Prior art consulted:** Kazakov & Klinov, *Incremental Reasoning in OWL EL without
Bookkeeping* (ISWC 2013); KM (`bio-ontology-research-group/kobayashi-marust`)
`docs/INCREMENTAL-REASONING.md`.

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
`next_id` seeded at `num_classes()`, across five keyed maps (`by_body`, `by_existential`,
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

**`ConceptPool` is safe** (verified 2026-08-18). `ConceptPool::intern_raw`
(`crates/owl-dl-core/src/ir.rs:303`) is monotone-append with dedup by expression and never
removes or re-interns — `ConceptId`s are stable under append, so `convert_delta` can intern
new concept expressions into an existing pool. `ConceptExpr::Atomic(ClassId)` embeds class
ids, which the headroom scheme (§4) keeps stable, so the two interning layers stay
consistent.

### 4. Id-space headroom

Session build sets `synth_base = num_user + slack`, with `slack = max(64, num_user / 16)`.
All five `TseitinAllocator` maps and the nominal region base off `synth_base` rather than
`num_classes()`. New named classes and individuals occupy slack without disturbing any
retained structure.

On slack exhaustion: one full rebuild with doubled slack. Amortized O(1) per edit.

**No-regression property.** The non-session path passes `slack = 0`, which reproduces
today's layout exactly. Every existing corpus gate must pass unchanged — this is the
cheapest possible proof that the refactor did not disturb the batch engine, and it is a
merge gate for P1.

Memory cost of slack: the dense per-class `Vec<Vec<usize>>` indices in `WorklistEngine`
(`existential_triggers_by_body`, `disjoints_by_class`) grow by `slack` empty `Vec`s.
At `num_user / 16` that is a ~6 % overhead on those structures. Acceptable; measured in P1.

### 5. Deletion — ELK-style context invalidation

A *context* here is a class together with its subsumer row (`Subsumers` `IdMatrix` rows) and
its ∃-facts — the existing state is already organized this way, which is what makes this
approach cheap to graft on.

**New state:** a reverse ∃-index, `target ClassId → Vec<subject ClassId>`, maintained as
facts are pushed. This is the only new persistent structure the algorithm requires.

**`mark_affected(delta)`:**

1. *Seed.* For each changed axiom (added or removed), replay only that axiom's rule trigger
   pattern against the current closure to find the contexts where it could fire. Mark them.
2. *Close.* Transitively mark backward along the reverse ∃-index and the conjunctive-trigger
   reverse index — any context that could have received a propagation from a marked context
   is itself marked.

**Repair.** For each marked context: drop its subsumer row, its ∃-facts, its
`merged_atom_sets` entries and its unsat bit; re-seed and re-run the worklist restricted to
the marked set. Unmarked contexts are never read or written.

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
pairwise verdict matrix. Retain it across revisions.

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
| **whelk** (Scala) | Designed for reusable/incremental EL state | EL | Second EL datapoint. **Verify** whether `whelk-rs` (already wired) exposes incremental state or only batch — if batch-only it stays a control, not a baseline. |
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
  revision, not only at the end of the script. Sessions drift; end-state-only checking would
  not catch it.
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

1. **Identity gate (primary).** For each curated ontology: from-scratch classification is the
   oracle; a session that reaches the same axiom set via a seeded random add/remove edit
   script must produce an IRI-identical hierarchy and unsat set. Compared at IRI level (see
   F1), and at **every** revision.
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
| **P1** | Id headroom, `convert_delta`, axiom tombstoning, session skeleton, addition-only saturation; **full rebuild on any delete** | A working session with real speedup on additions |
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
3. **Openllet availability.** The DL-profile incremental baseline depends on Openllet's
   `IncrementalClassifier` still working against a current OWLAPI. If it does not, the DL
   incremental column has only KM in it, and the evaluation should say so plainly rather than
   quietly dropping the axis.
