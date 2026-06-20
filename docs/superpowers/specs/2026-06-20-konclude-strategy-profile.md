# Konclude source profile → implementation strategy for rustdl

Profiled Konclude's C++ source (cloned `github.com/konclude/Konclude`, 5525 files) across
three subsystems via parallel read-only agents: **Cache** (`Kernel/Cache/`), **calculus
core / nominal / dependency** (`Kernel/{Process,Task,Algorithm,Strategy}/`), and
**saturation + classification orchestration** (`Consistiser/`, `Classification/`,
`Consistence/`). Goal: find techniques that attack rustdl's measured walls — wine
(combinatorial nominal/disjunction search) and family (undetected inconsistency).

## Headline 1 (CONFIRMS + closes SP4): caching is NOT the wine lever — Konclude would also get 0%

Konclude's SAT cache (`CSignatureSatisfiableExpanderCache`) is keyed by an order-independent
concept-label *signature* (`CConceptSetSignature.cpp:54`), with a full label-equality
**compatibility** recheck on read (`CSatisfiableExpanderCacheHandler.cpp:79`). Crucially, the
**write side refuses to store any context-dependent verdict**: `hasDependencyToAncestor`
(`:982`) suppresses caching whenever a label concept's dependency arc reaches outside the
node's subtree — and nominal-merge consequences carry a `DNTMERGEDCONCEPT` dependency that is
traced through, so **a nominal-merge-dependent node is never cached**. Nodes with
`PRFSUCCESSORNOMINALCONNECTION` are excluded from the SAT cache and signature-blocking
entirely (`:4521,4858,5360`). UNSAT caching (`COccurrenceUnsatisfiableCache`, a subset-testing
trie) is sound by monotonicity but explicitly **deactivated for nominals** (`TODO ... currently
deactivated`, calc `:7469`).

⇒ **Konclude's own architecture predicts the exact 0% we measured** (our SP4 sound-lemma
probe: 0 context-independent UNSAT of 451k). Caching is genuinely *not* how Konclude is fast on
wine. **SP4 is closed, now with external corroboration.**

## Headline 2 (REVISES "unfixable"): the difference is the DEPENDENCY REPRESENTATION

Our memory said wine's wall is "unified root cause: nominal-merge folds causation into
`birth_deps` → every clash context-dependent → defeats backjumping/caching/learning; closing
it = multi-month rewrite." The profiling sharpens this: **the defeat is an artifact of our
dependency *representation*, not inherent.**

- rustdl: a clash dep is a **set/union** of decision levels; `merge_with_cause` folds the
  merge causation into `birth_deps`, so every downstream concept accumulates the *full*
  ancestor set ⇒ bjgap≈1 (0 backjumps / 76k).
