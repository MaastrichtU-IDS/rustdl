# SP-A: Approximated Saturation (forced-disjunct + common-disjunct precomputation) — Design

**Sub-project 1 of 4** in the Konclude-style classification redesign (build-once /
classify-many). The redesign's layering and decomposition live in this session's
notes; SP-A is the **foundation** — the layer that makes per-test/single-model
construction cheap by resolving disjunction structure *before* the tableau branches.

## Why this is the critical path (measured, 2026-06-23)

The viability gate established the bottleneck is **branch count**, not per-branch cost:

- `sat(AlsatianWine)` builds a ~10-node model in **68,796 branches** (`restores ≈
  branches` — the search thrashes through dead disjunct assignments). Per-branch cost
  is cheap (0.043 ms, match-dominated; the wedge's full-graph clone per branch is real
  but ~10–20% of cost, so trail-backtracking is *not* the lever).
- Konclude classifies the whole ontology in 230 ms (all caching off) ≈ ~tens of
  branches per class — **~1000× fewer branches**, because its approximated saturation
  resolves the disjunction structure deterministically up front.

SP-A reproduces that mechanism with two **precomputed, sound** rules. Distinct from
the prior NO-GO levers: MOMS/1-UIP/semantic-branching reordered the same brute-force
search; runtime nogoods found wine's clashes context-dependent. SP-A is **precomputed
from ontology structure**, not learned from runtime clashes — the untried mechanism.

## Scope (this sub-project)

SP-A covers **atomic-class disjunctions** only: `C ⊑ D₁ ⊔ … ⊔ Dₙ` where each `Dᵢ` is
an atomic class. This is sound, FP-safe, and the general-purpose win (the
SAO/BFO disjunctive-domain pattern closed earlier this session is exactly this shape).

**Deliberately deferred** (separate, FP-gated increments — this is where the SP1
increment-3 FP lived, so it is NOT bundled here):
- **Nominal value-partition forced-disjunct** (`∀R.{a,b,c}` — wine's sugar/body
  partitions). Requires nominal disjointness, the exact construct that made SP1
  increment-3 produce 33,272 spurious unsat. Built only after SP-A's atomic machinery
  is proven, with its own ORE FP gate.
- **Context-dependent forced-disjunct during construction** (BCP at a live node) — that
  is SP-B (saturation-guided construction), not preprocessing.

## Architecture

A preprocessing pass in `owl-dl-core`, run inside `convert_ontology` (alongside the
existing `disjunction_existential` pass), emitting derived deterministic class axioms
into the `InternalOntology` that the saturator and wedge already consume.

**New module:** `crates/owl-dl-core/src/approx_saturation.rs`.
**Inputs (existing, sound):** the told-subsumer and told-disjoint tables (`told.rs`,
both transitively closed at build) and the interned `ConceptPool` (`ir.rs`).
**Output:** zero or more derived `Axiom::SubClassOf { sub: C, sup: E }` (E atomic or
`Bot`), appended to `InternalOntology.axioms`.

### Rule 1 — common-disjunct extraction (generalize the existing pass)

`disjunction_existential.rs::minimal_common_subsumers` already computes the minimal
common told-subsumer of a set of atomic classes, but only fires for existential bodies
(`X ⊑ ∃R.(D₁⊔…⊔Dₙ)`). Generalize the same helper to a **direct** disjunction on a GCI
RHS:

For `C ⊑ D₁ ⊔ … ⊔ Dₙ` (all `Dᵢ` atomic), let `E = minimal_common_subsumers({Dᵢ})`
(the told-subsumers shared by every disjunct). Emit `C ⊑ F` for each `F ∈ E`.

Soundness: a union is subsumed by anything that subsumes every disjunct. Sound by
construction (each `F` is a told-subsumer of all `Dᵢ`).

### Rule 2 — forced-disjunct (the branch-count lever)

For `C ⊑ D₁ ⊔ … ⊔ Dₙ` (all `Dᵢ` atomic): a disjunct `Dᵢ` is **incompatible with C**
iff some told-subsumer `G` of `C` is told-disjoint from `Dᵢ` (`C ⊑ G`, `G ⊓ Dᵢ ⊑ ⊥`).
Let `K = { i : Dᵢ not incompatible with C }`.

- `|K| == 1` (say `{k}`) → emit `C ⊑ Dₖ` (the survivor is forced).
- `|K| == 0` → emit `C ⊑ Bot` (every disjunct clashes; C unsatisfiable).
- `|K| ≥ 2` → emit nothing (genuinely undetermined; the wedge still branches).

