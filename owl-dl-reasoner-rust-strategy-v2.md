# Building an OWL DL Reasoner in Rust: Implementation Strategy (v2)

A phased, hand-offable plan for Claude Code (or any engineering team) targeting parity with HermiT/Konclude on standard OWL 2 DL benchmarks.

---

## 0. Changes from v1

This revision adjusts six things in the v1 strategy. The architecture, target benchmarks, and overall risk frame are unchanged.

1. **License chosen up front: `Apache-2.0 OR MIT`** (Rust ecosystem norm). Retroactive relicensing is painful; deciding now removes future friction.
2. **Phase ordering swap.** Optimization stack (old Phase 4) is pulled forward and runs against ALCHIQ *before* nominals are added. Nominals and full SROIQ now sit in a new Phase 5. Debugging nominal interactions on an unoptimized tableau is rough; doing it on a fast, well-instrumented base is much easier.
3. **Saturation engine reframed as "build, with whelk-rs as reference," not "extend whelk-rs."** Sharing an IR across an EL completion engine and a tableau engine is a re-implementation in practice. Budget and scope adjusted.
4. **Minimal datatype slice pulled into Phase 3.** A subset of `xsd:integer`, `xsd:string` equality, and `xsd:boolean` unlocks differential testing on real-world ontologies much earlier. Full concrete domains stay in Phase 7.
5. **30-day plan re-scoped.** Oracle harness wiring (HermiT and Konclude as subprocesses) extends into month 2; month 1 ends with a passing parse-and-normalize dashboard.
6. **Testing weight rebalanced toward differential fuzzing.** Random-small-ontology cross-checks against HermiT find more real bugs than property tests over tableau rule semantics. Both stay, but the emphasis flips.

---

## 1. Executive Summary

**Goal.** A sound, complete, performant OWL 2 DL reasoner (SROIQ(D)) in Rust. Competitive with HermiT (Java, hypertableau) and Konclude (C++, tableau + saturation hybrid) on ORE benchmarks within 24–36 months of focused work.

**Approach.** Konclude-style hybrid: a consequence-based saturation engine handles the EL-ish subset cheaply; a tableau engine handles the rest. This empirically beats pure tableau on real-world ontologies (mostly EL with a little DL) and is the architecture that has won most ORE competitions.

**Build on, don't rebuild:** `horned-owl` (parsing + model, 20–40× faster than the Java OWL API) is a foundation we depend on. `whelk-rs` (EL reasoner) is a *reference implementation* whose rule structure we mirror in our own IR-aware engine; see §4 Phase 6 for why we don't simply extend it.

**Risk frame.** Tableau optimizations are 25 years of accumulated tricks; production parity is multi-year. The phased plan delivers a *correct* ALC reasoner in ~3 months, a *correct* ALCHIQ reasoner with an optimized tableau in ~12 months, and a competitive SROIQ(D) reasoner in 24–36 months. Useful artifacts ship at every phase.

---

## 2. Foundation: What Exists, What's Missing

### 2.1 Existing Rust crates to depend on

| Crate | Role | Status |
|---|---|---|
| `horned-owl` | OWL model, parsers for OWL/XML, OWL/RDF, functional syntax | Production-quality, actively maintained |
| `whelk-rs` | EL reasoning (Kazakov et al. consequence-based rules) | Working; **reference implementation** for our saturation engine, not a direct dependency in the hot loop |
| `rio` / `oxigraph` | RDF parsing | Production-quality |
| `petgraph` | General graph manipulation | Class hierarchy only; *not* for the completion graph |
| `rayon` | Data parallelism | Parallel classification |
| `dashmap`, `hashbrown` | Fast concurrent / sequential hash maps | Caches |
| `bumpalo` | Arena allocation | Tableau nodes, transient concept expressions |
| `bitvec` | Compact bitsets | Label sets, dependency sets when class count < 1024 |
| `smallvec` | Small-buffer-optimized vectors | Sorted concept-label lists |
| `criterion` | Microbenchmarks | Per-PR perf regression tracking |
| `proptest` | Property + differential fuzzing | Critical for tableau correctness |

### 2.2 What we must build

