# Lazy unfolding of residual GCIs — implementation plan

Drafted 2026-05-26. Mirrors the
[`moms-plan.md`](moms-plan.md) /
[`model-caching-plan.md`](model-caching-plan.md) /
[`module-extraction-plan.md`](module-extraction-plan.md) format.
Multi-session scope; this file tracks the design across sessions.

Per [`architecture-roadmap.md`](architecture-roadmap.md), this is
**Lever A** — expected impact on RO / SIO / family default-mode
walls; less so on pizza (only 4 residuals).

## Goal

Stop materialising every residual GCI on every node. Each
residual has a *trigger condition*; the residual is added to a
node only when the trigger fires. Sound by construction —
triggers are derived from the residual's body so that "no trigger
fires" implies "the residual is trivially satisfied at this
node."

Naive `⊤ ⊑ φ` semantics says every node must satisfy `φ`.
"Satisfies" is cheap for some shapes (`Atomic` — just be a member)
and structural for others (`Or` — at least one disjunct must
hold). The lazy variant defers the structural ones until the
node's existing labels actually require the branching.

## The 2026-05-26 dead-end this builds on

A simple lazy-fire attempt (skip residual Or when any disjunct
is already a label of the node) shipped + reverted earlier today
([`architecture-roadmap.md`](architecture-roadmap.md) status
table). Measurement showed zero wall movement because the
case "Or with disjunct already labelled" is rare in practice —
disjuncts only become labelled after the disjunction is
*resolved*, not before.

The proper trigger-driven version is different: it doesn't wait
for a disjunct to appear; it waits for a *reason the disjunction
matters at this node*, e.g.:
- the node has an explicit `Not(di)` for some disjunct (forcing
  another `dj`),
- the node is on the model-relevant path of a current pair
  query,
- the node is a witness whose role-membership forces the body.

The session-scoped Phase 1 here is the **trigger taxonomy +
analysis**, not the integration. Phase 2 wires it in and
measures.

## Trigger taxonomy

A residual GCI body `φ` produces a `ResidualTrigger` value at
absorption time. The variants:

```rust
enum ResidualTrigger {
    /// `φ = ⊤` (vacuous), `φ = Atomic(C)` (everything is C),
    /// `φ = And(only atomics)`. Cheap to materialise; no defer.
    /// Materialises on every node, same as today.
    Eager,

    /// `φ = Or(d1, ..., dn)`. Defer until either:
    ///  - some `Not(di)` is on the node (forcing a `dj`), or
    ///  - saturation has otherwise stabilised and the Or has
    ///    not been discharged by any di becoming a label.
    /// The Phase-2 wiring needs to interact with saturate's
    /// fixed-point loop — see §"Integration with saturate".
    DeferOr { disjuncts: Box<[ConceptId]> },

    /// `φ = Not(C)`. Materialises only when C appears on the
    /// node. Reactive — needs a "label-add hook" to fire.
    DeferNot { complement: ConceptId },

    /// `φ = ∀R.D`. Materialises only on nodes with an outgoing
    /// R-edge (or inverse-R incoming edge). Reactive on edge
    /// creation.
    DeferAll { role: Role, body: ConceptId },

    /// `φ = ∃R.D`. Forces a successor; defer is unsound (the
    /// existential is a real model commitment). Treat as Eager
    /// but flag — could be lazy in a future "demand-driven
    /// expansion" phase.
    Eager_Exists { role: Role, body: ConceptId },

    /// `φ = Min(n, R, D)` / `Max(n, R, D)`. Cardinality
    /// commitments. Same as ExistsEager — defer is unsound.
    Eager_Cardinality,

    /// Fallback for shapes we don't have a smart trigger for.
    Eager,
}
```

The key win is `DeferOr` — that's the bulk of the residuals on
RO (165 / 165), SIO (54 / 56), family (53 / 53), and SULO
(11 / 14). All `residual_or` per
[`perf-2026-05-24-new-server.md`](perf-2026-05-24-new-server.md)
§7 / `rustdl tbox-stats`.

## Soundness argument for DeferOr

