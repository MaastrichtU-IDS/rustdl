# Fragment-blocker diagnostic

**Date:** 2026-07-29
**Status:** Design — approved for implementation planning
**Motivation:** `docs/2026-07-29-fragment-lever-selection-findings.md`

## Why this exists

Four "widen the fragment gate" levers have been measured. Payoff is decided by one
property: **is this construct the last thing keeping the ontology off the saturation fast
path?** Nothing else predicts it — not how often the construct appears, not how expensive
it is. Lever 1 (~40 recoveries), Lever 1b (fast-path-eligible 46 → 100) and the
2026-07-29 atomic-negation lever (13 ontologies, 5 from DNF) all paid because the
construct they lifted was the last blocker. The Domain/Range-negation sibling paid **zero**
because it never is — even though it is mechanically free.

We currently cannot see that property. The gates compute it and throw it away:
`is_pure_el_impl` and `saturator_complete_fragment_impl` are both
`internal.axioms.iter().all(|ax| is_*_axiom(ax, …))`. The only blocker histogram in the
repo is regex-over-source-text, and its buckets do not map to gate decisions — it counts
role chains and transitivity as blockers although `saturator_complete_fragment` already
admits them. That is the **grep ≠ gate** trap, which has now over-estimated two levers
(a 67-ontology estimate whose real gate-eligible count was ~40; 80 complement-bearing
ontologies for 13 actual flips).

This diagnostic makes lever selection a query. It would have answered
"Domain/Range negation: 0 ontologies" in seconds instead of a probe.

Secondary benefit, worth having on its own: a user whose EL-looking ontology classifies
slowly currently gets `# fragment: out-of-EL` and no way to learn why. That is a real
usability gap.

## Scope

**In scope.** A per-axiom blocker reporter that the existing bool gates derive from; an
aggregate `fragment_blockers` API; a CLI banner line and a `classify --json` field; tests.

**Out of scope.** Fixing any blocker. Ranking levers inside the tool. The ORE sweep itself
(a follow-on measurement, run once the tool exists). Any change to routing or verdicts.

## Architecture

### The single-source-of-truth requirement

The reporter must not be a parallel reimplementation of the gate predicates. If it is, the
two drift, and a drifted diagnostic is worse than none — it would send lever work at
constructs that are not actually blocking.

So: the **per-axiom predicate becomes the reporter**, and the bool is derived.

```rust
/// Which gate a blocker was observed against.
pub enum FragmentGate { PureEl, SaturatorComplete }

/// One reason an axiom is outside a fragment. `Copy`, no allocation.
pub struct FragmentBlocker {
    /// The axiom form, e.g. "SubClassOf", "ObjectPropertyDomain", "DisjointUnion".
    pub axiom: &'static str,
    /// The disqualifying construct, e.g. "Or", "All", "Not", "Max", "Min",
    /// "Nominal", "inverse role", "DisjointUnion", "role characteristic".
    pub construct: &'static str,
}
```

`axiom_blocker(ax, pool, gate, ctx) -> Option<FragmentBlocker>` returns the first
disqualifying construct in that one axiom (per-axiom "first" is sufficient — the aggregate
is taken across axioms).

**"First" means outermost-first**: the construct at which the recursive concept descent
first rejects, matching how `is_el_concept` / `is_saturator_concept` already recurse. For
`SubClassOf(A, Or(B, Not(C)))` the reported blocker is `Or`, not `Not`. This is the
decision-relevant choice: `Or` is what you would have to lift *first* for that axiom to
have any chance of entering the fragment, so the histogram counts the actual frontier
rather than a construct buried behind another blocker.

Then:

- `is_el_axiom(ax, pool)` becomes `axiom_blocker(ax, pool, PureEl, ctx).is_none()`
- `is_saturator_axiom(ax, pool, functional_roles, disjoint_ok)` likewise with
  `SaturatorComplete`

The top-level gates keep their `.all(…)` form, so **the routing path short-circuits exactly
as today and allocates nothing** — `FragmentBlocker` is two `&'static str`.

`ctx` carries what the saturator gate already needs and the EL gate does not:
`functional_roles: &HashSet<Role>` and `disjoint_ok: bool`. The `PureEl` arm **ignores both
fields** — `is_el_axiom` takes no such parameters today and must not start depending on
them — so callers of the EL arm may pass a default context. Threading one struct rather
than two loose parameters keeps the two gate arms symmetric at the call sites.

### Report both gates, because they are not nested

```rust
pub struct FragmentBlockerReport {
    pub pure_el: BTreeMap<FragmentBlocker, usize>,
    pub saturator_complete: BTreeMap<FragmentBlocker, usize>,
}
pub fn fragment_blockers(internal: &InternalOntology) -> FragmentBlockerReport
```

