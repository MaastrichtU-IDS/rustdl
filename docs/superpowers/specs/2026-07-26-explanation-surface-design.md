# Design: rustdl explanation surface in Protégé (justify / laconic / proofs)

**Date:** 2026-07-26
**Status:** design approved (brainstorming) → pending spec review → implementation plan.
**Author:** Claude + Michel, session: rdl-m-plugin.

## 1. Goal

Surface rustdl's explanation capabilities in Protégé — the v1.x follow-up the
plugin design (`2026-07-24-protege-plugin-design.md` §2) explicitly deferred.
Three surfaces, all driven by the existing reasoner engine:

- **Justifications** — minimal responsible-axiom sets for any entailed axiom,
  shown in Protégé's standard "?" **Explanation** dialog (`rustdl justify`).
- **Laconic justifications** — the same, narrowed to the responsible *fragment*
  of each axiom (`rustdl justify --laconic`), offered as a distinct choice.
- **Proof trees** — rustdl's step-level DL proof (`rustdl prove`) in Protégé's
  proof view (the ELK-style proof extension).

Reference precedents (mapped in research): **km** for the OWL Explanation API
(justifications); **ELK** for the liveontologies proof service (proofs).

## 2. Non-goals (v1)

- **Repair/diagnose UI** — rustdl has `repair`/`diagnose`; surfacing them is a
  later follow-up, not this spec.
- **Bundling the proof-explanation plugin** — the proof view relies on Protégé's
  separately-installed proof-explanation plugin (which *defines* the proof
  extension point). We import its packages `resolution:=optional` (like ELK), so
  the rustdl plugin loads fine without it; the proof service simply won't appear.
  We document the prerequisite; we do not vendor it.
- **Minimality on SROIQ** — rustdl justifications are subset-minimal on EL/Horn,
  guaranteed-*entailing* but possibly non-minimal on SROIQ. We surface that
  honestly; we do not attempt full SROIQ minimization here.

## 3. Architecture — two parts

### 3a. rustdl CLI `--json` (the machine bridge)

Two subcommands gain `--json`, matching the existing `schema_version: 1` contract
(`docs/json-schema.md`), golden-tested on the rustdl side.

**`justify [--all] [--laconic] --json <file> <query…>`** — mirrors km's report
shape so the Java side mirrors km's parsing:
```json
{ "schema_version": 1,
  "status": "entailed" | "not-entailed",
  "enumeration_complete": true,        // all justifications enumerated (not capped)
  "minimal": true,                     // subset-minimal (EL/Horn) vs guaranteed-entailing (SROIQ)
  "laconic": false,
  "prefix_declarations": "Prefix(:=<…>)\n…",   // OFN prefixes for round-trip parsing
  "justifications": [ { "axioms": ["SubClassOf(:A :B)", "…"] }, … ] }
```
- `justify` (default) → one justification; `--all` → all (up to an internal cap;
  `enumeration_complete=false` when capped). `--laconic` → laconic justifications
  (`laconic=true`), via `owl_dl_reasoner::{find_laconic_justification,
  find_all_laconic_justifications}`.
- Query forms are the existing `justify` CLI grammar (`subclass S T`,
  `unsat C`, `inconsistent`, `instance I C`, `equivalent`, `disjoint`,
  property/individual queries — `owl_dl_reasoner::justify::parse_query`).
- Axioms are **OWL Functional Syntax** fragments (from horned-owl rendering).
  `minimal` = `Justification.minimal_guaranteed`. `not-entailed` ⇒ empty
  `justifications`.

**`prove --json <file> <sub> <sup>`** — the EL proof tree, or a justification
fallback outside EL:
```json
{ "schema_version": 1, "entailed": true, "has_proof": true,
  "prefix_declarations": "…",
  "proof": {                                       // null when has_proof=false
    "conclusion": "SubClassOf(:A :C)",             // OFN of the derived axiom
    "rule": "CR5",                                 // ElRule name
    "axioms": ["SubClassOf(:A :B)", …],            // source axioms used at this step (OFN)
    "premises": [ { … recursive node … }, … ] },
  "justification_fallback": ["SubClassOf(…)", …] }  // null when has_proof=true
```
- From `owl_dl_reasoner::prove_entailment_rcstr` → `ProveEntailmentResult`:
  `SaturatorProof` → the `ProofNode` tree (recursive:
  `conclusion`/`rule`/`axiom_refs`/`premises`); `JustificationFallback` →
  `justification_fallback`; `NotEntailed` → `entailed=false`.