1. Internal IR for concepts/roles/axioms in normalized form (DAG with structural sharing).
2. Preprocessing: structural transformation, absorption, lazy unfolding.
3. Consequence-based saturation engine over our IR (whelk-rs is the algorithmic reference).
4. Tableau engine with full SROIQ rule set and optimizations.
5. Hybrid orchestrator: decide per-query whether saturation suffices.
6. Classification, realization, entailment, query interfaces.
7. Minimal datatype reasoner first (booleans, integer ranges, string equality); full concrete domains later.
8. Benchmark harness against ORE corpus, with results checked against HermiT/Konclude.

---

## 3. Architectural Strategy

### 3.1 Why hybrid, not pure tableau

Pure tableau (HermiT, FaCT++, Pellet) handles hard logical content well but pays heavy constant overhead on the large, mostly-EL terminologies that dominate real-world OWL use (SNOMED CT, NCI, GO, FMA). Konclude's hybrid wins ORE because it handles those terminologies at saturation-time and only invokes tableau where genuinely needed. Replicating this design is the single most important architectural choice.

### 3.2 Tableau flavor: standard vs. hypertableau

Start with standard tableau plus the full optimization stack (absorption, dependency-directed backtracking, semantic branching, caching). Hypertableau-style rewriting is optional in Phase 7+ if benchmarks show it helps. Standard tableau is far better documented, easier to debug, and the optimization stack is what actually closes the gap to competitive performance.

### 3.3 Top-level module layout

```
crates/
  owl-dl-core/        # IR, normalization, common utilities
  owl-dl-saturation/  # Consequence-based engine over our IR
  owl-dl-tableau/     # Tableau engine
  owl-dl-datatypes/   # Datatype reasoners (separate crate so it can grow)
  owl-dl-reasoner/    # Hybrid orchestrator + public API
  owl-dl-cli/         # Command-line tool (OWLlink protocol later)
  owl-dl-bench/       # Benchmark harness
xtask/                # Build automation, corpus fetch, oracle install
ontologies/           # Test ontology corpus (gitignored, fetched by xtask)
```

### 3.4 Licensing decided up front

Dual `Apache-2.0 OR MIT`, the Rust-ecosystem default. Add a `LICENSE-APACHE`, `LICENSE-MIT`, and `licenses/` README on day 1. This avoids LGPL's linking ambiguities and is the minimum-friction choice for both academic citations and industrial adoption. Revisit only if a hard dependency forces it.

---

## 4. Phased Implementation Plan

Each phase ends with a *shippable* artifact and a passing benchmark slice.

### Phase 0 — Foundation (2–3 weeks)

**Deliverable:** Workspace scaffold; `horned-owl` integration; internal IR; passing ALC normalization tests.

- Cargo workspace per §3.3.
- Internal IR `Concept` enum: `Top, Bot, Atomic(ClassId), And, Or, Not, Some(Role, Concept), All(Role, Concept), Min(n, Role, Concept), Max(n, Role, Concept), Nominal(IndId), Self(Role)`.
- `ClassId`, `RoleId`, `IndId` as `u32` indices into interned tables — no IRIs in the hot loop.
- Concept expressions in a **DAG with structural sharing**: a `ConceptPool` arena, each unique sub-expression interned to one `ConceptId(u32)`. Roughly: `HashMap<ConceptExpr, ConceptId>` plus `Vec<ConceptExpr>`. **The single biggest performance lever; do not skip it.**
- Conversion from `horned_owl::model::Component` into the internal IR.
- A test harness that loads ORE corpus ontologies and produces summary stats (axiom counts, expressiveness profile).

**Why this matters:** Concept-equality checks must be O(1) pointer comparison, not structural. Every later phase assumes this.

### Phase 1 — Preprocessing & Normalization (4–6 weeks)

**Deliverable:** A normalizer that produces (i) NNF, (ii) absorbed TBox, (iii) precomputed told-subsumer table.

- Negation Normal Form: push `Not` to atoms.
- Structural transformation: replace complex concepts with fresh names where useful (Tseitin-style).
- **Absorption** — three flavors:
  - *Binary absorption:* `⊤ ⊑ ¬A ⊔ B ⊔ ¬C` → `A ⊓ C ⊑ B`.
  - *Role absorption:* `⊤ ⊑ ∀R.C` becomes a rule "when an R-edge is added, propagate C to the target."
  - *Concept absorption:* nominal absorption for `{a} ⊑ C`.
