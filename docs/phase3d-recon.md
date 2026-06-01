# Phase 3d recon — `apply_deferred_concept_or_rules` internals

Source: post-Phase-3c SIO flamegraph
(`docs/flamegraphs/sio-classify-2026-06-01-post-phase3c.svg`, pprof-rs @ 199 Hz,
60 s on `ontologies/real/sio-stripped.ofn`) +  code-trace of
`crates/owl-dl-tableau/src/rules.rs:546-617`. HEAD at recon time: `7f7e490`
(equivalent to Phase 3c HEAD `0b5ed36` after the Phase 2c ship-then-revert
sequence).

## Flamegraph drill-down

The function appears at three call-site contexts in the SVG. The dominant one
is the 2,838-sample (18.16%) frame at SVG geometry `fg:x=1039, fg:w=2838,
y=1461`. Its immediate children (geometry `y=1445`, `1039 ≤ fg:x < 3877`) are:

| Child frame                                            | Samples | % of total | % of parent |
|--------------------------------------------------------|--------:|-----------:|------------:|
| `eq` (leaf, no children)                               |   1,376 |     8.81%  |      48.5%  |
| `next<owl_dl_core::absorb::ConceptRule>`               |   1,337 |     8.56%  |      47.1%  |
| `collect<FilterMap<Enumerate<Iter<ConceptId>>, …>, Vec<(ClassId, SmallVec<[u32; 1]>)>>` |  72 | 0.46% | 2.5% |
| `drop_in_place<Vec<(ClassId, SmallVec<[u32; 1]>)>>`    |      53 |     0.34%  |       1.9%  |

Total: 1,376 + 1,337 + 72 + 53 = **2,838** — exact match. Decomposition is
airtight.

`next<ConceptRule>` has one child `eq<owl_dl_core::absorb::ConceptRule>` of
identical width (1,337); both bare-`eq` and the `<ConceptRule>`-parameterised
`next/eq` cluster (combined **2,713 samples, 17.36%**) are the iterator and
comparison frames from a generic loop over `Vec<ConceptRule>`.

The two other contexts confirm this by exhibiting the expected, healthy
breakdown. The 372-sample sibling frame splits cleanly into
`collect<FilterMap>` (306) + `get<ClassId, Vec<ConceptId>>` (64) +
`add_label_with_deps` (2) — i.e. trigger snapshot + indexed HashMap lookup +
mutation, *no* `next<ConceptRule>` / `eq<ConceptRule>` cluster.

## Identified dominant inner cost

**The per-trigger fallback at `rules.rs:577-593` doing an O(R) linear scan
over `&tbox.concept_rules` whenever `concept_rules_by_trigger.get(trigger)`
returns `None`.**

Roughly **96 % of the 18.16 %** function frame (2,713 of 2,838 samples,
~17.4 pp of total time) is the `for rule in &tbox.concept_rules { if
rule.trigger == *trigger { … } }` loop at line 580 plus its inlined `rule.trigger
== *trigger` comparison.

The function dispatches per-trigger: for each atomic-class label, it calls
`concept_rules_by_trigger.get(trigger)`. The `else` branch is documented as a
fallback for hand-built TBoxes that never invoke `finalize()`. In practice on
SIO every classified TBox flows through
`owl_dl_core::absorb::absorb`, which always calls
`tbox.finalize()` (`absorb.rs:263`). `finalize()` only inserts a map entry for
each trigger that has at least one rule (`absorb.rs:110-119`). Therefore the
fallback fires not when the index is missing — it fires **every time a label
class has zero concept_rules**, which on real ontologies is the common case:
most class labels appearing on nodes have no Or-residual rule. Each such
trigger triggers an O(R) scan over every concept_rule, finds no match, and
returns nothing. SIO's `concept_rules` vector is large enough (and node-visit
frequency is high enough) that this dominates the flame.

This candidate is essentially **an unenumerated variant of the plan's
candidate E** (HashMap miss handling). The plan correctly characterised E as
"already O(1)" for the indexed path; the surprise is that this function's
fallback turns an O(1) miss into an O(R) scan, which only became visible after
Phase 3a/3b/3c stripped the larger costs.

## Code-trace evidence

`crates/owl-dl-tableau/src/rules.rs:576-593`:

```rust
for (trigger, deps) in &triggers {
    let Some(conclusions) = tbox.concept_rules_by_trigger.get(trigger) else {
        // Hand-built TBox without finalize(): fall back to a
        // linear scan over concept_rules for this trigger.
        for rule in &tbox.concept_rules {                       // ← the hot loop
            if rule.trigger == *trigger {                       // ← the hot `eq`
                let (needs, bloom_hit) =
                    needs_deferred_or(pool, rule.conclusion, labels, label_sig);
                ...
            }
        }
        continue;
    };
    for &c in conclusions { … }                                 // indexed path
}
```

Compare with the sibling rule `apply_concept_rules` (`rules.rs:199-226`),
which gates the fallback once at the function top:

```rust
let pending: Vec<(ConceptId, DepSet)> = if tbox.concept_rules_by_trigger.is_empty() {
    // single linear-scan branch
    ...
} else {
    let mut out = Vec::new();
    for (trigger, deps) in &triggers {
        if let Some(conclusions) = tbox.concept_rules_by_trigger.get(trigger) {
            for &c in conclusions { … }
        }
        // miss => skip; no fallback per trigger
    }
    out
};
```

`apply_concept_rules` totals 5.79 % across its three call-site contexts
(293 + 53 + 560 of 15,627 samples) on the same flame, *despite* doing the same
work, because per-trigger misses are skipped instead of falling through to a
scan. That asymmetry is the strongest single piece of evidence that the
fallback shape is the bug.

`finalize()` semantics that make the fallback fire on misses:
`crates/owl-dl-core/src/absorb.rs:110-119` — only inserts entries for triggers
with rules.

## Discarded candidates

- **A: trigger collection alloc.** The `collect<FilterMap<…>>`+
  `drop_in_place<Vec<…>>` pair under the dominant frame totals
  72 + 53 = 125 samples (0.80 % of total, 4.4 % of parent). Real but
  too small to be the Phase 3d target.
- **B: per-conclusion `needs_deferred_or` call.** Not visible as a named
  child of the 2,838-sample frame at all — the bloom prefilter (Phase 3a) is
  doing its job; this path is not hot.
- **C: `DepSet::clone`.** Not visible as a named child. (The Phase 3a flame
  showed it at 4.08 %, but Phase 3a's optimisation pushed the dominant cost
  elsewhere, and per the post-3c flame `DepSet::clone` no longer ranks among
  apply_deferred_concept_or_rules's children.)
- **D: `add_label_with_deps`.** Visible at 2 samples (0.01 %) in the 372-sample
  sibling context. Not the bottleneck.
- **E (as enumerated in the plan): HashMap lookup hit cost.** Visible at 64
  samples (0.41 %) in the 372-sample sibling context. The hit path itself is
  cheap. The actual bug is the miss-path **fallback** beneath the same `get()`.

## Proposed fix shape (handoff to T3)

Mirror `apply_concept_rules`'s control-flow shape (`rules.rs:199`): gate the
fallback **once** by `tbox.concept_rules_by_trigger.is_empty()` at the top of
the snapshot block; inside the indexed branch make a missing trigger an
implicit no-op (`if let Some(conclusions) = … { for &c in conclusions { … } }`
or equivalent). Drop the per-trigger fallback `for rule in &tbox.concept_rules`
loop entirely. The dispatch invariant — that any concept_rule whose trigger
appears in a node label gets a chance to fire — is preserved exactly:
`finalize()` is idempotent, always covers every rule, and is always run by
`absorb`. For the hand-built-TBox unit-test scenario, the single
`is_empty()`-gated linear branch still works.

Counter: add `apply_deferred_concept_or_skip_missing_trigger` (mirror of
Phase 3a's `needs_deferred_or_bloom_rejects`) bumped on each miss inside the
indexed branch, to give T4's structural canary a positive signal.

Predicted impact (flame-attribution math, not wall): function frame from
18.16 % to ~0.80 %, absolute ~17.4 pp drop. Wall improvement on GALEN is
likely sub-proportional due to rayon overlap with other workers, but should
be the next-largest single-phase delta after Phase 3c.

## Cross-references

- Phase 3c results: `docs/phase3c-results.md`
- Post-3c flame findings: `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md`
- Sibling rule reference shape: `crates/owl-dl-tableau/src/rules.rs:199-226`
  (`apply_concept_rules`)
- finalize() semantics: `crates/owl-dl-core/src/absorb.rs:110-119`
