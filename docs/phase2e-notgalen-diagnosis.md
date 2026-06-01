# Phase 2e — notgalen residual MISSED diagnosis

Run 2026-06-01 at HEAD 4151edd (post Phase 2d + 2c-redux merge to main).
Notgalen state: FP=0, MISSED=18, rustdl_closure=32721, konclude_closure=32739.

**UPDATE 2026-06-01 (post HEAD e55d7b2):** §Cluster C (the Anonymous-349
sub-cluster, 2 of 18 raw rows) was investigated as a single-canary deep
trace and the **gap was real and tractable**. Fix shipped as
`docs/phase2e-anon349-fix.md` — the Phase 2c-redux back-propagation
loop was skipping the triggering fact's own role, and removing that skip
closes Anonymous-349 ⊑ Anonymous-324 (and ⊑ IPBP) on the minimal local
axiom set. Hypothesis 2 from §Cluster C was the actual root cause. The
remaining 16 notgalen MISSED rows (the ICF cluster) are still blocked
on the missing anonymous-LHS bridge GCI — see §"Recommended approach
for any future Phase 2e" below; orthogonal to this fix.

Diagnosis-only (T101): no code changes. Per-pair reasoning is by axiom
inspection on `ontologies/external/notgalen.ofn`; the full `explain`
runner was not used (notgalen pair_06-style probes can run minutes-plus
on the unmodified TBox). Where direct evidence is missing this doc says
so.

## The 18 MISSED pairs

Raw list (from `/tmp/p2d-final-notgalen.log`):

```
Anonymous-349                                ⊑ Anonymous-324
Anonymous-349                                ⊑ IntrinsicallyPathologicalBodyProcess
CardiacInsufficiencyDueToProsthesis          ⊑ Anonymous-324
CardiacInsufficiencyDueToProsthesis          ⊑ IntrinsicallyPathologicalBodyProcess
CardiacInsufficiencyFollowingCardiacSurgery  ⊑ Anonymous-324
CardiacInsufficiencyFollowingCardiacSurgery  ⊑ IntrinsicallyPathologicalBodyProcess
CongestiveCardiacFailure                     ⊑ Anonymous-324
CongestiveCardiacFailure                     ⊑ IntrinsicallyPathologicalBodyProcess
IneffectiveCardiacFunction                   ⊑ Anonymous-324
IneffectiveCardiacFunction                   ⊑ IntrinsicallyPathologicalBodyProcess
LeftIneffectiveCardiacFunction               ⊑ Anonymous-324
LeftIneffectiveCardiacFunction               ⊑ IntrinsicallyPathologicalBodyProcess
PostcardiotomySyndrome                       ⊑ Anonymous-324
PostcardiotomySyndrome                       ⊑ IntrinsicallyPathologicalBodyProcess
PostvalvulotomySyndrome                      ⊑ Anonymous-324
PostvalvulotomySyndrome                      ⊑ IntrinsicallyPathologicalBodyProcess
RightIneffectiveCardiacFunction              ⊑ Anonymous-324
RightIneffectiveCardiacFunction              ⊑ IntrinsicallyPathologicalBodyProcess
```

9 distinct subjects (one of which is an anonymous class) × 2 distinct
targets, all in the `http://galen.org/galen.owl#` namespace.

## Cluster structure

### Cluster A: `Anonymous-324 ≡ IntrinsicallyPathologicalBodyProcess`

Both targets have the **same** EquivalentClasses RHS in notgalen
(lines 4160 and 6031):

```
EquivalentClasses(:Anonymous-324
  ObjectIntersectionOf(
    ObjectSomeValuesFrom(:hasIntrinsicPathologicalStatus :pathological)
    :BodyProcess))

EquivalentClasses(:IntrinsicallyPathologicalBodyProcess
  ObjectIntersectionOf(
    ObjectSomeValuesFrom(:hasIntrinsicPathologicalStatus :pathological)
    :BodyProcess))
```

