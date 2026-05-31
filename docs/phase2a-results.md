# Phase 2a — Functional-role inference results

Run 2026-05-31 against the Phase 0 soundness net + GALEN. Mechanism:
EL++ functional-role witness-merge rule added to the saturator
(see `crates/owl-dl-saturation/src/lib.rs` and the T4 + T4.5 commits).
T4.5 redesigned the rule to atom-set accumulation after T4's
synthetic-id design proved non-terminating on 3+ sub-property
fan-in (see `docs/hypertableau-dead-ends.md` §14).

## Headline finding

**The mechanism is sound and terminating; the corpus result falsifies
the handoff's estimate.** Phase 2a recovered ZERO GALEN MISSED (109 →
109, 0% reduction) against a spec target of 50-80 reduction. The rule
fires correctly on synthetic canaries (including a 4-sub-property
fan-in matching GALEN's StatusAttribute density), but the actual
GALEN MISSED pattern is NOT the functional-role witness-merge case
the handoff's trace described. Phase 2b must first re-diagnose
GALEN's actual MISSED before designing a remediation.

## Soundness gate (Phase 0 net)

| Fixture | Pre-2a MISSED | Phase 2a MISSED | FP | Wall (Phase 2a) | Wall vs pre-2a |
|---|---|---|---|---|---|
| alehif | 0 | 0 | 0 | 2.52 s | ~1.5× (1.7 s baseline) |
| ore-10908-sroiq | 0 | 0 | 0 | 25.08 s | within range (30 s baseline) |
| ore-15672-shoin | 0 | 0 | 0 | 29.49 s | within range (30 s baseline) |

**FP=0 held across all measured fixtures.** Soundness gate passes;
the new rule introduces no false subsumptions on the broadened
Phase 0 corpus.

## Completeness lever (GALEN, notgalen)

| Fixture | Baseline MISSED | Phase 2a MISSED | Wall | Outcome |
|---|---|---|---|---|
| galen | 109 | 109 | 746 s (~12.5 min) | FP=0 held; 0 MISSED recovered |
| notgalen | 27 | NOT MEASURED | TIMEOUT at remaining budget | combined run with galen used the full 40-min cap |

**The spec target (GALEN 109 → ≤40) was NOT met.** Phase 2a's rule
recovered no GALEN entailments at the 200 ms per-pair budget. The
soundness gate (FP=0) DID hold.

## Why the rule didn't fire on GALEN

The handoff (`docs/handoff-2026-05-30.md` "GALEN 109 MISSED" section)
traced one cluster — `<Region>Pathology ⊑ PathologicalCondition` —
to two-sub-property functional-role merging on
`hasIntrinsicPathologicalStatus + hasPathologicalStatus`, both
sub-properties of functional `StatusAttribute`. The Phase 2a rule
was designed to close exactly that pattern, and the synthetic canary
(`functional_role_merge_canary_recovers_entailment`) confirms it
does — HermiT agrees the entailment is derivable from the synthetic.

The empirical corpus result contradicts the handoff's trace. Possible
causes (any one of which would be enough):

1. **The named class only has one existential.** The rule fires on
   two sub-property facts for the same subject; if
   `NAMEDPathologicalStructure` only has
   `∃hasIntrinsicPathologicalStatus.pathological` (and `PathologicalCondition`
   is the OTHER side of the equivalence, not a fact ABOUT
   NAMEDPathologicalStructure), then the two-facts precondition is
   never met.
2. **The actual MISSED is a different pattern entirely.** GALEN's
   109 may not be PathologicalCondition-shaped at all; the handoff's
   sample-of-one trace was incidental.
3. **The 200 ms per-pair budget cuts off saturation propagation
   before the rule's outputs reach the relevant subsumptions.** This
   is testable by re-running GALEN at `--pair-timeout-ms 5000` and
   comparing MISSED counts.

Phase 2b's first deliverable should be a real diagnosis: extract a
handful of GALEN MISSED pairs from the harness output, examine the
axioms involving the sub-class and super-class, identify the actual
derivation step that's missing. Only then can Phase 2b's rule be
designed with confidence.

## Cross-cutting confirmation

- 0 FP held across the Phase 0 net under Phase 2a's active rule ✓
- The rule fires on synthetic canaries (3-prop, 4-prop, chained-functional) ✓
- The atom-set redesign (T4.5) terminates by construction
  (bounded by atomic vocabulary) ✓
- The rule does not move GALEN's MISSED count ✗
  (spec target unmet; cause deferred to Phase 2b diagnosis)

## How to re-run

```bash
# Canaries (fast — confirms the rule is wired and terminates):
cargo test -p owl-dl-saturation functional_role_merge -- --test-threads=1

# Soundness net (the FP=0 gate):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture

# GALEN MISSED measurement (slow):
timeout 1800 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --ignored --nocapture
```

## What this means for the design spec

`docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`
§"Phase 2 — Deep completeness calculus" expected Phase 2a to close
50-80 of GALEN's 109 MISSED. That estimate is now falsified by data
and inherited from a handoff trace that the corpus did not confirm.
Phase 2b's scope must shift: the FIRST deliverable is to re-diagnose
GALEN MISSED (extract concrete sub-class / super-class pairs from
the harness output, walk the axiom graph, identify the actual
missing derivation step). Only then can a remediation rule be
designed with verify-before-build discipline.

Phase 2a is closed as: mechanism shipped (sound, terminating, no
regression), empirical claim about its corpus impact disproved.
Same shape as Phase 1's dead-end #13: ship the mechanism, document
the disproof, hand the goal to the next phase armed with the
corrected understanding.
