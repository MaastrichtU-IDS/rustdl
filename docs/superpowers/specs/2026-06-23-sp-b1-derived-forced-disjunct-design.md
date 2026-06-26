# SP-B1: derived-closure forced-disjunct (in-saturator, fixpoint) — Design

**Increment B1 of SP-B** (saturation-guided construction) of the build-once redesign.
The FP-safe, measurable foundation: move forced-disjunct from SP-A's *told-table*
preprocessing into the saturator's *derived* subsumer closure, fired inside the
fixpoint so it iterates automatically. This is the deep-saturation architecture
B2 (∀/≤n) and B3 (nominal) extend.

## Why in-saturator (not per-branch, not iterate-preprocessing)

- Per-branch forward-checking is the measured net-negative lever — rejected.
- SP-A runs at preprocessing on the *told* tables (transitively closed over explicit
  SubClassOf/EquivalentClasses only). It cannot force a disjunct that becomes
  incompatible via a **derived** subsumer — one the saturator computes through
  existential / domain-range / role reasoning that is not a told subsumption.
- Firing the rule inside the saturator's fixpoint uses the **live derived closure**
  and re-fires automatically as subsumers accumulate (forcing `C⊑Dₖ` adds `Dₖ` as a
  subsumer, which may force the next disjunction) — Konclude-style approximated
  saturation, the correct foundation.

## The rule

For a class `X` with an effective disjunction `X ⊑ D₁⊔…⊔Dₙ` (all `Dᵢ` atomic): a
disjunct `Dᵢ` is **excluded** iff some current subsumer `G` of `X` is disjoint from
`Dᵢ` (`X⊑G`, `G⊓Dᵢ⊑⊥`). Let `K` = the non-excluded disjuncts.
- `|K| == 1` ⟹ `enqueue_subsumer(X, Dₖ)` (the survivor is forced).
- `|K| == 0` ⟹ `enqueue_unsat(X)` (every disjunct excluded).
- `|K| ≥ 2` ⟹ nothing.

`X` carries disjunction `⊔Dᵢ` iff some subsumer `C` of `X` was declared
`C ⊑ ⊔Dᵢ` (disjunctions propagate down subsumption: `X⊑C⊑⊔Dᵢ ⟹ X⊑⊔Dᵢ`).

**Soundness:** in every model an `X`-instance lies in some `Dᵢ` (the inherited
disjunction) but not in any excluded `Dᵢ` (`X⊑G ∧ G⊓Dᵢ⊑⊥`); so it lies in the lone
survivor (or none ⟹ `X⊑⊥`). Both emissions are entailed — sound by construction. The
closure is sound (subset of true entailment), so a disjunct is excluded only when
genuinely entailed-disjoint: can only miss a forcing, never invent one. FP=0 by
construction (the increment-3 trap is structurally absent — B1 touches only atomic
disjuncts and the existing `disjoint_pairs`/subsumer closure; no nominal disjointness,
no functional-merge pooling).

## Components

**Ingestion (seed time, `collect_el_rules` in `owl-dl-saturation/src/lib.rs`):**
- New field `ElRules.disjunctions_by_class: HashMap<ClassId, Vec<Box<[ClassId]>>>` —
  for each `Axiom::SubClassOf { sub, sup }` (and the `EquivalentClasses` `⊔`-RHS
  direction) where `sub` is `Atomic(C)` and `sup` is `Or(...)` with **all disjuncts
  atomic**, push the disjunct `ClassId`s under `C`. Non-atomic disjunct ⟹ skip the
  whole disjunction (B3 scope). The saturator currently drops `Or`-RHS; this ingests it.
- A dense `disjunctions_present: bool` (or `is_empty()` guard) so the rule is a no-op
  on the EL/Horn corpus (zero overhead when there are no atomic disjunctions).