Two named classes with the **same** definition are logically equivalent.
Konclude/HermiT recognise this via structural saturation of equivalence;
the saturator's told-subsumer table should too (if `X ⊑ Anonymous-324`
then `X ⊑ ∃hasIntrinsicPathologicalStatus.pathological ⊓ BodyProcess`,
which then triggers `X ⊑ IPBP` and vice versa). Whether rustdl's
told-subsumer pipeline actually collapses these two named classes
through their shared RHS is one of the two open mechanism questions
below; the empirical signal (every subject that misses one target
also misses the other) is consistent with rustdl treating both names
as a unified target.

There is no direct `SubClassOf(:Anonymous-324 :IPBP)` axiom; the
equivalence is implicit in the matched RHS. There IS one extra hint:

```
SubClassOf(:Anonymous-324
  ObjectSomeValuesFrom(:hasPathologicalStatus :pathological))   # line 4161
```

so Anonymous-324 carries a `hasPathologicalStatus.pathological` fact that
IPBP does not (a leftover from the dl-approximation source ontology).

**Consequence:** the 18 raw pairs reduce to **9 source-class entailments**
when targets are unified.

### Cluster B: the 8 named subjects all reduce to `IneffectiveCardiacFunction ⊑ IPBP`

Inspection of the named subjects' definitions (verbatim where short):

```
ICF ≡ CardiacFunction ⊓ ∃hasEffectiveness.(Effectiveness ⊓ ∃hasState.ineffective)    # 5895
ICF ⊑ Anonymous-349                                                                    # 5896
LeftICF  ≡ ICF ⊓ ∃isSpecificFunctionOf.LeftSideOfHeart                                 # 6239
RightICF ≡ ICF ⊓ ∃isSpecificFunctionOf.RightSideOfHeart                                # 7515
CongestiveCardiacFailure                  ≡ ICF ⊓ ∃hasConsequence.RaisedVenousPressure # 5063
CardiacInsufficiencyDueToProsthesis       ≡ ICF ⊓ ∃isSpecificConsequenceOf.ProstheticHeartValve  # 4827
CardiacInsufficiencyFollowingCardiacSurgery ≡ ICF ⊓ ∃isSpecificConsequenceOf.CardiacSurgery      # 4828
PostcardiotomySyndrome                    ≡ ICF ⊓ ∃isSpecificConsequenceOf.Cardiotomy            # 7166
PostvalvulotomySyndrome                   ≡ ICF ⊓ ∃isSpecificConsequenceOf.CardiacValvotomy     # 7192
```

Every one of the 8 named subjects has `ICF` as a conjunct of its
definition, so each `Subject ⊑ ICF` and `Subject ⊑ Anonymous-349` are
implied. The 8 named subjects collapse to **a single root-cause
entailment**: `IneffectiveCardiacFunction ⊑ IPBP`. (Equivalently,
`IneffectiveCardiacFunction ⊑ Anonymous-324`.)

If that one entailment closes, the other 7 close by sub-subsumer
propagation that rustdl already does.

### Cluster C: the `Anonymous-349` row is an artifact, but probably a real entailment

```
Anonymous-349 ≡ BodyProcess
              ⊓ ∃hasEffectiveness.(Effectiveness ⊓ ∃hasState.ineffective)
              ⊓ ∃hasIntrinsicPathologicalStatus.physiological              # 4214
Anonymous-349 ⊑ ∃hasPathologicalStatus.pathological                        # 4215
```

Anonymous-349 is the "abstract shape" of "an ineffective body process
whose intrinsic pathological status is physiological" — i.e., the
syntactic body of ICF + the inherited NAMEDPhysiologicalProcess fact,
lifted out. Per line 5896 ICF ⊑ Anonymous-349; under the EL+ closure
the reverse direction would also hold (because ICF's RHS implies
BodyProcess via `CardiacFunction ⊑ NAMEDCirculatoryProcess ⊑
NAMEDPhysiologicalProcess ⊑ BodyProcess`, and brings in the
`hasIntrinsicPathologicalStatus.physiological` fact from
NAMEDPhysiologicalProcess).

So `Anonymous-349 ⊑ IPBP` is "the same entailment as `ICF ⊑ IPBP`"
modulo the extra named atoms `CardiacFunction` etc. that ICF carries
but Anonymous-349 does not. Konclude proves it; rustdl misses it.