- Lazy unfolding: store definitions `A ≡ C` separately; expand only when needed during tableau.
- Told subsumers: traverse axioms to build the trivial subsumption graph; pre-compute its transitive closure.

**HermiT reference:** `org.semanticweb.HermiT.structural` — particularly `OWLAxioms`, `ObjectPropertyInclusionManager`, `BuiltInPropertyManager`. `Clausification` shows hypertableau-style rewriting; read for context, defer using it.

**Konclude reference:** `Source/Reasoner/Preprocess/` — `CConceptNormalizer`, `CAbsorber`, `CTBoxBinaryAbsorberPreProcessor`, `CTBoxRoleAbsorberPreProcessor`. The clearest exposition of which absorptions to do in what order.

### Phase 2 — ALC Tableau (8–10 weeks)

**Deliverable:** A correct (but unoptimized) ALC satisfiability checker, validated against HermiT on small ontologies.

- Completion graph:
  - `Node { id: NodeId, labels: SmallVec<[ConceptId; 8]>, edges: SmallVec<[(RoleId, NodeId); 4]> }`.
  - Global `Vec<Node>`, addressable by `NodeId(u32)`. `labels` and `edges` sorted for fast subset checking.
- Six core expansion rules: `⊓`, `⊔`, `∃`, `∀`, `⊑` (apply GCIs), clash check.
- Naive subset blocking.
- DFS with backtracking using a log-and-undo `TableauTrail`. Persistent vectors are an alternative; the trail approach is faster in practice.

**Test oracle:** For every ontology in the test set, run HermiT in a subprocess and compare classification results. Disagreement blocks merging. Konclude is added as a second oracle once Phase 5 oracle harness work lands.

**Source to study:** universome/dl-reasoner (an ALCQ tableau in Rust) as a shape reference, not a quality bar. HermiT's `org.semanticweb.HermiT.tableau.Tableau` is the heavyweight model — `runCalculus()` and the `*Manager` classes (`ExistentialExpansionManager`, `DisjunctionManager`, `MergingManager`).

### Phase 3 — Extensions to ALCHIQ + Minimal Datatypes (8–10 weeks)

Adds the *non-nominal* SROIQ letters plus a minimal datatype layer that unlocks differential testing on more of the ORE corpus.

- **H (role hierarchies):** sub-role axioms; closure during preprocessing.
- **S (transitive roles):** `∀R.C` propagates through R-chains when R is transitive. Direct rule first; automaton-based handling deferred to Phase 5 with R.
- **I (inverse roles):** edges become bidirectional with role labels in both directions; affects blocking — see Phase 4 for the move to anywhere pair-wise blocking.
- **Q (qualified number restrictions):** `≥ n R.C`, `≤ n R.C`. Merging and clash detection get harder; this is where most ALCHIQ correctness bugs live.
- **F (functional roles):** trivial special case of `≤ 1 R.⊤`.
- **Minimal datatype slice (separate crate, `owl-dl-datatypes`):**
  - `xsd:boolean`.
  - `xsd:integer` with min/max facet constraints (open interval reasoning).
  - `xsd:string` equality only — no regex, no length facets, no language tags.
  - Per-datatype satisfiability check; integrate into clash detection.
- Differential testing against HermiT extended to any ORE ontology whose datatype usage falls within the supported slice. This is the cohort the next phase tunes against.

**Why datatypes now, not later:** A meaningful slice of ORE 2015 Live uses integer-range axioms. Without minimal support, those ontologies are skipped in differential testing and bugs accumulate undetected.

### Phase 4 — Tableau Optimizations on ALCHIQ (10–12 weeks)

**This is where most of the speedup lives.** Doing it before nominals means nominal bugs surface on a fast, instrumented engine — much easier to localize than on an unoptimized one.

Implement in roughly this priority order:

