# How Konclude classifies wine in ~200ms vs rustdl's DNF — instrumentation findings (2026-06-23)

> **CORRECTION (2026-06-23, from the SP0 spike — `docs/sp0-saturation-spike-results-2026-06-23.md`):**
> §6 below attributed wine's per-pair cost to the nominal value-partitions. That is WRONG.
> A cheap construct-ablation probe showed removing value-partitions alone (or cardinality
> alone, or ∀ alone) leaves the matched test DNF; only removing **all three (∀ + ≤n +
> nominal) jointly** collapses it to EL-instant. Wine's cost is their **joint interaction**,
> not value-partitions. Consequence: there is no cheap value-partition on-ramp — a saturation
> that helps wine must soundly cover the joint ∀+≤n+nominal structure (the full hard build).

Goal: understand *mechanistically* why Konclude is ~10,000× faster than rustdl on wine
(SHOIN, nominal- + disjointness-heavy), and what rustdl does differently. No Konclude
source available (native binary v0.7.0-1138 from `konclude/konclude:latest`); "instrumented"
via the binary's native `-v`/`-a` diagnostics + config keys + descriptions extracted from
the binary's strings.

## 1. Konclude's wine phase profile (empirical, `-v`)

```
parse 9ms → preprocess 10ms → PRECOMPUTE ~45-74ms (SHOIN) → CLASSIFY ~107-126ms   (≈200ms total)
```

The work splits into a **one-time precomputation** (~70ms) and **classification** (~120ms).
Konclude reads `wine.ofn` directly (OWL functional syntax).

## 2. Konclude's architecture (confirmed in source — github.com/konclude/Konclude)

Source read (shallow clone) confirms the optimizations inferred from the binary. The
relevant subsystem is `Source/Reasoner/Consistiser/` (consistency + **saturation**), with
`Classifier/`, `Cache/`, and `Preprocess/`. Every win is a form of **compute-once,
reuse-everywhere**, and the specific techniques are:

- **Approximated saturation** (`CApproximatedSaturationCalculationJob`,
  `Consistiser/CSaturation*`): a sound, *deterministic* over-approximation of each concept's
  consequences computed in the ~70ms precompute, used to decide most subsumptions and prune
  tableau branching without search. This is the "keep the tree small" lever.
- **Common-disjunct extraction** (`CSaturationCommonDisjunctConceptsExtractor`,
  `Preprocess/CCommonDisjunctConceptExtractionPreProcess`): a concept common to all disjuncts
  of a `⊔` is asserted deterministically, removing a branch point.
- **KPSet known/possible-subsumer classification**
  (`Classifier/CClassificationInitializePossibleClassSubsumption*`): derives *possible*
  subsumers from the saturation and tests only those, pruning the O(n²) pairwise frontier.
- **Representative backend memory cache** (`Kernel/Cache/CBackendRepresentativeMemoryCache`):
  the precomputed model that per-test completion graphs expand **from** instead of rebuilding.

The optimization *surface* (from the binary's config keys + help text, all backed by the
above source) — every one a compute-once / reuse form:

- **Saturation with a satisfiability cache** — `Konclude.Calculation.Optimization.
  SaturationExpansionSatisfiabilityCacheCount`, `SaturationReferredNode...`. Precomputation
  saturates concepts once; results are cached.
- **Backend cache + local completion-graph expansion** — "*non-deterministic consequences
  stored in the backend cache are reused for expanding the local completion graph*"; "*the
  amount of individuals loaded from the backend cache into the local completion graph is
  limited*". A global precomputed model (the "backend") that per-test completion graphs
  expand **from**, instead of rebuilding.
- **Completion-graph REUSE cache, deterministic AND non-deterministic** — "*queries the
  completion graph reuse cache for compatible entries*"; "*reuse deterministic completion
  graph reuse cache entries*"; "*reuse non-deterministic completion graph reuse cache
  entries if certain conditions are satisfied (which check whether reuse could save many
  further expansions)*". **This is the crux:** Konclude soundly reuses *non-deterministic*
  (branch-dependent) completion-graph fragments across tests, gated by compatibility checks.