**However, the failure mechanism for Anonymous-349 is NOT obviously the
same as for ICF**, and it deserves to be the recommended trace target
for any future Phase 2e effort. Anonymous-349 has **both** required
facts present **directly in its own axioms**, with no inheritance needed:

- `∃hasIntrinsicPathologicalStatus.physiological` in its EquivalentClasses
  RHS (line 4214).
- `∃hasPathologicalStatus.pathological` as a separate SubClassOf
  consequence (line 4215).

The required role characteristics are also present in notgalen:

```text
SubObjectPropertyOf(:hasIntrinsicPathologicalStatus :StatusAttribute)   # 8902
SubObjectPropertyOf(:hasPathologicalStatus          :StatusAttribute)   # 8944
FunctionalObjectProperty(:StatusAttribute)                              # 8768
```

So the Phase 2a witness-merge precondition (two sibling sub-roles of
a functional super-role, each with a fact on the same node) is
satisfied on Anonymous-349 by axiomatic inspection, with **none of
the Phase 2d subsumer-chain inheritance** required that ICF needs.
Yet Anonymous-349 ⊑ IPBP is still in the MISSED list. This means
**at least one of these holds**, and a future Phase 2e investigation
should disambiguate which:

1. The Phase 2d+2c-redux mechanism docstring
   (`docs/phase2d-2c-redux-results.md` §Mechanism) has an unstated
   precondition the implementation enforces beyond what is documented
   — e.g. the merge or back-propagation only fires from facts derived
   *during* Phase 2d's subclass-fact pass, not from facts present on
   the node from the start (told subsumption).
2. The merge and back-propagation do fire on Anonymous-349, producing
   `(Anonymous-349, hasIntrinsicPathologicalStatus, F)` with
   `F ⊑ pathological`, but the IPBP-trigger match step
   (Phase 2d+2c-redux §Mechanism step 5) doesn't see it for some
   indexing / signature reason.
3. The Phase 2a accumulator does not treat `physiological` and
   `pathological` as elements of the same merged-atom carrier
   because their told-disjoint relationship is not part of the
   functional-merge calculus — i.e. the merge fires only when there
   is a target-class subsumption signal to propagate, not when the
   pair is just "two different named atoms on the same functional
   role" (recovery would then require an axiom-level disjointness
   the dl-approximation surfaces in galen.ofn).

The framing in the rest of this doc — "the missing bridge GCI is
the whole story" — fully explains the **8 named subjects + ICF**
(they need step 2 of the Phase 2d chain to fire, which is blocked
by the missing bridge). It does **not** fully explain Anonymous-349,
which has the bridge consequence as a direct axiom. Anonymous-349
is therefore the cleanest single-pair canary for a future Phase 2e
trace: `Anonymous-349 ⊑ IPBP` should fire on a sound EL+ engine with
the Phase 2a+2c-redux machinery in place, given only the local
axioms 4214 + 4215 + 8768 + 8902 + 8944 + 6031. The fact that it
doesn't is either a documentation gap in the mechanism description
or a real implementation gap; the rustdl trace would disambiguate.

The pair-row count summary after collapsing:

| Cluster | Raw rows | Distinct entailments |
|---|---|---|
| A: `Anonymous-324` and `IPBP` are equivalent | 18 → 9 | — |
| B: 8 named subjects all `⊑ ICF` | 9 → 2 | ICF ⊑ IPBP |
| C: Anonymous-349 is an ICF-shaped artifact | — | Anonymous-349 ⊑ IPBP |
| **Root-cause N** | | **2** (and the two are tightly coupled) |

Practical upshot: at most **2 distinct mechanism gaps** to close —
- (i) `ICF ⊑ IPBP` (and by sub-subsumer propagation the 7 other named
  subjects + the matching `Anonymous-324` column): blocked by the
  missing anonymous-LHS bridge GCI; needs either pattern materialisation
  or completeness on the covering pattern.
- (ii) `Anonymous-349 ⊑ IPBP` (and matching `Anonymous-324` column):
  may share gap (i)'s mechanism, OR may be an independent gap given
  the local axiom set is already complete (see §Cluster C). Trace
  required to disambiguate.

