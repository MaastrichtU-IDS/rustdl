# Phase 2c.0 — residual MISSED diagnosis

Phase 2b/2b.5 recovered 92 of 136 GALEN+notgalen MISSED (GALEN 109→17, notgalen
27→27). This doc diagnoses the remaining 44 residual MISSED (17 GALEN + 27
notgalen) based on T1 (histograms, commit 4216f4a) + T2 (cluster comparison,
commit 57eb3c6). T3 (per-pair analysis for new shapes) was SKIPPED per the plan's
"SKIP if no new shape" branch — T2 confirmed all residual super-classes are either
known cluster C or were in Phase 2b.0's unsampled F-tail.

Supporting data:

- `docs/phase2c-galen-missed-pairs.txt` — 17 GALEN residual pairs (T1).
- `docs/phase2c-notgalen-missed-pairs.txt` — 27 notgalen residual pairs (T1).
- `docs/phase2c-cluster-shift.md` — cluster comparison against Phase 2b.0 (T2).

## Headline finding

**No genuinely new shapes vs Phase 2b.0.** All 44 residual MISSED map to Phase
2b.0's known cluster C (functional-role + covering / sibling-collapse) or the
Phase 2b.0 F-tail body-structure pairs that were always in the original 109 but
unsampled. F-tail emergence is expected behaviour — clusters A/B/D/E clearing
made the tail visible.

The rule gap is the same Option 3 lever flagged in `docs/phase2b-galen-pair-analysis.md`
(pairs 06/07): an EL+ approximation that materialises the consequence
`∃hasIntrinsicPathologicalStatus.pathological` by pattern-matching the
functional-role triangle in the absorbed TBox. No tableau extension or classical
disjointness needed.

## Cluster summary (44 residual MISSED)

| Cluster | GALEN | notgalen | Total | Confidence | Origin |
|---|---|---|---|---|---|
| C — IntrinsicallyPathologicalBodyProcess (named) | 12 | 12 | **24** | HIGH (same sub-class set as pair 06) | Phase 2b.0 cluster C |
| Anonymous-324 / Anonymous-351 (likely cluster-C variant) | 0 | 15 | **15** | MEDIUM (co-occurrence with named super-class; unconfirmed per-pair) | notgalen only; not characterized in Phase 2b.0 |
| F-tail body-structure (AbnormalBodyStructure + UnusualBodyStructure + VariantBodyStructure) | 5 | 0 | **5** | MEDIUM (no per-pair analysis; F-tail from original 109) | Phase 2b.0 F-tail (unsampled) |
| Clusters A/B/D/E | 0 | 0 | **0** | N/A | Fully recovered by Phase 2b/2b.5 |

**24-pair confident floor:** the 12 GALEN + 12 notgalen pairs sharing the named
super-class `IntrinsicallyPathologicalBodyProcess`. These share the exact sub-class
set (cardiac/respiratory dysfunction concepts:
`CongestiveCardiacFailure`, `LeftIneffectiveCardiacFunction`,
`RightIneffectiveCardiacFunction`, `IneffectiveCardiacFunction`, et al.)
characterised by Phase 2b.0 pair 06, where the derivation requires materialising
`∃hasIntrinsicPathologicalStatus.pathological` via functional-role +
covering-axiom collapse.

**15-pair anonymous-notgalen middle:** pairs where the super-class is an anonymous
IRI (`Anonymous-324`, `Anonymous-351`) from the `http://galen.org/galen.owl#`
namespace. Every such sub-class co-occurs with `IntrinsicallyPathologicalBodyProcess`
in the notgalen histogram (same cardiac/dysfunction set), strongly suggesting these
anonymous nodes are GCI class expressions that encode the same cluster-C shape.
**Unconfirmed per T3 — a per-pair trace is recommended before building.**

**5-pair F-tail body-structure top end:** `AbnormalBodyStructure` (3 pairs),
`UnusualBodyStructure` (1 pair), `VariantBodyStructure` (1 pair). These were
present in the original 109-pair list (confirmed by grep against
`docs/phase2b-galen-missed-pairs.txt`) but were in the intentionally-unsampled
F-tail of Phase 2b.0's cluster survey. They may share the cluster-C/D axiom
pattern or have their own shape. **Unconfirmed — canary needed before building.**

## Phase 2c scope estimate (Option 3 EL+ approximation)

| Scenario | Pairs recovered | Basis |
|---|---|---|
| Lower bound (confident cluster C only) | **24 of 44** | Named `IntrinsicallyPathologicalBodyProcess`, high-confidence match |
| Middle estimate (cluster C + anonymous notgalen) | **39 of 44** | Assumes anonymous-324/351 shape-matches cluster C |
| Upper bound (all residual) | **44 of 44** | Assumes F-tail body-structure also matches the triangle |

