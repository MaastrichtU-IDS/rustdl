# SP-B2a: synthetic-conjunction forced-disjunct (deep incompatibility) — Design

**Increment B2a of SP-B** (saturation-guided construction), building on B1. Generalizes
the forced-disjunct *exclusion test* from B1's "a subsumer of C is told/derived-disjoint
with Dᵢ" to the strictly more general "**C ⊓ Dᵢ is derivably unsatisfiable**", detected by
the saturator's own unsat machinery. Reuses every existing rule (conjunctive, existential,
domain/range, functional-merge, ForallKey, MaxKey, disjointness); the same mechanism B3
will fire on wine's nominal partitions once nominal disjointness is supplied.

## Why

B1 excludes `Dᵢ` only via `disjoint_pairs` (a subsumer of C disjoint with Dᵢ). But `C ⊓ Dᵢ`
can be unsat for deeper reasons the saturator already proves — e.g. `C ⊑ ∃R.A`,
`Dᵢ ⊑ ∃R.B`, functional `R`, `A⊓B⊑⊥` (functional-merge clash); or `C ⊑ ∃R.A`,
`domain(R)`/`range`/conjunction interactions. B2a forces a disjunct out whenever the
saturator can prove `C⊓Dᵢ ⊑ ⊥`, catching these without per-rule special-casing.

**Note (scope):** the main saturator has **no general `∀R.K` rule** (only ForallKey =
`∀R.OneOf`). So `∀`-driven clashes (`C⊑∀R.K`, `Dᵢ⊑∃R.L`, `K⊓L⊑⊥`) are NOT yet caught —
that needs the general-∀ rule, deferred to **B2b**. B2a delivers the forcing *mechanism*
+ all non-`∀` deep incompatibilities; B2b adds the `∀` rule to feed it.

## Mechanism

For each ingested atomic disjunction `C ⊑ D₁⊔…⊔Dₙ` (from B1's `disjunctions_by_class`):
introduce, at engine **seed** time, a synthetic `Sᵢ = C ⊓ Dᵢ` per disjunct via
`introduce_runtime_synthetic([C, Dᵢ])`. The saturator's fixpoint determines each `Sᵢ`'s
satisfiability through its normal rules. Track, per disjunction, the set of disjuncts
whose `Sᵢ` is **not** unsat (the survivors). When a survivor's `Sᵢ` becomes unsat:
- recompute survivors; `|surv| == 1` ⟹ `enqueue_subsumer(C, survivor)`; `== 0` ⟹
  `enqueue_unsat(C)`.

**Soundness:** `Sᵢ = C⊓Dᵢ` unsat ⟹ `C⊓Dᵢ ⊑ ⊥` ⟹ no model has an `X⊑C` instance in `Dᵢ`;
since `C ⊑ ⊔Dⱼ`, the instance is in a survivor (lone ⟹ forced; none ⟹ `C⊑⊥`). Sound by
construction (the saturator's unsat is sound). Subsumes B1: B1's "subsumer of C disjoint
with Dᵢ" makes `Sᵢ` clash on that disjoint pair, so B1's forcings are a subset of B2a's.

**FP boundary:** B2a uses only the existing (atomic) unsat machinery; no nominal
disjointness (B3), no global `disjoint_pairs` mutation. The increment-3 trap is absent.

## Components (`crates/owl-dl-saturation/src/lib.rs`)

- New `WorklistEngine` fields:
  - `disjunction_states: Vec<DisjunctionState>` where
    `DisjunctionState { class: ClassId, disjuncts: Box<[ClassId]>, synthetics: Box<[ClassId]>, excluded: FixedBitSet (or Vec<bool>), survivor_count: u32, fired: bool }`.
  - `synth_to_disjunction: HashMap<ClassId, (usize /*disj idx*/, usize /*disjunct idx*/)>`.
- **Seed** (`WorklistEngine::seed`, after normal seeding): build `disjunction_states`
  from `self.rules.disjunctions_by_class` (atomic, ≥2 disjuncts), creating the `Sᵢ`
  synthetics and the reverse map. `survivor_count = n`, `excluded` all-false.
- **Hook** in `process_unsat(c)`: if `synth_to_disjunction[c] = (di, dj)` and the
  disjunct isn't already excluded, set excluded, `survivor_count -= 1`; if
  `survivor_count == 1` and `!fired`, find the lone survivor `Dₖ`, set `fired`,
  `enqueue_subsumer(class, Dₖ)`; if `survivor_count == 0` and `!fired`, set `fired`,
  `enqueue_unsat(class)`. (Collect the enqueue target first; the `disjunction_states`
  borrow is released before the mutating enqueue.)
- **Replace** B1's `process_subsumer` forced-disjunct block: B2a's synthetic test
  subsumes it (B1's disjointness exclusion ⟹ `Sᵢ` unsat ⟹ same forcing via the hook).
  Removing B1's block avoids double-firing; the B1 canaries must still pass (they now
  fire via the synthetic path). If any B1 canary regresses, keep B1's block as a
  cheap complement instead of replacing.

## Testing (negatives-first)

- **B1 canaries still pass** (told/derived-subsumer forcing now via the synthetic).
- **B2a differentiator (deep, non-disjoint incompatibility):**
  `SubClassOf(:X ObjectUnionOf(:A :B))`, `SubClassOf(:A ObjectSomeValuesFrom(:r :P))`,
  `SubClassOf(:X ObjectSomeValuesFrom(:r :Q))`, `FunctionalObjectProperty(:r)`,
  `DisjointClasses(:P :Q)`. Then `X⊓A` has `∃r.P` (from A) and `∃r.Q` (from X) under
  functional `r`, so the single `r`-successor is `P⊓Q⊑⊥` ⟹ `X⊓A` unsat (functional-merge)
  ⟹ force `X⊑B`. B1 does NOT force this (no subsumer of X is disjoint with A) — exactly
  what B2a adds. Assert `X⊑B`, and the precondition that `X⊓A` is genuinely unsat in the
  saturator (the functional-merge clash fires).
- **Forced-to-bot via deep:** both disjuncts' `Sᵢ` unsat ⟹ `X⊑⊥`.
- **Undetermined negative control:** no incompatibility ⟹ no forcing, X satisfiable.
- **Nominal not touched:** `X⊑{a}⊔{b}` ⟹ no synthetics (atomic-only ingestion from B1).

## FP=0 gate (sacred)

Tuned closure-diff FP=0/MISSED=0 (12 fixtures) + ORE pilot/pool `--saturation-only`
before(B1)/after(B2a) sweep: zero spurious-unsat (`a_unsat>b_unsat`); any closure change
additive + saturation ≤ oracle on oracled onts. Watch perf: synthetics are bounded by
(atomic disjunctions × disjuncts) — log if it grows the saturation wall on disjunctive onts.

## Success criteria

B1 canaries green; B2a differentiator forces via a non-disjoint-pair incompatibility
(proves it catches more than B1); workspace green; fmt/clippy clean; FP=0 gate clean.
Foundation for B2b (general-∀ rule) and B3 (nominal disjointness feeds the same synthetics).