Both maps are always populated. This is not redundancy: the final review of the
conjunctive-unsat branch established that these gates are **not nested** —
`is_el_concept` accepts `ConceptExpr::Bot`, and `is_saturator_concept` has no `Bot` arm. An
ontology can therefore pass one and fail the other, and a lever widening one may not widen
the other. A single merged report would hide the very distinction that governs whether a
lever pays.

Counts rather than a set, so a single-ontology query reads "3 axioms blocked by `Or`", and
a bulk sweep can weight by axiom count as well as by ontology count.

The aggregate walk is a full scan (no early exit — the whole point is the complete set),
run once per `classify`, `O(#axioms)`. The conversion pipeline already makes several such
passes; this is not a hot path.

**Two traversals, one predicate — intentionally.** The bool gates walk with `.all(…)` and
short-circuit; `fragment_blockers` walks exhaustively. That is not the drift this design
exists to prevent: both call the *same* `axiom_blocker`, and only the traversal differs.
Drift would mean two copies of the per-axiom logic, which is exactly what deriving the
bool from the reporter rules out.

**`skip_abox` (Lever 1).** `tbox_only_saturator_eligible` runs the gates with ABox axioms
filtered out. `fragment_blockers` reports the **un-filtered** view and labels ABox-derived
blockers by their axiom form (`ClassAssertion` etc.), so a reader can see both that the
ABox blocks the strict gate and that Lever 1 may bypass it. Splitting the report by
`skip_abox` as well would give four maps for no decision-relevant gain.

## Surface

- **CLI banner**: `# fragment-blocker: <gate> <axiom> <construct> ×<n>`, one line per
  distinct blocker, emitted after the existing `# fragment:` line
  (`crates/owl-dl-cli/src/main.rs:706`). Greppable, which is what a bulk sweep needs.
- **`classify --json`**: a `blockers` field alongside the existing `incomplete` field
  (`crates/owl-dl-cli/src/json_out.rs`).
- **Library**: `fragment_blockers`, `FragmentBlocker`, `FragmentBlockerReport`,
  `FragmentGate` re-exported from the crate root beside `analyze_fragment`
  (`crates/owl-dl-reasoner/src/lib.rs:64`).

**Emitted only when the ontology misses the fast path.** A fast-path ontology has no
blockers, prints no extra lines, and gets an empty `blockers` map — so no existing output
changes and no corpus baseline moves.

## Testing

The load-bearing test is an **agreement property**, because "derived from one predicate" is
the design's central claim and must be checkable rather than asserted in a comment:

> For every curated fixture and every synthetic in the existing fragment-gate tests:
> `report.pure_el.is_empty() == is_pure_el(o)` and
> `report.saturator_complete.is_empty() == saturator_complete_fragment(o)`.

Then:
- One canary per construct (`Or`, `All`, `Not`, `Max`, `Min`, nominal, inverse role,
  `DisjointUnion`) asserting the blocker names that construct and the expected axiom form.
- A canary for the **non-nesting** case: an ontology using only `Bot` in an EL position
  must report **no** `pure_el` blocker and **a** `saturator_complete` blocker. This pins
  the asymmetry that justifies two maps; if a future change nests the gates, this test
  says so.
- A negative: a pure-EL ontology reports no blockers and emits no `# fragment-blocker`
  line.
- Corpus non-regression: `classify` output on the curated fixtures is byte-identical to
  before for fast-path ontologies, and differs only by added `# fragment-blocker` lines
  for the rest.

## Soundness

Read-only reporting. No routing decision, verdict, or entailment changes — the FP surface
is untouched by construction. The one behavioural risk is the refactor itself: if
`axiom_blocker` disagrees with the predicate it replaces, a gate verdict could move. The
agreement property plus the corpus byte-identity check are the guards, and any fragment
verdict change on the curated corpus is a stop-and-diagnose, not a tuning matter.

## What this does not claim

It does not close any completeness gap or speed anything up. Its value is that the next
lever gets chosen from measurement instead of from a guess — and that it can return a
negative cheaply, which is what the Domain/Range probe showed is the common case.

**Expected outcome worth stating up front:** the regex histogram suggests inverse/symmetric
dominates the DNF tail. That construct carries two recorded NO-GOs in this repo —
inverse-aware classification (refuted on perf: the saturator answers 100% of positives, the
residual cost is refutation) and backward propagation (NO-GO on payoff-vs-cost, no FP net
at giant scale). So the honest expected result is that this tool concludes **the DNF tail is
out of reach of gate levers**, redirecting to the dense-timeout working-set memory (10 of 12
measured timeouts exceed 8 GB resident; `ore_ont_9347` at 35.7 GB). That is a useful
answer, and cheaper to learn from a histogram than from another engine build.
