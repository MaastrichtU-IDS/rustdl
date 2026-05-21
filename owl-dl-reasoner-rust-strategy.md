# Building an OWL DL Reasoner in Rust: Implementation Strategy

A phased, hand-offable plan for Claude Code (or any engineering team) targeting parity with HermiT/Konclude on standard OWL 2 DL benchmarks.

---

## 1. Executive Summary

**Goal.** Build a sound, complete, performant OWL 2 DL reasoner (SROIQ(D)) in Rust. Target competitiveness with HermiT (Java, hypertableau) and Konclude (C++, tableau + saturation hybrid) on ORE benchmarks within 24-36 months of focused work.

**Approach.** Konclude-style hybrid: a consequence-based saturation engine handles the EL-ish subset cheaply, and a tableau engine handles the rest. This empirically beats pure tableau on real-world ontologies (mostly EL+a-little-DL) and is the architecture that has won most ORE competitions.

**Build on, don't rebuild:** the Rust ecosystem already has `horned-owl` (parsing + model, 20-40× faster than the Java OWL API) and `whelk-rs` (EL reasoner). The strategy uses both as foundations.

**Risk frame.** Tableau optimizations are 25 years of accumulated tricks; reaching production parity is a multi-year project. The phased plan delivers a *correct* ALC reasoner in ~3 months, a *correct* ALCHIQ reasoner in ~12 months, and a competitive SROIQ reasoner in 24-36 months. Useful artifacts ship at every phase.

---

## 2. Foundation: What Exists, What's Missing

### 2.1 Existing Rust crates to depend on

| Crate | Role | Status |
|---|---|---|
| `horned-owl` | OWL model, parsers for OWL/XML, OWL/RDF, functional syntax | Production-quality, actively maintained |
| `whelk-rs` | EL reasoning (Kazakov et al. consequence-based rules) | Working, will be our saturation kernel for the EL fragment |
| `rio` / `oxigraph` | RDF parsing | Production-quality |
| `petgraph` | General graph manipulation | Use for class hierarchy; *probably not* for the completion graph (custom representation will be faster) |
| `rayon` | Data parallelism | For parallel classification |
| `dashmap`, `hashbrown` | Fast concurrent / sequential hash maps | Caches |
| `bumpalo` | Arena allocation | Tableau nodes, transient concept expressions |
| `criterion` | Microbenchmarks | Per-PR perf regression tracking |
| `proptest` | Property testing | Critical for tableau correctness |

### 2.2 What we must build

1. An internal IR for concepts/roles/axioms in normalized form (DAG with structural sharing).
2. Preprocessing: structural transformation, absorption, lazy unfolding.
3. Consequence-based saturation engine (extending whelk-rs for our IR).
4. Tableau engine with full SROIQ rule set and optimizations.
5. Hybrid orchestrator: decide per-query whether saturation suffices.
6. Classification, realization, entailment, query interfaces.
7. Datatype reasoners (xsd:integer, xsd:string with regex, etc.).
8. Benchmark harness against ORE corpus, with results checked against HermiT/Konclude.

---

## 3. Architectural Strategy

### 3.1 Why hybrid, not pure tableau

Pure tableau (HermiT, FaCT++, Pellet) does well on hard logical content but pays a heavy constant overhead on the large, mostly-EL terminologies that dominate real-world OWL use (SNOMED CT, NCI, GO, FMA). Konclude's hybrid wins ORE because it handles those terminologies in saturation-time and only invokes tableau where genuinely needed. Replicating this design is the single most important architectural choice.

### 3.2 Tableau flavor: standard vs. hypertableau

HermiT's hypertableau avoids OR-branching by rewriting axioms into rules with conjunctive heads, reducing non-determinism. It's elegant but ties you to HermiT's specific preprocessing. **Recommendation: start with standard tableau with full optimization stack (absorption, dependency-directed backtracking, semantic branching, caching).** Add hypertableau-style rewriting in Phase 4+ if benchmarks show it helps. Standard tableau is far better documented and easier to debug; the optimization stack is what actually gets you to competitive performance.

### 3.3 Top-level module layout

