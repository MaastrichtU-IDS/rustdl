# Module extraction — implementation plan

Drafted 2026-05-26. Mirrors the [`moms-plan.md`](moms-plan.md) /
[`model-caching-plan.md`](model-caching-plan.md) format. Multi-
session scope; this file tracks the design.

## Goal

Skip tableau pair-queries that signature analysis can prove
"definitely not subsumed." The classify-pair-loop's timeout-bound
walls (pizza, SIO default-mode) are dominated by exactly these
probes — every one consumes 200 ms before defaulting to
not-subsumed. If a cheap pre-filter can detect a subset of those
pairs structurally, that subset's contribution to the wall
collapses.

## Soundness argument (the easy direction)

Standard locality-based result, simplified:

> Let `sig(C)` be the set of class and role IRIs mentioned in
> the smallest axiom subset closed under co-occurrence starting
> from `C`. If `sig(A) ∩ sig(B) = ∅`, then `A ⊑ B` is *not*
> entailed by the ontology (assuming `A` is satisfiable, which
> the per-class unsat probes already verify, and assuming `B` is
> not equivalent to `⊤`).

Intuition: pick any model `M` of the KB with `A` non-empty;
because `A`'s signature and `B`'s signature share no axioms, we
can construct a model `M'` that agrees with `M` on `A`'s
signature and makes `B` empty — hence `A ⊑ B` fails.

This is a **sound under-approximation of negative subsumption**:
if the filter says "definitely not subsumed" the verdict is
correct; otherwise the filter falls through and the tableau
runs as before.

## Algorithm

### Step 1 — Build the co-occurrence graph

For every TBox axiom, every class/role IRI it mentions becomes
a node. Every pair of IRIs co-mentioned in the same axiom is
joined by an undirected edge. The result is a graph whose
connected components are precisely the locality partitions.

Cost: O(|axioms| × |max mentions per axiom|^2). For pizza
(~150 axioms, ~10 mentions each), 150 × 100 = 15 k edges. For
SIO (~3 k axioms, ~10 mentions each), 3 k × 100 = 300 k edges.
Both finish in milliseconds.

### Step 2 — Compute connected components

Union-Find over the IRIs gives each class a `component_id : u32`.
Pre-classify each class with its component id.

### Step 3 — Pre-filter pair queries

Inside `find_direct_parents_top_down`, before dispatching to
`subsumes_via_tableau`:

```rust
if component[c] != component[d] && d != THING && d != NOTHING {
    stats.module_filtered_pairs += 1;
    return Ok(Some(false));
}
```

Special-case `⊤` (every class subsumes it) and `⊥` (subsumed by
everything). Both are detected by the existing unsatisfiability
probes / closure.

### Step 4 — Plumb the stats

Add `ClassificationStats::module_filtered_pairs: u64` so
benchmarks can see how many timeouts the filter avoided.

## Cost / win estimate

The win is bounded by the fraction of timed-out pairs that
land in different signature components.

| Workload | timed_out_pairs | est. cross-component fraction | est. wall delta |
|---|---|---|---|
| pizza | 1172 | low (mostly one big component) | < 10% |
| sio-stripped | 33 394 | medium (many sub-ontologies in SIO) | 20–40% |
| ro-stripped | (not measured under default) | high (RO is a meta-ontology of small modules) | 30–50% |

These are guesses; the real value comes from measurement at
Phase 2. If components are mostly singletons (pathological
ontology shape) or one giant component (pizza), the filter
does nothing.

## Phases

**Phase 1 (this session):**
- Plan doc (this file).
- Signature/component computation in
  `crates/owl-dl-core/src/locality.rs` (new module).
- Unit tests on small fixtures with known component structure.
- No orchestrator integration yet.

**Phase 2 (next session):**
- Wire the component-id check into
  `classify::find_direct_parents_top_down`.
- Add `module_filtered_pairs` to `ClassificationStats`.
- Measure pizza wall, SIO wall.

**Phase 3 (if Phase 2 moves walls):**
- Lift the filter into `PreparedOntology` so
  `is_subclass_of` / `is_instance_of` benefit too.
- Add `--no-module-filter` flag for ablation.

## Validation strategy