**Firing (`process_subsumer(c, d)` — fires when class `c` gains subsumer `d`):**
Add a block (guarded by `!disjunctions_by_class.is_empty()`), after the existing
disjointness-clash block, that runs the rule for `c`:
1. Gather `c`'s effective disjunctions: `⋃ { disjunctions_by_class[g] : g ∈
   subsumers(c) }` (includes `c` itself; `subsumers.contains(c, g)`). For efficiency,
   only the disjunctions reachable via the new subsumer `d` plus `c`'s own need
   re-checking, but a correct-first implementation may scan all of `c`'s effective
   disjunctions (the fixpoint bounds re-fires).
2. For each disjunction, compute survivors: `Dᵢ` excluded iff
   `∃ G ∈ disjoints_by_class[Dᵢ] : subsumers.contains(c, G)`.
3. `|surv|==1` ⟹ `enqueue_subsumer(c, surv[0])`; `==0` ⟹ `enqueue_unsat(c)`; else nothing.

This reuses the exact primitives the disjointness-clash block already uses
(`disjoints_by_class`, `subsumers.contains`, `enqueue_subsumer`, `enqueue_unsat`),
and fires within the fixpoint so it iterates.

**No engine wiring change** beyond the saturator: `enqueue_subsumer`/`enqueue_unsat`
already drive the closure; the orchestrator consumes the richer closure unchanged.

## Scope

Atomic disjuncts only (FP boundary). Deferred: nominal `⊔` (B3), `∀`/`≤n`-derived
exclusion (B2), the `EquivalentClasses`-`⊔`-RHS direction is included (cheap) but
disjunctions appearing only inside `∃`/`∀` fillers are not (B2).

## Testing (negatives-first)

Unit canaries in `owl-dl-saturation` (mirror `told.rs`/existing saturator tests):
1. **Derived-subsumer forcing (the B1 differentiator):** `X ⊑ ∃r.⊤`,
   `ObjectPropertyDomain(r, G)` (⟹ the saturator derives `X ⊑ G` via the domain rule —
   `G` is a DERIVED, not told, subsumer of `X`), `X ⊑ A⊔B`, `DisjointClasses(G, A)`.
   Expect: B1 derives `X ⊑ B` (forced via the derived subsumer `G`). Contrast control:
   SP-A's told-only pass (`told.super_classes(X)` excludes `G` since domain reasoning
   is not a told subsumption) does NOT force it — this is exactly what B1 adds.
2. **Told-subsumer forcing still works:** `X⊑A⊔B`, `X⊑G`, `Disjoint(G,A)` ⟹ `X⊑B`.
3. **Forced-to-bot:** add `Disjoint(G,B)` ⟹ `is_unsatisfiable(X)`.
4. **Inherited disjunction:** `X⊑C`, `C⊑A⊔B`, `X⊑G`, `Disjoint(G,A)` ⟹ `X⊑B`
   (disjunction inherited from C, forced by X's own subsumer).
5. **Undetermined (negative control):** `X⊑A⊔B`, no disjointness ⟹ no `X⊑A`/`X⊑B`,
   X satisfiable.
6. **Nominal `⊔` not touched (negative control):** `X⊑{a}⊔{b}` ⟹ B1 ingests nothing,
   no derivation.

Integration: reuse `approx_saturation_forced_disjunct.rs`-style reasoner canary for
the derived-subsumer case end-to-end.

## FP=0 gate (sacred)

- Tuned closure-diff (`konclude_closure_diff`): FP=0 / MISSED=0 (or MISSED-down with
  oracle-confirmed recoveries) on all 12 fixtures.
- ORE `--saturation-only` before/after sweep (main-base-without-B1 vs B1), corrected
  exit-code handling, watching the spurious-unsat signature (`a_unsat > b_unsat`).
  Any closure change must be additive + oracle-sound.

## Files

- Modify: `crates/owl-dl-saturation/src/lib.rs` — `ElRules.disjunctions_by_class`
  field + ingestion in `collect_el_rules` + the firing block in `process_subsumer` +
  unit canaries.
- Test (integration): extend `crates/owl-dl-reasoner/tests/` with a derived-subsumer
  forcing canary.

## Success criteria

All 6 unit canaries + integration green; `cargo test --workspace` green; FP=0 gate
clean. B1 forces at least one disjunct via a DERIVED subsumer that SP-A's told-only
pass cannot (the differentiator). Measurable branch reduction on a synthetic atomic
value-partition (the implicit viability signal for the deeper B2/B3). Wine unaffected
(nominal — deferred to B3), FP=0 preserved.
