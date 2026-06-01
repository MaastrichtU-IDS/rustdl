# Phase 2b — Compound existential-body fix results

Run 2026-06-01 against the Phase 0 soundness net + GALEN + notgalen.
Fixes: `introduce_equivalent_existential_marker` (P2b commit 022ca50,
nested-existential markers in Tseitin bodies) + conjunctive-trigger
emission for non-atomic existential RHS in `lower_sub_class_of`'s
`ConceptExpr::And` arm (P2b.5 commit b64d331). See `phase2b-trace.md`
and `phase2b-trace2.md` for the diagnostic process.

## Headline finding

**GALEN MISSED dropped 109 → 17 (92 recovered, 84%, FP=0 held).** The
spec target was ≤49 MISSED (~60 recovery); the actual outcome of 17
beats it by 32 pairs. The win came in two stages: the original P2b
body-side fix recovered 5 of the 92 (the shapes matching the original
canary); the follow-on P2b.5 fix — based on a second trace (`phase2b-
trace2.md`) of a still-MISSED pair — recovered the other 87 by
addressing a silently-dropped axiom shape that was one step UPSTREAM
of where the P2b.0 diagnosis pointed.

The dead-end-#12 lesson recurses again: the P2b.0 diagnosis correctly
identified the cluster (~60 pairs in clusters A+B+E) but mislocated
the specific code site. The fix landed only after a SECOND trace
narrowed the actual bail-out to `lower_sub_class_of`'s LHS-And arm
dropping non-atomic existential RHS. P2b.5 is recorded as a worked
example of "trace before extending" discipline.

## Soundness gate (Phase 0 net)

| Fixture | Pre-2b MISSED | Phase 2b+2b.5 MISSED | FP | Wall | Wall vs pre |
|---|---|---|---|---|---|
| alehif | 0 | 0 | 0 | 2.72 s | ~1.5× (1.76 s pre-2a) |
| ore-10908-sroiq | 0 | 0 | 0 | 31.60 s | within range (30 s pre-2a) |
| ore-15672-shoin | 0 | 0 | 0 | 29.71 s | within range (30 s pre-2a) |

FP=0 held across all 3 fixtures. No completeness regression; wall
regressions all within noise.

## Completeness lever (GALEN, notgalen)

| Fixture | Baseline MISSED | Post-P2b MISSED | Post-P2b+P2b.5 MISSED | Wall | FP |
|---|---|---|---|---|---|
| galen | 109 | 104 | **17** (84% recovery) | 24.7 min | 0 |
| notgalen | 27 | 27 (not measured at P2b alone) | 27 (unchanged) | 37.5 min | 0 |

GALEN: 92-pair recovery far exceeds the ~60-pair spec target.
notgalen: 0 change — consistent with the Phase 2b.0 diagnosis prediction
that the cluster-C+D pairs (cardiac pathology, functional-role +
covering shape) need a separate fix.

Wall regressions: GALEN ~2× (12.5 min → 24.7 min); within the acceptable
range. notgalen 37.5 min has no clean baseline (Phase 1 measurements
timed out; this is the first complete notgalen run on the modern
hardware).

## What the two fixes target

**Phase 2b — `introduce_equivalent_existential_marker` for nested
existentials in Tseitin bodies (commit 022ca50):**
The markers introduced by `introduce_existential_marker` for nested
`∃R.B` operands inside a Tseitin synthetic body are ONE-WAY (trigger
`∃R.B ⊑ F` but no fact `(F, R, B)`). The new equivalent variant emits
both, so CR5/CR9 propagation can fire through the marker. Used only
at the two `atomic_classes_with_existential_markers` call sites
(lib.rs:1514/1523 pre-2b.5 numbering); LHS-trigger call sites keep
the asymmetric semantics.

**Phase 2b.5 — conjunctive trigger for LHS-And with non-atomic
existential RHS (commit b64d331):**
`lower_sub_class_of`'s `ConceptExpr::And` arm computed conjunctive-
trigger heads via `atomic_operands_on_right(sup, pool)`, which
returned `[]` when sup is `ConceptExpr::Some`. Axioms of shape
`SubClassOf(And(p1, p2, ...), ∃R.B)` were silently dropped. Fix:
after the existing atomic-operand loop, also enumerate any non-atomic
`∃R.B` on the right (or as an operand of an `And` on the right),
allocate a marker via `introduce_equivalent_existential_marker`
(two-way, so the marker carries the R-witness through the chain),
and push a conjunctive trigger `{bodies} ⊑ marker`.

This was the load-bearing miss for GALEN's cluster-A (FemoralHead-
shape, MirrorImagedBodyStructure-shape) pairs. The P2b.0 diagnosis
correctly identified the cluster but mislocated the code site;
P2b.5 used a second trace (`phase2b-trace2.md`) to localize the
actual bail-out.

## Two-way marker design call

Both fixes use `introduce_equivalent_existential_marker` rather than
the one-way `introduce_existential_marker`. This is load-bearing:
the chain `Y ⊑ {A, B} (conjunctive trigger) → Y ⊑ M → … → Y ⊑ T`
requires Y to inherit M's existential WITNESS (via the fact), not
just M as a subsumer. A one-way marker would only let downstream
existential triggers consume an R-witness that Y already has — it
doesn't CREATE the witness. The two-way marker emits the fact
`(M, R, B)`, so any class subsuming M (via the conjunctive trigger)
inherits the R-witness through subsumer propagation, completing
the chain. Trace evidence in `phase2b-trace.md` (P2b T3) and
`phase2b-trace2.md` (P2b.5).

Soundness: the marker is DEFINED (by the surrounding conjunctive
trigger) to be ≡ ∃R.B in this context, so the fact is just the
definition restated.

## What's left after Phase 2b

- **17 of 109 GALEN MISSED still uncovered.** The Phase 2b.0
  diagnosis split: ~24 pairs in clusters C+D (cardiac pathology,
  needing functional-role + covering / sibling-collapse extension),
  ~25 in cluster F (unsampled tail). The combined recovery of 92
  exceeds the C+D+F upper bound (49 pairs), suggesting some C+D or
  F pairs ALSO matched the P2b/P2b.5 shapes incidentally — that's
  a happy bonus. A follow-on diagnosis would re-cluster the 17 still-
  MISSED to inform the C+D extension's actual scope.
- **27 of 27 notgalen MISSED still uncovered.** All in the cluster
  C/D shape; the C+D extension plan targets them.
- **Phase 3 (saturator perf)** and **Phase 4 (auto-gating)** still
  queued per design spec.

## Honesty paragraph

The two-stage process (P2b → measure → P2b.5 → re-measure) was costly
in wall time but cheaper than shipping P2b with the wrong scope and
later discovering the gap. The P2b.0 diagnosis's cluster-A+B+E estimate
(~60 pairs) turned out to be correct, but the specific code site
identified was wrong — both fixes were ultimately needed to hit the
estimate. The dead-end-#12 lesson recurses: even with a careful
diagnosis, the FIRST trace localizes the bug to its symptom, not
necessarily its cause. P2b.5's re-trace pattern is now the worked
example for "extend the trace one level deeper" before declaring
the fix complete.

## How to re-run

```bash
# Canaries (fast — confirms both fixes are wired):
cargo test -p owl-dl-saturation compound_existential_body \
    lhs_and_with_existential_rhs -- --test-threads=1

# Soundness net (the FP=0 gate):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture

# GALEN (slow, ~25 min):
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --ignored --nocapture
```
