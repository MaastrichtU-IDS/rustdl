# CB engine — lightweight inference recording for DL proofs (opt-in)

**Date:** 2026-06-16
**Crate:** `crates/owl-dl-cb` (Sequoia ordered consequence-based engine; default-OFF)
**Scope:** add a faithful, opt-in, zero-cost-when-off **inference trace** to the
ordered engine so it can emit forward **DL proofs** (derivation trees) for entailed
subsumptions — the ELK→Evee/Evonne model. Designed in BEFORE S2 so the cardinality
/ inverse / nominal rules are proof-aware from birth (far cheaper than a retrofit).

## Why the consequence-based engine (not the tableau)
Each derived consequence is the output of ONE inference rule from explicit premises,
so the saturation run already *is* a derivation DAG — recording `(rule, premises)` per
derived clause makes the proof fall out by backward traversal. (The tableau is
refutational; its closed-graph proof is the wrong shape and harder to render. See the
proof-feasibility discussion in the project notes.) This is a genuine differentiator —
Konclude does not emit proofs.

## Design principles (what makes it "lightweight")
1. **Opt-in, zero-cost-when-off.** Gate on a runtime flag `RUSTDL_CB_PROOF=1`, read
   once at engine start into `Engine.record_proofs: bool`. When off: a single bool
   check at each insert, no allocation, no trace. Default OFF (classification stays
   exactly as fast as today).
2. **Side-table, NOT in the clause.** `DerivedClause`'s `Ord`/`Eq`/`Hash` are
   load-bearing for `seen`-dedup and the redundancy gate — provenance must NOT change
   clause identity. Store the trace in a separate structure keyed by a stable
   `ClauseId`. Two derivations of the same clause are deduped as one clause; we keep
   the FIRST inference that produced it (sufficient for one proof; alternative
   derivations are not needed for a single proof tree).
3. **One recording chokepoint.** All rules already route their conclusion through the
   insert path (`add_clause`). Recording happens there: the caller passes the
   `Inference` alongside the clause; `add_clause` assigns/looks-up the `ClauseId` and,
   if `record_proofs`, stores the inference. Exactly one code path to audit.
4. **Record redundancy/Elim edges too.** The read-off is "contained up to redundancy"
   (`∈̂`, Def-4): a conclusion may be witnessed by a *subsuming* clause. So when a
   clause is forward-subsumed (dropped) or read off via a subsumer, record a
   `Subsumes{ survivor, subsumed }` edge so proof extraction redirects to the
   surviving clause's derivation rather than dead-ending.
5. **Faithful, not fabricated.** The recorded rule+premises must be the ACTUAL
   inference that produced the clause. The proof is then independently *checkable*
   (re-verify each step is a valid instance of its rule) — a property worth keeping.

## Types (new module `seq_proof.rs` + small hooks)
```rust
/// Stable identity of a derived clause within a context (assigned on first insert).
pub type ClauseId = (ContextId, u32);   // (context, per-context sequence no.)

/// Which calculus rule produced a clause (extend per stage).
pub enum SeqRule {
    Core,                       // seeded core unit ⊤→A  (leaf: a core atom)
    Axiom(AxiomRef),            // ontology axiom used by Hyper (leaf)
    Hyper { axiom: AxiomRef, premises: Vec<ClauseId> },
    Succ  { premise: ClauseId },        // ∃R.B successor edge
    Forall{ edge: ClauseId, forall: ClauseId },  // R∀ augmentation
    BackProp { successor_bot: ClauseId, edge: ClauseId },  // ⊥ reflected under residual
    Subsumes { survivor: ClauseId },    // redundancy redirect (Elim / ∈̂)
    // B2+: Choose { at_most, witnesses }, Merge { eq_disjunct }, EqRes { eq, neq }
}

pub struct Inference { pub rule: SeqRule, pub conclusion: ClauseId }

/// Opt-in trace: clause-id ↔ head map + the inference that first produced each clause.
#[derive(Default)]
pub struct ProofTrace {
    inferences: HashMap<ClauseId, Inference>,   // conclusion → its inference
    // (clause head text is recoverable from the context's clause store via ClauseId)
}
```
`AxiomRef` = an index into the normalized ontology's axiom list (so leaves point at the
*source axiom*, renderable via the existing Manchester writer / justification rendering).

