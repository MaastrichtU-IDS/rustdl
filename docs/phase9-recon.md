# Phase 9 — ORE-15672 NoVerdict cluster recon

Run 2026-06-03 at HEAD `420f78d` (Phase 8 shipped). Temporary
instrumentation in `crates/owl-dl-reasoner/src/classify.rs::classify_top_down_internal`
(reverted after probe) captured the classes that hit
`LabelOracle::NoVerdict` during Phase 7's label-cache build on
ORE-15672-shoin, plus the `(sub, sup)` pairs that hit
`subsumes_via_tableau`'s per-pair deadline in the top-down walk. Goal:
identify which SROIQ construct(s) make these classes intractable for
the wedge, and recommend the saturator extension that would yield the
most coverage.

Companion to:
- Phase 7 results: `docs/phase7-results.md`.
- Phase 8 results: `docs/phase8-results.md` (where ORE-15672 was
  flagged as having a genuine 46-class hard cluster vs ORE-10908's
  deadline-bound stallers).

## Premise correction (load-bearing)

**Phase 8 documented a "46-class NoVerdict cluster" for ORE-15672. The
actual cluster is 3 classes, not 46.** The `# label heuristic:
misses=46` counter records consultation events during the tier walk,
not distinct classes — each of the 3 stallers is consulted ~15 times
as the top-down walker visits it across the 82-class lattice. Phase 8
also reports `# timed-out pairs: 109` separately; the timeout probe
confirms all 109 pairs have one of the same 3 class indices as `sub`:

```
37 timeouts with sub=45 (e-interaction-situation)
36 timeouts with sub=51 (epistemic-workflow-enactment)
36 timeouts with sub=41 (e-collaboration-situation)
```

So both the completeness story (3 NoVerdict at cache-build) and the
wall story (109 timed-out per-pair tableau calls) collapse to the
**same 3 classes**. The cluster has not been mis-discovered — Phase 8's
sweep numbers stand — but the right framing is "3 hard classes, each
of which fails to converge in any granularity (cache-build, per-pair,
or even a 30s cache-build budget retested in this probe)" rather than
"46 distinct hard classes."

Wall arithmetic at the default 200 ms per-pair deadline: 109 pair
timeouts × 200 ms ÷ 8 cores ≈ 2.7 s parallel wall, plus the 5000 ms
cache-build budget × 3 stallers parallel ≈ 5 s, plus the
tractable-class baseline (~5 s by analogy with ORE-10908 at 7.8 s).
That accounts for the observed 30.6 s within noise. **Konclude
classifies the same 3 classes — the gap is engine effectiveness on
this specific axiom shape, not a wrong-budget problem.**

## Ontology shape

ORE-15672-shoin: 82 named classes, 809 lines, SHOIN expressivity.
Construct counts:

| Construct | Count |
|---|---|
| SubClassOf | 102 |
| EquivalentClasses | 24 |
| ObjectSomeValuesFrom | 61 |
| ObjectAllValuesFrom | 18 |
| ObjectMinCardinality | 4 |
| ObjectMaxCardinality | 0 |
| ObjectExactCardinality | 0 |
| ObjectOneOf | 1 |
| ObjectHasValue | 1 |
| InverseObjectProperties | 15 |
| ObjectInverseOf (inline) | 0 |
| TransitiveObjectProperty | 7 |
| SymmetricObjectProperty | 1 |
| ReflexiveObjectProperty | 0 |
| FunctionalObjectProperty | 0 |
| Role chain (SubObjectPropertyOf chain) | 0 |

Notable: 15 named inverse-property pairs (≈30% of all roles are part
of an inverse pair), 7 transitive roles (including `proper-part`,
`predecessor`, `specializes`, `superordinated-to`,
`subordinated-to`). The single `ObjectHasValue` and `ObjectOneOf` are
both inside `epistemic-workflow-enactment`'s equivalent body. ≤n /
exact cardinality are absent (confirming the SHOIN name — no Q).

## The 3 NoVerdict classes

1. `OD/OntologyDesign.owl#e-collaboration-situation` (idx 41)
2. `OD/OntologyDesign.owl#e-interaction-situation` (idx 45)
3. `OD/OntologyDesign.owl#epistemic-workflow-enactment` (idx 51)