- `ProofNode.conclusion` (a `DerivedFact`) and `axiom_refs` (indices into the
  ontology's logical axioms) render to OFN via the same machinery `render_proof`
  uses; the plugin re-parses them with OWLAPI.

Both: one JSON object on **stdout**, diagnostics to **stderr**, fail-closed
(non-zero exit / parse error / `schema_version` mismatch → throw on the Java
side). Additive to the existing text output (the text `justify`/`prove` paths
are unchanged; `--json` short-circuits before them).

### 3b. Plugin integrations (`protege/` module)

Three components, each a focused unit; all reuse the existing `RustdlBinary`
(binary resolution) and `FlattenedOntology` (imports-closure → OFN) + a new
JSON-parse layer.

**(A) `RustdlExplanationGeneratorFactory` / `RustdlExplanationGenerator`** —
`implements org.semanticweb.owl.explanation.api.ExplanationGeneratorFactory<OWLAxiom>`
/ `ExplanationGenerator<OWLAxiom>` (owlexplanation 2.0.1). Mirrors km's
`KMExplanationGeneratorFactory`/`Generator`:
- `getExplanations(OWLAxiom entailment[, int limit])` → `queryArguments(entailment)`
  maps a **named** entailed axiom to a rustdl `justify` query. Support (superset
  of km): `SubClassOf(named,named)` → `["subclass",sub,sup]`;
  `SubClassOf(X, owl:Nothing)` → `["unsat",X]`; `owl:Thing ⊑ owl:Nothing` →
  `["inconsistent"]`; `EquivalentClasses`/`DisjointClasses` (pairwise);
  `ClassAssertion(a,C)` → `["instance",a,C]`; `ObjectPropertyAssertion`;
  `SubObjectPropertyOf`; `SameIndividual`. Anonymous/unsupported →
  `UnsupportedEntailmentException`.
- `invoke` → serialise imports closure (`FlattenedOntology`) → spawn
  `rustdl justify [--all] [--laconic] --json <ofn> <query…>` (deadline + cancel
  via the progress monitor, like km) → parse JSON → `materialize`: re-parse each
  functional-syntax axiom via OWLAPI (`OWLManager…loadOntologyFromOntologyDocument`
  over `Ontology(<prefixes><fragment>)`), assert it `.contains()` in
  `ontology.getAxioms(Imports.INCLUDED)` (anti-fabrication), build
  `new Explanation<>(entailment, Set<OWLAxiom>)`; `progressMonitor.foundExplanation`.
- Honesty: when `minimal=false`, the explanations are still sound (real
  responsible axioms) but possibly non-minimal — logged; not falsely certified.
- `META-INF/services/org.semanticweb.owl.explanation.api.ExplanationGeneratorFactory`
  names the factory (the ServiceLoader hook the "?" dialog uses).

**(B) Laconic** — the same generator with `--laconic`, exposed as a **second,
distinctly-named** explanation service so the user picks it in the dialog. Two
clean routes (plan picks one): (i) a second `ExplanationGeneratorFactory`
subclass whose generator sets `laconic=true`; or (ii) km-style native
`org.protege.editor.owl.explanation` `ExplanationService`s ("rustdl
justifications" / "rustdl laconic justifications") giving named entries + a
custom panel. Recommendation: **(i)** for v1 (least UI surface; the OWL
Explanation API already lists factories by name), revisit the native panel if a
richer UX is wanted.

**(C) `RustdlProofService extends org.liveontologies.protege.explanation.proof.service.ProofService`**
— mirrors ELK's `ElkProofService`:
- `hasProof(OWLAxiom)` = the axiom maps to a supported `prove` query (named
  `SubClassOf`, primarily; extendable).
- `getProof(OWLAxiom) → DynamicProof<Inference<? extends OWLAxiom>>` built from
  `rustdl prove --json`: each JSON proof node → a `puli` `Inference<OWLAxiom>`
  (`getName()` = `rule`, `getConclusion()` = the re-parsed conclusion axiom,
  `getPremises()` = the child conclusions), assembled into a `Proof<Inference>`
  (`getInferences(conclusion)` lookup). When `has_proof=false`, expose the
  `justification_fallback` as a single one-step inference (conclusion ← the
  fallback axioms) so the view still shows *something* sound.
- Registered on `org.liveontologies.protege.explanation.proof.service`. The puli
  / owlapi-proof / protege-proof-explanation / protege-proof-justification
  packages are **optional OSGi imports** (`resolution:=optional`, version
  `[0.1,1)`), compiled against as `provided`/`optional` Maven deps — **not
  embedded** (mirrors ELK exactly). If Protégé's proof-explanation plugin is
  absent, the rustdl bundle still resolves; the proof service is simply inactive.

## 4. Dependencies (mirror the two references precisely)

- **Justify (embedded, like km):** `net.sourceforge.owlapi:owlexplanation:2.0.1`
  + `net.sourceforge.owlapi:telemetry:2.0.0`, embedded as nested jars
  (`Embed-Dependency … inline=false`), with the km exclusions
  (owlexplanation excludes owlapi-osgidistribution + telemetry; telemetry
  excludes the owlapi distributions + slf4j). gson stays embedded (existing).
- **Proofs (optional imports, like ELK — NOT embedded):**
  `org.liveontologies:puli:0.1.0`, `org.liveontologies:owlapi-proof:0.1.0`,
  `org.liveontologies:protege-proof-explanation:0.1.0`,
  `io.github.liveontologies:protege-proof-justification:0.1.0` — all Maven
  `provided`/`optional`; OSGi `Import-Package … resolution:=optional; version="[0.1,1)"`.
- Existing: owlapi-distribution 4.5.29 (provided), protege-editor-owl 5.6.6
  (provided). Bundle manifest: `Export-Package` re-exports
  `org.semanticweb.owl.explanation.api` (+ telemetry) so the embedded API is
  visible; `Import-Package` excludes re-importing the embedded telemetry.

## 5. Config (system property → env → default), mirroring km

- `rustdl.explain.max.justifications` / `RUSTDL_EXPLAIN_MAX_JUSTIFICATIONS` —
  cap for the unbounded `getExplanations` (default 8).
- `rustdl.explain.timeout.seconds` / `RUSTDL_EXPLAIN_TIMEOUT_SECONDS` — per-call
  deadline (default = the existing `rustdl.timeout.seconds`, 600).
- Reuses the existing `rustdl.bin` / `RUSTDL_BIN` binary resolution.

## 6. Error handling (OWLReasoner/Explanation contracts)

- Binary missing / non-zero exit / timeout / JSON parse / `schema_version`
  mismatch → `ExplanationException` (justify) / return no proof (proofs), with
  captured stderr; never hang (deadline + cancel).
- Unsupported/anonymous entailment → `UnsupportedEntailmentException`
  (justify) / `hasProof=false` (proofs) — never a silent empty that reads as
  "no explanation exists".
- `status:"not-entailed"` (shouldn't happen for a Protégé-derived axiom, but) →
  no explanations, logged.
- An axiom that fails the source-`.contains()` check is dropped with a warning
  (anti-fabrication) — a sound guard, matching km.

## 7. Testing

- **rustdl side:** golden JSON tests for `justify --json` (entailed/not-entailed,
  `--all`, `--laconic`, `minimal` true/false across EL vs SROIQ fixtures) and
  `prove --json` (EL proof tree shape + the non-EL justification-fallback path).
  Schema-version assertion. Reuse the oracle discipline where a HermiT/ROBOT
  justification oracle is available.
- **Java side:** unit-test the JSON parse layer (fixtures); the
  entailment→query mapping (canned axioms → query args, incl. the unsupported
  cases); the functional-syntax re-parse + `.contains()` guard; the proof-node →
  puli `Inference` mapping (canned proof JSON → tree). A registration test
  (`META-INF/services` + plugin.xml proof-service extension present).
- **Integration smoke IT (real binary):** on a tiny EL ontology entailing
  `A ⊑ C`, drive `RustdlExplanationGenerator.getExplanations(SubClassOf(A,C))`
  → non-empty justification of source axioms; and `RustdlProofService.getProof`
  → a non-trivial proof tree. `assumeTrue`-gated on a reachable binary; the proof
  test additionally `assumeTrue`-gated on the proof-explanation API being on the
  classpath.

## 8. Scope summary (v1)

**Wired:** justifications (all supported entailment types) + laconic in the "?"
Explanation dialog; step-level proof trees (EL) with a justification fallback in
the proof view. **Honest:** minimality surfaced (EL-minimal vs SROIQ-guaranteed);
proofs EL-only with graceful fallback; proof view requires Protégé's
proof-explanation plugin. **Deferred:** repair/diagnose UI; native custom
explanation panel; SROIQ minimization.

## 9. Ships as

One tag: the rustdl `justify`/`prove --json` (new rustdl version) + the plugin
explanation jar (built by the existing `release-cli.yml` `build-plugin` job).
Patch cadence per prior releases. The plan breaks execution into SDD tasks:
(1) `justify --json`, (2) `prove --json`, (3) plugin justify (Explanation API +
laconic), (4) plugin proofs (proof service + deps), (5) docs + smoke IT + release.

## 10. Risks / open items for the plan

- **Proof-explanation plugin availability** in the user's Protégé (optional; the
  plan documents installing it; the smoke IT gates on it).
- **Functional-syntax round-trip fidelity** — every axiom rustdl emits in a
  justification must re-parse via OWLAPI to a `.contains()`-equal axiom (prefix
  handling; blank nodes in property-assertion justifications). Plan includes a
  round-trip test on a feature-rich justification.
- **`prove` conclusion/axiom rendering to OFN** — `DerivedFact`/`AxiomRef` must
  render to parseable OFN axioms; the plan verifies against `render_proof`.
- **owlexplanation/telemetry OSGi resolution** in stock Protégé 5.6.9 — km's
  exact pins + embedding are the mitigation; the antrun jar-content check
  (extended to the two new embedded jars) guards it.