## Hooks in the engine (added at S2 build time, behind `record_proofs`)
- `Engine`: add `record_proofs: bool` + `trace: ProofTrace` + a per-context clause-id
  counter. `add_clause(v, head, inference: Option<Inference>)` — assign `ClauseId` on
  first insert; if `record_proofs && inference.is_some()`, store it. (When off, the
  `Option` is `None` and nothing is built; callers can pass `None` cheaply.)
- Each rule (`Core`, `apply_hyper`, `Succ`, `R∀`, back-prop, and the B2 `Choose`/
  `Merge`/`EqRes`) constructs its `Inference` from the premise `ClauseId`s it already
  has in hand and passes it to `add_clause`. Marginal cost when off ≈ zero (don't build
  the `Inference`; guard on `record_proofs`).
- Forward subsumption / `Elim`: when dropping `c` because `s` subsumes it, record
  `Subsumes { survivor: s }` for `c`'s id (so a later reference to `c` redirects to `s`).

## Extraction — `seq_proof::prove(trace, norm, a: ClassId, b: ClassId) -> Option<Proof>`
1. In context `q_A` (core `{A}`), find the witnessing clause: head `⊆ {B}` (a unit
   `→B`, redirect through `Subsumes` if needed) or empty head (`A⊑⊥` ⟹ `A⊑B` vacuously
   — emit the unsat proof).
2. Backward DAG traversal from that `ClauseId` through `inferences[·].rule.premises`;
   leaves are `Core` (a core/told atom) and `Axiom` (ontology axioms). Memoize visited
   clause-ids ⟹ a proof **DAG** (shared sub-proofs not duplicated); present as a tree on
   demand.
3. Render: reuse the justification/Manchester rendering (memory: `justify` already
   prints `A SubClassOf B`); each proof step = `premises ⊢_rule conclusion`.

## Soundness / faithfulness / completeness
- **Faithful**: records the real producing rule (logging, not inventing). Each step is a
  sound calculus inference (the engine's per-rule soundness, Sequoia Thm 1), so the
  whole proof is sound and independently checkable.
- **Complete-relative-to-the-engine**: a proof exists for `A⊑B` iff the engine derives
  it. So proof completeness rides on the engine's completeness (the R2/R1 order work) —
  no extra completeness burden.
- **FP-irrelevant**: recording is observational; it cannot change which clauses are
  derived ⟹ cannot affect FP=0 or any verdict. The flag is purely additive.

## Explicitly OUT of scope (lightweight ≠ optimal)
- **Proof minimality / optimal proofs** (smallest size/depth — Alrabbaa–Baader–
  Borgwardt–Koopmann). The raw DAG can be large; finding a *small* proof is a separate
  optimization (analogous to minimal justifications; the shipped QuickXplain/HST infra
  is the related lever). The record ENABLES proofs; optimizing them is later.
- **Equality/merge proof rendering** (B2 `Merge`/`EqRes`, B4 nominal `≈`): the records
  exist (Choose/Merge/EqRes variants) but readable rendering of equational steps is a
  rendering concern, deferred with the rest of the proof UI.

## Sequencing
Add the `record_proofs` flag + `add_clause` signature + `seq_proof.rs` skeleton
**immediately after the S1 order fix lands and BEFORE S2**, so S2/S3/S4 rules record
inferences as they're written. Validate with a smoke test: `prove(A,D)` on the by-cases
ontology returns a 3-step proof (`A⊑B⊔C` + `B⊑D` + `C⊑D` ⊢ `A⊑D`); `prove(K1,K2)` on
the minimal-gap ontology returns a proof routing through `K3⊑⊥`. Off-by-default; one
integration test with `RUSTDL_CB_PROOF=1`.
