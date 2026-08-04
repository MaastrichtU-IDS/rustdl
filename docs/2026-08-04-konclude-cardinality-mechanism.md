# Why Konclude does `ore_ont_10019` in 0.04 s: measured, not inferred

**Date:** 2026-08-04
**Question:** rustdl takes 97 s on `ore_ont_10019` (47 classes); Konclude takes 0.04 s. Which
published mechanism accounts for the difference, and does it make the sufficient (⇐) direction of
a cardinality-bearing definition **deterministic** or merely **cheaper**?

**Answer, in one line:** Konclude's absorbed rules have **exactly two guards each — one named class
plus one freshly-minted surrogate ("implication trigger") concept — and *zero* of its 47 absorbed
rules fire on a bare node.** rustdl's `ConceptRule` has **one** atomic guard, and **10** of them fire
on every bare `CarbonAtom`. The ⇐ direction is *not* made deterministic in either reasoner; it is
made **demand-driven**, which is a different and cheaper thing.

**This document supersedes two conclusions in the existing record.** See §5.

Notation used throughout, kept rigidly separate:

- **[LIT]** = claim from the published literature (no local measurement).
- **[MEAS]** = claim measured here against the local binary, with the command shown.

Binary under test: `Konclude v0.7.0-1138 - 500e11d9 (Jun 19 2021)`, static Linux x64, at
`/data/dumontier/reasoners/konclude`. rustdl `0.4.14` at `target/release/rustdl`.
All scratch artefacts, configs and scripts:
`/tmp/claude-1007/-data-dumontier-rustdl/8e753f2f-e24e-4be2-8c66-c6e13e322bae/scratchpad/konclude-probe/`
(including the two absorbed-TBox dumps `absorbed-tbox-{ON,OFF}.txt`, and `config-keys.txt`, the
662 configuration keys extracted from the binary).

---

## 0. The instrument, and why it can be trusted

Everything below rests on two capabilities that turned out to be available, and on one validation.

**[MEAS] Konclude accepts a config file (`-c`) keyed by a 662-entry namespace, extracted from the
static binary:**

```sh
strings -n 6 /data/dumontier/reasoners/konclude | grep -E '^Konclude\.' | sort -u   # 662 keys
```

**[MEAS] It will dump its own absorbed TBox.** This is the single most valuable finding of the
session, because it removes all guesswork about what absorption does:

```xml
<Set key='Konclude.Debugging.WriteDebuggingData'><Literal>true</Literal></Set>
<Set key='Konclude.Debugging.WritePreprocessingDebuggingData'><Literal>true</Literal></Set>
<Set key='Konclude.Debugging.WritePreprocessingDebuggingData.AbsorbedTBox'><Literal>true</Literal></Set>
<Set key='Konclude.Debugging.BaseWritingPath'><Literal>dbg/</Literal></Set>
```
→ `dbg/Preprocessing/{1-BuildedTBox,2-NormalizedTBox,3-GroundedTBox,4-AbsorbedTBox,5-ProcessedTBox}.txt`

**[MEAS] Validation of the pair-extraction pipeline.** Comparing *Hasse* rows across
configurations is invalid — disabling a flag reshapes the transitive reduction, so rows move rather
than vanish. (This produced a wrong intermediate reading in this very session: a config that
*loses* 12 entailments printed **more** `SubClassOf` rows, 134 vs 126, because orphaned classes get
listed against grandparents.) All numbers below are **transitive closures** with equivalence
classes folded, via `closure.py`. Validated against the committed oracle:

| | non-`Thing` pairs |
|---|---|
| Konclude default | **162** |
| HermiT (`hermit.txt`, committed) | **162** |
| Konclude-only | 0 |
| HermiT-only | 0 |

Reproducing the root-cause doc's "162 pairs, zero disagreement" exactly. The oracle is
uncontested, so **no incompleteness explanation is available** for Konclude's 0.04 s (§3, EXP-6
makes this direct).

