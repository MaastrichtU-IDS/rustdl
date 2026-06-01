# Phase 2b.0 — GALEN MISSED diagnosis

Phase 2a's EL++ functional-role witness-merge rule recovered 0 of
GALEN's 109 MISSED (see `phase2a-results.md`). The handoff's
`PathologicalCondition` trace did not describe what's actually
missing in the corpus. This doc replaces that trace with an
empirical analysis based on:

- `phase2b-galen-missed-pairs.txt` — full 109-pair list (Phase 2a measurement).
- `phase2b-galen-sample.md` — stratified sample of 8 pairs across 5 IRI clusters.
- `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.{ofn,hermit.owx}` — minimal HermiT-verified repros.
- `phase2b-galen-pair-analysis.md` — per-pair derivation analysis.

## Headline finding

**The MISSED are dominated by an implementation gap in an existing
rule, not by a missing calculus extension.** 6 of 8 sampled pairs —
extrapolated to ~60 of 109 (~55%) via T2's cluster shares — need the
saturator's compound existential-body lowering (LHS conjunction with nested existential operand) to handle
nested existentials: calculus already documented; lowering doesn't
fire. This is a BUG, not a new-rule design problem. The remaining 2
sampled pairs (~24 of 109, ~22%) need functional-role witness-merge
with covering / sibling-collapse — adjacent to Phase 2a's implementation
but with a subtly different shape.

**The spec's named Phase 2b target (≥n + disjointness for
PairedBodyStructure) is empirically falsified.** The minimal modules
contain ZERO cardinality axioms and ZERO disjointness axioms in any
of the 8 sampled pairs. Whatever HermiT does to derive the paired-
anatomy MISSED, it isn't via ≥n + disjointness on those modules.

## Cluster summary

| Cluster | T4 pairs | Rule pattern | Estimated share of 109 |
|---|---|---|---|
| A (paired/mirror anatomy) | 01, 02, 03 | Compound existential-body lowering bug (LHS conjunction with nested existential operand) | ~40 (~37%) |
| B (hollow/recess) | 04, 05 | Compound existential-body lowering bug (LHS conjunction with nested existential operand) | ~15 (~14%) |
| E (joint stability) | 08 | Compound existential-body lowering bug (LHS conjunction with nested existential operand) | ~5 (~5%) |
| C (pathological process) | 06 | Functional-role + covering / sibling-collapse | ~12 (~11%) |
| D (digestive pathology) | 07 | Functional-role + covering / sibling-collapse | ~12 (~11%) |
| F (misc tail, UNSAMPLED) | — | Unknown | ~25 (~23%) |

(Cluster letters carried over from `phase2b-galen-sample.md`. Estimated
shares from T2's IRI histograms; cluster-to-rule mapping from T4's
per-pair derivations.)

**Terminology note:** "covering / sibling-collapse" means a covering axiom
that mutually excludes sibling sub-properties or sub-classes (e.g.,
`pathological ⊑ PathologicalOrPhysiologicalStatus` with the covering axiom
forcing a single witness to fall into one branch). It is NOT OWL
`DisjointClasses` — the modules have ZERO `DisjointClasses` axioms.

## Recommended Phase 2b rule order

### Phase 2b — main: fix the compound existential-body lowering bug

**Target:** ~60 of 109 MISSED (~55%) across clusters A, B, E.

The saturator already has lowering infrastructure for compound
existential bodies (Tseitin allocator + `atomic_or_tseitin_body_with_extras`).
The gap is in the specific shape: `<class-or-conjunction> ⊑ ∃R.(B ⊓
∃S.C)` — the NESTED existential inside the conjunctive body — is not
being lowered into a chain of synthetic classes that CR5 propagation
can consume.

The cleanest canary is pair 08 (`KneeJointStability ⊑ JointStability`):
a single-hop pure EL+ derivation, no `mirrorImaged`/`normal` clutter,
no recursive GCIs. The fix should be developed and verified against
pair 08 first, then checked against pairs 01–05.

Verify-before-build canary: build a minimal synthetic with the
`A ⊓ ∃R.(B ⊓ ∃S.C) ⊑ D` shape, confirm rustdl misses it, fix the
lowering, confirm rustdl recovers. Then run the corpus diff.