## Why does notgalen miss what GALEN (Phase 2d+2c-redux) closes?

The Phase 2d+2c-redux mechanism (per `docs/phase2d-2c-redux-results.md`
§Mechanism) closes GALEN's `ICF ⊑ IPBP` like this:

1. Phase 2d inherits `(ICF, hasIntrinsicPathologicalStatus, physiological)`
   from NAMEDPhysiologicalProcess.
2. **Phase 2d inherits `(ICF, hasPathologicalStatus, pathological)`
   from PathologicalBodyProcess.**
3. Phase 2a witness-merges the StatusAttribute super-role; Phase 2c-redux
   propagates the merged witness back down to sub-roles; the
   `∃hasIntrinsicPathologicalStatus.pathological ⊑ IPBP` trigger fires.

Step 2 requires `ICF ⊑ PathologicalBodyProcess`. In GALEN that holds
because the dl-approximation produced this anonymous-LHS GCI bridge
(galen.ofn:7515 ≈ pair_06.ofn:1235):

```
SubClassOf(
  ObjectIntersectionOf(
    :BodyProcess
    ∃:hasEffectiveness.(Effectiveness ⊓ ∃:hasState.ineffective)
    ∃:hasIntrinsicPathologicalStatus.:physiological)
  ∃:hasPathologicalStatus.:pathological)
```

Fired against ICF (which has BodyProcess from the NAMEDPhysiologicalProcess
chain, hasEffectiveness from its own RHS, and hasIntrinsicPathologicalStatus.physiological
from Phase 2d step 1), this directly produces `(ICF, hasPathologicalStatus,
pathological)` — the precondition of step 2.

**In notgalen, there are ZERO such bridge axioms:**

```text
$ grep -c "^SubClassOf(ObjectIntersectionOf("  galen.ofn  notgalen.ofn
galen.ofn:     357
notgalen.ofn:    0
```

So step 2 never fires; the Phase 2a/2c-redux witness merge never
sees a `pathological` atom alongside the `physiological` atom; the
IPBP trigger never matches. The notgalen file is literally
"GALEN minus the 357 left-anonymous GCIs the dl-approximation
generates" — `notgalen.ofn` is the raw OWL-DL ontology, while
`galen.ofn` is the post-approximation EL-equivalent that lifts
GCIs onto named LHSes (and ships extra bridge consequences as a
side effect).

This is consistent with the Phase 2c.0 diagnosis (the 24-pair
"confident floor / 39-pair middle / 44-pair upper bound" estimate
explicitly carved out an "anonymous-324 / anonymous-351 cluster"
flagged MEDIUM confidence and "anonymous-named super-classes the
diagnosis had flagged as uncertain" — which is exactly the 18-pair
notgalen residual we see here, projected through the
Anonymous-324 ≡ IPBP equivalence onto two columns instead of one).

## Recommended approach for any future Phase 2e

For the **single root-cause** entailment `ICF ⊑ IPBP` in notgalen:

| Option | Shape | Same as Phase 2d+2c-redux? | Tractable? |
|---|---|---|---|
| (i) Restore the bridge GCI inside rustdl's preprocessing | Recognise the EL pattern `BodyProcess ⊓ ∃hasEffectiveness.(∃hasState.ineffective) ⊓ ∃hasIntrinsicPathologicalStatus.physiological → ∃hasPathologicalStatus.pathological` as a derived consequence and emit it as a synthetic axiom before saturation. | No — this is Phase 2c.0 Option 3 (EL+ pattern materialisation), which was rejected in favour of the Phase 2d/2c-redux subsumer-chain approach. | High effort: requires designing the pattern recogniser, defending soundness (the bridge is a GALEN-specific consequence of the dl-approximation; recognising it from first principles in EL+ probably requires the covering/sibling-collapse rule that pair 06's analysis flagged). |
| (ii) Hypertableau extension (covering + functional-role merge) | The pair_06.0 "Option 1" — extend the hypertableau calculus with explicit `Z ⊑ A ⊔ B` + functional-role merge to derive the bridge consequence semantically. | No — orthogonal layer (wedge), not saturator. | Months of work; rejected in `docs/phase2c-galen-diagnosis.md` §Option 1. |
| (iii) Generic disjointness propagation in the saturator | Pair_06.0 "Option 2"; breaks the monotone Horn assumption. | No. | Rejected in `docs/phase2c-galen-diagnosis.md` §Option 2. |
| (iv) Accept the gap as a "dl-approximation artifact" | Document that notgalen's 18 MISSED reflect a deliberate axiom omission rather than a reasoner gap; recommend running notgalen via konclude/HermiT for full completeness while keeping rustdl for galen. | N/A. | Free. |