- **KPSet subsumer-set classifier** — `OptimizedKPSetClassSubsumptionClassifier`
  (Known/Possible subsumer sets): prunes the O(n²) pairwise tests to a small frontier.
- **Nominal-node merging + consistency-graph reuse for nominals** — "*constructed individual
  nodes are merged into nominal nodes*"; "*uses the completion graph from the consistency
  test to improve the saturation of concepts that have a connection to nominals*". Nominal
  reasoning is done once in the consistency model and reused.

## 3. rustdl's wine profile (bjprobe instrumentation, per-pair classify)

90s capped run, branchy pairs only (`branches > 50`):

```
455 branchy pairs sampled in 90s   Σbranches=4,675,775   clashes=4,273,263
stable-node% = 46.8%   backjumpable% = 26.15%   (overall: DNF at 1991s)
```

**Backjumping FIRES on wine's classify path (26% of clashes jump).** This counter measures
backjump *frequency* (does the decision get skipped at all), not *distance*. It is
consistent with the prior `bjgap≈1` finding (`perf/wine-backjump-probe`), which measured
backjump *distance* ≈ 1 level: wine backjumps **often but shallowly**, so each jump prunes
~1 level and the search still explores most of the combinatorial tree. (It also refines the
conflict-learning §3b "0 of 1.07M", which was a different probe — per-class satisfiability,
not per-pair subsumption.) Net: backjumping is not *failing*, but it is not *rescuing* wine
either — the per-pair search remains expensive.

## 4. The actual difference: model reuse, not search quality

rustdl runs **a fresh from-scratch tableau/wedge per subsumption pair** (`sub ⊓ ¬sup`),
re-deriving wine's entire nominal/disjunction structure every time — ~10k branches and
thousands of clashes *per pair*, thousands of pairs → DNF. Backjumping fires (26% frequency)
but shallowly (`bjgap≈1`), so it neither fails nor rescues; either way you recompute the
whole hard disjunctive/nominal model on every one of O(n²) pairs.

Konclude derives that hard structure **once** (precompute, ~70ms) and **reuses** it across
all tests via the backend cache + completion-graph reuse cache (deterministic *and*
non-deterministic) + KPSet pruning. Per-pair work is then near-zero.