Soundness: in every model a `C`-instance lies in some `Dᵢ` (the disjunction holds) but
cannot lie in any incompatible `Dᵢ` (that contradicts `C ⊑ G` + `G ⊓ Dᵢ ⊑ ⊥`); so it
lies in the single survivor `Dₖ` (or none exists ⇒ `C ⊑ ⊥`). Both emissions are
entailed — sound by construction. Over-approximation is impossible: incompatibility is
tested via the *transitively-closed* told tables (a subset of true entailment), so a
disjunct is dropped only when genuinely entailed-disjoint; this can only *miss* a
forcing, never invent one.

**FP boundary (explicit, the increment-3 lesson):** Rule 2 consults only the atomic
told-disjoint table. It does NOT introduce nominal disjointness, does NOT merge
witnesses, and does NOT touch the functional-merge atom-set pooling that increment-3's
nominal disjointness made unsound. Atomic told-disjoint pairs come from declared
`DisjointClasses` and are sparse and sound; pooling/merge is untouched.

## Data flow

```
convert_ontology(horned-owl)
  → InternalOntology (told tables built)
  → approx_saturation::derive(internal)   [NEW: Rules 1 & 2 over atomic disjunctions]
       emits derived SubClassOf axioms (incl. C ⊑ Bot)
  → axioms appended; saturator + wedge consume them unchanged
```

The saturator already turns `Atomic ⊑ Bot` into unsat (Phase D4 `directly_unsat` /
`enqueue_unsat`), and `C ⊑ E` into a told subsumption — so no engine change is needed;
SP-A only feeds the existing machinery more deterministic facts.

## Soundness contract

- Both rules emit only entailed axioms (proven above). **FP=0 by construction.**
- Gate is empirical regardless (per the increment-3 lesson — the tuned corpus is not
  sufficient): closure-diff byte-identical-or-recovery on the tuned corpus **AND** an
  ORE `--saturation-only` before/after sweep (mainbase vs SP-A), checking for any
  spurious-unsat cascade (the increment-3 signature: removed≈before / mass unsat).
- Any closure *change* must be additive (recovered MISSED), oracle-confirmed sound on
  a sample (HermiT∩Konclude). Spurious unsat ⇒ revert.

## Testing

Negatives-first canaries (`crates/owl-dl-core` unit + a reasoner integration test):
1. **Forced-disjunct fires:** `C ⊑ A⊔B`, `C ⊑ G`, `DisjointClasses(G,A)` ⟹ `C ⊑ B`.
2. **Forced-to-bot:** `C ⊑ A⊔B`, `C ⊑ G`, `DisjointClasses(G,A)`, `DisjointClasses(G,B)`
   ⟹ `C ⊑ ⊥` (unsat).
3. **Undetermined (negative control):** `C ⊑ A⊔B` with no disjointness ⟹ nothing emitted,
   C stays satisfiable, no spurious `C ⊑ A` or `C ⊑ B`.
4. **Common-disjunct:** `C ⊑ A⊔B`, `A ⊑ P`, `B ⊑ P` ⟹ `C ⊑ P`.
5. **Nominal disjunction is NOT touched (negative control):** `C ⊑ {a}⊔{b}` (nominal
   disjuncts) ⟹ SP-A emits nothing (scoped out; guards against re-introducing the
   increment-3 FP surface).
6. Corpus closure-diff + ORE sweep harness (reuse this session's `--saturation-only`
   before/after script + the konclude_closure_diff tests).

## Files

- Create: `crates/owl-dl-core/src/approx_saturation.rs` (Rules 1 & 2 + unit canaries).
- Modify: `crates/owl-dl-core/src/lib.rs` (mod line), `convert.rs` (call `derive` in
  `convert_ontology` after told tables built), reuse
  `disjunction_existential::minimal_common_subsumers` (make it `pub(crate)`).
- Test: `crates/owl-dl-reasoner/tests/approx_saturation_forced_disjunct.rs`
  (integration canaries 1–5).

## Success criteria

- All 6 test groups green; FP=0 corpus-wide + ORE sweep clean.
- Any disjunctive-domain ontology where forced/common disjuncts are entailed gets them
  deterministically (measure: a synthetic atomic value-partition collapses to 0 wedge
  branches; corpus: any recovered MISSED is oracle-sound).
- Foundation in place for SP-B (wire these + context-dependent forcing into
  construction) and the deferred nominal value-partition increment.

## Out of scope (recap)

Nominal value-partition forcing (wine's sugar/body); construction-time BCP (SP-B);
build-once loop + KPSet (SP-C); reuse cache (SP-D). SP-A alone will NOT close wine —
wine's partitions are nominal — but it is the sound, general, FP-safe foundation the
rest builds on, and it closes the atomic disjunctive-domain class outright.