The honest characterisation of these 18 residuals: **NOT the same shape
as GALEN's recovered cluster.** Phase 2d+2c-redux exploited a derived
super-class fact (`ICF ⊑ PathologicalBodyProcess`) that the
dl-approximation surfaces in galen.ofn but the raw notgalen.ofn does
not. The underlying entailment is a SROIQ-style consequence of the
covering pattern (intrinsic status is binary {pathological,
physiological}, an ineffective body process must in fact be
pathological), and EL+ alone — without the bridge axiom or a covering
rule — cannot derive it.

**Confidence estimate for fraction closable with a small new rule:**
- With a `BodyProcess ⊓ ∃hasEffectiveness.∃hasState.ineffective →
  ∃hasIntrinsicPathologicalStatus.pathological` synthetic pattern
  (very narrowly scoped to the cardiac/ineffectiveness fragment):
  **plausibly all 18 of 18** would close, but that pattern is essentially
  hard-coding a galen.owl idiom and is not defensible as a general EL+
  rule — soundness on arbitrary ontologies is not obvious without
  re-deriving the covering argument.
- With a soundness-preserving generic rule (Option 3 done correctly):
  **all 18 of 18** but at the cost of months of design (per the existing
  Phase 2c.0 Option 1/3 cost estimate).
- Without any new rule, **0 of 18** will close from saturation alone.
  The full tableau could close some via `RUSTDL_HYPERTABLEAU_TRUST_SAT=0`,
  at materially higher wall cost.

**Recommendation:** treat notgalen's 18 residual as a known dl-approximation
artifact (Option iv), defer Phase 2e proper unless completeness on raw
notgalen is a hard requirement. The 27,997 closure parity already achieved
on GALEN is the operationally important number; notgalen is a stress
fixture for the saturator's ability to derive what dl-approximation
pre-materialised.

## Surprises

- **Anonymous-349 should close from purely local axioms.** Its
  definition (line 4214) plus its SubClassOf consequence (line 4215)
  give it `∃hasIntrinsicPathologicalStatus.physiological` and
  `∃hasPathologicalStatus.pathological` directly, on a node where the
  functional super-role `StatusAttribute` (line 8768) and both
  sub-role declarations (lines 8902, 8944) are present. By the
  Phase 2d+2c-redux mechanism description, the witness merge +
  back-propagation should fire here without any subsumer-chain
  inheritance. That it doesn't is either an undocumented mechanism
  precondition or a real implementation gap — disambiguating this
  via a trace is the single highest-value Phase 2e investigation
  step. (See §Cluster C for the three concrete hypotheses.)
- **`Anonymous-324 ≡ IntrinsicallyPathologicalBodyProcess` is exact.**
  Both classes have the syntactically identical EquivalentClasses RHS.
  The MISSED list shows them in parallel for every subject because the
  saturator hasn't (per the empirical signature) collapsed them into a
  single subsumer target. Whether unifying these two via a "same-RHS
  collapse" optimisation would change the failure mode is open: if both
  classes share a told-subsumer-table entry the saturator would prove
  `X ⊑ A ⇔ X ⊑ B` trivially, but the 18-row pattern shows neither side
  is being proven, not that one is proven and not the other. So unifying
  them doesn't recover any pairs; it just cuts 18 → 9 rows in the next
  failure report.
- **8 named subjects collapse into 1 via the `⊑ ICF` chain.** Every
  subject in the cluster has `ICF` as a conjunct of its
  EquivalentClasses RHS. So the "9 distinct entailments" further
  collapse to 2 (ICF and Anonymous-349), and Anonymous-349's
  derivation is structurally the same as ICF's.