- All ≥260 in-tree unit tests pass.
- 87-fixture differential corpus: zero verdict diff vs. baseline.
- Real-corpus regression: pizza, sio-stripped, family, RO unsat
  sets match HermiT-via-ROBOT reference. The filter must not
  make a `not-subsumed` claim where the full classifier finds
  subsumption.

## Acceptance criteria

- Phase 1: data structure correct on hand-built fixtures.
- Phase 2: pizza or SIO wall moves measurably (target: ≥10 %
  reduction on at least one).
- If Phase 2 ships with zero wall change, revert per the
  [`moms-plan.md`](moms-plan.md) §A lesson.

## §A — Phase 1 measurement: one component everywhere that matters

Before shipping Phase 2, the new `rustdl locality-stats` CLI was
run against the real-ontology corpus. The headline: **every
workload that needs help has exactly one connected component.**

```
pizza:           1 component out of 99 classes    (100% dominance)
SULO:            1 component out of 17            (100%)
SIO-stripped:    1 component out of 1585          (100%)
family-stripped: 1 component out of 58            (100%)
RO-stripped:    10 components, largest 47/58      (81%)
GO basic:    13380 components, largest 24428/51937 (47%)
```

The signature-disjointness filter prunes pair-queries `(A, B)`
where `A` and `B` live in different components. On the workloads
where this matters — pizza and SIO, where the default-mode wall
is timeout-bound on 1172 and 33 394 pairs respectively — every
class shares a component. The filter would prune **zero** pairs.

GO has rich partition structure (~26 % of pair-queries cross
components) but is already on the pure-EL fast path; default-mode
classify already finishes in seconds. The filter would compound
trivially with the existing path but the headline wall is already
satisfactory.

RO sits between the two — 10 components on 58 classes — but RO is
small enough that the absolute number of skipped pairs is at most
~tens, well below noise.

### Why every "interesting" workload collapses to one component

The co-occurrence graph unions classes mentioned in the same
axiom. In real ontologies the **top-level `⊤` / `Thing`** or a
similarly universal class (e.g. SIO's `entity`, pizza's
`DomainConcept`) is referenced — directly or transitively via
domain/range axioms — by virtually every class definition.
Removing `⊤` from the graph is not enough; the top-level domain
ontology class plays the same role.

A more sophisticated locality construction — e.g. the standard
⊥-locality module algorithm with locality-class signatures
rather than naive co-occurrence — could in principle do better,
but the gain is upper-bounded by what fraction of pair-queries
genuinely cross modules. For pizza-shaped ontologies the answer
is "very few" by inspection: every class is connected to every
other class through `hasTopping` / `hasBase` chains.

### Decision

**Do not ship Phase 2.** Per the §A revert criterion in
[`moms-plan.md`](moms-plan.md), an integration that we've
already measured to skip zero pairs would be ceremony, not
optimization.

Phase 1 stays:
- `crates/owl-dl-core/src/locality.rs` — data structure + 4
  unit tests + soundness proof in the module docs.
- `crates/owl-dl-reasoner/src/lib.rs` — `LocalityStats` +
  `locality_stats(ontology)` helper.
- `rustdl locality-stats FILE` — diagnostic CLI.

Future work that could resurrect this: a richer locality
construction (⊥-locality module algorithm), or applying the
signature filter to a *different* angle of work (e.g. per-pair
TBox reduction inside the tableau — feed the tableau only the
axioms in `module({A, B})` instead of the full TBox). That's
multi-week scope and a separate plan doc.

## Open questions

- **Role-IRI edges.** Should `r` and `s` count as co-occurring
  when `SubObjectPropertyOf(r, s)`? Initial pass: yes — the role
  hierarchy is part of axiomatic connectivity. Iterate if
  measurements show false positives.
- **`⊤` and `⊥` handling.** These IRIs touch every axiom in
  principle. We exclude them from the co-occurrence graph so
  they don't collapse the partition to one giant component.
- **Inverse roles.** `Role::Inverse(r)` shares its `RoleId` with
  the named version — no separate node needed.
- **Annotation axioms.** Already dropped during conversion, so
  they don't contribute to the graph. (Cf. `convert.rs` § 270.)
