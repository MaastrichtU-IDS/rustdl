# rustdl explanation surface — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Surface rustdl's `justify` (+ `--laconic`) and `prove` in Protégé — justifications in the "?" Explanation dialog and step-level proof trees in the proof view. Design: `docs/superpowers/specs/2026-07-26-explanation-surface-design.md`.

**Architecture:** Two rustdl CLI `--json` subcommands (`justify`, `prove`) bridge to two plugin surfaces: the OWL Explanation API (`ExplanationGeneratorFactory<OWLAxiom>`, mirror km — justifications + laconic) and the liveontologies proof service (`ProofService`, mirror ELK — proof trees). owlexplanation/telemetry are embedded (like km); the proof-service APIs are optional OSGi imports (like ELK).

**Tech Stack:** Rust (serde_json, horned-owl OFN writer); Java 11, owlapi 4.5.29, owlexplanation 2.0.1 + telemetry 2.0.0, puli/owlapi-proof/protege-proof-explanation/protege-proof-justification 0.1.0, maven-bundle-plugin.

## Global Constraints

- rustdl `--json`: `schema_version: 1` (reuse `json_out::SCHEMA_VERSION`); one JSON object on stdout, diagnostics to stderr; additive (the text paths are unchanged, `--json` short-circuits before them). Build Rust with `RUSTUP_TOOLCHAIN=stable`; `cargo clippy --workspace --all-targets --all-features -- -D warnings` + `cargo fmt` clean.
- Justification/proof axioms are **OWL Functional Syntax**, emitted as self-contained OFN ontology documents (horned-owl `horned_owl::io::ofn::writer::write`), so the plugin parses each with OWLAPI in one shot and reconstructs `OWLAxiom`s.
- **Soundness/honesty:** every returned axiom is a source axiom (rustdl justifies over the ontology's own logical axioms) — the plugin still verifies each is `.contains()` in the imports closure (anti-fabrication). `minimal` is true only on EL/Horn (`Justification.minimal_guaranteed`); surfaced, never falsely certified. Proof trees are EL-only; outside EL, `prove` returns a justification fallback.
- Plugin toolchain: `export PATH="/opt/homebrew/bin:$PATH"; export JAVA_HOME="/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home"`; all Maven via `mvn -f protege/pom.xml …`.
- **Dependency strategy (mirror the references exactly):** owlexplanation 2.0.1 + telemetry 2.0.0 → `Embed-Dependency … inline=false` (embedded nested jars, with km's exclusions). puli/owlapi-proof/protege-proof-explanation/protege-proof-justification 0.1.0 → Maven `provided` + OSGi `Import-Package … resolution:=optional;version="[0.1,1)"` (NOT embedded).
- Reference sources to mirror (read them): km at `…/scratchpad/km/KMExplanation*.java` + the `META-INF/services` file; ELK/puli at `/tmp/ElkProofService.java`, `/tmp/ElkOwlProof.java`, `/tmp/puli_{Proof,Inference,ProofStep}.java`, `…/scratchpad/elk/elk_plugin.xml` + `elk_protege_pom.xml`.
- Package root `com.github.maastrichtu_ids.rustdl.protege`.

## File Structure

- rustdl: `crates/owl-dl-cli/src/json_out.rs` (+ `JustifyJson`/`ProveJson` builders), `crates/owl-dl-cli/src/main.rs` (`--json` on Justify/Prove), `crates/owl-dl-cli/tests/json_output.rs` (+ fixtures), `docs/json-schema.md`.
- plugin: new `RustdlExplain*.java` (justify), `RustdlProof*.java` (proofs), `RustdlOfn.java` (parse OFN docs → OWLAxioms + the `.contains()` guard); `RustdlProcess.java` (+ `justify`/`prove` calls + JSON POJOs in `RustdlJson`); `protege/pom.xml` (deps + bundle instructions); `plugin.xml` (proof-service extension); `META-INF/services/org.semanticweb.owl.explanation.api.ExplanationGeneratorFactory`; tests + smoke IT; `docs/protege-plugin.md`.

---

### Task 1: `justify --json` (rustdl CLI)

**Files:** `json_out.rs`, `main.rs` (modify); `tests/json_output.rs` + fixtures.

**Interfaces:** Produces `justify --json` per §3a of the spec. `json_out::build_justify_json(&[Justification<RcStr>], laconic, enumeration_complete) -> JustifyJson`.

- [ ] **Step 1: Add `JustifyJson` + builder to `json_out.rs`.**
```rust
#[derive(Serialize)]
pub(crate) struct JustifyJson {
    pub(crate) schema_version: u32,
    pub(crate) status: String,               // "entailed" | "not-entailed"
    pub(crate) enumeration_complete: bool,
    pub(crate) minimal: bool,                // all justifications minimal_guaranteed
    pub(crate) laconic: bool,
    pub(crate) justifications: Vec<JustificationJson>,
}
#[derive(Serialize)]
pub(crate) struct JustificationJson { pub(crate) ofn: String }  // self-contained OFN ontology document
```
Builder: render each `Justification`'s `axioms: Vec<Component<RcStr>>` into a **self-contained OFN ontology document** string — build a `SetOntology<RcStr>` (or the ontology type horned-owl's writer takes) containing exactly those components + the source ontology's prefix mapping, and write it with `horned_owl::io::ofn::writer::write(&mut buf, &ont, Some(&prefix_mapping))`. (Read the existing `parse_ofn_with_pm` / how the CLI obtains the `PrefixMapping` `pm`; reuse it so the emitted doc carries the same prefixes.) `status` = `"entailed"` if any justification (or the engine says entailed) else `"not-entailed"` (empty `justifications`). `minimal` = every justification's `minimal_guaranteed`. `enumeration_complete` from the `--all` cap (true unless capped).

- [ ] **Step 2: Wire `--json` into the `Justify` handler in `main.rs`.** Add `#[arg(long)] json: bool` to the `Justify` variant. In the handler, after computing the justifications (use `find_one_justification` for the default, `find_all_justifications` / `find_all_laconic_justifications` / `find_laconic_justification` per `all`/`laconic`), if `json`: `println!("{}", serde_json::to_string_pretty(&json_out::build_justify_json(&justs, laconic, complete))?); return Ok(());` BEFORE the existing text `render` closure. (Confirm the exact reasoner fn names: `owl_dl_reasoner::justify::{find_one_justification, find_all_justifications}` and `owl_dl_reasoner::{find_laconic_justification, find_all_laconic_justifications}`.)

- [ ] **Step 3: Golden tests** (`tests/json_output.rs`, mirror the existing e2e style using `env!("CARGO_BIN_EXE_rustdl")`): (a) an EL fixture entailing `A ⊑ C` → `justify --json subclass A C` yields `status:"entailed"`, `minimal:true`, one justification whose `ofn` parses and contains the responsible source axioms; (b) `--all` on a two-justification fixture → 2 justifications, `enumeration_complete:true`; (c) `--laconic` → `laconic:true`; (d) a not-entailed query → `status:"not-entailed"`, empty; (e) a SROIQ fixture → `minimal:false`. Fixtures under `tests/fixtures/json/`.

- [ ] **Step 4: Commit** (`feat(cli): justify --json`).

---

### Task 2: `prove --json` (rustdl CLI)

**Files:** `json_out.rs`, `main.rs`; `tests/json_output.rs` + fixtures.

**Interfaces:** Produces `prove --json` per §3a. `json_out::build_prove_json(&ProveEntailmentResult, &InternalOntology, &PrefixMapping) -> ProveJson`.

- [ ] **Step 1: Add `ProveJson` + recursive `ProofNodeJson` + builder to `json_out.rs`.**
```rust
#[derive(Serialize)]
pub(crate) struct ProveJson {
    pub(crate) schema_version: u32,
    pub(crate) entailed: bool,
    pub(crate) has_proof: bool,
    pub(crate) proof: Option<ProofNodeJson>,
    pub(crate) justification_fallback: Option<String>,  // OFN ontology document
}
#[derive(Serialize)]
pub(crate) struct ProofNodeJson {
    pub(crate) conclusion: String,      // OFN of the derived axiom
    pub(crate) rule: String,            // ElRule name (Debug/Display)
    pub(crate) axioms: Vec<String>,     // source axioms used at this step (OFN fragments)
    pub(crate) premises: Vec<ProofNodeJson>,
}
```
Builder: match `ProveEntailmentResult`:
- `SaturatorProof(data)` → `entailed=true, has_proof=true`, recursively map `data.root: ProofNode`. For each node: `rule` = `format!("{:?}", node.rule)` (or a Display if one exists); `axioms` = each `node.axiom_refs` (`AxiomRef(usize)` index into `logical_axioms(onto).1` — read how `render_proof_with_defs`/`check_proof_with_content` resolve `axiom_refs` to the source `Component`) rendered to an OFN fragment; `conclusion` = `node.conclusion: DerivedFact` rendered to an OFN axiom (read `render_proof_with_defs` for how a `DerivedFact` + `synthetic_defs` renders to a subsumption; produce the OFN `SubClassOf(...)` equivalent — reuse the vocabulary + synthetic_defs the CLI passes to `render_proof_with_defs`). `premises` = recurse over `node.premises`.
- `JustificationFallback(j)` → `entailed=true, has_proof=false`, `justification_fallback` = the OFN ontology document of `j.axioms` (reuse Task 1's OFN-document helper — factor it into a shared `pub(crate) fn axioms_to_ofn_doc(...)`), `proof=None`.
- `NotEntailed` → `entailed=false, has_proof=false`, both `None`.

> Implementer note: the `DerivedFact`/`AxiomRef` → OFN rendering is the load-bearing risk. Read `crates/owl-dl-saturation/src/proof.rs` (`render_proof`, `render_proof_with_defs`) and `owl_dl_reasoner::justify::logical_axioms` to get the exact resolution of `axiom_refs` and the `DerivedFact` → axiom mapping, then render via horned-owl OFN. If a `DerivedFact` cannot be expressed as a single OFN axiom (e.g. an internal synthetic), render the best-effort SubClassOf using the vocabulary + `synthetic_defs` (same as `render_proof_with_defs` does for text) — the plugin only needs a parseable OFN `SubClassOf`.

- [ ] **Step 2: Wire `--json` into `Prove`** (add `#[arg(long)] json: bool`; on `json`, build the internal ontology (the handler already `convert_ontology`s for rendering), call `build_prove_json`, print, return — before the text path).

- [ ] **Step 3: Golden tests:** (a) an EL fixture with a multi-step `A ⊑ C` proof → `has_proof:true`, a `proof` tree whose `conclusion`/`axioms` parse as OFN, `premises` non-empty; (b) a non-EL entailment → `has_proof:false`, `justification_fallback` present + parseable; (c) not-entailed → `entailed:false`. **Commit** (`feat(cli): prove --json`).

- [ ] **Step 4: Update `docs/json-schema.md`** with the `justify`/`prove --json` schemas. **Commit.**

---

### Task 3: Plugin — justifications + laconic (OWL Explanation API)

**Files:** new `RustdlExplanationGeneratorFactory.java`, `RustdlExplanationGenerator.java`, `RustdlLaconicExplanationGeneratorFactory.java`, `RustdlOfn.java`; `RustdlProcess.java` + `RustdlJson.java` (+ justify call/POJO); `pom.xml` (owlexplanation/telemetry); `META-INF/services/…ExplanationGeneratorFactory`; tests.

**Interfaces:** Consumes `justify --json`. Produces `Set<Explanation<OWLAxiom>>` via the OWL Explanation API. Read km's `KMExplanationGeneratorFactory`/`Generator`/`Configuration`/`Run` (scratchpad) as the structural template — mirror them, applying the deltas below.

- [ ] **Step 1: pom deps + bundle.** Add `net.sourceforge.owlapi:owlexplanation:2.0.1` (with exclusions of `owlapi-osgidistribution` + `telemetry`) and `net.sourceforge.owlapi:telemetry:2.0.0` (excluding the owlapi distributions + slf4j). Bundle: `Embed-Dependency: gson|owlexplanation|telemetry;scope=compile;inline=false`; `Export-Package` adds `org.semanticweb.owl.explanation.api;version="2.0.1"`; `Import-Package` prepends `!org.semanticweb.owl.explanation.telemetry`. Extend the antrun `verify-bundle-contents` to assert `owlexplanation-2.0.1.jar` + `telemetry-2.0.0.jar` are embedded. Verify `mvn -f protege/pom.xml package` builds and both nested jars are present.

- [ ] **Step 2: `RustdlJson` POJO + `RustdlProcess.justify`.** POJOs: `JustifyJson{ int schema_version; String status; boolean enumeration_complete; boolean minimal; boolean laconic; List<JustificationJson> justifications; }`, `JustificationJson{ String ofn; }`. `RustdlProcess.justify(Path ofn, boolean laconic, int maxJustifications, long timeoutSec)` → spawns `rustdl justify --all --json [--laconic] <ofn> <query…>` — but the query args are passed by the generator; refactor `RustdlProcess` to accept an explicit arg list (a package-visible `runCommand`, already present) so the generator can pass `["justify","--all","--json", laconic?"--laconic":…, ofn, query…]`. Parse + `checkVersion`. Unit-test parse on a fixture.

- [ ] **Step 3: `RustdlOfn.java`** — `static Set<OWLAxiom> parse(String ofnDocument)`: load the OFN document via `OWLManager.createOWLOntologyManager().loadOntologyFromOntologyDocument(new StringDocumentSource(ofn, IRI…, new FunctionalSyntaxDocumentFormat(), null))` → `getAxioms(Imports.EXCLUDED)`. `static Set<OWLAxiom> verifiedAgainst(Set<OWLAxiom> parsed, OWLOntology source)` drops any axiom not `source.containsAxiom(ax, INCLUDED, …)` (anti-fabrication), logging a warning. Unit-test round-trip on a sample OFN doc.

- [ ] **Step 4: `RustdlExplanationGenerator implements ExplanationGenerator<OWLAxiom>`** (mirror `KMExplanationGenerator`): `queryArguments(OWLAxiom)` maps a named entailment to rustdl `justify` query args — support `SubClassOf(named,named)`→`["subclass",sub,sup]`, `SubClassOf(X,owl:Nothing)`→`["unsat",X]`, `owl:Thing⊑owl:Nothing`/inconsistency→`["inconsistent"]`, `EquivalentClasses`/`DisjointClasses` (pairwise), `ClassAssertion(a,C)`→`["instance",a,C]`, `ObjectPropertyAssertion(a,p,b)`, `SubObjectPropertyOf`, `SameIndividual` — else `UnsupportedEntailmentException`. `getExplanations(entailment[,limit])` → serialize (`FlattenedOntology`), spawn justify (deadline + `progressMonitor.isCancelled()`), parse, `materialize`: for each `JustificationJson.ofn` → `RustdlOfn.parse` → `RustdlOfn.verifiedAgainst(source)` → `new Explanation<>(entailment, axioms)`; `progressMonitor.foundExplanation`. If `minimal==false`, `LOG.warning` (sound, possibly non-minimal). The unbounded `getExplanations(entailment)` caps at the config max and requires `enumeration_complete` (else the km-style "raise the cap" `ExplanationException`).

- [ ] **Step 5: Factories + config + registration.** `RustdlExplanationGeneratorFactory implements ExplanationGeneratorFactory<OWLAxiom>` (four `createExplanationGenerator` overloads → the generator, `laconic=false`); `RustdlLaconicExplanationGeneratorFactory` (same, `laconic=true`, and a distinct `getExplanationGeneratorName()` e.g. "rustdl (laconic)"). `RustdlExplainConfiguration` (max justifications `rustdl.explain.max.justifications`/env default 8; timeout `rustdl.explain.timeout.seconds`/env default 600). `META-INF/services/org.semanticweb.owl.explanation.api.ExplanationGeneratorFactory` lists BOTH factory FQNs (one per line).

- [ ] **Step 6: Tests** — entailment→query mapping (canned axioms incl. unsupported→exception); `RustdlOfn` round-trip + `.contains()` guard; `getExplanations` over a canned `JustifyJson` (inject via a seam, no subprocess) → correct `Explanation`s. `mvn -f protege/pom.xml test`. **Commit.**

---

### Task 4: Plugin — proof trees (liveontologies proof service)

**Files:** new `RustdlProofService.java`, `RustdlProof.java` (puli `Proof`/`Inference` impl), `RustdlProcess.prove` + `ProveJson` POJO; `pom.xml` (optional proof deps); `plugin.xml` (proof-service extension); tests. Read ELK's `ElkProofService`/`ElkOwlProof` (`/tmp/…`) + `puli_{Proof,Inference,ProofStep}.java` as the template.

- [ ] **Step 1: pom optional deps + imports.** Add `org.liveontologies:puli:0.1.0`, `:owlapi-proof:0.1.0`, `:protege-proof-explanation:0.1.0`, `io.github.liveontologies:protege-proof-justification:0.1.0` — all `<optional>true</optional>`. Bundle `Import-Package` adds `org.liveontologies.puli;version="[0.1,1)";resolution:=optional, org.liveontologies.owlapi.proof;version="[0.1,1)";resolution:=optional, org.liveontologies.protege.explanation.proof.service;version="[0.1,1)";resolution:=optional` (NOT embedded — mirror ELK). Confirm the bundle still resolves/builds without them present at compile via the Maven deps.

- [ ] **Step 2: `ProveJson` POJO + `RustdlProcess.prove`** (`{schema_version, entailed, has_proof, proof:ProofNodeJson?, justification_fallback:String?}`, `ProofNodeJson{conclusion, rule, axioms:List<String>, premises:List<ProofNodeJson>}`). `prove(Path ofn, String sub, String sup, long timeoutSec)` → `rustdl prove --json <ofn> <sub> <sup>`. Parse + `checkVersion`. Unit-test parse.

- [ ] **Step 3: `RustdlProof` — a `puli` `Proof<Inference<OWLAxiom>>`.** Build from `ProveJson`: recursively convert each `ProofNodeJson` into a puli `Inference<OWLAxiom>` (`getName()`=`rule`; `getConclusion()`=`RustdlOfn.parse(oneAxiom(conclusion))` single axiom; `getPremises()`= the child nodes' conclusions), indexed so `Proof.getInferences(conclusion)` returns the inference(s) deriving that conclusion. `has_proof=false` → a single synthetic one-step inference: conclusion = the entailed axiom, premises = the `justification_fallback` axioms, name = "justification". (Reuse `RustdlOfn.parse` for conclusion + `axioms`.)

- [ ] **Step 4: `RustdlProofService extends ProofService`** (mirror `ElkProofService`): `hasProof(OWLAxiom)` = the axiom maps to a supported prove query (named `SubClassOf`, extend as feasible); `getProof(OWLAxiom)` → serialize + `rustdl prove --json` → `RustdlProof` wrapped in a `DynamicProof` (a minimal `DynamicProof` that re-runs on `dispose`/change is fine — mirror ELK's `DynamicOwlProof` shape; a non-incremental recompute-on-demand is acceptable for v1); `getExample`/`postProcess`/`dispose` per the base. Register in `plugin.xml`: `<extension point="org.liveontologies.protege.explanation.proof.service"><name value="rustdl ${project.version}"/><class value="…RustdlProofService"/></extension>`.

- [ ] **Step 5: Tests** — `ProveJson` parse; `RustdlProof` from a canned proof-tree JSON → a `Proof` whose `getInferences(root)` yields the root inference with correct rule/premises; the `has_proof=false` fallback path → the synthetic justification inference. (These compile against the `optional` puli/proof deps, present at test time.) `mvn -f protege/pom.xml test`. **Commit.**

---

### Task 5: Docs + smoke IT + release

**Files:** `RustdlSmokeIT.java` (extend), `docs/protege-plugin.md`, `CHANGELOG` (release step).

- [ ] **Step 1: Extend `RustdlSmokeIT`** (real binary): on a tiny EL ontology entailing `A ⊑ C`, `new RustdlExplanationGeneratorFactory().createExplanationGenerator(o).getExplanations(SubClassOf(A,C))` → a non-empty `Explanation` whose axioms are source axioms; and (gated additionally on the proof-explanation API being on the classpath) `RustdlProofService.getProof(SubClassOf(A,C))` → a `Proof` with a non-trivial root inference. `assumeTrue` on a reachable binary. Build a binary + run `-Dtest='*Test,*IT' -Drustdl.bin=…`; confirm it RAN (not skipped) and passed.

- [ ] **Step 2: `docs/protege-plugin.md`** — add an "Explaining inferences" section: the "?" Explanation dialog offers **rustdl** (minimal justifications) and **rustdl (laconic)**; the proof view shows rustdl step-level proofs (EL; degrades to a justification otherwise) **and requires Protégé's proof-explanation plugin installed** (state the prerequisite, like ELK). Note the config knobs. **Commit.**

- [ ] **Step 3 (controller, after merge):** release — bump version (patch), CHANGELOG entry (justify/prove `--json` + the plugin explanation surface + the proof-plugin prerequisite), `cargo update -w`, commit, tag, push. The release workflow rebuilds the plugin jar with the explanation surface + refreshes `update.properties`.

## Self-Review

- **Spec coverage:** §3a justify/prove `--json` → Tasks 1–2; §3b(A) Explanation API + (B) laconic → Task 3; §3b(C) proof service → Task 4; §4 deps → Tasks 3–4 poms; §5 config → Task 3; §7 testing → each task + Task 5 smoke IT; §2 prerequisites/honesty → Tasks 4–5 + the `minimal`/`has_proof` flags. ✔
- **Type consistency:** JSON field names identical across the Rust builders (Task 1–2) and the Java POJOs (Task 3–4): `status/enumeration_complete/minimal/laconic/justifications[].ofn`; `entailed/has_proof/proof/justification_fallback`, `ProofNodeJson{conclusion,rule,axioms,premises}`. Dep versions match the references (owlexplanation 2.0.1, telemetry 2.0.0, puli/owlapi-proof/proof-explanation/proof-justification 0.1.0).
- **Known risks flagged in-task:** the `DerivedFact`/`AxiomRef`→OFN rendering (Task 2 Step 1 note) and the proof-explanation optional-dep resolution (Task 4 Step 1) are the two load-bearing spots; the plan directs the implementer to the exact reference code (`render_proof_with_defs`, ELK's manifest) rather than guessing.