1. **Anywhere pair-wise blocking** (Motik & Horrocks). Subset blocking is unsound with inverses; this replaces it. Highest correctness scrutiny in the project. Property-test against HermiT exhaustively before moving on. HermiT references: `blocking.AnywhereBlocking`, `blocking.PairWiseDirectBlockingChecker`.
2. **Dependency-directed backtracking (backjumping):** track which branching choices each label depends on; jump past irrelevant choices on clash. Bitset over branch points (≤ 256 typical) per HermiT's `tableau.DependencySetFactory`.
3. **Lazy unfolding** wired into the tableau loop (preprocessing already produced the unfolded definitions in Phase 1).
4. **Semantic branching:** when expanding `C ⊔ D`, on the second branch add both `¬C` and `D`. Avoids re-exploring `C`-models.
5. **Caching:**
   - *Subsumption cache:* `(C, D) → bool`.
   - *Model cache (pseudo-model):* compressed satisfying tableau for each named class; intersect for quick `C ⊓ D` checks.
   - *Completion graph caching* (Konclude's bigger trick): reuse fragments of the completion graph across satisfiability tests. **Use persistent / copy-on-write data structures here so incremental classification remains a future possibility** (see §11.5).
6. **Ordered branching heuristics:** MOMS-style from SAT.
7. **Told disjoints / clash precomputation:** O(1) bitset intersection.
8. **Bitset label indexing** for ontologies with < 1024 named classes; fall back to sorted `SmallVec` above.

**Konclude reference:** `Source/Reasoner/Kernel/Cache/` — `COptimizedComplexConceptCache`, `CSatisfiableExpanderCacheHandler`, `CReuseCompletionGraphCacheHandler`. Industrial-strength caching, well worth a multi-day study session.

**Phase exit criterion:** ALCHIQ classification on the EL-free subset of ORE 2015 within 10× of Konclude wall-clock on the same hardware. If we can't hit that, the optimization plan is wrong; do not move on.

### Phase 5 — Nominals (O), Complex Role Hierarchies (R), Full SROIQ (8–10 weeks)

Now we add the two SROIQ features that interact most painfully with the rest of the engine — on an already-fast, already-trusted base.

- **R (complex role hierarchies):** general `R₁ ∘ ... ∘ Rₙ ⊑ S`. Implement via finite-state automata over role names per Horrocks & Sattler. Substantial work; budget ~3 weeks alone.
- **O (nominals):** `{a}` concepts. Triggers individual merging. Major source of bugs — allocate test time generously. HermiT: `tableau.MergingManager`. Konclude: `Reasoner/Kernel/Process/CIndividualProcessNode`, `CBackendRepresentativeMemoryCache`, the `Process*Task` family. Konclude's individual-reuse machinery deserves its own study session.
- Re-validate every Phase 2–4 invariant. Differential fuzzing budget doubles for the duration of this phase.

### Phase 6 — Consequence-Based Saturation & Hybrid Orchestration (8–10 weeks)

**Deliverable:** EL fragment classified by saturation; tableau only invoked for non-EL parts.

- **Build a saturation engine over our IR.** Kazakov et al.'s 11 completion rules for `ℰℒ⁺⁺` are the algorithm. `whelk-rs` is the working reference; we **do not** import whelk-rs as a hot-path dependency because its IR is its own — copying axioms across IR boundaries on every call would defeat the structural-sharing wins from Phase 0. Expected effort comparable to a fresh implementation that happens to be guided by a known-correct prior art.
- Extend the rule set toward `ℰℒℋℛ⁺` (role hierarchies, role chains) — still polynomial.
- **Profile checker:** identify which axioms fall in EL, which need DL. Important: a single non-EL axiom does *not* contaminate the whole ontology — many subsumptions can still be settled by saturation. The orchestrator's job is to be precise about this.
- Orchestrator logic for classification of a class `C`:
  1. Run saturation. Get told and derived subsumptions.
  2. For each pair `(C, D)` not decided by saturation, fall back to tableau satisfiability of `C ⊓ ¬D`.
  3. Use the saturation closure as a *guide* — pre-populated labels when initializing the tableau.

This is the trick that lets Konclude classify SNOMED CT in seconds. **Don't skip the orchestration design**; the handoff between engines is where the wins are.

**Reference:** Steigmiller, Liebig, Glimm. *Konclude: System Description.* Journal of Web Semantics, 2014. Sections 4–5 describe the saturation/tableau interface directly.

**Phase exit criterion:** SNOMED CT (or a publicly available substitute of comparable size) classifies in under 5× Konclude time.

### Phase 7 — Full Datatypes & Concrete Domains (4–6 weeks)

- Numeric: `xsd:decimal`, `xsd:double`, full `xsd:integer` facets.
- Strings: `xsd:pattern` regex over Unicode, length facets, language tags.
- Date/time: `xsd:dateTime`, `xsd:date`.
- Per-datatype satisfiability checkers; integrate into clash detection.
- HermiT's `org.semanticweb.HermiT.datatypes` is a clean blueprint — one Rust module per datatype.

### Phase 8 — ABox, Realization, Queries (8–10 weeks)

- Realization: for each individual, compute the most specific named class(es).
- Instance checking: `KB ⊨ C(a)`?
- Reusable-individuals optimization (Konclude's `IndividualReuse`).
- Conjunctive query answering: cost/benefit decision; likely Phase 9+.

### Phase 9 — Optional: Hypertableau Rewriting, OWLlink Server

Defer unless benchmarks demand it.

---

## 5. Optimizations: Lessons from HermiT & Konclude

### 5.1 From HermiT (hypertableau, Java)

| Optimization | Where in source | Port to Rust as |
|---|---|---|
| Clausification (hypertableau rewriting) | `tableau.HyperresolutionManager` | Phase 9 optional |
| Anywhere blocking | `blocking.AnywhereBlocking` | Phase 4 — replaces subset blocking once inverses enabled |
| Core blocking (pair-wise direct) | `blocking.PairWiseDirectBlockingChecker` | Phase 4 — required for SHOIQ correctness |
| Told subsumer pre-classification | `hierarchy.DeterministicClassification` | Phase 1 → Phase 6 fast classifier |
| Branch dependency sets | `tableau.DependencySetFactory` | Phase 4 — bitset over branch points |
| Existential rule strategy | `tableau.ExistentialExpansionStrategy` | Strategy trait for A/B-testable heuristics |

### 5.2 From Konclude (tableau + saturation, C++)

| Optimization | Where in source | Port to Rust as |
|---|---|---|
| Saturation as TBox preprocessor | `Reasoner/Saturation/` | Phase 6 — `owl-dl-saturation` crate |
| Cached completion graph reuse | `Reasoner/Kernel/Cache/CReuseCompletionGraph*` | Phase 4 — persistent / copy-on-write |
| Individual reuse (binary-tree / RBox-aware) | `Reasoner/Realization/COptimizedKPSetRealization` | Phase 8 |
| Pseudo-model merging | `Reasoner/Kernel/Cache/CCompletionGraphCachedModelMerging*` | Phase 4 |
| Parallel task scheduling | `Reasoner/Taskings/`, `Concurrent/` | `rayon` + `tokio`; avoid the Qt QThreadPool model |
| Nominal schema handling | `Reasoner/Kernel/Process/CIndividualProcessNominalSchema*` | Phase 5 |
| OWLlink server interface | `Network/` | Phase 9 |

### 5.3 Rust-specific performance principles

- **Indices over pointers:** `ConceptId(u32)` beats `Arc<Concept>` for both speed and predictable memory.
- **Arena allocation:** tableau nodes live for the duration of one satisfiability test; allocate in a `bumpalo::Bump` dropped wholesale at the end. No per-node `Drop` chains.
- **Sorted SmallVec for labels:** `SmallVec<[ConceptId; 16]>` + "always sorted" invariant gives fast subset checks without HashSet overhead.
- **Bitsets for small universes:** when class count is small, label and dependency sets become single `u128` or `bitvec::BitVec` ops.
- **No `String` in the hot loop:** IRIs interned to `u32` at parse time; the original `Arc<str>` lives in a side table touched only at I/O boundaries.
- **Persistent / copy-on-write data structures in the cache layer** so incremental classification (§11.5) stays open.
- **Profile early.** `cargo flamegraph` in xtask. Profile after Phase 2 and Phase 4 before committing to optimization design changes.

---

## 6. Benchmarking Strategy

### 6.1 Corpora

| Corpus | Tests | Source |
|---|---|---|
| **ORE 2015 Live** | DL classification, EL classification, DL consistency, DL realisation | Manchester ORE 2015 repository |
| **ORE 2015 User** | Same tracks, harder ontologies | Same |
| **LUBM 1/10/100/1000** | ABox query answering, scaling | SWAT Projects, Lehigh |
| **UOBM** | ABox + more DL constructs | University Ontology Benchmark |
| **SNOMED CT** | Large EL classification | UMLS license; substitute with public versions if needed |
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
| ELK | Java | Consequence-based, EL only | Active | EL perf baseline |
| Whelk (Scala) | Scala | Consequence-based, EL only | Active | EL perf baseline |
| whelk-rs | Rust | Consequence-based, EL only | Algorithmic reference for Phase 6 | EL perf baseline |
| JFact | Java | FaCT++ port | Maintenance only | Tertiary |

### 6.3 Metrics

- **Correctness:** classification result identical to consensus of HermiT + Konclude + Openllet (majority vote on disagreement).
- **Wall-clock:** classification, consistency, realisation. Median of 5 runs after 1 warmup.
- **Peak RSS:** memory ceiling.
- **Robustness:** % of corpus completed within 60s, 300s, 1800s timeouts.
- **Throughput:** ontologies/hour for batch reasoning.

### 6.4 Continuous benchmarking

- Nightly job on a fixed cloud VM (avoid laptop variance).
- Dashboard; flag regressions > 10%.
- Per-PR microbenchmarks via `criterion` for hot paths (label intersection, clash detection, blocking check).

### 6.5 Reporting

Output in ORE format (one row per (ontology, reasoner, task), columns for time / result / correctness). Directly comparable to published ORE numbers.

---

## 7. Testing Strategy

Layered defense, weighted by what historically catches bugs:

1. **Differential fuzzing (primary).** `proptest`-generated small ontologies (≤ 20 axioms) classified by our reasoner, HermiT, and Konclude. Any disagreement is a P0 bug with the offending ontology saved to a permanent regression corpus. This finds the long-tail correctness bugs faster than any other technique.
2. **Integration tests against HermiT/Konclude as black-box oracles** on the curated ORE corpus.
3. **Unit tests** for every preprocessing transformation, every tableau rule, every normalization. Target 80%+ branch coverage in `owl-dl-core`.
4. **Property tests (proptest):**
   - Random ALC concept → NNF → idempotent.
   - Random TBox → saturation output ⊆ true subsumption graph (soundness; checked against HermiT).
   - Property-testing tableau rule semantics directly is hard to formalize without a reference semantics; lean on differential fuzz (#1) instead.
5. **Regression corpus:** every bug that ships gets a permanent ontology test case.

---

## 8. Risk Register & Mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| Tableau correctness with inverses + nominals + QNRs | High | Phase 4 hardens optimizations *before* Phase 5 adds nominals; extensive differential fuzz against HermiT; anywhere blocking treated as the highest-scrutiny module |
| Performance plateau far behind Konclude | Medium-High | Phase exit criteria with measurable targets (Phase 4: 10× Konclude on EL-free ORE; Phase 6: 5× Konclude on SNOMED); do not advance until met |
| Datatype reasoning rabbit holes (`xsd:dateTime`, regex over Unicode) | Medium | Minimal slice in Phase 3 unlocks differential testing; full concrete domains deferred to Phase 7 |
| Project velocity — DL expertise is rare | High | Build in public from day 1; weekly status thread on the DL workshop mailing list; co-author Phase 1 normalization with at least one external DL researcher before Phase 2 begins; treat sole-maintainer status as itself a risk to be retired |
| Specification drift (OWL 2 errata, datatypes profile updates) | Low | Pin to OWL 2 W3C Rec 2012 + Direct Semantics; track errata explicitly in `docs/spec-deltas.md` |
| Memory blow-up on large ABox | Medium | ABox is Phase 8; design around individual reuse from the start of that phase |
| Re-implementing saturation takes longer than budgeted | Medium | Phase 6 has 8–10 weeks (vs. v1's 6–8); whelk-rs is read as a working reference, so the algorithm is not in doubt — only the IR integration |

---

## 9. First 30 Days: Concrete Tasks for Claude Code

Hand this list directly to Claude Code; each item is sized for an autonomous session. Month 1 deliberately stops short of full oracle integration — that lands in month 2.

1. **Day 1–2:** Create Cargo workspace per §3.3. Pin `horned-owl = "0.16"` (or latest), drop the `whelk-rs` hot-path dependency, keep a `dev-dependencies` link for reference benchmarks. Add `clippy`, `rustfmt`, `cargo-deny` config. CI on GitHub Actions: build + test + clippy + fmt-check on Linux + macOS + Windows. License files (`Apache-2.0 OR MIT`) committed.

2. **Day 3–5:** `owl-dl-core::ir::Concept` enum (see §4 Phase 0) and `ConceptPool` arena with structural sharing. Property test: every two equal concept expressions get the same `ConceptId`.

3. **Day 6–8:** `Role`, `RoleId`, `RoleHierarchy` (sub-role closure). Tests against canonical role-hierarchy examples.

4. **Day 9–12:** `From<horned_owl::ontology::SetOntology> for InternalOntology` — convert horned-owl axioms to internal IR. Round-trip property test: convert → convert back → identical axiom set (modulo normalization).

5. **Day 13–16:** NNF transformation. Property test: `nnf(nnf(c)) == nnf(c)`; `nnf(c)` has `Not` only at atom leaves.

6. **Day 17–20:** Told subsumers + told disjoints. Build a directed graph; transitive closure (small graph, naive is fine). Public API `told_subsumers(class_id) -> &[ClassId]`.

7. **Day 21–25:** Benchmark harness skeleton — `owl-dl-bench` binary that takes a directory of ontologies, runs the current (trivial) reasoner, records timing/memory to JSONL, generates a Markdown summary. **Scope cap:** no external oracle integration this week — wiring HermiT and Konclude as subprocesses is month 2 work.

8. **Day 26–30:** Fetch the ORE 2015 Live corpus (`xtask fetch-ore-2015`). Run the harness; produce a baseline report showing "parse + normalize + told-subsumer" times for all ontologies. This is the first dashboard.

**End-of-month deliverable:** a workspace that parses every ORE 2015 ontology, normalizes it, produces a told-subsumer hierarchy, and has a working benchmark dashboard. No reasoning yet, but the infrastructure for everything later is in place and tested. Month 2 opens with HermiT-as-subprocess oracle integration and the first real differential tests.

---

## 10. Reading List

**Books**
- Baader, Calvanese, McGuinness, Nardi, Patel-Schneider. *The Description Logic Handbook* (2nd ed., 2007). Chapter 2 (tableau), Chapter 9 (optimizations).

**Foundational papers**
- Horrocks, Sattler, Tobies. *Practical Reasoning for Very Expressive Description Logics.* Logic Journal of the IGPL, 2000.
- Horrocks, Sattler. *A Tableau Decision Procedure for SHOIQ.* IJCAI 2005.
- Motik, Shearer, Horrocks. *Hypertableau Reasoning for Description Logics.* JAIR 2009. (HermiT)
- Steigmiller, Liebig, Glimm. *Konclude: System Description.* Journal of Web Semantics, 2014.
- Kazakov, Krötzsch, Simančík. *The Incredible ELK: From Polynomial Procedures to Efficient Reasoning with EL Ontologies.* JAR, 2014.
- Glimm, Horrocks, Motik, Stoilos, Wang. *HermiT: An OWL 2 Reasoner.* JAR, 2014.
- Matentzoglu, Parsia, Gonçalves, Glimm, Steigmiller. *The OWL Reasoner Evaluation (ORE) 2015 Competition Report.* JAR, 2017.

**Rust ecosystem**
- Lord et al. *Horned-OWL: Flying Further and Faster with Ontologies.* TGDK, 2024.

---

## 11. Open Questions to Resolve Early

1. ~~License choice~~ — **Resolved: Apache-2.0 OR MIT.**
2. Public API style — mirror the OWL API for familiarity, or design idiomatic Rust API with optional OWL-API-style wrapper? Recommend the latter; confirm before Phase 8.
3. OWLlink server vs. native protocol — defer to Phase 9.
4. SWRL support — out of scope for v1; revisit after SROIQ is stable.
5. Incremental reasoning — out of scope for v1, but Phase 4 data structures must not foreclose it (persistent / copy-on-write where cheap; see §5.3).
6. **New:** External contributor recruitment cadence. Without a second DL-literate contributor by Phase 4, completion risk rises sharply. Define a recruitment milestone in the project README before Phase 2 ships.

---

*End of strategy (v2). This document is intended as a living plan; revise after each phase based on what the benchmarks actually show.*