```
crates/
  owl-dl-core/        # IR, normalization, common utilities
  owl-dl-saturation/  # Consequence-based engine (extends whelk-rs)
  owl-dl-tableau/     # Tableau engine
  owl-dl-reasoner/    # Hybrid orchestrator + public API
  owl-dl-cli/         # Command-line tool (OWLlink protocol later)
  owl-dl-bench/       # Benchmark harness
xtask/                # Build automation
ontologies/           # Test ontology corpus (gitignored, fetched by xtask)
```

---

## 4. Phased Implementation Plan

Each phase ends with a *shippable* artifact and a passing benchmark slice.

### Phase 0 — Foundation (2-3 weeks)

**Deliverable:** Workspace scaffold; `horned-owl` integration; internal IR; passing ALC normalization tests.

- Cargo workspace per §3.3.
- Internal IR `Concept` enum (Top, Bot, Atomic(ClassId), And, Or, Not, Some(Role, Concept), All(Role, Concept), Min(n, Role, Concept), Max(n, Role, Concept), Nominal(IndId), Self(Role)).
- `ClassId`, `RoleId`, `IndId` as `u32` indices into interned tables (don't store IRIs in the hot loop).
- Concept expressions stored in a **DAG with structural sharing**: a `ConceptPool` arena where each unique sub-expression appears once, addressable by `ConceptId(u32)`. Roughly: `HashMap<ConceptExpr, ConceptId>` plus `Vec<ConceptExpr>`. This is the single biggest performance lever; do not skip it.
- Conversion from `horned_owl::model::Component` to internal IR.
- A test harness that loads ORE corpus ontologies and produces summary stats (axiom counts, expressiveness profile).

**Why this matters:** Concept-equality checks must be O(1) pointer comparison, not structural. Every later phase assumes this.

### Phase 1 — Preprocessing & Normalization (4-6 weeks)

**Deliverable:** A normalizer that produces (i) NNF, (ii) absorbed TBox, (iii) precomputed told-subsumer table.

- Negation Normal Form: push `Not` to atoms.
- Structural transformation: replace complex concepts with fresh names where useful (Tseitin-style).
- **Absorption** — three flavors, all needed:
  - *Binary absorption:* turn `⊤ ⊑ ¬A ⊔ B ⊔ ¬C` into `A ⊓ C ⊑ B` (move atomic class names out of disjunctive GCIs).
  - *Role absorption:* `⊤ ⊑ ∀R.C` becomes a rule "when an R-edge is added, propagate C to the target."
  - *Concept absorption:* nominal absorption for `{a} ⊑ C`.
- Lazy unfolding: store definitions `A ≡ C` separately; only expand `A` to `C` when needed during tableau.
- Told subsumers: traverse axioms to build the trivial subsumption graph; pre-compute its transitive closure.

**HermiT reference:** `org.semanticweb.HermiT.structural` package — particularly `OWLAxioms`, `ObjectPropertyInclusionManager`, `BuiltInPropertyManager`. The `Clausification` class shows the hypertableau-style rewriting; you don't need it yet but read it to understand the form HermiT's tableau ultimately consumes.

**Konclude reference:** `Source/Reasoner/Preprocess/` — particularly `CConceptNormalizer`, `CAbsorber`, `CTBoxBinaryAbsorberPreProcessor`, `CTBoxRoleAbsorberPreProcessor`. The class hierarchy under `CTBox*PreProcessor` is the clearest exposition of which absorptions to do in what order.

### Phase 2 — ALC Tableau (8-10 weeks)

**Deliverable:** A correct (but unoptimized) ALC satisfiability checker, validated against HermiT on small test ontologies.

- Completion graph data structure:
  - `Node { id: NodeId, labels: SmallVec<[ConceptId; 8]>, edges: SmallVec<[(RoleId, NodeId); 4]> }`.
  - Store globally as `Vec<Node>`; addressable by `NodeId(u32)`.
  - `labels` and `edges` are sorted for fast subset checking.
- Six core expansion rules: `⊓-rule`, `⊔-rule`, `∃-rule`, `∀-rule`, `⊑-rule` (apply GCIs), `clash check`.
- Naive subset blocking.
- Depth-first search with backtracking; use `im` or hand-rolled persistent vectors for cheap state checkpoints — or alternatively log-and-undo with a `TableauTrail`. Pick the trail approach; it's faster in practice.

**Test oracle:** For every input ontology in the test set, run HermiT in a subprocess and compare classification results. Discrepancies block merging.

**Source to study:** universome/dl-reasoner (an existing ALCQ tableau in Rust) as a sanity check for shape, not as a quality reference. HermiT's `org.semanticweb.HermiT.tableau.Tableau` is the heavyweight model — read `runCalculus()` and the `*Manager` classes (`ExistentialExpansionManager`, `DisjunctionManager`, `MergingManager`).

### Phase 3 — Extensions to SROIQ (10-12 weeks)

Add each letter incrementally; each addition must keep all prior tests green.

- **H (role hierarchies):** sub-role axioms; closure during preprocessing.
- **S (transitive roles):** `∀R.C` propagates through R-chains when R is transitive. Done either by direct rule or by axiom transformation `R ∘ R ⊑ R` → automaton.
- **R (complex role hierarchies):** general `R₁ ∘ ... ∘ Rₙ ⊑ S`. Implement via finite-state automata over role names per Horrocks & Sattler. Substantial work; ~3 weeks alone.
- **I (inverse roles):** edges become bidirectional with role labels in both directions; affects blocking (need pair-wise / anywhere blocking).
- **O (nominals):** `{a}` concepts. Triggers individual merging. Major source of bugs; allocate test time.
- **Q (qualified number restrictions):** `≥ n R.C`, `≤ n R.C`. Merging and clash detection get harder.
- **F (functional roles):** trivial special case of `≤ 1 R.⊤`.

**Blocking:** subset blocking is unsound with inverses; switch to **anywhere pair-wise blocking** (Motik & Horrocks). This is the most subtle correctness point in the whole project. Implement, then property-test against HermiT exhaustively before moving on.

**HermiT reference:** `org.semanticweb.HermiT.blocking` — `AnywhereBlocking`, `PairWiseDirectBlockingChecker`. `org.semanticweb.HermiT.tableau.MergingManager` for nominal-induced merging.

**Konclude reference:** `Source/Reasoner/Kernel/Process/` — particularly `CIndividualProcessNode`, `CBackendRepresentativeMemoryCache`, the `Process*Task` family. Konclude's individual reuse machinery is worth a separate study session.

### Phase 4 — Tableau Optimizations (8-10 weeks)

This is where most of the speedup lives. Implement in roughly this priority order:

1. **Lazy unfolding** (likely already done in Phase 1) — only expand defined names when their negation appears.
2. **Dependency-directed backtracking (backjumping):** track which branching choices each label depends on; jump past irrelevant choices on clash. Cuts search dramatically.
3. **Semantic branching:** when expanding `C ⊔ D`, on the second branch add both `¬C` *and* `D` (not just `D`). Avoids re-exploring `C`-models.
4. **Caching:**
   - *Subsumption cache:* `(C, D) → bool` for known sub/non-sub.
   - *Model cache (pseudo-model):* a compressed representation of a satisfying tableau for each named class; quick checks for `C ⊓ D` satisfiability by intersecting pseudo-models.
   - *Completion graph caching* (Konclude's bigger trick): reuse fragments of the completion graph across satisfiability tests.
5. **Ordered branching heuristics:** branch on labels least likely to cause re-work. MOMS-style heuristic from SAT.
6. **Told disjoints / clash precomputation:** build a disjointness oracle from `DisjointClasses` axioms; clash check becomes O(1) bitset intersection.
7. **Index labels with bitsets** when there are < 256 named classes; falls back to sorted SmallVec above.

**Konclude reference:** `Source/Reasoner/Kernel/Cache/` — every file here is an optimization worth understanding. `COptimizedComplexConceptCache`, `CSatisfiableExpanderCacheHandler`, `CReuseCompletionGraphCacheHandler` show what "industrial-strength caching" looks like.

### Phase 5 — Consequence-Based Saturation & Hybrid Orchestration (6-8 weeks)

**Deliverable:** EL fragment classified by saturation; tableau only invoked for non-EL parts.

- Reuse / extend `whelk-rs`'s rule set (Kazakov et al. 2011: 11 completion rules for `ℰℒ⁺⁺`).
- Extend toward `ℰℒℋℛ⁺` (role hierarchies, role chains) — still polynomial.
- Add the **profile checker**: identify which axioms are in EL, which need DL.
- Orchestrator logic for classification of a class `C`:
  1. Run saturation. Get a set of told and derived subsumptions.
  2. For each pair `(C, D)` not decided by saturation, fall back to tableau satisfiability of `C ⊓ ¬D`.
  3. Use the saturation closure as a *guide* (pre-populated labels) when initializing the tableau.

This is the trick that lets Konclude classify SNOMED CT in seconds. **Don't skip the orchestration design**; getting the handoff right between engines is where the wins are.

**Reference:** Steigmiller, Liebig, Glimm. *Konclude: System Description*. Journal of Web Semantics, 2014. Sections 4 and 5 describe the saturation/tableau interface directly.

### Phase 6 — Datatypes & Concrete Domains (4-6 weeks)

- Boolean, numeric (xsd:integer, xsd:decimal, xsd:double), strings (with xsd:pattern).
- Per-datatype satisfiability checkers; integrate into clash detection.
- HermiT's `org.semanticweb.HermiT.datatypes` is a clean blueprint; one Rust module per datatype.

### Phase 7 — ABox, Realization, Queries (8-10 weeks)

- Realization: for each individual, compute the most specific named class(es) it belongs to.
- Instance checking: `KB ⊨ C(a)`?
- Reusable individuals optimization (Konclude's `IndividualReuse`): if many individuals share a "type", classify the type once.
- Conjunctive query answering: cost/benefit decision — likely a Phase 8+ item.

---

## 5. Optimizations: Lessons from HermiT & Konclude

### 5.1 From HermiT (hypertableau, Java)

| Optimization | Where in source | Port to Rust as |
|---|---|---|
| Clausification (hypertableau rewriting) | `tableau.HyperresolutionManager` | Optional Phase 4+ rewrite of preprocessing |
| Anywhere blocking | `blocking.AnywhereBlocking` | Replaces subset blocking when inverses enabled |
| Core blocking (pair-wise direct) | `blocking.PairWiseDirectBlockingChecker` | Required for SHOIQ correctness |
| Told subsumer pre-classification | `hierarchy.DeterministicClassification` | Phase 1 told-subsumer table feeds a Phase 5 fast classifier |
| Branch dependency sets | `tableau.DependencySetFactory` | Bitset over branch points; small (≤256 typically) |
| Existential rule strategy | `tableau.ExistentialExpansionStrategy` | "Strategy" abstraction lets you A/B test heuristics |

### 5.2 From Konclude (tableau + saturation, C++)

| Optimization | Where in source | Port to Rust as |
|---|---|---|
| Saturation as TBox preprocessor | `Reasoner/Saturation/` | Phase 5 — `owl-dl-saturation` crate |
| Cached completion graph reuse | `Reasoner/Kernel/Cache/CReuseCompletionGraph*` | Critical Phase 4 work; persistent data structure with copy-on-write |
| Individual reuse (binary-tree / RBox-aware) | `Reasoner/Realization/COptimizedKPSetRealization` | Phase 7 |
| Pseudo-model merging | `Reasoner/Kernel/Cache/CCompletionGraphCachedModelMerging*` | Phase 4 |
| Parallel task scheduling | `Reasoner/Taskings/`, `Concurrent/` | Use `rayon` and `tokio` (avoid Qt's QThreadPool model; Rust idiom is different) |
| Nominal schema handling | `Reasoner/Kernel/Process/CIndividualProcessNominalSchema*` | Phase 3 nominals + later for SROIQV |
| OWLlink server interface | `Network/` | Skip until Phase 8; provide CLI first |

### 5.3 Rust-specific performance principles

- **Indices over pointers:** `ConceptId(u32)` beats `Arc<Concept>` for both speed and predictable memory.
- **Arena allocation:** tableau nodes live for the duration of one satisfiability test; allocate them in a `bumpalo::Bump` that's dropped wholesale at the end. No per-node Drop chains.
- **Sorted SmallVec for labels:** `SmallVec<[ConceptId; 16]>` plus invariant "always sorted" gives fast subset checks without HashSet overhead.
- **Bitsets for small universes:** when class count is small (< 1024), label sets and dependency sets become single `u128` or `bitvec::BitVec` operations.
- **Avoid `String`:** all IRIs are interned to `u32` IDs at parse time. The original `Arc<str>` lives in a side table accessed only for I/O.
- **Profile early.** Add `cargo flamegraph` to xtask. Profile after Phase 2 and Phase 3 before committing to optimization design.

---

## 6. Benchmarking Strategy

### 6.1 Corpora

| Corpus | Tests | Source |
|---|---|---|
| **ORE 2015 Live** | DL classification, EL classification, DL consistency, DL realisation | Manchester ORE 2015 repository |
| **ORE 2015 User** | Same tracks, harder ontologies | Same |
| **LUBM 1/10/100/1000** | ABox query answering, scaling | SWAT Projects, Lehigh |
| **UOBM** | ABox + more DL constructs | University Ontology Benchmark |
| **SNOMED CT** | Large EL classification | UMLS license required; substitute with public versions if needed |
| **GALEN** | DL classification, historically hard | Public |
| **FMA, NCI, GO** | Real-world biomedical, mixed expressiveness | OBO Foundry |
| **DL 2017 Workshop benchmarks** | Modular reasoning, atomic decomposition | Workshop website |

### 6.2 Comparison reasoners

| Reasoner | Language | Algorithm | Status | Role |
|---|---|---|---|---|
| HermiT | Java | Hypertableau | Stable, low activity | Primary correctness oracle |
| Konclude | C++ | Tableau + saturation | Active, perf leader | Primary perf baseline |
| Openllet / Pellet | Java | Tableau | Maintained fork | Secondary correctness oracle |
| FaCT++ | C++ | Tableau | Effectively retired (2018) | Historical baseline |
| ELK | Java | Consequence-based, EL only | Active | EL fragment perf baseline |
| Whelk (Scala) | Scala | Consequence-based, EL only | Active | EL fragment perf baseline |
| whelk-rs | Rust | Consequence-based, EL only | Foundation we extend | EL fragment perf baseline |
| JFact | Java | FaCT++ port | Maintenance only | Tertiary |

### 6.3 Metrics

- **Correctness:** classification result identical to consensus of HermiT + Konclude + Openllet (majority vote on disagreement).
- **Wall-clock time:** classification, consistency, realisation. Median of 5 runs after 1 warmup.
- **Peak RSS:** memory ceiling.
- **Robustness:** % of corpus completed within 60s, 300s, 1800s timeouts.
- **Throughput:** ontologies/hour for batch reasoning.

### 6.4 Continuous benchmarking

- Nightly job runs the full suite on a fixed cloud VM (avoid laptop variance).
- Results posted to a dashboard; flag regressions > 10%.
- Per-PR microbenchmarks via `criterion` for hot paths (label intersection, clash detection, blocking check).

### 6.5 Reporting

Report results in the ORE format (one row per (ontology, reasoner, task), columns for time/result/correctness). This makes results directly comparable to published ORE numbers.

---

## 7. Testing Strategy

Layered defense:

1. **Unit tests** for every preprocessing transformation, every tableau rule, every normalization. Aim for 80%+ branch coverage in `owl-dl-core`.
2. **Property tests** (proptest):
   - Random ALC concept → NNF → idempotent.
   - Random tableau state → applying a rule preserves satisfiability semantics.
   - Random TBox → saturation produces a subset of the true subsumption graph (soundness).
3. **Integration tests** against HermiT and Konclude as black-box oracles. Disagreement is a P0 bug.
4. **Differential fuzzing:** random small ontologies (≤ 20 axioms) generated by `proptest`, classified by all three reasoners, any disagreement filed as a fuzz finding. This finds the long-tail correctness bugs.
5. **Regression corpus:** every bug that ships gets a permanent ontology test case.

---

## 8. Risk Register & Mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| Tableau correctness with inverses + nominals + QNRs | High | Extensive property testing; differential fuzz against HermiT; treat anywhere blocking as the highest-scrutiny module |
| Performance plateau far behind Konclude | Medium-High | Phased optimization with measurable targets per phase; don't move on until phase target is met |
| Datatype reasoning rabbit holes (xsd:dateTime, regex over Unicode) | Medium | Defer complex datatypes; ship integer + string-equality first |
| Project velocity — DL expertise is rare | High | Build in public from day 1; recruit collaborators from DL community (CADE, DL workshop) |
| Specification drift (OWL 2 errata, datatypes profile updates) | Low | Pin to OWL 2 W3C Rec 2012 + Direct Semantics; track errata explicitly |
| Memory blow-up on large ABox | Medium | ABox is Phase 7; design around individual reuse from the start of Phase 7 |

---

## 9. First 30 Days: Concrete Tasks for Claude Code

Hand this list directly to Claude Code; each item is sized for an autonomous session.

1. **Day 1-2:** Create Cargo workspace per §3.3. Pin `horned-owl = "0.16"` (or latest), `whelk-rs` git dep. Add `clippy`, `rustfmt`, `cargo-deny` config. CI on GitHub Actions: build + test + clippy + fmt-check on Linux + macOS + Windows.

2. **Day 3-5:** Write `owl-dl-core::ir::Concept` enum (see §4 Phase 0) and `ConceptPool` arena with structural sharing. Property test: every two equal concept expressions get the same `ConceptId`.

3. **Day 6-8:** Implement `Role`, `RoleId`, `RoleHierarchy` (sub-role closure). Tests against canonical role-hierarchy examples.

4. **Day 9-12:** `From<horned_owl::ontology::SetOntology> for InternalOntology` — convert horned-owl axioms to internal IR. Round-trip property test: convert → convert back → identical axiom set (modulo normalization).

5. **Day 13-16:** NNF transformation. Property test: nnf(nnf(c)) == nnf(c); nnf(c) has Not only at atom leaves.

6. **Day 17-20:** Told subsumers + told disjoints. Build a directed graph; transitive closure (small graph, naive is fine). Add API `told_subsumers(class_id) -> &[ClassId]`.

7. **Day 21-25:** Benchmark harness skeleton — `owl-dl-bench` binary that takes a directory of ontologies, runs a (currently trivial) reasoner, records timing/memory to a JSONL file, generates a Markdown summary. Wire HermiT and (if installable) Konclude as subprocess oracles.

8. **Day 26-30:** Fetch the ORE 2015 Live corpus (`xtask fetch-ore-2015`). Run the harness; produce a baseline report showing "parse + normalize + classify (trivial)" times for all ontologies. This is the first dashboard.

End-of-month deliverable: a workspace that parses every ORE 2015 ontology, normalizes it, produces a told-subsumer hierarchy, and has a working benchmark dashboard. No reasoning yet, but the infrastructure for everything later is in place and tested.

---

## 10. Reading List

**Books**
- Baader, Calvanese, McGuinness, Nardi, Patel-Schneider. *The Description Logic Handbook* (2nd ed., 2007). Chapter 2 for tableau; Chapter 9 for optimizations.

**Foundational papers**
- Horrocks, Sattler, Tobies. *Practical Reasoning for Very Expressive Description Logics.* Logic Journal of the IGPL, 2000.
- Horrocks, Sattler. *A Tableau Decision Procedure for SHOIQ.* IJCAI 2005.
- Motik, Shearer, Horrocks. *Hypertableau Reasoning for Description Logics.* JAIR 2009. (HermiT)
- Steigmiller, Liebig, Glimm. *Konclude: System Description.* Journal of Web Semantics, 2014.
- Kazakov, Krötzsch, Simančík. *The Incredible ELK: From Polynomial Procedures to Efficient Reasoning with EL Ontologies.* Journal of Automated Reasoning, 2014.
- Glimm, Horrocks, Motik, Stoilos, Wang. *HermiT: An OWL 2 Reasoner.* Journal of Automated Reasoning, 2014.
- Matentzoglu, Parsia, Gonçalves, Glimm, Steigmiller. *The OWL Reasoner Evaluation (ORE) 2015 Competition Report.* Journal of Automated Reasoning, 2017.

**Rust ecosystem**
- Lord et al. *Horned-OWL: Flying Further and Faster with Ontologies.* TGDK, 2024. (Background on the foundation we build on.)

---

## 11. Open Questions to Resolve Early

1. License choice — LGPL aligns with HermiT/Konclude precedent and OWL community norms. Apache 2.0 lowers friction for industry adoption. Decide before Phase 2 lands.
2. Public API style — mirror the OWL API for familiarity, or design idiomatic Rust API with optional OWL-API-style wrapper? Recommend the latter.
3. OWLlink server vs. native protocol — defer; not on the critical path.
4. SWRL support — out of scope for v1; revisit after SROIQ is stable.
5. Incremental reasoning — out of scope for v1; the data structures should not foreclose it (favour persistent / copy-on-write where cheap).

---

*End of strategy. This document is intended as a living plan; revise after each phase based on what the benchmarks actually show.*