- **The decisive structural difference between galen.ofn and
  notgalen.ofn is that notgalen has zero anonymous-LHS GCIs**
  (`grep -c "^SubClassOf(ObjectIntersectionOf("`: galen 357, notgalen 0).
  Phase 2d+2c-redux relied on one of those 357 GCIs (galen.ofn:7515)
  to derive the `(ICF, hasPathologicalStatus, pathological)` fact
  that completes the witness-merge chain. Without it, the chain
  breaks at step 2.

## Cross-references

- Phase 2c.0 diagnosis (predicted exactly this anonymous-324 residual):
  `docs/phase2c-galen-diagnosis.md` §Cluster summary + §"15-pair
  anonymous-notgalen middle".
- Phase 2b.0 per-pair analysis (Option 1/2/3 trilemma): `docs/phase2b-galen-pair-analysis.md` §"Pair 06".
- Phase 2d+2c-redux results (the mechanism that closed GALEN's 17):
  `docs/phase2d-2c-redux-results.md` §Mechanism (steps 1-6).
- pair_06 fixture (synthetic GALEN extract with the bridge axiom at line 1235):
  `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_06.ofn`.
- T7 measurement log (transient):
  `/tmp/p2d-final-notgalen.log`.
- Source notgalen ontology:
  `ontologies/external/notgalen.ofn` (key axioms: 4160, 4161, 4214, 4215,
  5895, 5896, 6031, 6032, 6763, 6764, 6766, 6771, 6772).

## Addendum 2026-06-02: Anonymous-349 is a closure-realization anomaly, not a saturator gap

The original framing above called Anonymous-349 "the cleanest single-pair
canary for a real implementation gap." **That claim is retracted by direct
evidence.** A follow-up investigation at HEAD `4151edd` extracted the
minimal Anonymous-349 axiom set into `/tmp/anon349.ofn` (~30 axioms,
HermiT-verified to derive both Anon-349 ⊑ Anon-324 and Anon-349 ⊑ IPBP),
then ran:

- `rustdl explain ontologies/external/notgalen.ofn Anon-349 Anon-324`
  → **yes — answered by saturation** (closure produced a positive witness).
- `rustdl classify --pair-timeout-ms 200 ontologies/external/notgalen.ofn`
  (same per-pair budget as the corpus test, same `classify_top_down_internal`
  code path) → prints `direct Anon-349 → Anon-324` **AND**
  `direct Anon-349 → IPBP` as separate edges.

The saturator + classifier closes the pair correctly on full notgalen.
The corpus closure-diff test reporting it as MISSED is inconsistent with
those two direct measurements on the same code path.

**The triple-direct anomaly** worth noting: Anon-324 and IPBP have
identical `EquivalentClasses` RHS (so they are semantically equivalent),
yet `direct_subsumers` on full notgalen emits Anon-349 → Anon-324 and
Anon-349 → IPBP as **parallel direct edges** instead of merging them
into one direct + one equivalence partner. This is evidence of *partial
closure realization* in the large-TBox context — the saturation closure
sees Anon-349 ⊑ both targets but doesn't realize the Anon-324 ≡ IPBP
equivalence cleanly enough for the closure-diff's `is_subclass` matrix
to include the same entailment the CLI prints as `direct`.

**Diagnosed but not fixed.** Scope: 1 of 18 residual notgalen MISSED.
The other 17 (the 8 dl-approximation cluster + the duplicate Anon-324/IPBP
target rows) are out of scope per the original diagnosis. The fix, if
pursued, would investigate why two equivalence-partner classes don't get
merged in full-notgalen closure realization despite the saturator closing
both subsumption directions individually. Not a saturator-rule gap; not
a dead-end ledger entry. Just a partial-realization anomaly worth one
paragraph here.

**Net session-end status** (2026-06-02, post HEAD `4151edd` merge to main):
GALEN MISSED = 0 (full Konclude parity); notgalen MISSED = 18 with the
above accounting; FP=0 across the corpus.