**rustdl already tried exactly this and had to disable it.** The Phase-1 snapshot cache
(per-class model reuse) was flipped **default-OFF** as a soundness fix: its non-deterministic
reuse was **FP-unsound** on the disjunctive fragment (the "reuse-trap" — replaying one
satisfying model is unsound on non-Horn `sup ∈ model ≠ sub ⊑ sup`; ORE 2015 surfaced 30+ FP).
**Konclude has the machinery rustdl's snapshot cache lacked: compatibility/condition checks
that make non-deterministic completion-graph reuse sound** ("*reuse … if certain conditions
are satisfied*").

## 5. Implication — consistent with the prior whole-corpus finding

The wine wall is **not** a conflict-learning problem (backjumping already fires per-pair;
that gate was correctly NO-GO). Prior whole-corpus profiling
([[wine-wall-bjgap1-genuine]]) measured it as **raw combinatorial scale** — ~92M branch
decisions even with working backjumping — and named the lever as **branch-count reduction:
"Konclude's saturation keeping the tree small."** This instrumentation **confirms and details
that**: Konclude's ~70ms precompute is exactly a saturation/precompletion that resolves most
disjunctive/nominal structure deterministically, so each per-test completion graph stays
small. Two distinct mechanisms, both visible in Konclude's config surface:

1. **Branch-count reduction (primary, per rustdl's own measurement).** Saturation +
   deterministic precompletion shrink each test's search tree. rustdl's analogous levers
   (L2 deterministic-first ordering, L3 common-disjunct) target this and remain the
   honest direction — they keep the per-pair tree small without a soundness risk.
2. **Cross-test reuse (secondary, architecture-dependent).** Konclude's backend +
   completion-graph reuse cache (det + non-det, compatibility-gated) amortizes across tests.
   **rustdl measured low reuse potential** in its own caching experiments (SP4 within-search
   caching ~0 pot; the snapshot cache was FP-unsound on the disjunctive fragment and is off).
   So Konclude-style sound non-deterministic reuse is a *real* difference, but rustdl's data
   says it is not where rustdl's cheap wins are — the reuse-trap soundness problem stands.

**The striking confirmation: Konclude implements exactly the levers rustdl's own analysis
already named.** rustdl's whole-corpus wine study independently identified "L2
deterministic-first ordering, L3 common-disjunct, Konclude's saturation keeping the tree
small" as the lever — and the source shows Konclude has precisely those: approximated
saturation, a common-disjunct extraction preprocessing pass, and KPSet possible-subsumer
pruning (rustdl's label heuristic is a weaker cousin of KPSet). So this isn't a mysterious
trick — it's a known, named set of techniques rustdl could in principle adopt; they are just
each a substantial, careful build (and the saturation must be a *sound* approximation, which
is the hard part).

**Net:** Konclude wins wine by keeping per-test trees small (saturation + common-disjunct +
KPSet) *and* reusing model fragments across tests (sound, compatibility-gated backend cache).
rustdl matches neither: no saturation-grade precompletion for SROIQ, and its model-reuse
cache is off for soundness.
Both are multi-month, architecture-level changes — the branch-count-reduction direction
(saturation/ordering) is the lower-soundness-risk of the two, but neither is a cheap lever,
consistent with everything measured this arc. No action implied beyond recording the
mechanism; wine stays an accepted, MISSED=0 perf gap on a single nominal-heavy fixture.

## 6. Is the *bounded* Konclude lever (common-disjunct extraction) usable for wine? No.

rustdl has common-subsumer extraction (`disjunction_existential.rs`) but only at EL
preprocessing — the **wedge's `find_open_disjunction` branches without it**, exactly where
Konclude's `CCommonDisjunctConceptExtractionPreProcess` applies. That looked like a bounded,
adoptable lever. But wine's actual disjunction structure (from `clause-stats` + the `.ofn`)
rules it out for wine:

- 76 disjunctive clauses, dominated by **33 `ObjectOneOf` nominal value-partitions**
  (`{Moderate,Strong}`, `{Full,Medium}`, …) + 117 `∀` + 34 cardinality; only **2
  `ObjectUnionOf`** (genuine class ⊔).
- Wine's cost is the **combinatorial value-assignment** across those nominal partitions over
  ~206 individuals — *which* value each individual takes. A common subsumer of `{Full,Medium}`
  (the partition class, e.g. `WineBody`) is already implied, so asserting it deterministically
  saves nothing; it does not reduce the combinatorial choice that is the actual cost.
- So common-disjunct extraction would touch only the 2 `ObjectUnionOf` (and genuine-⊔ DL-tail
  onts — the ≤8-obscure-ont prize), **not wine**.

**Confirmed: there is no *bounded* Konclude-inspired lever for wine.** Wine needs the
multi-month one — approximated saturation + nominal-node merging to resolve the value-
partition structure once, + backend reuse so each test doesn't re-explore all individuals'
value choices. That is the precise (now structurally-scoped, not vague) shape of the wine
lever: a sound saturation/precompletion over nominals+cardinality, which rustdl's EL-only
saturator does not provide.

## Caveats

- Konclude internals here are confirmed from source structure + config + phase timings, not
  from reading the full calculus implementation. The named techniques are real (source files
  cited above). Exact runtime counts (cache hits, completion-graph nodes) were NOT obtained:
  Konclude's classification statistics are *collected* into the query object
  (`CollectProcessStatistics`) but only surfaced via an OWLlink statistics response, not the
  CLI log — there is no per-classification "log statistics" key (only SPARQL-answering +
  backend-cache-storage log keys; the latter didn't fire on wine). Config format (the earlier
  blocker) is now cracked: OWLlink `<RequestMessage>` with `<Set key='…'><Literal>…</Literal></Set>`.
  The mechanism, not the counts, is the finding.
- The per-pair 26%-backjump figure is from the rustdl bjprobe build (`diag/ore-bjgap-probe`,
  throwaway); it refines, not contradicts, the per-class §3b result (different probe).
