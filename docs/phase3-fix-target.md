# Phase 3 — first fix target

Chosen from the GALEN flamegraph (`docs/flamegraphs/galen-classify-2026-06-01.svg`
+ `docs/flamegraphs/galen-classify-2026-06-01-findings.md`) and the SIO
flamegraph (`docs/flamegraphs/sio-classify-2026-06-01.svg`
+ `docs/flamegraphs/sio-classify-2026-06-01-findings.md`) per the
Phase 3 plan `docs/superpowers/plans/2026-06-01-phase3-tableau-perf.md`
Task 2.

GALEN dominates this Phase 3 first fix: its hottest frame is
`apply_deferred_concept_or_rules` at 31.40% inclusive, with the
`PartialEq::eq` leaf inside `needs_deferred_or` at 18.13%. SIO's hot
frame is different (`apply_max` 27.93% + `are_declared_inverses` 25.76%)
and is targeted in Phase 3b — not in this fix.

## The hot path

`apply_deferred_concept_or_rules` (`crates/owl-dl-tableau/src/rules.rs:546-606`)
is part of the lazy-unfolding sweep. It runs once per node at saturate
stable-state (`saturate.rs:155-160`) and additionally inside the
`verify_node_local_clash` fixpoint (`saturate.rs:237`). For every
atomic label on the node it looks up `tbox.concept_rules_by_trigger`
to get the list of conclusions for that trigger; for every conclusion
that is shaped `ConceptExpr::Or(args)` it calls
`needs_deferred_or` (`rules.rs:612-620`) to decide whether the
disjunction should be materialised as a new label on the node.

`needs_deferred_or` answers "yes" iff the `Or` itself is not already
labelled **and** none of its disjuncts is. It does this with one
`labels.binary_search(&c)` for the Or, and up to one
`labels.binary_search(d)` per disjunct. On GALEN's saturating nodes
(2748 named classes, label vectors typically dozens of entries deep,
multi-disjunct Or rules common in the absorbed TBox) this is the
inner loop hit thousands of times per saturate pass per node, which
is itself called per backtracking step. The 18.13% `eq` leaf is the
`Ord::cmp`/`PartialEq` invocations inside `binary_search`; an
additional 4.48% sits in `needs_deferred_or + binary_search<ConceptId>`
and 2.41% in another linear `eq<ConceptId>` scan from the rule body.
Together the deferred-OR check is the single biggest target on GALEN.

The infrastructure for a cheap necessary-condition prefilter already
exists. `Node`-adjacent `BlockingSummary` (`graph.rs:222-227`) carries
a `label_sig: u64` bloom signature mirroring the node's label set:
each `ConceptId` contributes one bit via `label_sig_bit` (Knuth's
multiplicative hash on the index, masked to a 6-bit position).
`label_sig` is maintained incrementally on insert
(`lib.rs:813`) and rebuilt on rollback (`trail.rs:200`). It is
currently consulted in exactly one place — the ancestor pair-blocking
walk at `lib.rs:711-743` — and is *not* read by the deferred-OR rule.
That is the gap this fix closes.

## The fix

**Option A: extend the existing `label_sig` bloom to short-circuit
`needs_deferred_or`.** This is a small, additive change against the
already-deployed B.4 infrastructure — no new node state, no
invariants to maintain, no rollback work.

Signature and call-site:

```rust
// rules.rs:612 — was: fn needs_deferred_or(pool, c, labels)
fn needs_deferred_or(
    pool: &owl_dl_core::ConceptPool,
    c: ConceptId,
    labels: &[ConceptId],
    label_sig: u64,
) -> bool {
    match pool.get(c) {
        ConceptExpr::Or(args) => {
            // Prefilter 1: the Or itself.
            // If bit(c) not in label_sig, c is provably absent from
            // labels — skip the binary_search.
            let c_bit = crate::graph::label_sig_bit(c);
            let c_maybe_present = (label_sig & c_bit) != 0;
            if c_maybe_present && labels.binary_search(&c).is_ok() {
                return false;
            }
            // Prefilter 2: the disjuncts.
            // OR the bits of every disjunct. If none of them is
            // plausibly in labels, no disjunct can be present —
            // materialize without any per-disjunct binary_search.
            let mut args_mask: u64 = 0;
            for d in args.iter() {
                args_mask |= crate::graph::label_sig_bit(*d);
            }
            if (label_sig & args_mask) == 0 {
                return true;
            }
            // Mixed case: at least one disjunct's bit is set; we
            // still need binary_search to disambiguate (bloom can
            // false-positive). Skip disjuncts whose bit is absent.
            !args.iter().any(|d| {
                (label_sig & crate::graph::label_sig_bit(*d)) != 0
                    && labels.binary_search(d).is_ok()
            })
        }
        _ => false,
    }
}
```