**[MEAS] Baseline, min-of-3, outcome judged from content not exit code:** 0.044 s, 16 892 bytes,
162 pairs. `-v` decomposition: parse 3 ms, preprocess 2 ms, precompute 5 ms, **classify 30 ms**,
total 47 ms ⇒ ~0.18 ms/pair. rustdl on the same file: **96.54 s** (independently reproducing the
doc's 97 s).

---

## 1. Hypotheses, ranked, with prior confidence

Stated before the experiments, so the refutations below are legible as refutations.

| # | hypothesis | prior | mechanism class | verdict |
|---|---|---|---|---|
| H1 | **Tableau/saturation coupling** — a completion-based saturation computes the consequences deterministically; the tableau is only consulted for concepts saturation cannot settle | **high** | makes it *deterministic* for the covered part | **REFUTED as the explanation** (§3 EXP-3, EXP-5) |
| H2 | **Binary absorption** (a second guard in the rule body) | medium | *demand-driven*, not deterministic | **CONFIRMED, and it is sufficient on its own** (§3 EXP-4, EXP-7) |
| H3 | **Definitorial/surrogate absorption** — a fresh atom `M ≡ hardᵢ` turns the ⇐ direction Horn | medium | *deterministic* | **PARTIALLY CONFIRMED — surrogates exist and are load-bearing, but the head stays disjunctive** (§3 EXP-2, EXP-7) |
| H4 | Classification-level avoidance (told subsumers, known/possible subsumer sets) | medium | cheaper only | **REFUTED as the explanation** (§3 EXP-5) |
| H5 | Semantic branching / backjumping / dependency-directed backtracking | low–medium | cheaper only | **NOT individually sufficient, but jointly load-bearing** (§3 EXP-7) |
| H6 | Caching (unsatisfiability, satisfiable-expansion, completion-graph, pseudo-model) | low–medium | cheaper only | **NOT individually sufficient** (§3 EXP-7) |
| H7 | Konclude simply does not derive the ⇐ direction (incompleteness) | low | n/a | **REFUTED** (§0 oracle; §3 EXP-6) |
| H8 | Algebraic / integer-programming cardinality reasoning (Faddoul & Haarslev) | low | deterministic for cardinality | **NO EVIDENCE** (§4) |

### [LIT] What the literature actually claims for each

**Absorption variants.** [LIT] Classical absorption (Horrocks; **Tsarkov & Horrocks, "Efficient
Absorption for Description Logics", 2007**) rewrites a GCI `C ⊑ D` into `A ⊑ …` guarded by an
*atomic* concept `A` occurring negatively, so the rule fires only on nodes labelled `A` instead of
universally. [LIT] **Hudek & Weddell, "Binary Absorption in Tableaux-Based Reasoning for
Description Logics" (DL 2006)** generalises the guard to a **conjunction of two** atomic concepts:
`A ⊓ B ⊑ D`. Its stated purpose is exactly to reduce how often a disjunctive consequent is opened —
a rule with two guards fires on the *intersection* of two label sets. [LIT] Neither technique, as
published, puts a **cardinality** conjunct into a guard: both require the guard to be atomic. [LIT]
The way a non-atomic conjunct is admitted is by **definitorial (surrogate) introduction** — mint a
fresh name `M` for the conjunct and guard on `M` — at the cost of needing `M` to be *derived*,
which for `≤n r.C` is itself a hard-antecedent problem. This is precisely the sub-problem
`docs/2026-08-04-absorption-on-10019-is-fully-blocked.md` identified as binding.

**Lazy unfolding.** [LIT] Standard practice (Baader et al.; FaCT/FaCT++ lineage) is that for
`D ≡ C`, the **necessary** (⇒) direction unfolds lazily on nodes labelled `D`, while the
**sufficient** (⇐) direction cannot be unfolded on demand from `D` — it must be *detected*, which
is what makes it an absorption problem rather than an unfolding problem. Lazy unfolding therefore
does **not** address this pattern.

**Told subsumers / completely-defined concepts.** [LIT] **Glimm, Horrocks, Motik & Stoilos,
"Optimising Ontology Classification" (ISWC 2010)**, extended as **"A Novel Approach to Ontology
Classification" (JWS 2012)**, maintains per-class sets of **known** and **possible** subsumers
(the "KPSet" of Konclude's `OptimizedKPSetClassClassifier`) and exploits told/completely-defined
information to skip subsumption tests. [LIT] This is a *cheaper*, never a *deterministic*,
mechanism — it reduces the number of probes, not the cost of one. The task brief correctly notes
this cannot be the explanation on its own, since rustdl already has told-subsumer tables and a
label heuristic pruning 96–100% of pairs.

**Saturation coupling.** [LIT] **Steigmiller, Glimm & Liebig, "Coupling Tableau Algorithms for
Expressive Description Logics with Completion-Based Saturation Procedures" (IJCAR 2014)**, journal
version **"Pay-as-you-go Description Logic Reasoning by Coupling Tableau and Saturation
Procedures" (JAIR 2015)**, is Konclude's own mechanism and matches its
`Konclude.Calculation.Optimization.ConceptSaturation` /
`SaturationCriticalConceptTesting` / `SaturationDirectCriticalToInsufficient` key family. The
design: a completion-based saturation computes sound consequences; where saturation is
**insufficient** the concept is **critical** and must be handed to the tableau. [LIT] For the
covered part this *is* deterministic. This was my leading hypothesis and the measurements refute it
as the explanation *for this ontology* (§3 EXP-3/EXP-5) — a good example of a mechanism being real,
published, implemented, and nonetheless not the operative one on a given input.

**Completion-graph / pseudo-model caching.** [LIT] **Steigmiller, Liebig & Glimm, "Extended
Caching, Backjumping and Merging for Expressive Description Logics" (IJCAR 2012)** — Konclude's
`CompletionGraphCaching`, `UnsatisfiableCacheRetrieval`, `SatisfiableExpansionCache*`,
`PseudoModelSubsumptionMerging` (the last being the Horrocks-style pseudo-model merging used to
refute subsumptions cheaply). [LIT] All *cheaper*, none deterministic.

**Semantic branching / DDB / BCP.** [LIT] Semantic branching (Horrocks & Patel-Schneider) replaces
syntactic `⊔`-branching with a case split on a literal and its complement, so sibling branches are
disjoint and the same refutation is not re-derived; dependency-directed backtracking / backjumping
prunes branches that did not contribute to a clash. [LIT] Both *cheaper*, neither deterministic.
rustdl has backjumping with `DepSet`s; the root-cause doc measures its dep-sets at 110–112 on this
ontology, i.e. near-useless here.

**Algebraic cardinality reasoning.** [LIT] **Faddoul & Haarslev, "Algebraic Tableau Reasoning for
the Description Logic SHOQ" (2010)** replaces the `≤n`/`≥n` choose-and-merge machinery with an
integer-programming/atomic-decomposition solver, which *is* a deterministic treatment of
cardinality. **I have no evidence Konclude uses it, and its key namespace contains no term
suggesting it** (no `algebraic`, `simplex`, `integer program`, `atomic decomposition`). Konclude's
cardinality keys are all of the choose/merge/expansion family (`PairwiseMerging`,
`MinimizedMergingBranches`, `BackendCriticalNeighbourExpansion*`). **Treat H8 as unsupported, not
as refuted.**

---

## 2. The decisive structural observation

**[MEAS] Konclude's absorbed ⇐ rule for `KetoneGroup` has the *same disjunctive shape* as
rustdl's.** From `absorbed-tbox-ON.txt`, concept 334 (`CCIMPL`; `IMPLTRIG`/IRIs abbreviated):

```
334 := (¬CarbonAtom ---->>> ( KetoneGroup
                            ⊔ ¬(ATLEAST [2] hasSingleBondWith.Alkyl)
                            ⊔ ¬(ATMOST  [2] hasSingleBondWith.Alkyl)
                            ⊔ ¬(ATMOST  [1] hasDoubleBondWith.OxygenAtom) ))
```

`CCIMPL` denotes the clause `¬guard ⊔ consequent`; the occurrence annotation `(| +335 | -88 |)`
confirms it (`-88` = negative occurrence of `CarbonAtom`, `+335` = the `CCOR` head). This is
**exactly** the form `crates/owl-dl-core/src/clause.rs:471-477` produces — trigger on the atomic
soft conjunct, negate the hard conjuncts into a disjunctive head. **So Konclude does not make the
⇐ direction deterministic, and the surrogate-Horn story in
`docs/2026-08-04-ore-10019-rootcause.md` §8 is wrong as an account of Konclude.**

**[MEAS] The difference is the *guard count*.** Parsing all `CCIMPL` rules and resolving each
rule's guards from both its own negative occurrences *and* the `CCIMPLTRIG (| +rule |)` occurrence
lists (the printed `---->>>` form shows only the **class** guard; surrogate guards are recoverable
only from the occurrence lists):

```
total CCIMPL rules: 47
guard-count distribution: Counter({2: 47})        # every rule has EXACTLY 2 guards
rules whose only guard is a named class (no surrogate): 0
```

Per-trigger, with how many of those rules are disjunctive and how many fire on a bare node:

| class trigger | rules | disjunctive | fire on a **bare** node |
|---|---|---|---|
| `CarbonAtom` | 7 | **1** | **0** |
| `CarbonGroup` | 6 | 5 | **0** |
| `OrganicGroup` | 5 | 0 | **0** |
| `SulfurAtom` | 5 | 0 | **0** |
| `OxygenAtom` | 3 | 1 | **0** |
| `NitrogenAtom` | 3 | 2 | **0** |
| 12 surrogate triggers | 18 | 6 | **0** |

Against rustdl, from the existing record: `ConceptRule { trigger: ClassId, conclusion: ConceptId }`
(`absorb.rs:145-148`) carries a **single atomic** trigger, and **10** of the 26 conjunctive
definitions have `CarbonAtom` as their soft conjunct, so *"every node labelled `CarbonAtom` acquires
10 open disjunctions at once, purely from being a carbon atom"* (root-cause doc §5).

**That is the whole gap, quantified: bare `CarbonAtom` opens 10 disjunctions in rustdl and 0 in
Konclude.** It also explains the root-cause doc's §4b partition — *"the 11 [classes] that complete
are exactly the atoms that are not a soft trigger of any defined class"*, and bare `CarbonAtom` /
`NitrogenAtom` / `OxygenAtom` hanging. In Konclude a bare atom triggers nothing at all.

**[MEAS] The second guard is a surrogate, and it is propagated along roles.** Konclude mints 89
`CCIMPLTRIG` concepts, outside the ontology signature, and propagates them with `CCIMPLALL`
(`∀r.TRIG`). Traced example (`AcylGroup`):

```
441 := ( ALL hasSingleBondWith. (440~IMPLTRIG) )                    # push surrogate to successors
443 := (¬(440~IMPLTRIG) ---->>> ( AcylGroup
                                 ⊔ ¬(ATLEAST [2] hasSingleBondWith.OrganicGroup)
                                 ⊔ ¬(ATMOST  [2] hasSingleBondWith.OrganicGroup)
                                 ⊔ ¬(ATMOST  [1] hasDoubleBondWith.OxygenAtom) ))
```

and (`Alkyl`, whose printed form is the misleading-looking `(¬CarbonAtom ---->>> Alkyl)`, whose real
guard set is `{CarbonAtom, TRIG402}` because `402: CCIMPLTRIG (| +405 |)`):

```
403 := ( ALL hasSingleBondWith. (402~IMPLTRIG) )
405 := CarbonAtom ⊓ TRIG402  →  Alkyl        # deterministic: no disjunction at all
```

So the answer to the brief's sharpest question — *which technique can put a cardinality conjunct
into a rule body?* — is: **[MEAS] Konclude does not put the cardinality conjunct itself into the
body. It mints a surrogate for a *role-reachability* condition derivable from the definition,
propagates it with `∀r.TRIG`, and uses that as the second guard.** The cardinality conjuncts stay
negated in the head. The rule is therefore still nondeterministic *when it fires* — it just fires
on a far smaller node set, and never on a node that has no relevant role successor.

### 2b. The full `KetoneGroup` chain — and it is the actionable result

`KetoneGroup` is the hard case precisely because it has **no existential conjunct**: it is
`CarbonAtom ⊓ =1 hDBW.OxygenAtom ⊓ =2 hSBW.CarbonGroup`. Resolving its guards transitively through
the occurrence lists gives the complete provenance (all **[MEAS]**, from `absorbed-tbox-ON.txt`):

```
75  OxygenAtom : CCSUB (| … +284 … |)      # OxygenAtom asserts 284
284 := ( ALL hasDoubleBondWith. (283~IMPLTRIG) )
103 Alkyl      : CCSUB (| … +330 … |)      # Alkyl asserts 330
330 := ( ALL hasSingleBondWith. (329~IMPLTRIG) )
332 := (¬(283~IMPLTRIG) ---->>> (331~IMPLTRIG))   + surrogate guard 329
                                            # i.e.  TRIG283 ⊓ TRIG329 → TRIG331
334 := (¬CarbonAtom     ---->>> Or335)      + surrogate guard 331
                                            # i.e.  CarbonAtom ⊓ TRIG331 → Or335
```

and the RBOX confirms both roles are their own inverse (`inverse roles: -7~hasSingleBondWith`,
`-8~hasDoubleBondWith`), so `∀r.TRIG` asserted **at the filler** marks exactly the nodes satisfying
`∃r.filler`. Composing:

```
CarbonAtom ⊓ ∃hDBW.OxygenAtom ⊓ ∃hSBW.Alkyl
    →  KetoneGroup ⊔ ¬(≥2 hSBW.Alkyl) ⊔ ¬(≤2 hSBW.Alkyl) ⊔ ¬(≤1 hDBW.OxygenAtom)
```

**[MEAS] So Konclude *does* get a guard out of a cardinality conjunct — by using its `≥1`
consequence.** `=1 hDBW.O` entails `∃hDBW.O`; `=2 hSBW.CG` entails `∃hSBW.Alkyl` (via the
`CarbonGroup ≡ Aryl ⊔ Alkyl` covering). Those existential consequences become guards. The `≤n`
halves, and the exact `≥2`, stay negated in the head. **The `≤n` "reverse derivation of a
cardinality surrogate" that both existing documents identify as the binding sub-problem is never
solved — it is never attempted.**

**Why this matters for rustdl, concretely.** `docs/2026-08-04-absorption-on-10019-is-fully-blocked.md`
argues the main tableau cannot host `∃`-conjunct bodies because *"There is no edge-join rule kind:
`RoleRule` is `∀R.D` propagation, a different shape"*, and prices the fix as "giving the main
tableau the wedge's clause machinery — a substantial engine change". **[MEAS] Konclude shows the
edge join is not needed: `∀r.surrogate` propagation *is* the implementation of the edge join, and
that is exactly the shape `RoleRule` already has.** The guard test at rule-firing time is then a
pure label lookup, not a join.

On this reading the missing capability in rustdl is not an engine rewrite but two much smaller
things: (i) a **multi-guard `ConceptRule`** — the `absorb.rs:45` "Phase 4 refinement" already named
as unbuilt, and a label-set membership test rather than a join; and (ii) an absorption step that,
for each `∃r.F` / `≥n r.F` conjunct of a definition body, mints a surrogate `T` and emits
`F ⊑ ∀r⁻.T` (`∀r.T` when `r` is symmetric, as all five roles here are). **This is a hypothesis
about rustdl derived from a measurement of Konclude, not a measured claim about rustdl, and it is
unbuilt and unvalidated** — see §5 for what would have to be established first. Note in particular
that soundness of (ii) needs the *inverse* role in general, and that the `≥n`-as-guard step is only
a guard (a necessary condition), so it can only ever reduce how often the head opens — it cannot
change which subsumptions are entailed.

---

## 3. Experiments

Each: prediction → command → result. Predictions were written before running.

### EXP-1 — Does any flag disable absorption, and does the disjunction survive it?

*Prediction:* if Konclude's speed comes from preprocessing the disjunction away, the absorbed TBox
will show no disjunctive ⇐ head.
*Result:* **REFUTED** — the disjunctive head is present (§2). Absorption is not eliminating it.

### EXP-2 — Absorbed TBox with binary absorption ON vs OFF

```sh
# cfg-dump-nobin.xml adds:
# Konclude.Calculation.Preprocessing.GCIAbsorption.TriggeredImplicationBinaryGCIAbsorption = false
konclude classification -c cfg-dump-nobin.xml -i t10019.owl -o /dev/null
```

*Prediction:* if the pass is what supplies the guards, disabling it should widen the trigger.

| | lines | `CCIMPLTRIG` | `CCIMPL` | `KetoneGroup` ⇐ form |
|---|---|---|---|---|
| ON (default) | 1479 | **89** | **47** | `¬CarbonAtom ---->>> (…4-way ⊔…)` |
| OFF | 927 | **2** | **0** | raw top-level `Or(¬(body), KetoneGroup)` — **unguarded, universal** |

**CONFIRMED, and stronger than predicted.** With the pass off there are **no absorbed implications
at all**: the ⇐ direction reverts to a universally-applicable GCI. So
`TriggeredImplicationBinaryGCIAbsorption` is the pass that produces *both* the class guard and the
surrogate guard.

### EXP-3 — Single-flag sweep over all 243 `Konclude.Calculation.*` keys

```sh
for k in $(grep '^Konclude\.Calculation\.' config-keys.txt); do mkcfg "$k=false"; run min-of-3; done
```

*Prediction:* one optimisation will be load-bearing and disabling it will make Konclude slow.
*Result:* **REFUTED — no single flag makes Konclude slow.** Anomalies (all others within noise of
0.044 s / 162 pairs):

| flag `=false` | wall | pairs |
|---|---|---|
| `Preprocessing.GCIAbsorption.TriggeredImplicationBinaryGCIAbsorption` | **0.222 s** (5.0×) | 162 |
| `Preprocessing.NegationNormalization` | **0.229 s** (5.2×) | 162 |
| `Optimization.SaturationCriticalConceptTesting` | 0.024 s | **150** (−12) |
| `Optimization.ConceptSaturation` | 0.023 s | **0** real pairs |
| `Preprocessing.{Preprocessing,SubroleTransformation,ProcessingDataExtender,DisjunctionToImplicationAbsorptionByExistingTriggers}`, `Classification.Classifier` | SIGSEGV | — |

The worst single-flag arm is **0.23 s**, still ~420× faster than rustdl.

### EXP-4 — The 12 pairs lost to `SaturationCriticalConceptTesting=false`

*Prediction:* if this guard is what routes hard concepts to the tableau, the lost pairs will be the
cardinality-definition ones.
*Result:* **CONFIRMED, exactly.** All 12:

```
Acyl{,Bromide,Chloride,Fluoride,Iodide,Halide}Group ⊑ CarbonylGroup   (6)
AldehydeGroup ⊑ CarbonylGroup      EsterGroup ⊑ CarbonylGroup
KetoneGroup ⊑ CarbonylGroup                                    ← the headline pair
SulfonicAcidGroup ⊑ SulfonicAcidDerivativeGroup
SulfonylHalideGroup ⊑ SulfonicAcidDerivativeGroup
SulfoxideGroup ⊑ SulfinicAcidGeneralGroup     ← the ONE pair rustdl cannot decide at any budget
```

These are exactly the pairs whose derivation needs the ⇐ direction of a cardinality-bearing
definition, and they cost about half of Konclude's classify wall (0.024 → 0.044 s), ~2 ms each.
For scale: rustdl spends **373 919 branches / 5 001 ms** on `KetoneGroup` alone and still does not
decide `SulfoxideGroup ⊑ SulfinicAcidGeneralGroup`.

### EXP-5 — RETRACTION: `ConceptSaturation=false` does *not* show saturation is load-bearing

An intermediate reading of EXP-3 — "saturation supplies 162/162 pairs" — was **wrong**, and the
control that caught it is worth recording. `ConceptSaturation=false` leaves the **automated
classifier selector** in a degenerate state that reports nothing. Pinning the classifier:

```sh
mkcfg  Konclude.Calculation.Classification.AutomatedOptimizedClassifierSelection=false \
       Konclude.Calculation.Classification.Classifier.OptimizedKPSetClassClassifier=true \
       Konclude.Calculation.Optimization.ConceptSaturation=$sat
```

| classifier | `ConceptSaturation` | wall | pairs |
|---|---|---|---|
| `OptimizedKPSetClassClassifier` | true / **false** | 0.051 / **0.047** | 162 / **162** |
| `OptimizedSubClassClassifier` | true / **false** | 0.052 / **0.046** | 162 / **162** |
| `OptimizedClassExtractedSaturationClassifier` | true / **false** | 0.053 / **0.043** | 162 / **162** |

**[MEAS] Konclude computes all 162 pairs in ~0.045 s with concept saturation entirely disabled,
under all three of its classifiers.** H1 is refuted as the explanation, and H4 with it (no
classifier choice matters). Also refuted, same control: `ForceCompleteSatisfiableTest=true`
(0.042 s, 162), `SaturationSubsumerExtraction=false` (0.051 s, 162),
`PruneSubsumptionRelations=false`, `DerivateSubsumptionRelations=false`,
`ObviousSubsumptionCalculation=false` — all 162 pairs, all within noise. **Konclude does not need
to avoid the probes; it is fast at them.**

### EXP-6 — Does Konclude derive the ⇐ direction at all, with a discriminating control?

Four definitions differing only in body shape, each with a class asserted to satisfy the body:

```
EquivalentClasses(:Datom  ObjectIntersectionOf(:A :B))                              SubClassOf(:Eatom  …same…)
EquivalentClasses(:Dexist ObjectIntersectionOf(:A ObjectSomeValuesFrom(:r :F)))     SubClassOf(:Eexist …same…)
EquivalentClasses(:Dcard  ObjectIntersectionOf(:A ObjectExactCardinality(1 :r :F))) SubClassOf(:Ecard  …same…)
EquivalentClasses(:Dmax   ObjectIntersectionOf(:A ObjectMaxCardinality(2 :r :F)))   SubClassOf(:Emax   …same…)
```

*Prediction:* if Konclude's speed were partly incompleteness, the `card`/`max` rows would be
missing while the atomic control (`Eatom ⊑ Datom`) is present.
*Result:* **REFUTED — no incompleteness.** Konclude reports `Eatom ⊑ Datom`, `Eexist ⊑ Dexist`,
`Ecard ⊑ Dcard`, `Emax ⊑ Dmax`, **and additionally** `Ecard ⊑ Dexist` and `Ecard ⊑ Dmax` (i.e.
`=1 r.F ⊨ ∃r.F` and `⊨ ≤2 r.F`). The atomic case is the discriminating control and it fires, so the
cardinality cases are not silence-of-the-under-reporter. Combined with the 162 = 162 HermiT
agreement in §0, **H7 is closed.**

### EXP-7 — The combination arm, and the bisection (the decisive experiment)

*Prediction:* if no single optimisation is necessary (EXP-3), maybe they are jointly necessary.

Disabling 14 optimisations at once (saturation, critical-concept testing, binary absorption,
semantic + atomic semantic branching, backjumping, dependency tracking, completion-graph caching,
unsatisfiability-cache retrieval, satisfiable-expansion-cache retrieval, branch triggering,
pseudo-model subsumption merging, common-disjunct extraction, disjunct sorting), on the pinned
classifier: **DNF at 180 s.** So the speed *is* optimisation-borne — 0.05 s → ≥180 s, ≥3 600×.

Then, from ALL-OFF, **re-enabling exactly one** (30 s cap):

| re-enabled | wall | pairs |
|---|---|---|
| **`GCIAbsorption.TriggeredImplicationBinaryGCIAbsorption`** | **0.055 s** | **162 (complete)** |
| `Optimization.Backjumping` | 0.683 s | 161 |
| `Optimization.ConceptSaturation` | 0.031 s | 116 (incomplete) |
| `SaturationCriticalConceptTesting` | DNF30 | — |
| `SemanticBranching` | DNF30 | — |
| `AtomicSemanticBranching` | DNF30 | — |
| `DependencyTracking` | DNF30 | — |
| `CompletionGraphCaching` | DNF30 | — |
| `UnsatisfiableCacheRetrieval` | DNF30 | — |
| `SatisfiableExpansionCacheRetrieval` | DNF30 | — |
| `BranchTriggering` | DNF30 | — |
| `PseudoModelSubsumptionMerging` | DNF30 | — |
| `CommonDisjunctConceptExtraction` | DNF30 | — |
| `DisjunctSorting` | DNF30 | — |

**Binary/triggered-implication absorption is the only mechanism that alone recovers both full speed
and full completeness from a >3 600× regression.** 12 of the other 13 leave it DNF; backjumping
alone gets within 14× at the cost of one pair; saturation alone is fast but incomplete (116/162 —
the pay-as-you-go signature: sound, under-approximating, needs the tableau for the rest).

### EXP-8 — Scaling in N (number of shared-trigger ⇐ directions), on the real pattern

`ablate.py k` keeps the first *k* of the 29 `EquivalentClasses` as `≡` and converts the rest to
`SubClassOf` (dropping only the ⇐ half), leaving everything else — the 55-clique disjointness, the
5 symmetric roles, the `Alkyl` generator — intact.

*Prediction:* exponential growth in *k* ⇒ Konclude branches too; flat ⇒ deterministic or
demand-driven mechanism.

| k | Konclude `sat KetoneGroup` | rustdl `sat KetoneGroup` | Konclude classify | rustdl classify |
|---|---|---|---|---|
| 0 | 0.023 | 0.007 | 0.025 | 0.014 |
| 1 | 0.021 | 0.008 | 0.026 | 0.015 |
| 2 | 0.019 | 0.008 | 0.028 | 0.016 |
| 3 | 0.020 | 0.007 | 0.026 | 0.013 |
| 5 | 0.020 | **DNF 60** | 0.052 | 0.016 |
| 8 | 0.023 | **DNF 60** | 0.038 | 0.018 |
| 12 | 0.022 | **DNF 60** | 0.051 | **DNF 60** |
| 20 | 0.031 | **DNF 60** | 0.044 | 58.18 |
| 29 | 0.027 | **DNF 60** | **0.076** | **DNF 60** |

**Konclude is flat: 0.025 → 0.076 s (3×) as k goes 0 → 29. rustdl's `sat` crosses into DNF between
k=3 and k=5**, consistent with the root-cause doc's two-axiom minimal set, **and classify from
k=12.** This is the cleanest single discrimination in the study.

Pushing N *beyond* what the ontology supplies, by duplicating the whole definition block under
fresh names (same shared triggers, same fillers):

| variant | Konclude | pairs | rustdl classify |
|---|---|---|---|
| ×1 (N=29) | 0.053 s | 162 | **96.54 s** |
| ×2 (N=58) | 0.079 s | 430 | **DNF 120** |
| ×4 (N=116) | 0.147 s | 1 362 | **DNF 120** |
| ×8 (N=232) | **0.317 s** | **4 810** | **DNF 120** |

Konclude's wall grows **sub-linearly in the pair count** (30× pairs for 6× wall). No branching
signature.

### EXP-9 — Scaling in n (the cardinality value)

Adding a constant to every `ObjectExactCardinality`:

| bump | Konclude | pairs |
|---|---|---|
| +0 | 0.053 s | 162 |
| +1 | 0.066 s | 162 |
| +3 | 0.057 s | 162 |
| +8 | 0.103 s | 162 |

Weak growth (~2× for n up to ~11), consistent with ordinary choose/merge cardinality handling and
**not** with an algebraic/IP treatment being *needed*. Does not discriminate H8 either way.

### EXP-10 — Synthetic isolation (reported because it failed)

A generator producing `Dᵢ ≡ A ⊓ =n r.Fᵢ` for N classes over a shared trigger `A`, with a
disjointness clique, symmetric roles, and a generator class, **did not reproduce rustdl's blowup**
(N=20: rustdl 0.013 s, Konclude 0.020 s). The pathology needs the fillers to be *defined/covering*
classes so the ⇐ rules feed each other recursively; with primitive fillers there is no recursion.
**Reported as a negative result and as the reason the study pivoted to ablating the real ontology
(EXP-8), which is the stronger instrument anyway.** A synthetic that showed both reasoners fast
would have been vacuous evidence.

---

## 4. What the evidence supports

1. **[MEAS] The 0.04 s is not incompleteness.** 162 = HermiT's 162, zero disagreement, and the ⇐
   direction is derived for atomic, existential, exact-cardinality and max-cardinality bodies with
   a firing discriminating control (EXP-6). H7 closed.
2. **[MEAS] The ⇐ direction is NOT deterministic in Konclude.** Its absorbed head is the same
   4-way disjunction with negated cardinality conjuncts that rustdl builds (§2). The
   surrogate-Horn account in the root-cause doc §8 does not describe Konclude.
3. **[MEAS] The operative mechanism is a *second guard*: binary/triggered-implication absorption.**
   Every one of the 47 absorbed rules has exactly 2 guards; **0 fire on a bare node**; and this
   pass alone recovers 0.055 s / 162 pairs from a ≥3 600× regression (EXP-7). The mechanism is
   **demand-driven, not deterministic** — the disjunction still exists and is still branched on,
   but only on nodes that have already earned both guards.
4. **[MEAS] The second guard is a minted surrogate propagated along roles** (`∀r.TRIG`, 89
   `CCIMPLTRIG` concepts), which is how a guard is obtained for a definition whose only non-atomic
   conjuncts are `∃`/cardinality. This is the *combination* of H2 and H3, and it is why the atomic
   restriction assumed in the published statements of binary absorption is not binding in practice.
5. **[MEAS] Saturation, classifier choice, caching, semantic branching and backjumping are each
   insufficient**, and saturation is not even necessary here (EXP-5). They are jointly load-bearing
   (EXP-7 combination arm) but none is the answer to *this* pattern.
6. **[MEAS] Konclude's cost is flat in N and weak in n**, where rustdl's is explosive in N
   (EXP-8/9). Growth in N is the signature to test any candidate fix against.

### The two record corrections

**(a) `docs/2026-08-04-ore-10019-rootcause.md` §8 is wrong about Konclude.** It asserts Konclude
applies a definitorial transformation making the ⇐ direction "**Horn** … zero disjuncts, zero
don't-know nondeterminism". **[MEAS]** The dumped absorbed TBox shows a 4-way disjunctive head on a
`CarbonAtom` trigger. The §8 inference — *"the correct target is … to make the ⇐ direction
deterministic"* — is therefore not what the reference implementation does, and the "delicate
sub-problem" it defers (reverse derivation of a cardinality surrogate) is **not** a problem Konclude
solves. It sidesteps it.

**(b) `docs/2026-08-04-absorption-on-10019-is-fully-blocked.md` measured the right number and drew
the wrong inference.** Its central table — 0 atomic-only heads, 0 heads with ≥2 atomic conjuncts,
`..with extra ¬Atomic: 0` — is **not disputed**; rustdl's instrument is counting correctly. But the
conclusion *"Zero of 26 heads can be absorbed by any technique that only manipulates atomic
conjuncts … no third variation on atomic absorption will work either"* rests on the guard having to
be an **atomic conjunct of the definition**. **[MEAS] Konclude's guards are not that.** It mints
surrogates that are in no ontology and propagates them through roles, obtaining a second guard for
26 of 26 heads where the atomic count is 0. So **binary absorption is not inert on
`ore_ont_10019`; it is the single sufficient mechanism** (EXP-7). The corpus census in that
document (285 pool / 65 DNF-survivor ontologies with `extra ¬Atomic > 0`) is consequently an
**under-count of the addressable population**, because it counts only atomic second guards.

That said, the same document's *prioritisation* argument survives intact and should still be
honoured: `ore_ont_10019`'s unique remaining completeness prize is one pair, and a decision to
build this must be justified on a population, not on this ontology.

---

## 5. What remains unknown

- **Whether the `≥n`-as-guard pattern generalises.** §2b traces it completely for `KetoneGroup`,
  `Alkyl` and `AcylGroup` — three chains, all following "filler asserts `∀r.T`; rule guards on
  `T`". I have **not** verified it for all 47 rules, nor for a non-symmetric role (all five roles
  here are their own inverse, so `∀r.T` at the filler suffices and the inverse case is untested),
  nor for `≥n` with `n` large, nor what Konclude does when a definition's only non-atomic conjunct
  is a bare `≤n` (no `≥` consequence to exploit — there is no such head in this ontology). **Three
  traced chains are not the algorithm.** Cheapest next step: dump `5-ProcessedTBox.txt` and the
  per-test completion graphs
  (`Konclude.Debugging.WriteDebuggingDataCompletionTasksForClassificationTests`), and re-run the
  guard census on an ontology with asymmetric roles.
- **The rustdl implication in §2b is unvalidated.** It is inference from Konclude's absorbed output,
  not measurement of rustdl. Before it could be trusted: confirm `RoleRule` really can carry a
  minted surrogate (id-space, told-table, fragment-gate and output-leak hazards were flagged for
  surrogate minting in `docs/reviews-2026-08-04/R1-technical.md` and are **not** addressed here);
  confirm a 2-guard `ConceptRule` does not perturb the tuned dispatch in `rules.rs:171-179`; and
  gate on the 285/65 census **recounted for non-atomic second guards**, since the existing count is
  now known to measure the wrong population.
- **Whether the mechanism is exactly Hudek–Weddell binary absorption.** The key name says
  `…BinaryGCIAbsorption` and the measured guard count is exactly 2, which is strong. But the
  published technique is stated for *atomic* guards, and Konclude's guards include minted
  surrogates, so this is an extension whose published description I have not located. **[LIT]**
  confidence that the *family* is right: high. That the *paper* describes what the binary does:
  medium.
- **H8 (algebraic/IP cardinality) — unsupported, not refuted.** No key-namespace evidence, and the
  n-scaling is too weak to discriminate. Do not cite either way.
- **Why Konclude's `SaturationCriticalConceptTesting` decides `SulfoxideGroup ⊑
  SulfinicAcidGeneralGroup` in ~2 ms** when rustdl cannot at any budget. I have localised *which*
  component decides it, not *how*.
- **Generality beyond this ontology.** Every measurement here is on `ore_ont_10019` and its
  ablations/rescalings. The 162 = 162 oracle and the k- and N-sweeps make the finding solid *for
  this pattern*; nothing here establishes the population-level value of building it. Per the
  project's own standing rule, that needs the corpus-scale MISSED net **and** a full sweep, and the
  census column to be recounted for **non-atomic** second guards first — the current 285/65 figure
  is now known to be the wrong measurement.
- **Four `Konclude.Calculation.Preprocessing.*` keys SIGSEGV when disabled**
  (`Preprocessing`, `SubroleTransformation`, `ProcessingDataExtender`,
  `DisjunctionToImplicationAbsorptionByExistingTriggers`) and `Classification.Classifier` segfaults
  for every value. Those passes are therefore untested here.

---

## 6. Method notes

- **The binary shipped its own answer.** 662 config keys and five TBox-stage dumps were recoverable
  from a static binary with `strings` and one config file. Reading the reference implementation's
  *output* beat every inference drawn from its *timings* — the guard-count table (§2) settles in one
  measurement what a wall-clock study cannot.
- **Comparing Hasse rows across configurations is invalid**, and it produced a confidently wrong
  intermediate reading (a config that *loses* 12 entailments printed *more* rows). Compare
  transitive closures with equivalences folded, and validate the pipeline against a committed oracle
  before trusting any delta — mine reproduced 162 = 162 exactly, which is why the later deltas are
  believable.
- **A control caught a wrong mechanism attribution.** "Saturation supplies 162/162" survived one
  flag and died to the classifier-pinned control (EXP-5). Any flag whose effect is mediated by an
  *auto-selector* needs the selector pinned before the flag means anything.
- **Single-flag sweeps can find nothing while the joint arm finds everything.** No single flag made
  Konclude slow; 14 together made it DNF; and then a single-flag *re-enable* from that floor
  isolated the mechanism. When one-at-a-time ablation comes back null, invert it.
- **A failed synthetic is a result.** EXP-10 could not reproduce the pathology, which is itself the
  finding that the pattern needs *recursive* (defined/covering) fillers — and it is why the real
  ontology's ablation became the instrument.
- **`--help` is not the interface.** Konclude's CLI exposes 9 flags; its behaviour is governed by
  662 config keys none of which `--help` mentions.