Most likely: **24–39**. Cluster C recovery is near-certain if Option 3 is
implemented correctly; anonymous-notgalen (15 pairs) are likely but unconfirmed;
F-tail (5 pairs) needs a canary trace before claiming them.

## Why Option 3 (EL+ approximation)

Three options are on record from `docs/phase2b-galen-pair-analysis.md` (pair 06):

**Option 1 — Hypertableau extension** (add explicit covering / sibling-collapse
rules to the wedge):
- Requires extending the hypertableau calculus with a non-Horn covering rule that
  handles `Z ⊑ A ⊔ B` + functional-role merge.
- Estimated effort: months of design + correctness proof + corpus validation.
- Benefit: principled completeness on the SROIQ covering fragment.
- **Decision: DEFER.** Months of work for 24–44 pairs is not payoff-ranked ahead
  of Phase 3 performance or Phase 4 verification-net work. The hypertableau's
  existing double-blocking gives theoretical coverage; the incremental completeness
  delta doesn't justify the extension risk now.

**Option 2 — Full classical-disjointness propagation in the saturator**:
- Adds a general disjointness-propagation rule to the saturator (if `A ⊓ B ⊑ ⊥`
  and `X ⊑ A ⊔ B`, infer the appropriate existential).
- Requires a calculus extension with a new disjunction-splitting step — breaks the
  monotone Horn assumption that makes the saturator fast and sound without
  backtracking.
- Estimated effort: significant; adds worst-case branching and needs a soundness
  proof for the fixed-point.
- **Decision: DEFER.** The perf cost on non-GALEN ontologies is unknown and likely
  high; the covering axiom is not universally present, so the gain is
  GALEN-specific.

**Option 3 — EL+ approximation (saturator-side pattern-matching)** [recommended]:
- In the absorbed TBox, recognise the triangle:
  1. `R_i ⊑ R_f` where `R_f` is functional.
  2. Some class `X` has `∃R_i.A` in its definition.
  3. Some class `T` has `∃R_f.B` in its definition.
  4. A class `Z` covers both branches via `Z ⊑ A ⊔ B` or axiom-level
     covering/disjointness on the `R_f`-target range.
- Lower the triangle to the materialised consequence `X ⊑ ∃R_f.B`
  (and thus `X ⊑ T` once `T`'s body fires).
- No negation, no case-splitting, no tableau extension — entirely
  within the Horn EL+ closure.
- Estimated effort: small-to-moderate. Pattern is localised to the
  absorbed-TBox representation; reuses existing `ConceptRule` / `RoleRule`
  trigger infrastructure.
- **Decision: RECOMMENDED.** Highest payoff-to-effort ratio. Sound by
  construction (the materialisation only fires when the covering axiom
  structurally forces the single branch). Incremental: can be added
  without changing the tableau or breaking the saturation fixed-point.

## Phase 2c implementation outline

Abbreviated — Phase 2c proper gets its own plan doc.

- **T1 — Canary:** build a minimal synthetic ontology with the cluster-C triangle
  shape (`R_i ⊑ R_f` functional + `∃R_i.X` + covering on `R_f`-target range);
  confirm rustdl misses it; confirm HermiT finds it. Use pair 06's
  `hasIntrinsicPathologicalStatus` / `pathological` / `physiological` module as
  the primary canary.
- **T2 — Absorbed-TBox pattern detection:** identify where in `absorb.rs` / `told.rs`
  the triangle's components are materialised; write the recogniser for the
  `R_i ⊑ R_f` + `R_f`-functional + covering shape.
- **T3 — Structural canary (if anonymous-notgalen shape is unconfirmed):** trace
  one anonymous-324 pair to verify the absorbed-TBox shape matches Option 3's
  triangle before building the rule.
- **T4 — Implement lowering:** add the pattern-matched materialisation to the
  saturation pass; wire into the existing `ConceptRule` / `NominalRule` trigger
  infrastructure.
- **T5 — Measure:** run the GALEN + notgalen corpus diff. Verify: FP=0 held;
  MISSED reduces from 44 toward the 24–39 prediction; wall time not regressed.
- **T6 — Docs:** update `phase2c-results.md`, append to this design spec.

## Cross-references

- Phase 2b.0 cluster characterization: `docs/phase2b-galen-sample.md`.
- Phase 2b.0 per-pair analysis (pairs 06, 07 — Option 3 detail): `docs/phase2b-galen-pair-analysis.md`.
- Cluster shift comparison (T2): `docs/phase2c-cluster-shift.md`.
- GALEN residual pair list (17 pairs): `docs/phase2c-galen-missed-pairs.txt`.
- notgalen residual pair list (27 pairs): `docs/phase2c-notgalen-missed-pairs.txt`.
- Phase 2b.0 original 109 pairs: `docs/phase2b-galen-missed-pairs.txt`.
- Design spec: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`.
- Phase 2 closeout: `docs/phase2-closeout.md`.