Caller (`apply_deferred_concept_or_rules`, lines 575-592): fetch the
signature once outside the rule loop and thread it in.

```rust
// Before the trigger loop. label_sig lives on BlockingSummary,
// not Node — see lib.rs:711 for the existing access pattern.
let label_sig = ctx.graph().blocking(node).label_sig;

// ...then at lines 580 and 588:
if rule.trigger == *trigger
    && needs_deferred_or(pool, rule.conclusion, labels, label_sig)
{ ... }
// and
if needs_deferred_or(pool, c, labels, label_sig) { ... }
```

The args-mask branch is the big win: a typical 4-disjunct Or against
a node whose bloom doesn't intersect the mask saves 4 binary_searches
and produces the same answer (`true` — materialize).

Scope: ~30 lines in `rules.rs`, no changes to `graph.rs` /
`trail.rs` / `lib.rs` (the bloom is already maintained correctly).
A counter (`needs_deferred_or_bloom_rejects` on `RuleCounters`) is
added behind `#[cfg(feature = "counters")]` to give Task 3's
structural canary something to assert against.

## Expected impact

GALEN inclusive 31.40% → realistic floor around 18-22%. The 18.13%
`eq` leaf is dominated by `binary_search`'s comparison loop; the
prefilter eliminates the vast majority of those calls on rules whose
disjuncts don't share bits with the node's `label_sig`. The 4.48%
`needs_deferred_or + binary_search<ConceptId>` frame should collapse.

Unchanged by this fix (and so a floor on the remaining cost):

- The per-call `Vec` / `SmallVec` allocations: 5.63% (`spec_extend`
  ClassId tuples in the triggers snapshot) + 4.08% (`DepSet` clones
  for the conclusion deps). The trigger snapshot is built before the
  prefilter runs, so its cost stays.
- The rule's outer dispatch via `concept_rules_by_trigger.get(...)`
  (under 3% in the profile).
- The 2.41% `eq<ConceptId>` slice scan, which is in a different
  match arm and not bloom-eligible.

Wall-clock target: 24.7 min → roughly 21-22 min on GALEN (a 10-15%
end-to-end reduction). This is a deliberate conservative estimate —
the bloom degrades on heavily saturated nodes (see "Caveat" below).

### Caveat: 64-bit bloom saturation

The signature is one 64-bit word with one bit per label via a
6-bit hash position. Pigeonhole: after roughly 50 labels the bloom is
~54% full; after ~200 labels it is ~95% full. On GALEN's most
saturated nodes the prefilter will largely become "yes — bit might be
present" for every test and degrade to the pre-fix behaviour. That
is still sound (the binary_search runs as before) and the prefilter
remains a clear win on (a) early saturate passes before the node has
accumulated many labels and (b) deferred-OR rules whose disjuncts
collectively miss the signature entirely. Mentioning this here so
Task 5's measurement doesn't surprise — if the wall reduction comes
in at the low end of the 10-15% window, this is why.

## Soundness considerations

The fix is a sound necessary-condition prefilter: it can only skip a
`binary_search` whose result is already determined to be "absent",
or take the fast `true` return when no disjunct can possibly be in
labels.

The invariant is maintained by existing code:
- `add_label_with_deps` (`lib.rs:813`) sets the bit on insert.
- `LabelAdded` rollback (`trail.rs:200`) recomputes the signature via
  `label_sig_of(&n.labels)` so the bloom always reflects the current
  label list after backtracking.
- Therefore `label ∈ labels ⟹ label_sig_bit(label) & label_sig != 0`
  is a graph invariant for the entire lifetime of a node.
- Contrapositive: `label_sig_bit(label) & label_sig == 0 ⟹ label ∉ labels`.
  The prefilter only short-circuits on this contrapositive, so every
  `(node, concept_id)` pair the rule materialises is exactly the same
  set as before.

VERDICTS unchanged:
- GALEN MISSED stays at 17 (Phase 2b baseline).
- alehif / ORE-fragment / pizza / ro / sulo MISSED stay at 0.
- FP=0 everywhere (no false-positive subsumptions).

The canary in Task 3 enforces both halves: a verdict-preservation
test on an Or-heavy synthetic confirms classification outputs are
unchanged, and a structural test on the bloom counter confirms the
prefilter is actually consulted (not silently dead code).
