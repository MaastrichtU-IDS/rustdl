# Architecture roadmap — closing the default-mode gap

Drafted 2026-05-26 after the day's three architectural experiments
(MOMS, model-caching root-labels, syntactic module extraction) all
hit measured dead-ends. This doc consolidates what the diagnosis
data actually says and ranks the genuinely multi-week levers by
expected pizza/SIO impact.

Earlier docs:
- [`outperform-hermit-plan.md`](outperform-hermit-plan.md) — strategy memory.
- [`perf-2026-05-24-new-server.md`](perf-2026-05-24-new-server.md) — running benchmarks; §6 / §7 / §8.
- [`moms-plan.md`](moms-plan.md) — MOMS attempt + revert.
- [`model-caching-plan.md`](model-caching-plan.md) — root-labels caching + Phase-2 rethink.
- [`module-extraction-plan.md`](module-extraction-plan.md) — syntactic locality + corpus measurement.

## ⚠ 2026-05-27 finding: per-class `sat(A)` does NOT converge on pizza/SIO

A model-caching idea looked promising: the classify per-class
unsat probe runs `sat(A)` for every class; if that converges, we
could cache A's satisfying model and answer each pair `sat(A ⊓
¬B)` by checking whether `B` is in the model (a model of `A`
lacking `B` soundly witnesses `A ⋢ B`). This sidesteps the
[`model-caching-plan.md`](model-caching-plan.md) §A objection
because it caches from the *pure* `sat(A)` probe, not a pair
probe.

**Measured and falsified.** `owl-dl-bench sat --deadline-ms 5000`
on SIO classes (`SIO_000000`, `_000004`, `_000009`) **all time
out at 5 s**. The tableau cannot build a model of a *single* SIO
class, never mind a pair. So:

- The classify per-class probes on SIO silently time out (treated
  as satisfiable via `unwrap_or(true)` — correct, since SIO has no
  unsat classes, but via timeout not via a completed model).
- There is no completed model to cache. **Model caching (Lever C)
  is dead for SIO** — and by the same explosion, for pizza.

**Root cause: blocking never fires** (`is_blocked_true = 0` on
pizza, per `perf-2026-05-24-new-server.md` §6). The naive tableau
grows models whose label sets stay pairwise non-subset (pizza
toppings, SIO's deep class structure), so pair-blocking can't cap
growth. Under naive SROIQ tableau, model size is worst-case
exponential; pizza/SIO classes hit that worst case.

**Consequence for the roadmap.** Every lever that depends on a
*completed model* (caching, snapshot/replay) is out for pizza/SIO.
Every *per-step* efficiency lever (MOMS, lazy unfolding,
module-extraction, inverse-Vec, early-presence) has been measured
not to move timeout-bound walls. The remaining levers that could
actually make pizza/SIO classes *converge* are:

1. **Hypertableau** (Motik 2008) — smaller branching by
   construction; the algorithm HermiT uses. Multi-month rewrite of
   the tableau core.
2. **Anywhere/core blocking strong enough to fire on
   pizza/SIO-shaped models** — research-grade; naive subset
   blocking demonstrably doesn't fire here.
3. **Accept `--saturation-only`** as the production answer for
   mostly-EL workloads (SIO 266 s → 0.22 s, sound
   under-approximation, 0.19 % edge loss). Already shipped.

(3) is the pragmatic truth today. (1)/(2) are the only paths to
default-mode parity and both are multi-month. **Lazy unfolding
(Lever A, shipped) remains the right call for
convergent-pair-dominated workloads like family (−28 %)** — those
DO build models, so shrinking them helps.

## Grounded diagnosis (data, not assumptions)

`rustdl tbox-stats` on the corpus:

| Workload | concept_rules | residual GCIs | residual_or | default wall |
|---|---|---|---|---|
| pizza | 663 | 4 | 4 | 29 s |
| SULO | 46 | 14 | 11 | 0.23 s |
| SIO-stripped | 2244 | 56 | 54 | 266 s |
| family-stripped | 114 | 53 | 53 | 9 s |
| RO-stripped | 64 | 165 | 165 | 28 s |
| GO basic | 72 697 | 0 | 0 | 22 s (pure EL) |

The pizza-trace finding "38 unique disjunctions branched 75× each
across 92 nodes" — those 38 disjunctions are **not** residual
GCIs. Pizza has only 4 residuals. They come from absorbed
concept-rules whose conclusion is `Or(_)`: a node carrying the
trigger atomic gets the Or label, and every R-successor that
inherits the trigger via propagation gets it too.

**This means the levers are different for different workloads.**

## Levers ranked

### Lever A — Lazy unfolding of residual GCIs

> Don't materialise `⊤ ⊑ Or(d1, …, dn)` on every node. Only add
> the Or label on nodes whose existing labels could matter for
> the disjunction (e.g. nodes that already carry one of `di`'s
> downstream consequences).

| Workload | Residual_or | Expected impact |
|---|---|---|
| RO | 165 | very high — RO's wall is dominated by per-node residual branching |
| SIO | 54 | high |
| family | 53 | high |
| SULO | 11 | medium (already fast) |
| pizza | 4 | low — residuals aren't pizza's bottleneck |

**Scope:** 1–2 sessions. The change touches `apply_residual_gcis`
and adds a per-residual "trigger condition" predicate.
Soundness: every node still gets every residual whose downstream
consequences could possibly fire; we only skip nodes where the
residual is provably inert. This is the standard "lazy
expansion" optimisation from the description-logic literature.

### Lever B — Successor-trigger pruning for concept_rules

> When `apply_forall` propagates a label `T` from parent to
> R-successor, only do it if `T` actually triggers something on
> the successor (or its descendants). Pizza's pair-loop trace
> showed many successors getting Or labels via this path that
> they didn't actually need.

| Workload | concept_rules | Expected impact |
|---|---|---|
| GO | 72 697 | none (pure EL, no successors of interest) |
| SIO | 2244 | high — many concept_rules fire on every successor |
| pizza | 663 | very high — 663 × ~10 successors × ~10 saturate passes = explosion |
| family / RO / SULO | < 200 | medium |

**Scope:** 2–3 sessions. The change adds a "reachable triggers"
analysis to absorption: for each role R and each label T, does
`T` ever appear in any rule a non-T-carrying ancestor would
care about? If no, `apply_forall` can drop T at the successor.
Soundness is the hard part — needs care for the
inverse-role-and-cardinality interactions.

### Lever C — HermiT-style deep model caching

> Cache full completion-graph snapshots (root + all
> R-successors + their labels) keyed by the concept being
> tested. Future queries `key ⊓ extra` replay the snapshot and
> only saturate the delta.

| Workload | Expected impact |
|---|---|
| Any workload where the classify pair-loop tests the same `key` against many `extra`s | high |

**Scope:** 3–4 sessions. The Phase-1 [`model_cache.rs`](../crates/owl-dl-reasoner/src/model_cache.rs)
stub is the data-structure foundation; [`model-caching-plan.md`](model-caching-plan.md)
§B is the revised design (full snapshot + replay, not root
labels).

### Lever D — Real ⊥-locality module extraction

> Build module-per-pair via the standard ⊥-locality algorithm,
> strip the prepared TBox to just the module's axioms for the
> tableau probe.

| Workload | Expected impact |
|---|---|
| Workloads with multiple sub-ontologies (e.g. RO is a hub of small modules) | medium |
| Pizza/SIO (one signature component each, per [`module-extraction-plan.md`](module-extraction-plan.md) §A) | none |

**Scope:** 2–3 sessions. Real ⊥-locality is more sophisticated
than the syntactic co-occurrence already shipped — uses
locality-class structure rather than direct mention.

### Lever E — Hypertableau

> Replace the tableau algorithm with resolution-style hypertableau
> (Motik 2008) — disjunctions become rule clauses, branching
> factor drops by construction.

**Scope:** multi-month. Effectively rewriting the tableau
engine. Listed here for completeness, not a near-term target.

## Recommended sequencing

Given the data, the highest expected wins per session are:

1. **Lever A (lazy unfolding of residual GCIs)** — 1–2 sessions,
   targets RO + SIO + family. Pizza's residual count is too low
   for A alone to move pizza's wall.
2. **Lever B (successor-trigger pruning)** — 2–3 sessions,
   targets pizza specifically. Compounds with A on SIO/family.
3. **Lever C (deep model caching)** — 3–4 sessions, broadest
   impact but biggest implementation surface.

Levers D and E sit further out; D only after we see a workload
where the syntactic-locality measurement found components but
the orchestrator wastes work crossing them, E only if everything
else plateaus.

**Total scope for the realistic "default-mode rustdl is
competitive with HermiT" claim: ≥ 6 sessions of focused work**,
each ending in a measurable diff (pass/revert per the
[`moms-plan.md`](moms-plan.md) §A criterion).

## Acceptance criteria for this roadmap

Not a deliverable — this doc just declares the next 6–10
sessions' direction. Each lever has its own plan doc on landing.

## What is *not* in scope here

- Coverage extensions (data properties, more axiom shapes).
  Important for ontology compatibility but orthogonal to the
  default-mode perf gap.
- Saturation engine improvements. Already strong on workloads
  it handles; not the bottleneck on the timeout-bound walls.
- `--saturation-only` extensions. Already shipped (5 entry
  points). Sound under-approximation is the pragmatic answer
  for mostly-EL workflows.

## Status of architectural attempts so far

| Lever | Attempt date | Status | Plan doc |
|---|---|---|---|
| Top-down classifier | 2026-05-25 | shipped, default | n/a |
| MOMS disjunct ordering | 2026-05-25/26 | reverted, dead-end documented | [`moms-plan.md`](moms-plan.md) §A |
| Per-call optimisations (early-presence check, inverse Vec) | 2026-05-25 | shipped, real but small | n/a |
| Model caching (root labels) | 2026-05-26 | Phase 1 shipped, Phase 2 rethink | [`model-caching-plan.md`](model-caching-plan.md) §B |
| Syntactic module extraction | 2026-05-26 | Phase 1 shipped, integration not warranted | [`module-extraction-plan.md`](module-extraction-plan.md) §A |
| `--saturation-only` user-facing mode | 2026-05-26 | shipped, 5 entry points | n/a |
| Lazy-fire residual GCIs when a disjunct is already labelled | 2026-05-26 | attempted, reverted (zero wall change on pizza/SIO/family — residual Or-with-already-present-disjunct is rare in practice) | inline in this doc |
| Lever A — lazy unfold of residual GCIs (proper trigger analysis) | 2026-05-26/27 | **Phase 1 + 2 shipped — first architectural win.** family ~8.7s→~6.9s (~20%), verdicts unchanged (all real-corpus regression tests pass). pizza/RO flat (their bottleneck is elsewhere). | [`lazy-unfolding-plan.md`](lazy-unfolding-plan.md) |
| Lever B — successor-trigger pruning | future | not started | TBD |
| Lever C — deep model caching (Phase 2a/2b/2c) | future | Phase 1 shipped | [`model-caching-plan.md`](model-caching-plan.md) |
| Lever D — real ⊥-locality modules | future | not started | TBD |
| Lever E — hypertableau | future | out of scope | n/a |
