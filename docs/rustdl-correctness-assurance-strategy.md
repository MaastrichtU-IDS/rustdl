# RustDL Correctness Assurance Strategy

Prepared for Michel Dumontier  
26 October 2026

## 1 Assurance decision and evidence boundary

**Recommendation.** Retain the ambition of a sound and complete RustDL procedure for its declared logic. Put a shared, source-bound answer-validation and publication layer above its engines and commands. Establish a reference calculus and complete fallback; require shortcuts to preserve semantics or defer. Alongside the Claude team’s fixes, address shared defect mechanisms with independent evidence and enforceable release gates.

Separate three outcomes: a sound and complete procedure; a resource-limited run returning sound partial results; and an implementation defect that returns a wrong definitive answer. Large ontologies may defeat a practical time or memory budget without invalidating the first ambition. The user accepts an incomplete result containing what has been established; this should be a designed operating mode, not an error-shaped afterthought.

### Current evidence and its implications

**Observed.** At commit `60eca181037c308f25f732da4136883d3c89942b`, re-checked at `42457595292bceee9143c413091d08f74ca9a32c` (31 October 2026), the Rust completeness guard `completeness_guaranteed()` is restricted to PureEl and additional checks. CLI `classify --json` emits that guard as `completeness_guaranteed`. Python exposes no equivalent: its `complete` only tests timed-out pairs, and the other conditions appear as separate getters. Its comments acknowledge that entailments can still be missing. PureEl is the current guarded baseline, not the desired ceiling. These are contract observations, not reproduced failures. [Rust guard](https://github.com/MaastrichtU-IDS/rustdl/blob/42457595292bceee9143c413091d08f74ca9a32c/crates/owl-dl-reasoner/src/classify.rs#L962-L1010), [CLI field](https://github.com/MaastrichtU-IDS/rustdl/blob/42457595292bceee9143c413091d08f74ca9a32c/crates/owl-dl-cli/src/json_out.rs#L131-L147) and [Python result](https://github.com/MaastrichtU-IDS/rustdl/blob/42457595292bceee9143c413091d08f74ca9a32c/crates/owl-dl-py/src/classify.rs#L50-L116).

**Proposed.** Express the intended guarantee as a tuple: **fragment, operation, engine route, input preservation, resource policy and evidence**. The complete fallback must cover its declared domain; shortcuts must prove eligibility or defer. Keep execution completion, semantic answers and completeness eligibility separate throughout Rust, Python, CLI and serialized output.

**Unverified.** No executed tests, exhaustive audit or certification is asserted here. The Claude agent manager quoted by Michel reported repeated defects across engines and commands, beginning with issues #354, #356, #361 and #362. All four are now closed by individual fixes (#365, #366, #375, #378), and the follow-on #376 is closed by #381. The fixes support a shared *obligation* implemented separately per engine (nominal identity and counting under blocking, and individual seeding) rather than a single code defect. Issue #383, still open, shows the classify route still lacks that obligation. Section 3 records the cluster as a mechanism × engine × command table. Keep investigating it as one architectural question, while retaining individual reproducers.

### Decisions to make first

- Approve a capability and guarantee ledger, with a semantic owner. Record the intended full procedure separately from the narrower claims supported by current implementation evidence.
- Require an assurance evidence bundle for every current bug fix: minimal reproducer, semantic expectation, regression, relevant invariant and a nearby adversarial variant.
- Prioritize the independent oracle, complete reference route and stateful harness. Conventional calculus proofs plus implementation traceability support an engineering claim; whole-program machine verification is a stronger optional goal.

## 2 Define the operation contract

Use OWL 2 Direct Semantics with an explicit parser/import policy. Internal “PureEl” and “Horn” labels are not W3C profile conformance. Rust’s current completeness guard requires PureEl, no timed-out pairs, no hierarchy blindness, no refused clauses, no undecided derived individual equalities (added by #365), determined consistency and no dropped axioms; Horn shape alone is insufficient. The derived-equality condition depends on a budget: probing stops after eight consecutive stalled probes, and the remaining candidates count as undecided. That puts the resource policy directly into the guarantee. The guard’s own doc comment lists only part of its conditions. It already omitted refused clauses at the original baseline, and it now also omits undecided derived equalities. This drift between the stated claim and the code is the kind the guarantee ledger should catch. [Rust guard](https://github.com/MaastrichtU-IDS/rustdl/blob/42457595292bceee9143c413091d08f74ca9a32c/crates/owl-dl-reasoner/src/classify.rs#L962-L1010) and [OWL 2 profiles](https://www.w3.org/TR/owl2-profiles/).

| Operation | Meaning of a definitive answer | Required negative or opposite evidence |
| --- | --- | --- |
| Consistency | Consistent means at least one model of the complete ontology. | Inconsistent requires a sound refutation, not failed finite search. |
| Entailment or instance query | Entailed means true in every model of the ontology. | Not entailed needs a countermodel, or a complete decision procedure within its proved domain. |
| Subsumption and classification | Every returned relation is entailed; completeness applies to a named query universe. | A missing edge is not non-entailment unless the relevant completeness claim holds. |
| Class satisfiability | Satisfiable requires a model in which the queried class has an instance. | Unsatisfiable requires a sound refutation of that obligation. |

A model of the ontology establishes consistency, not entailment. For a non-entailment result, the model must also falsify the query. A refutation-based entailment proof must justify the operation-specific reduction to ontology plus negated query; not every query negation is expressible as one OWL axiom. Specify the reduction separately for each operation, including fresh-name conditions and query-signature handling.

### Make the result type carry the claim

A proposed envelope contains `answer`, `execution_status`, `coverage`, `input_integrity`, `engine_route`, `consistency_status` and evidence identifiers. Preserve definite answers with their justification and mark undecided queries unknown. Keep the reason for incompleteness and limits used. Deprecate misleading Boolean semantics through a compatibility path rather than silently breaking consumers.

- On timeout, publish only globally justified consequences, not branch-local tentative facts. Retain a definite negative only if its required evidence was already established. Absence of an answer is never a negative.
- A partial entailment closure can be sound; purported direct hierarchy edges are provisional because a missing intermediate class changes directness. Unknown must not become false or an empty complete hierarchy.
- Choose an explicit inconsistent-ontology policy. Classical entailment becomes vacuous when there are no models; a product may instead return an inconsistency status before query answering. All APIs must agree.
- Reject or explicitly mark invalid OWL 2 DL inputs, unsupported constructs, incomplete imports and lossy conversions. Record every omitted axiom and its consequence for the permitted answers.

For deletion-only weakening from O to O′, sound entailment and inconsistency established from O′ can transfer to O. Consistency and non-entailment generally cannot. Unbounded execution is not completeness; finished execution may still be incomplete.

## 3 Assign modular proof obligations

Review by semantic boundary, not just by source file. Each boundary needs a written invariant, executable assertions where practical, targeted counterexamples and an accountable reviewer. The goal is a chain from original input to public answer in which no undocumented assumption supplies the missing argument.

| Boundary | Obligation to establish | Evidence to retain |
| --- | --- | --- |
| Input and imports | The accepted ontology, declarations, imports and datatypes match the documented interpretation; loss is reported. | Source manifest; import hashes; unsupported-construct and invalid-input tests. |
| Normalization | The stated equivalence or equisatisfiability relation holds, including fresh symbols and query preservation. | Rule specification; small countermodels; property tests per rewrite. |
| Rule saturation | Each rule preserves the correct consequence or satisfiability invariant; scheduling reaches the promised fixed point. | Rule-level tests; derivation checks; worklist invariants. |
| Branch and merge state | Alternatives are exhaustive; rollback restores every affected structure; equality is a congruence. | State-machine tests; branch snapshots; merge and undo assertions. |
| Blocking and termination | Blocking has the required model/unravelling argument for every supported feature combination. | Formal conditions; adversarial cyclic cases; invalid-blocking mutants. |
| Caches and incrementality | Keys include all semantic context; invalidation preserves equivalence to a fresh run. | Trace tests; epoch checks; cold/warm comparisons. |
| Dispatch and public APIs | The selected engine meets the claim; errors and uncertainty survive conversion. | Forced-route comparisons; schema tests; Rust/Python/CLI parity. |

### Review the interactions where local arguments fail

Branching, equality, role propagation and blocking must be reviewed together. A merge can invalidate previously cached labels or blocking relations; a rollback can restore labels but leave stale indexes; a role-chain inference can create obligations after a node appears complete. Tests for each feature in isolation do not establish the combined invariant. An example is issue #373, fixed by #374. A `≤n` merge chosen by a search decision created a self-loop on an irreflexive role. The clash it raised kept only the label’s empty dependency set, not the edge’s, so backjumping skipped live alternatives. The result was a false `unsat` from `sat` and a false subsumption from `subclass`. This is a separate mechanism from the nominal cluster below.

A completion graph is not automatically a model. If a proof uses blocking, show how a model is constructed and how inverse roles, nominals, equality and counting survive. Establish the appropriate calculus conditions; a successful traversal is not that argument. [SROIQ calculus](https://www.cs.ox.ac.uk/people/ian.horrocks/Publications/download/2006/HoKS06a.pdf).

Audit the reported issue cluster in a mechanism × engine × command ledger: equality, counting, propagation, eligibility and publication across saturation, hypertableau, tableau, pre-checks and fallback. Centralize semantic contracts and reusable obligations without requiring a monolithic rewrite. Independent checkers should remain independent of that shared production code.

The initial cluster, as resolved by 31 October 2026 (`42457595`), gives a first instance of that ledger. Each row records the fix as merged; none is independently assured yet.

| Mechanism | Engine and command where it failed | Fix and status |
| --- | --- | --- |
| A blocker whose copy duplicates a nominal or a neighbour counted by a nominal’s ≤n (the SHOIQ NN-rule obligation) | Wedge, `consistent` (#361, #376) | #375 and #381; flags `RUSTDL_WEDGE_NOMINAL_BLOCK_GUARD` and `RUSTDL_WEDGE_NN_GUARD`, default on |
| Same obligation | Main tableau, `sat` and `subclass` (#362) | #378; flag `RUSTDL_NOMINAL_BLOCK_GUARD`, default on |
| Same obligation, plus nominal identity | Classify wedge, which is built without nominals, so neither wedge guard applies; reported with `incomplete: false` (#383) | Open |
| Individuals named only in TBox nominals seeded for consistency but not for per-query probes | Main tableau, `sat` versus `consistent` (#356) | #366; one shared seeding helper, `RUSTDL_QUERY_TBOX_INDIVIDUALS`, default on |
| Individual equalities forced by functional or ≤1 roles over nominals | `classify` (#354) | #365; probe-confirmed `derived_same`, budget-limited, undecided candidates reported |

The same nominal obligation had to be added three times. The first three rows are the duplicated semantic decision this strategy warns about.

## 4 Centralize checked publication across engines

Multiple engines are not intrinsically a defect. The risk is duplicated semantic decisions and unchecked transitions or publication. Use a common acceptance boundary, not an immediate rewrite into one engine. Saturation, hypertableau, tableau, pre-checks and future optimizations may propose answers or search hints. Every public command must pass definitive claims through the same source-bound contract before publication; stronger independent validation can then be introduced incrementally.

### Bind each answer to the problem it actually solved

Create an immutable problem descriptor containing input/import identity, normalization identity, semantic regime, complete requested signature, query, revision and limits. Evidence must match the source, signature and revision. Its route and rule schema must satisfy the declared validation policy; a common checker may accept valid cross-route evidence. Validate identities, evidence type and coverage before serialization.

For fixed source O and revision: accepted positive q implies O entails q; accepted negative q implies O does not entail q; unknown makes no truth claim. Complete means every requested q is decided. Positives need justified entailment; negatives need a countermodel or validated complete decision argument. Consistency is separate. A timeout stops production, not the validity of accepted evidence; branch-local tentative facts never qualify.

### Transfer the strongest ideas from KM

The supplied KM manuscript describes exactness of accepted taxonomy evidence and a narrow source-bound checking architecture. Its section 4, pages 13–14, separates that result from the trusted parsing, runtime and system boundary. The pinned theorem source works over normalized clauses and requested signatures; supervisor completeness assumes selected or fallback completeness. Accepted-output exactness and eventual producer completion are distinct obligations. [KM source adapter](https://github.com/bio-ontology-research-group/kobayashi-marust/blob/b5abf83a58268d9c2d2ab0b86e1befcd3b9ae89a/lean/ContextCalculus/KMConcreteWorkerAdapters.lean#L1180-L1220) and [supervisor assumptions](https://github.com/bio-ontology-research-group/kobayashi-marust/blob/b5abf83a58268d9c2d2ab0b86e1befcd3b9ae89a/lean/ContextCalculus/KMConcreteAutomaticSupervisor.lean#L39-L48).

Reuse the certificate schema principles, tamper tests and source/signature discipline with Michel’s KM collaboration. Do not assume its engine proofs transfer to RustDL, or that clauses already establish OWL parsing and normalization correctness. The manuscript’s runtime Rust checks and release-time Lean checking also require an explicit correspondence argument; a runtime Lean flag alone does not demonstrate a bypass or a defect.

### Adapt full acceptance to useful partial results

- Add proof emitters and adapters per RustDL route, preserving heterogeneous engines. Validate each accepted partial consequence without waiting for a complete matrix.
- Block unsupported, stale, malformed or unvalidated definitive claims at the publication boundary. Reject corrupted source, signature, evidence, closure and status metadata in tests.
- Keep an independently implemented semantic checker or oracle for external challenge. Shared production semantics reduces duplication but creates correlated failure risk; sharing the checker’s normalizer would weaken that independence.

This design preserves the full sound-and-complete ambition while accommodating finite budgets. It also concentrates cross-command consistency and uncertainty handling in one reviewable place, where a fix can protect every public route.

## 5 Build an independently trusted semantic oracle

Use evidence with different failure modes. Agreement between reasoners detects discrepancies but majority vote is not truth. State which components are independent, which assumptions are shared, and what each successful check establishes. Grammar-based testing has exposed defects in mature reasoners. [Language Based Testing for Knowledge Graphs](https://ebjohnsen.org/publication/25-eswc/25-eswc.pdf).

### A small interpretation evaluator

Implement a deliberately simple evaluator for a stated finite subset of OWL semantics. Read an independent syntax representation and directly evaluate axioms against interpretations; do not reuse RustDL normalization, rule code or branch state in the semantic core. Reuse and assess `owl-dl-verify` as an asset, but a model constructed from a reasoner’s own closure is not independent evidence for all of that closure’s entailments. [Existing verifier](https://github.com/MaastrichtU-IDS/rustdl/blob/60eca181037c308f25f732da4136883d3c89942b/crates/owl-dl-verify/src/lib.rs).

Enumerate or solve for tiny interpretations with nonempty domains. Account for equality without a unique-name assumption: different individual names may denote the same element. Treat datatypes as values, not merely lexical strings. The first oracle release should exclude unsupported datatype features explicitly rather than approximate them without notice.

A checked model of O proves consistency; a checked model also falsifying q proves non-entailment. Failure up to bound k proves only failure within that bound, unless a sufficient small-model bound is proved. Add an infinite-only negative control: a total injective functional successor with a named element having no predecessor. Natural numbers satisfy it; no finite model does. The oracle must not call it inconsistent.

### A proof and certificate lane

Introduce proof objects only for inference families that can be checked independently. For positive entailments, retain the premises, rule identifiers, substitutions and dependency structure needed to validate a derivation or refutation. For satisfiability and non-entailment, validate an actual model or a certificate whose reconstruction theorem is explicit. Checking a locally clash-free branch is insufficient by itself.

Use a checker that is smaller and simpler than the producer. Minimize shared code, run certificate validation in a separate process, and reject unknown rule versions. If an external solver supplies a bounded UNSAT certificate, validate both the encoding and the certificate, and preserve the bound in the result.

### Differential engines and adjudication

- Use applicable approved W3C fixtures and pin independent reasoners. Compare only common supported fragments, datatype maps, imports and operation semantics. Preserve original and normalized inputs. [OWL 2 conformance](https://www.w3.org/TR/owl2-conformance/).
- On disagreement, minimize first; then consult semantics, a checked model/proof, or a reviewed derivation. Keep unresolved cases visible as unresolved, rather than training the harness to accept the current RustDL result.
- Maintain a hold-out adversarial suite under a reviewer who did not implement the feature. Oracle bugs require their own regressions and versioned re-evaluation of dependent results.

## 6 Generate semantic interactions systematically

Build generators from a legal semantic grammar and feature-interaction model. Track regular role-hierarchy witnesses, simple-role restrictions and legality across imports. Keep deliberately illegal cases and syntax/import fuzzing in separate lanes. Tag every case with constructors, axiom shapes, intended obligation, operational stress and available oracle. [OWL 2 DL restrictions](https://www.w3.org/TR/owl2-syntax/#Global_Restrictions_on_Axioms_in_OWL_2_DL).

| Interaction family | Example obligation | Perturbations to explore |
| --- | --- | --- |
| Quantifiers and roles | Existential witnesses plus universal restrictions and role propagation. | Nested depth; inverse direction; cycles; domain/range; chains. |
| Equality and counting | Merge required by an upper bound while lower bounds demand distinct witnesses. | Nominals; same/different individuals; qualified counts; rollback. |
| Disjunction and dependencies | A clash closes one branch but not a valid alternative. | Branch order; shared premises; delayed clash; cache reuse. |
| Blocking and fresh witnesses | Termination without losing obligations that appear later. | Ancestor changes; inverses; repeated labels; blocked-node merges. |
| Normalization and loss | Equivalent input forms retain the same intended consequences. | Nested negation; fresh names; axiom order; parse/export round trips. |
| Keys and datatypes | Named-individual conditions, value equality and facet boundaries are respected. | Named/anonymous key values; equivalent lexical forms; empty ranges; boundaries. |

### Combine bounded enumeration with targeted generation

Exhaustively enumerate a tiny, explicitly bounded grammar and interpretation space for the oracle-supported subset. Report the actual bounds and number of unique semantic cases. Extend with pairwise coverage across all declared features, then targeted three-way or higher combinations where the proof obligations interact. Pairwise coverage is a baseline, not a claim that higher-order defects are absent.

Generate satisfiable cases from known interpretations, entailed queries from independently validated derivations, and non-entailments from checked countermodels. Also generate unconstrained adversarial cases; construction from a model alone can bias the suite toward easy consistent ontologies. Track semantic diversity rather than only test counts.

### Use metamorphic laws with explicit preconditions

- Bijective renaming, axiom reordering and duplicate insertion should preserve answers. Fresh names must remain fresh; annotations and serialization changes must respect the selected semantics.
- Adding axioms preserves existing classical entailments, but can change consistency and create new entailments. Removing axioms has the opposite asymmetry. Apply the product’s explicit inconsistency policy.
- Equivalent encodings and fresh-versus-reused execution should agree only on results whose contracts match. A changed timeout or route may change unknown status without changing semantic truth.

Shrink failures while preserving validity, the triggering interaction and the relevant oracle obligation. Save the original seed and trace as well as the minimized case; otherwise the reducer may erase the defect’s operational cause.

## 7 Test execution histories and engine routes

Treat a reasoner instance as a state machine. Compare reused execution with a genuinely fresh construction, and optimized routes with the complete reference route. Compare canonical entailment closures and equivalence classes over aligned signatures, not only direct hierarchy edges, counts or raw command Booleans.

### The reference execution loop

- Generate a valid ontology and operation sequence, then serialize the exact original input, imports and options. Record the commit, build features, seed, platform, engine route and limits.
- Execute each supported step on a reused instance and on a fresh instance. Where available, also force the baseline and optimized engines on the same operation.
- Compare answers, eligibility, loss diagnostics, consistency and certificates. Assert path witnesses: blocking, skipped expansion, unblocking, branching, replay and fallback must actually occur when claimed.
- For any discrepancy, retain all outputs and resource telemetry, replay deterministically, then minimize the ontology and the operation history together.

### Cross command laws and histories

Consistency of O agrees with satisfiability of `owl:Thing` in O under matched semantics. A consistent ontology can still contain an unsatisfiable named class, so consistency and sat(C) need not agree. A justified classification verdict that C is unsatisfiable must agree with sat(C). Unknown is not a contradiction; identical wrong answers across routes are not proof.

Cover classify → query → classify, reordered queries, consistency before and after classification, load/reload and multiple instances. Where mutation is supported, compare add/remove histories with fresh recomputation. Interrupt at deterministic checkpoints, retain justified partials, restart and check that timeout or cancellation has not poisoned state.

### Branch and cache state needs direct instrumentation

The existing blocked-disjunction regression says its successor never actually becomes blocked. Strengthen it with route witnesses. The incremental saturation identity test compares a narrower state than its graph-level description suggests; add edges, inequalities, redirects, indexes and per-fact dependencies to rollback equivalence. These are observed test-scope gaps, not confirmed solver defects. [Blocking test](https://github.com/MaastrichtU-IDS/rustdl/blob/60eca181037c308f25f732da4136883d3c89942b/crates/owl-dl-reasoner/tests/blocked_disjunction_soundness.rs#L1-L40) and [incremental identity test](https://github.com/MaastrichtU-IDS/rustdl/blob/60eca181037c308f25f732da4136883d3c89942b/crates/owl-dl-tableau/tests/incremental_saturate_identity.rs).

### One failure record across every test lane

Record source bytes and import-closure hash, normalized fingerprint, query/signature, semantic regime, feature vector, expected status and evidence, seed/shrinker, operation history, engine/config/build, artifact hashes, limits, path counters, certificate and replay command. Retain raw outputs. Label semantic defect, contract defect, input loss, resource outcome, oracle defect or unresolved discrepancy; do not overwrite expectations to make CI pass.

## 8 Measure whether the tests can detect wrong reasoning

Coverage is necessary for finding untouched code, but it does not measure whether assertions distinguish correct reasoning from incorrect reasoning. Add semantic mutation testing: intentionally introduce realistic faults and require the assurance suite to reject them. Keep mutants reviewable and do not merge them into the production implementation.

| Mutation class | Representative fault | Expected detector |
| --- | --- | --- |
| Inference | Reverse a rule implication; drop a premise; propagate a role in the wrong direction. | Independent derivation checker or semantic counterexample. |
| Search | Skip a branch; retain a clash across rollback; accept a branch prematurely. | Known satisfiable case; stateful trace; branch invariant. |
| Equality | Fail to transfer an edge after merge; treat all distinct names as unequal. | Equality/nominal/counting interaction corpus. |
| Blocking and cache | Use stale labels; omit context from a key; suppress a needed revisit. | Fresh versus warm execution; blocking stress; route comparison. |
| Contract | Turn unknown into false; omit a dropped-axiom flag; mark partial classification complete. | Public result schema and compatibility tests. |
| Input | Silently skip an import or constructor; compare datatype values lexically. | Manifest checks; preservation tests; datatype oracle. |

### Use a defensible mutation gate

Require every reviewed high-severity semantic mutant in the declared supported contract to be killed by a stable, relevant test. Classify equivalent, invalid and unreachable mutants separately, with a reason and reviewer. Report the denominator and surviving mutants by obligation. A high aggregate mutation score must not conceal an undetected false-entailment mutant.

Begin with a hand-curated set at the current high-risk boundaries, then use automated mutation tooling for breadth. Preserve the smallest killing test and map it to the rule or invariant. Re-run the curated semantic mutants when the associated code changes; rotate broader campaigns in scheduled CI.

### Robustness is a separate correctness dimension

Fuzz parsers, serializers, import resolution and public API boundaries. Include malformed inputs, extreme nesting, oversized counts, duplicate identifiers and limit exhaustion. Expected behaviour is a documented result or error without panic, memory unsafety, leaked state or falsely definitive semantic output. Resource limits must be explicit; they do not prove algorithmic termination.

Use Rust-appropriate memory and undefined-behaviour checks for unsafe code and foreign-language interfaces where applicable. Investigate dependencies and feature flags rather than assuming memory safety from the language alone. Keep this evidence separate from semantic soundness: a crash-free execution can still infer an invalid consequence.

### Do not optimize away the diagnostic route

Maintain an intentionally simple reference route with rich tracing, even if it is slower. Benchmark changes against the same pinned corpus and semantic result hashes. An optimization is acceptable only when it preserves the contract and passes its targeted mutants; a speed increase with more unknown results must be reported as a changed coverage trade-off.

## 9 Turn assurance into CI and release gates

Make the strongest affordable checks automatic on code that can affect semantics, and make deeper evidence a release condition. Jobs must fail on missing required fixtures, unrecognized oracle output or a checker that did not run. “No tests executed” must never look like a clean semantic result.

| Lane | Required checks | Blocking rule |
| --- | --- | --- |
| Every pull request | Contract/schema tests; current regressions; module invariants; fixed-seed interactions; cold/warm and route checks. | No unexplained semantic mismatch, contract regression or skipped required case. |
| Scheduled deep run | Larger generated corpus; bounded enumeration; independent reasoners; fuzzing; targeted mutations. | Triage every new failure; retain reproducers and disclose incomplete lanes. |
| Release candidate | Pinned full corpus; validated certificates; hold-out adversarial cases; supported-platform and feature matrix. | All claimed operations have accepted evidence; no open soundness failure in the advertised contract. |
| Post-release regression | Replay every confirmed defect and relevant nearby variants on supported releases. | Do not close the defect until the shipped behaviour and published guarantee agree. |

### Use risk based rules rather than one score

A wrong definitive answer is release-blocking for the affected claimed domain. A completeness failure blocks a completeness claim, but may permit a narrower release that explicitly returns unknown for the excluded cases. Performance or liveness failures need an explicit resource policy and operational acceptance; they cannot be relabelled as correct negatives.

Waivers require an owner, affected fragment and operations, user-visible limitation, compensating control and expiry. A waiver may narrow the supported contract. It must not authorize a known unsound result to remain advertised as sound. Quarantine should preserve the failing test and a tracked disposition, not remove it from the evidence denominator.

### Record evidence that can be reproduced

For each gate, store commit and dependency locks, build features, input/import hashes, tool and oracle versions, seeds, case counts by obligation, skipped cases with reasons, timeouts, mutation survivors and certificate-check results. Retain machine-readable summaries and downloadable minimal reproducers. Record the actual CI event and job that enforced the check, not only the existence of its workflow file.

Track contract cells with evidence, unresolved wrong-answer discrepancies, unknown/abstention rates and regressions, interaction and witnessed-path coverage, and mutant survival. For bug hunting, report unique adjudicated defect families per valid case and CPU-hour under stable generators, plus diverse held-out sweeps. Falling discovery rate alone can reflect weaker coverage; a flat rate alone does not decide scope.

### Integrate current workflows without overclaiming

Observed CI has a manually triggered differential lane with externally supplied fixtures, Python tests triggered by relevant engine changes, and Java wrapper tests whose path filter does not cover engine/CLI changes. Reuse these jobs, make fixtures reproducible, and widen the wrapper trigger. Verify required-check settings separately; a workflow file does not establish merge enforcement. [Core CI](https://github.com/MaastrichtU-IDS/rustdl/blob/60eca181037c308f25f732da4136883d3c89942b/.github/workflows/ci.yml), [Python CI](https://github.com/MaastrichtU-IDS/rustdl/blob/60eca181037c308f25f732da4136883d3c89942b/.github/workflows/python-ci.yml) and [Java CI](https://github.com/MaastrichtU-IDS/rustdl/blob/60eca181037c308f25f732da4136883d3c89942b/.github/workflows/java-ci.yml).

## 10 Give agents bounded work packages

Run the assurance work alongside the Claude team’s fixes, using a shared semantic contract and failure-record format. Each package must produce executable assets and an evidence report, not just a review narrative. Do not let the same agent implement a semantic rule, invent its expected result and approve that result without independent review.

| Package | Concrete deliverables | Acceptance reviewer |
| --- | --- | --- |
| A Contract and publication | Guarantee ledger; result schema; source/signature and loss manifests; shared acceptance boundary; compatibility tests. | Maintainer plus independent semantic reviewer. |
| B Semantic kernel | Independent evaluator; explicit supported grammar; hand-checked models; checker self-tests. | Reviewer who did not write the production normalizer. |
| C Implementation obligations | Module map; invariant catalogue; normalization rule specifications; blocking/merge review. | Algorithm specialist and code maintainer. |
| D Generators and shrinkers | Grammar generator; feature matrix; bounded enumerator; semantic and trace-preserving reducers. | Oracle owner validates labels and reductions. |
| E Stateful harness | Cold/warm and forced-route adapters; API parity; cancellation histories; replay format. | Implementation reviewer and contract owner. |
| F Mutation and gates | Curated semantic mutants; CI manifests; release evidence bundle; enforcement checks. | Release owner and independent assurance reviewer. |

### Require the same handoff from every package

- State the claim and scope, including excluded constructs, operations, routes and resource conditions. Identify all assumptions and dependencies on other packages.
- Deliver source, exact run commands, deterministic fixtures and expected outcomes with independent justification. Include at least one intentionally failing case proving that the checker or gate detects its target fault.
- List observed findings separately from proposed changes and unresolved hypotheses. Attach a minimal reproducer before calling a source concern a confirmed runtime defect.
- Provide a test-to-obligation map and a short residual-risk statement. An agent’s self-reported completion is not acceptance; another reviewer must reproduce the agreed checks.

### Give the current fixes an assurance intake path

For each fix supplied by the Claude team, identify the violated invariant, affected public contract cell and nearby interactions. Replay the original failure against a pinned before/after pair, add independent semantic justification, then add the smallest regression and at least one adversarial variant. Update the generator or mutant set when the failure reveals a missing defect class.

Keep proposed repository paths such as `assurance/contracts`, `assurance/cases`, `assurance/oracle` and `assurance/reports` clearly labelled as new deliverables. Reuse existing repository structures where appropriate after inspection; the strategy does not require parallel frameworks that duplicate working assets.

## 11 Accept milestones by evidence

Sequence the programme by dependency rather than promising a date before its scope is measured. Start with the narrowest valuable contract, then expand only when evidence supports another operation or feature combination. The release owner should be able to answer what is guaranteed, why, and where the guarantee stops.

| Milestone | Exit evidence | What it permits |
| --- | --- | --- |
| M0 Contract baseline | Every public operation mapped to route, fragment, input-loss policy and result semantics; ambiguity inventory reviewed. | An honest documented support boundary, not new correctness certification. |
| M1 Independent small domain | Reviewed evaluator and checker; positive and negative oracle self-tests; exhaustive declared tiny bounds. | Independently grounded tests for the oracle-supported subset. |
| M2 Interaction assurance | Stateful and interaction suites; minimized regressions; critical semantic mutants killed; reproducible CI. | A defensible empirical assurance claim within explicit bounds. |
| M3 Sound and complete procedure | Calculus soundness/completeness/termination arguments mapped to code and fallback; acceptance and producer obligations reviewed; release gates pass. | An engineering claim for the declared domain, with interrupted runs explicitly partial. |
| M4 Stronger mechanization | Machine-checked kernel/checker or selected state invariants; implementation correspondence and assumptions explicit. | Stronger assurance for the obligations actually mechanized, not automatic whole-program proof. |

### The release assurance case

Maintain one versioned record per claim: identifier; operation and fragment; selected semantics; route and build features; original-input preservation; resource assumptions; argument; tests and proofs; independent review; known limitations; and release decision. Evidence must be traceable to the shipped commit. A change to parsing, normalization, dispatch or result conversion can invalidate a claim even when the inference rules are unchanged.

Keep observed test behaviour, bounded exploration and proof arguments distinct. A conventional calculus proof plus disciplined implementation traceability is a legitimate engineering route; machine-checking every Rust line is not a prerequisite. Pilot mechanization on normalization/checker rules or tiny rollback invariants, preserving harness bounds and unwinding checks. [Kani bounds](https://model-checking.github.io/kani/tutorial-loop-unwinding.html) and [Verus scope](https://verus-lang.github.io/verus/guide/overview.html).

### Immediate implementation order

- First, reconcile `complete`, `completeness_guaranteed`, unknown results, loss diagnostics and public documentation. Add contract tests before refactoring their names or compatibility behaviour.
- Next, inventory and reuse the existing EL verifier, testing helpers, route controls, benchmark fixtures and CI jobs. Establish which production paths they exercise and which semantic components they share.
- Then build the independent small-domain oracle and stateful harness around the highest-risk interactions. Promote each new defect class into a generator, invariant or semantic mutant.

The stopping condition for this programme is not that every conceivable input has been tested. It is that the agreed release contract is supported by accepted evidence, all discrepancies affecting it are resolved or excluded explicitly, and the residual assumptions are stated accurately. Beyond that boundary, RustDL must preserve uncertainty rather than manufacture certainty.

## Sources and baseline notes

Repository observations are tied to the inspected commit `60eca181037c308f25f732da4136883d3c89942b`. They were re-checked on 31 October 2026 against `42457595292bceee9143c413091d08f74ca9a32c`, ten commits later (#365, #366, #367, #374, #375, #378, #379, #380, #381, #384). Changed:

- The completeness guard and the Python result changed (links above repointed to `42457595`).
- The issue cluster closed.
- #380 (`RUSTDL_MERGE_EXACT_DEPS`) and #384 (`RUSTDL_TREE_STICKY_BRANCHING`) merged as default-off options, not shipped behaviour.
- Issues #382 and #383 remain open.

Unchanged, so their permalinks remain at the original baseline:

- the cited blocked-disjunction and incremental-identity tests;
- the CI, Python CI and Java CI workflows (the manual closure-diff lane and the Java path filter still stand);
- `owl-dl-verify`;
- the README.

Proposed artefacts, schemas, gates and milestones are recommendations, not descriptions of implemented or executed work. A later commit requires rechecking the affected claims.

### Repository sources

[RustDL README](https://github.com/MaastrichtU-IDS/rustdl/blob/60eca181037c308f25f732da4136883d3c89942b/README.md)  
Scope statements and public reasoning claims.

[Python classification result](https://github.com/MaastrichtU-IDS/rustdl/blob/42457595292bceee9143c413091d08f74ca9a32c/crates/owl-dl-py/src/classify.rs#L50-L116)  
Observed distinction between completion of timed pairs and diagnostics for potentially missing entailments. Python has no counterpart to the Rust and CLI `completeness_guaranteed`.

[Rust completeness guard](https://github.com/MaastrichtU-IDS/rustdl/blob/42457595292bceee9143c413091d08f74ca9a32c/crates/owl-dl-reasoner/src/classify.rs#L962-L1010) and [CLI JSON field](https://github.com/MaastrichtU-IDS/rustdl/blob/42457595292bceee9143c413091d08f74ca9a32c/crates/owl-dl-cli/src/json_out.rs#L131-L147)  
The guard’s conditions, and its serialization in `classify --json`.

[CI workflow](https://github.com/MaastrichtU-IDS/rustdl/blob/60eca181037c308f25f732da4136883d3c89942b/.github/workflows/ci.yml)  
Workflow triggers and the manually invoked differential lane. Enforcement by repository settings is a separate fact to verify.

### KM reference

Supplied Kobayashi-MaRust manuscript draft, 26 October 2026, section 4 pages 13–14. Its accepted-answer theorem, producer limitations and trusted boundary inform the transfer recommendation.

[KM normalized source adapter](https://github.com/bio-ontology-research-group/kobayashi-marust/blob/b5abf83a58268d9c2d2ab0b86e1befcd3b9ae89a/lean/ContextCalculus/KMConcreteWorkerAdapters.lean#L1180-L1220), [supervisor assumptions](https://github.com/bio-ontology-research-group/kobayashi-marust/blob/b5abf83a58268d9c2d2ab0b86e1befcd3b9ae89a/lean/ContextCalculus/KMConcreteAutomaticSupervisor.lean#L39-L48) and [input format contract](https://github.com/bio-ontology-research-group/kobayashi-marust/blob/b5abf83a58268d9c2d2ab0b86e1befcd3b9ae89a/docs/INPUT-FORMATS.md). Code baseline `b5abf83a58268d9c2d2ab0b86e1befcd3b9ae89a`. No KM build or runtime certification is asserted.

### Semantic references

[OWL 2 Direct Semantics Second Edition](https://www.w3.org/TR/owl2-direct-semantics/)  
Normative model-theoretic reference and definitions of principal inference problems.

[OWL 2 Structural Specification and Functional Style Syntax](https://www.w3.org/TR/owl2-syntax/)  
Input structure, imports, datatype maps and OWL 2 DL restrictions.

[OWL 2 Profiles Second Edition](https://www.w3.org/TR/owl2-profiles/)  
Normative profile grammars and restrictions. Internal fragment names need an explicit mapping rather than presumed equivalence.

[OWL 2 Conformance Second Edition](https://www.w3.org/TR/owl2-conformance/)  
Conformance tests and result vocabulary; passing finite tests does not prove conformance.

### Terms used in this strategy

**Soundness** means that a definitive positive or negative semantic claim is justified under the declared semantics. **Completeness** means that every answer required by the stated operation and domain can be obtained, subject to its stated algorithmic assumptions. **Termination** means the algorithm finishes on all inputs in its domain; a timeout policy is a different operational property.

**Unknown** is an epistemic result of the implementation or its resource policy, not a third truth value in OWL semantics. **Independent** means separation of relevant failure modes, not merely a separate source file. **Bounded** means a stated finite scope of input, model or execution exploration; no universal claim follows from the word alone.