Claim: if a residual `⊤ ⊑ Or(d1, ..., dn)` is *not* materialised
on a node `y`, the eventual model still satisfies it iff at
saturate stable state some `di` is in `L(y)` *or* the trigger
condition is checked at stable state.

Proof sketch: classical tableau correctness says a saturate-
stable model satisfies every materialised constraint and every
constraint forced by the materialised ones. For `⊤ ⊑ Or`, the
universal quantification means every node must satisfy it;
omitting it at `y` is sound iff we can prove `y` satisfies it
some other way:

- *Direct witness*: some `di ∈ L(y)`. The Or is true at `y`.
- *Forced witness*: some constraint at `y` derives `di`. The Or
  is true at `y` through derivation.
- *Stable safety*: saturation has otherwise stabilised on `y`
  and no current constraint at `y` mentions any `di` or its
  complement. Then any model extension of `y` is free to satisfy
  the Or via an arbitrary `di` — sound.

The third case is the subtle one. It requires a "stable-state
check" at the end of saturate: walk the deferred Or-residuals
and verify none of them needs materialising. The pizza-shape
worry is that this stable-state check is itself O(|nodes| ×
|residuals|) and adds back what the deferral saved. Mitigation:
maintain a *single* "any deferred residual needs materialising"
flag updated reactively as labels are added.

## Phases

**Phase 1 (this session):**
- Plan doc (this file).
- `crates/owl-dl-core/src/residual_trigger.rs` — the
  `ResidualTrigger` enum + classify function. Pure analysis;
  no tableau coupling.
- Unit tests on hand-built fixtures covering every body shape.
- A `rustdl residual-triggers FILE` CLI that prints the trigger
  histogram for an ontology — lets us see ahead of integration
  how many residuals would be deferred vs eager.

**Phase 2 (next session):**
- Wire `ResidualTrigger` into `apply_residual_gcis`. Eager
  variants behave as today; DeferOr variants are stored in a
  per-node "pending" set and skipped at the rule body.
- A "stable-state sweep" at the end of saturate's outer loop:
  for each node, for each deferred Or, check whether the Or is
  trivially satisfied; if not, materialise it.
- Measure: pizza, SIO, family, RO walls. Acceptance criterion
  per [`moms-plan.md`](moms-plan.md) §A — revert if zero
  movement.

**Phase 3 (later session):**
- Reactive triggers for `DeferNot` and `DeferAll`. These need
  hooks in `add_label` / `add_edge` to fire pending residuals.
- Refine the stable-state sweep to be incremental (touch only
  nodes whose labels changed since the previous sweep).

## Validation strategy

- All in-tree unit tests pass (≥260 today).
- 87-fixture differential corpus: zero verdict diff vs. baseline.
- Real-corpus tests: pizza, sio-stripped, family, RO unsat sets
  match HermiT-via-ROBOT reference.

## Acceptance criteria

- Phase 1: trigger classification function correct on hand-built
  fixtures; CLI runs against the corpus and produces sensible
  histograms.
- Phase 2: at least one of RO/SIO/family default-mode wall moves
  ≥ 10 %. Pizza is expected to be flat (only 4 residuals); not
  a Phase-2 acceptance criterion.
- If Phase 2 ships with zero wall movement on every workload,
  revert per the
  [`moms-plan.md`](moms-plan.md) §A rule.

## §C — Phase 2 results (2026-05-27): one win, kept

Phase 2 shipped (`1b41023`): `apply_residual_gcis` skips Or-shaped
residuals; `apply_deferred_or_residuals` materialises them at
saturate stable-state only when no disjunct is present.

Measured (3-rep median, `--pair-timeout-ms 200`):

| Workload | before | after | delta |
|---|---|---|---|
| family-stripped | ~8.7 s | ~6.9 s | **−20 %** |
| RO-stripped | ~27 s | ~27 s | flat |
| SIO-stripped | ~268 s | ~268 s | flat |
| pizza | ~29 s | ~29 s | flat (only 4 deferred residuals) |
| GO | ~22 s | ~22 s | flat (no residuals) |

Verdicts unchanged everywhere — all 55 lib tests + real-corpus
regression (pizza/ro/family/go) pass; family's direct-edge set is
identical to the saturation-only reference.