All three are descendants of `epistemic-workflow-enactment` (#51 is
itself the root of the cluster; #41 and #45 are its `Equivalent` body
conjuncts).

### Inherited body (#51 epistemic-workflow-enactment)

`EquivalentClasses(epistemic-workflow-enactment ObjectIntersectionOf(`
- `epistemic-influence-situation`
- `∃proper-part.agent-co-participation-situation`
- `∃setting-for.(information-object ⊓ ObjectHasValue(classified-by, additional-resource))`
- `∃setting-for.(information-object ⊓ ObjectHasValue(classified-by, working-resource))`
- `∃setting-for.(rational-agent ⊓ ∃classified-by.accountablePerformer)`
- `∃direct-successor.knowledge-production-goal-situation` `))`

with `proper-part` transitive + inverse `proper-part-of`,
`classified-by` inverse `classifies`, `setting-for` inverse `setting`,
`direct-successor` inverse `direct-predecessor`. Plus
`SubClassOf(... ∃satisfies.epistemic-workflow)` and
`SubClassOf(... ∃setting-for.time-interval)`.

`epistemic-influence-situation` (the first conjunct) is itself a
4-conjunct compound with `ObjectMinCardinality(5 setting-for)`.

### Local additions on #41 / #45

Both add three extra `∃proper-part.X` axioms reaching into
`communication-situation`, `co-participation-situation`, and
`e-usage-situation` — and `co-participation-situation` carries
`ObjectMinCardinality(4 setting-for)` and an `ObjectMinCardinality(1
co-participates-with)` (the only symmetric role). They also each have
their own `EquivalentClasses` body differing in whether the
`setting-for` filler uses `∀classified-by.accountablePerformer` (#41)
or `∃classified-by.nonAccountablePerformer` (#45).

### Reconciliation: why does e-usage-situation resolve but these don't?

Critical control: `e-usage-situation` (idx 47) is **structurally
identical** to `e-interaction-situation` (#45) at the
`EquivalentClasses` level — same shape
`epistemic-workflow-enactment ⊓ ∃setting-for.(rational-agent ⊓
∃classified-by.X)`, differing only in the X filler. e-usage-situation
**resolves** to `Sat` in the cache build; e-interaction-situation
**times out**. The discriminator therefore is NOT the
`EquivalentClasses` construct shape — the wedge demonstrably handles
that shape.

The discriminator is the **local additions** that #41 and #45 carry
but #47 (e-usage-situation) does not:

- 3× `∃proper-part.X` axioms where `proper-part` is transitive +
  inverse — these multiply the model-completion frontier through the
  transitive closure of `proper-part`/`proper-part-of` edges, and the
  targets (`communication-situation`, `co-participation-situation`,
  `e-usage-situation`) each have their own multi-conjunct definitions.
- For #41: `∃satisfies.e-collaboration` (extra inverse-role hop).
- For #45: ditto with `e-interaction`.

In other words: the per-class cost is **inherited compound body (5
conjuncts + a cardinality-5 ancestor + ObjectHasValue) AMPLIFIED by
3 extra ∃proper-part hops into other compound classes**. The wedge
expansion under double-blocking explores enough of the model frontier
to exceed both the 200 ms per-pair and the 5000 ms cache-build budget;
even a 30000 ms cache-build retest (this probe) still leaves the same
3 as `NoVerdict`.

#51 (epistemic-workflow-enactment) doesn't have the extra ∃proper-part
hops itself, but it stalls because its own body is the source of the
cost, AND it's the body that #41/#45 inherit (so any optimisation
benefits all three).

## Construct cluster histogram

Single-cluster, n=3:

| Cluster | Class count | Sub-pattern |
|---|---|---|
| Compound-body × inverse-transitive proper-part | 3 | inherited 5-conjunct body + 3-5 inverse-role property hops + transitive role expansion |

All 3 sit in the same axiom cluster; no other classes have NoVerdict
at any cache-build budget tested. There is no second cluster.

## Diagnosis

The 17× gap to Konclude on ORE-15672 is **not a missing construct rule
in the saturator**. The wedge already handles every individual
construct in this ontology (proven by e-usage-situation, a
near-identical structural twin of e-interaction-situation,
resolving). What it fails to do is **converge on the joint
expansion** of:

- a 5-conjunct inherited body containing a nominal singleton
  (`ObjectHasValue`),
- ancestor `ObjectMinCardinality(5 setting-for)` from
  `epistemic-influence-situation`,
- 3 local `∃proper-part.X` hops with `proper-part` transitive and
  inverse,
- 9 inverse-role pairs in the surrounding role hierarchy
  (`setting-for`/`setting`, `classified-by`/`classifies`,
  `direct-successor`/`direct-predecessor`, etc.).

Konclude reaches `Sat` on the same body in ≈ 60 ms per class; rustdl's
wedge doesn't converge in 30000 ms. The performance differential lives
in the model-completion strategy itself (heuristic ordering of
disjunctive expansion, more aggressive blocking, model-caching across
classes), not in any single rule.

## Recommendation for saturator extension

**No targeted saturator extension is recommended.** The construct
discriminator analysis rules out the candidate options enumerated in
the task brief:

- **Option A — nominal-singleton handling.** Direct coverage on 3 of
  3 classes if it eliminated the body's `ObjectHasValue` conjuncts,
  BUT `epistemic-influence-situation` (also containing the same
  cardinality-rich body without `ObjectHasValue`) resolves to `Sat` —
  the nominal isn't the bottleneck.
- **Option B — qualified cardinality (≤n R.C).** No ≤n / =n
  cardinality in the ontology; only ≥n. Not applicable.
- **Option C — inverse-role propagation.** The wedge **already**
  handles inverse roles in this ontology's other classes
  (e-usage-situation resolves). The bottleneck isn't inverse-role
  capability but inverse-role expansion *budget* under combined
  workload.

The honest conclusion: **accept the 17× gap on ORE-15672 as
out-of-scope for the saturator-extension lever**. Closing it would
require a structurally different angle that the dead-end ledger
already records:

- Konclude-style **shared model caching** across classes (dead-end §2,
  Phase-1 stub deliberately un-integrated). The body of
  `epistemic-workflow-enactment` is computed 3× per pair across 3
  classes; a per-body model cache could amortize one expansion across
  all consultations. Risk per `docs/model-caching-plan.md`: high
  integration complexity vs uncertain benefit on small workloads.
- **Heuristic disjunctive-expansion ordering** (MOMS, dead-end §3 per
  `docs/moms-plan.md`) — same risk profile, decoupled from any
  individual construct rule.

If the goal is "biggest completeness + wall improvement across the
corpus" (not just ORE-15672), the lever ranking from
`docs/architecture-roadmap.md` should be re-consulted — ORE-15672 is a
single small ontology and over-fitting to it via model caching has
known risks on the larger workloads (GALEN, SIO) where the current
wedge is already well-tuned.

## Projected impact

- If model caching were implemented and worked: 3 of 3 NoVerdict
  classes convert from NoVerdict to Sat; 109 timed-out pairs fall
  through to a populated cache and become `pruned`. Estimated wall:
  30.6 s → ~5-8 s (Konclude-class). Risk: high (per dead-end §2).
- If a saturator construct rule were added: estimated 0 of 3 convert
  (per the discriminator analysis above). Wall unchanged.
- If the gap is accepted: ORE-15672 remains a 17× outlier; the rest of
  the corpus is unaffected. ORE-10908 (Phase 8 target) already hits
  the ≤5× Konclude target.

**Recommendation: accept the gap. Do not pursue a saturator extension
for ORE-15672.**

## Cross-references

- Phase 7 design / shipped: per-class label heuristic basis.
- Phase 8 results (`docs/phase8-results.md`): wall localized;
  ORE-15672 framed as "genuine intractability" — this recon refines to
  "3 classes, joint-expansion cost, not missing construct".
- Phase 2a/2b/2d: existing saturator-extension precedent (functional-
  role merge, nested-existential lowering, witness propagation).
- Dead-end §2 (`docs/model-caching-plan.md`): model caching, the
  structurally-different lever this recon would otherwise recommend.
- Dead-end §3 (`docs/moms-plan.md`): MOMS heuristic — same risk
  profile.