Implementation surface estimate: small (this is a bug-fix in an
existing function, not new infrastructure). High confidence in the
~60-pair recovery if the lowering fix is correct — 6 of 6 sampled
pairs in clusters A/B/E point to the same shape.

### Phase 2b — extension: functional-role witness-merge with covering / sibling-collapse

**Target:** ~24 of 109 MISSED (~22%) across clusters C and D.

The Phase 2a rule (atom-set witness-merge) is sound and terminating
but doesn't catch pairs 06/07 because their derivation involves an
additional disjointness inference: the witnesses coincide AND their
types must satisfy a covering/disjointness axiom. The missing step is
non-Horn — HermiT uses tableau-style negation + functional-role sibling
collapse to derive `∃hasIntrinsicPathologicalStatus.pathological` when
the `physiological` alternative is excluded by covering.

Phase 2b extension adds the disjointness-aware case: functional-role
witness-merge for sibling sub-properties of a functional super-property
(`R_i, R_j ⊑ R_f`, `R_f` functional ⇒ shared witness), combined with
covering / sibling-collapse through the merged witness.

Implementation surface estimate: moderate (extends Phase 2a's
mechanism with a new soundness check; reuse the existing atomic-
content tracking). This is open-lever #2 from the handoff.

### Phase 2b — out of scope: cluster F's ~25 pairs

The 25 pairs in the misc tail were intentionally not sampled. They
may be a third rule shape, or a long tail of one-off patterns. If
Phase 2b's main + extension closes the expected ~84 of 109, the
remaining ~25 is the Phase 2c (or Phase 3) scope. A follow-on
re-diagnosis on the actual measurement (after Phase 2b's main lands)
can re-cluster the still-MISSED set and decide.

## Out of scope (residual gaps)

- **Cluster F's tail (25 pairs)** — unsampled in Phase 2b.0; needs
  re-analysis after Phase 2b's main fix lands and changes the
  MISSED set.
- **Pairs whose derivation requires the full GALEN context** — in
  T3 all 8 pairs were derivable on the minimal modules, so no
  "non-local" pairs surfaced in the sample. But the F tail may
  include some.

## Honesty paragraph

Phase 2b.0's diagnosis is grounded in 8 sampled pairs out of 109,
covering 5 of 6 visible clusters (the 6th, F, is intentionally
untouched). The compound existential-body lowering (LHS conjunction with nested existential operand) bug hypothesis is
HIGH CONFIDENCE for clusters A/B/E (6 of 6 sampled pairs in those
clusters point to the same shape) and EMPIRICAL — confirmed via
HermiT cross-check on minimal modules. The functional-role +
disjointness hypothesis for C/D is supported by 2 pairs (less data
but still cleanly point at the same rule).

The "implementation gap, not calculus gap" attribution for the
compound-body pattern is a reasoned inference from reading the
lowering infrastructure (`crates/owl-dl-saturation/src/lib.rs:1263-1331`
and §1502) plus the empirical pair_02 and pair_08 results. A
definitive attribution requires tracing the saturator's lowering on
the specific GCI to find where it bails out, or finding a calculus
counterexample — neither was attempted in T4. Phase 2b proper should
start from a focused trace on pair 08 (the smallest reproduction).

The same caveat that applied to Phase 2a applies here recursively:
if Phase 2b's compound existential-body lowering fix recovers far fewer than
the estimated 60 pairs, the diagnosis was wrong about cluster sizes
or the rule has its own implementation gaps — re-run the harness,
extract the new MISSED list, re-cluster, iterate.

The 60-pair estimate assumes each cluster-A pair shares the shape of
pairs 01/02/03 (and similarly for clusters B/E vs pairs 04/05/08). This
is unverified for the 37 unsampled cluster-A pairs — if intra-cluster
shape varies meaningfully, the recovery will land below estimate.

## Cross-references

- Phase 2a empirical disproof: `phase2a-results.md`.
- Design spec Phase 2: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`.
- Per-pair derivation analysis: `phase2b-galen-pair-analysis.md`.
- Cluster/sample doc: `phase2b-galen-sample.md`.
- Handoff engine state: `docs/handoff-2026-05-30.md`.
- Phase 2b implementation plan (next): to be written after this
  diagnosis lands.