**Why family wins but SIO/RO don't.** family's wall has a healthy
fraction of *convergent* pairs (the tableau reaches a verdict
before the 200 ms deadline). Lazy unfolding shrinks the per-pair
model — fewer universal Or labels propagated to every successor —
so those convergent pairs finish faster. SIO and RO are dominated
by *timed-out* pairs (33 394 / 657 respectively): those pairs
don't converge either way, and each still burns its full 200 ms
budget. Making the futile search cheaper-per-step doesn't return
wall when the step count is capped by the timeout, not by the
work.

This is consistent with the whole-session finding: **per-pair
efficiency moves convergent-pair-dominated walls (family) but not
timeout-bound walls (SIO, pizza).** The timeout-bound walls need
the pairs to *converge* — which is Lever B (successor-trigger
pruning, to shrink pizza's concept-rule explosion) or Lever C
(model caching, to reuse work across pairs), not residual lazy
unfolding.

**Decision: keep Phase 2.** It meets the §A acceptance criterion
(family > 10 %), is sound, and adds no regressions. Phase 3
(reactive `DeferNot` / `DeferAll` triggers) is deferred until a
workload shows residual `Not` / `∀` GCIs worth the reactive-hook
complexity — none in the current corpus (`rustdl
residual-triggers` shows defer_not = defer_all = 0 everywhere).

## §D — Concept-rule Or extension (2026-05-27)

After Phase 2 (residual Ors), the same deferral was extended to
per-trigger disjunctions `A ⊑ Or(d1, ..., dn)` — the concept-rule
conclusions — since those are the dominant branching source on
pizza (15 vs 4 residual) and a big share on SIO (50) and family
(53). `rustdl tbox-stats` now reports `concept_rule_or`.

Mechanism (`749ddd3`):
- `apply_concept_rules` skips Or-shaped conclusions in the inner
  loop.
- `apply_deferred_concept_or_rules` materialises them at
  stable-state, per Atomic trigger, with the trigger label's
  deps (back-jumping correctness), unless the node already has
  the Or or a disjunct.

Measured (compounding on Phase 2):

| Workload | baseline | +residual defer | +concept-rule defer |
|---|---|---|---|
| family-stripped | 8.7 s | 6.9 s | **6.3 s** (−28% total) |
| pizza | 29 s | 29 s | 29 s (timeout-bound) |
| SIO | 268 s | 268 s | 268 s (timeout-bound) |

Verdicts unchanged: all 11 real-corpus regression tests pass,
including `sio_matches_hermit_exactly` and
`pizza_unsat_matches_hermit_exactly`.

**Confirmed boundary.** Lazy unfolding — whether of residual or
concept-rule Ors — helps *convergent-pair-dominated* walls
(family: −28%) and does nothing for *timeout-bound* walls (pizza,
SIO). This is now a thoroughly-tested invariant across the
session: per-pair efficiency cannot shorten a wall capped by the
per-pair timeout × non-converging-pair count. The timeout-bound
walls need the pairs to converge — Lever B (deeper search-tree
reduction) or Lever C (model caching), neither of which is
per-step efficiency.

## Open questions

- **Reactive cost vs sweep cost.** Reactive triggers (fire when
  `add_label` adds a relevant label) are precise but pay per
  label-add; a stable-state sweep is amortised but pays
  O(|pending| × |nodes|). On pizza/SIO this trade-off matters;
  the Phase-2 implementation should make the choice data-driven
  via counters.
- **Rollback.** When the trail rolls back a label, deferred
  residuals that were materialised because of that label need to
  un-materialise too (or the deferred-status needs to flip back).
  Simpler design: deferred residuals materialise via
  `add_label_with_deps` so the trail handles rollback
  symmetrically. Confirm in Phase 2.
- **Interaction with `--saturation-only`.** Saturation-only
  skips the tableau entirely, so the deferred Or never gets a
  branching opportunity — it stays deferred. That's correct: the
  EL closure already records its sound consequences; deferred
  Or-residuals don't add new EL-side facts. Confirm with a
  regression test in Phase 2.