- Konclude: a concept's dependency is a **single scalar `branchingTag`** = the *max* branching
  level needed to derive it, propagated by `max` (`CDeterministicDependencyNode.cpp:58`). On a
  nominal merge (`CNOMINALDependencyNode.cpp:40`) or ≤n merge
  (`CMERGEDCONCEPTDependencyNode.cpp:41`), the merged concept's tag = `max(original_tag,
  merge_tag)` — **never a union**. Clash analysis jumps to `max(all clashing tags)`
  (`...TaskHandleAlgorithm.cpp:7900`), skipping intermediate levels.

So Konclude's backjumping survives nominal merges because no concept ever holds a *set* of all
its ancestors — only the single highest level. **rustdl's bjgap≈1 may be a representational
artifact of set-union deps, fixable by switching to scalar max-branching-tag deps** — a
targeted (if invasive) change to the dependency model, NOT a from-scratch rewrite. This is the
most promising thread and reopens wine via backjumping (a *different* mechanism than caching).

**P0 (gates everything): is bjgap≈1 representational or genuine?** Measure on wine whether the
clash's `max` decision level is meaningfully below the current level (Konclude-style scalar)
vs always == current (genuine). If the max is below → scalar-max deps unlock backjumping →
build it. If always == current → bjgap is real and scalar-max won't help either.

## L1 P0 RESULT (2026-06-20): REFUTED — bjgap≈1 is genuine, not representational

Ran the gating P0: disabled rustdl's merge-causation fold (`nn_taint_disabled`, which sets
the fold's `cause_deps = EMPTY`) and re-measured wine `sat(CabernetFranc)` vs baseline.
**Identical**: FOLD-ON 1,011,250 branches / 112 backjumps / Stalled; FOLD-OFF 1,010,408 /
112 / Stalled. Removing the fold changes nothing ⇒ the dense dependencies are GENUINE, not
an artifact of the set-union fold. And scalar-max vs set-union make the SAME backjump
decision for the same dependency *content* (jump-past d ⟺ d∉deps ⟺ d<max(deps)); only the
content (density) matters, and it's genuinely dense. **L1 is dead for wine** — Konclude's
scalar-dep representation would not unlock backjumping here. The cheap P0 killed the
multi-week dep-model change before building it. wine stays closed (caching dead by #6 +
Konclude-confirmed; L1 dead by this P0). Konclude's wine speed must come from L2/L3
(keeping the tree small so the genuinely-dense search never explodes) + saturation, NOT the
dependency representation. The remaining LIVE, independent lever is **L4 (saturation
consistency cascade) for the family CORRECTNESS gap**.

## Adoptable levers (prioritized)

| # | Lever | Konclude evidence | rustdl target | Soundness | Effort |
|---|---|---|---|---|---|
| **L1** | **Scalar max-branching-tag dependencies** (replace set-union `birth_deps`/clash deps; merge ⇒ `max`, not fold) | `CDeterministicDependencyNode`, `CNOMINALDependencyNode`, `CMERGEDCONCEPTDependencyNode` | wine backjumping (bjgap≈1) + family | backjump under-approx must stay a superset — careful; FP=0 gated | high (dep-model change) — **P0 first** |
| **L2** | **Deterministic-first processing order**: never branch ⊔ until all AND/∀/∃/nominal-merge/≤n exhausted; OR-trigger delay | priority strategy (ATOM>AND>∀>NOMINAL>≤n>⊔), `planORProcessing` trigger-delay `:16600` | wine tree size | verdict-neutral (ordering) | med |
| **L3** | **Common-disjunct extraction** (generalize our `disjunction_existential`): after saturation, add concepts true in ALL disjuncts as deterministic consequences | `CSaturationCommonDisjunctConceptsExtractor.cpp:34` | wine branching depth | sound (entailed) | med |
| **L4** | **Saturation consistency cascade**: saturate ABox individuals; a saturation clash ⇒ inconsistent pre-tableau | `CTotallyPrecomputationThread.cpp:907-954` (5-level cascade) | **family** (the correctness gap) | sound (clash = real) | med |
| **L5** | **Pseudo-model subsumption precheck**: refute A⊑B by comparing lightweight label snapshots before a full sat test | `fastPseudoModelSubsumptionClassPrecheckTest :1599`, `isPseudoModelSubsumerPossible :1626` | per-pair rework (n²) | sound refute-only (like our label oracle) | med (we have Phase-7 oracle to extend) |

Note: Konclude also does **semantic branching** (`:16951`, assert ¬d_j for prior disjuncts) —
that is our SP1, confirmed inert on wine because wine's disjuncts are nominal (no complements).

## Recommendation

1. ~~L1 P0 first~~ **DONE — L1 REFUTED** (see L1 P0 RESULT above): bjgap≈1 is genuine; scalar-max deps won't help wine. wine stays closed.
2. **L4** is the concrete **family** (correctness) lever and is independently valuable
   regardless of wine — saturate ABox individuals and catch the clash pre-tableau.
3. **L2/L3** are sound, bounded tree-shrinkers worth doing if L1 lands.
4. **SP4 caching is closed** (Konclude confirms 0% on nominals).

Each lever is FP=0-gated against the canonical corpus (`docs/corpus.md`). Konclude source kept
at `scratchpad/konclude-src` (throwaway clone).

## Corpus-wide wedge profile (2026-06-20) — measured, corrects two earlier claims

Profiled the canonical corpus (`docs/corpus.md`) with process-global counters over a
1s/pair classify (branch `exp/corpus-profile`, unmerged — counters add hot-path overhead):

| fixture | branches | backjumps | bj-rate | CI-unsat | cache-pot |
|---|---:|---:|---:|---:|---:|
| galen/notgalen/bibtex/alehif | 0 | 0 | — | 0 | — |
| ro / sulo | 398 / 43 | 0 | 0% | 0 | 0% |
| pizza | 82,134 | 2,358 | 2.87% | 52 | 0.2% |
| ore-10908 | 5,489 | 0 | 0% | 0 | 0% |
| ore-15672 | 296 | 0 | 0% | 0 | 0% |
| sio | 3,735 | 0 | 0% | 5 | 12.8% |
| **wine** | **92,526,281** | **15,126,714** | **16.35%** | 16 | **0.0%** |

**Correction A (significant): wine is NOT bjgap≈1.** The full classify backjumps **16.35%**
(15M backjumps). The earlier "bjgap≈1 / backjumping defeated" was specific to
`sat(CabernetFranc)` — a 0.01%-backjump pathological *outlier* class — not wine-wide.
**wine's wall is raw combinatorial scale: 92.5M branch decisions even WITH 16% working
backjumping.** ⇒ L1 (scalar-deps / better backjumping) stays dead, but for the refined
reason that backjumping already fires; the only wine-relevant lever is **branch-count
reduction** (L2 deterministic-first ordering, L3 common-disjunct extraction, Konclude-style
saturation keeping the tree small).

**Correction B (minor): cache headroom is not strictly 0% everywhere** — sio is 12.8% (but
only 5 in absolute terms). SP4/caching stays dead (negligible absolute headroom corpus-wide;
wine 0.0%), but "0% everywhere" was an overstatement.

**Method note:** don't generalize a wall from one class — `sat(CabernetFranc) ≠ wine`. The
whole-corpus profile was necessary to catch Correction A.
