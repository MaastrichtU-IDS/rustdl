# Protégé plugin — inferred query surface (wire #44–47) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Graduate the Protégé plugin past its v1 "returns empty" boundary — back `getSub/SuperObjectProperties`, `getEquivalent{Object,Data}Properties`, `getDisjointClasses`, `getDisjoint{Object,Data}Properties`, `getSameIndividuals`, `getDifferentIndividuals`, `getObjectPropertyValues`, `getDataPropertyValues` with the new rustdl `--json` subcommands (from PR #50), instead of empty node sets.

**Architecture:** Mirror the existing plugin's classify/realize wiring. Add gson POJOs + `RustdlProcess` calls for the four new subcommands (`disjoint`, `property-hierarchy`, `individuals`, `property-values`), cache each per `InferenceType` in `RustdlReasoner`, and answer the corresponding query methods from the cache. `precomputeInferences` maps each new `InferenceType` to its subprocess; the `incomplete` flag on any result logs a warning (as classify already does). Ships in rustdl **v0.4.3** (patch — the plugin jar version tracks the tag).

**Tech Stack:** Java 11, `RustdlProcess`/`RustdlJson`/`RustdlReasoner` (existing), gson, OWLAPI 4.5.29, JUnit 4.13.2.

## Global Constraints

- Every task's requirements implicitly include this section.
- **The four new `--json` schemas (verbatim field names — gson maps by field name; must match `crates/owl-dl-cli/src/json_out.rs`):**
  - `disjoint --json` → `{ schema_version:1, incomplete:bool, disjoint_classes:[[iri,iri]], disjoint_object_properties:[[iri,iri]], disjoint_data_properties:[[iri,iri]] }`
  - `property-hierarchy --json` → `{ schema_version:1, incomplete:bool, object_properties:{equivalent_groups:[[iri]], direct_subsumptions:[[sub,sup]]}, data_properties:{equivalent_groups:[[iri]], direct_subsumptions:[[sub,sup]]} }`
  - `individuals --json` → `{ schema_version:1, incomplete:bool, same_groups:[[iri]], different_pairs:[[iri,iri]] }`
  - `property-values --json` → `{ schema_version:1, incomplete:bool, object_property_values:[[subj,prop,obj]], data_property_values:[[subj,prop,lexical,datatype]] }`
- `schema_version` MUST equal 1 (reuse `RustdlProcess.checkVersion`).
- `direct_subsumptions` are Hasse edges `[sub, sup]`; `equivalent_groups` are equivalence sets — SAME shape as `classify`, so the property hierarchy reuses the class-hierarchy walk logic.
- **Soundness posture:** all results are sound (rustdl's FP=0). `incomplete:true` ⇒ log a warning; the node set is a sound under-approximation (may miss, never wrong). Disjointness/property-values/same/different are entailment-backed; property hierarchy and disjoint-properties are structural.
- **`InferenceType` mapping** (all 9 OWLAPI 4.5.29 values now backed): `CLASS_HIERARCHY`→classify, `CLASS_ASSERTIONS`→realize (existing); `OBJECT_PROPERTY_HIERARCHY`+`DATA_PROPERTY_HIERARCHY`→property-hierarchy; `DISJOINT_CLASSES`→disjoint; `SAME_INDIVIDUAL`+`DIFFERENT_INDIVIDUALS`→individuals; `OBJECT_PROPERTY_ASSERTIONS`+`DATA_PROPERTY_ASSERTIONS`→property-values.
- Serialise the imports closure ONCE per precompute cycle (shared temp file across all subprocess calls of that cycle) — extend the existing pattern.
- Named entities only; anonymous/complex class-expression queries (#48) stay empty (deferred to the concurrent session's work).
- Build with the pinned toolchain: `export PATH="/opt/homebrew/bin:$PATH"; export JAVA_HOME="/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home"`; all Maven via `mvn -f protege/pom.xml …`.

## File Structure

- Modify: `protege/src/main/java/.../RustdlJson.java` (add 4 POJO groups), `RustdlProcess.java` (add `disjoint`/`propertyHierarchy`/`individuals`/`propertyValues` methods + `buildXCommand` seams), `RustdlReasoner.java` (cache fields, precompute wiring, replace ~10 empty query methods).
- Test: `RustdlProcessTest.java` (parse fixtures), `RustdlReasonerTest.java` (canned-JSON query mapping), `RustdlSmokeIT.java` (extend).
- Test resources: `protege/src/test/resources/json/{disjoint,prophier,individuals,propvalues}.json`.
- Docs: `docs/protege-plugin.md` (update the "Computes" scope), `CHANGELOG.md` (v0.4.3 entry — done in the release step, not here).

---

### Task 1: JSON POJOs + `RustdlProcess` calls for the 4 new subcommands

**Files:** `RustdlJson.java`, `RustdlProcess.java` (modify); `RustdlProcessTest.java` + 4 fixtures (create).

**Interfaces:**
- Produces: `RustdlJson.{DisjointJson, PropHierJson, PropHierSide, IndividualsJson, PropertyValuesJson}`; `RustdlProcess.{disjoint, propertyHierarchy, individuals, propertyValues}(Path ofn, long timeoutSec)` returning the matching POJO (parsed + schema-checked).

- [ ] **Step 1: Add the POJOs to `RustdlJson.java`** (append inside the class):

```java
    public static final class DisjointJson {
        public int schema_version;
        public boolean incomplete;
        public List<List<String>> disjoint_classes;
        public List<List<String>> disjoint_object_properties;
        public List<List<String>> disjoint_data_properties;
    }
    public static final class PropHierSide {
        public List<List<String>> equivalent_groups;
        public List<List<String>> direct_subsumptions;
    }
    public static final class PropHierJson {
        public int schema_version;
        public boolean incomplete;
        public PropHierSide object_properties;
        public PropHierSide data_properties;
    }
    public static final class IndividualsJson {
        public int schema_version;
        public boolean incomplete;
        public List<List<String>> same_groups;
        public List<List<String>> different_pairs;
    }
    public static final class PropertyValuesJson {
        public int schema_version;
        public boolean incomplete;
        public List<List<String>> object_property_values;   // [subj, prop, obj]
        public List<List<String>> data_property_values;     // [subj, prop, lexical, datatype]
    }
```

- [ ] **Step 2: Add parse + run methods to `RustdlProcess.java`** (mirror `classify`/`parseClassify` exactly):

```java
    public static RustdlJson.DisjointJson disjoint(Path ofn, long timeoutSec) throws IOException {
        return parseDisjoint(run("disjoint", ofn, timeoutSec));
    }
    public static RustdlJson.PropHierJson propertyHierarchy(Path ofn, long timeoutSec) throws IOException {
        return parsePropHier(run("property-hierarchy", ofn, timeoutSec));
    }
    public static RustdlJson.IndividualsJson individuals(Path ofn, long timeoutSec) throws IOException {
        return parseIndividuals(run("individuals", ofn, timeoutSec));
    }
    public static RustdlJson.PropertyValuesJson propertyValues(Path ofn, long timeoutSec) throws IOException {
        return parsePropertyValues(run("property-values", ofn, timeoutSec));
    }

    static RustdlJson.DisjointJson parseDisjoint(String json) {
        RustdlJson.DisjointJson c = fromJson(json, RustdlJson.DisjointJson.class); checkVersion(c.schema_version); return c;
    }
    static RustdlJson.PropHierJson parsePropHier(String json) {
        RustdlJson.PropHierJson c = fromJson(json, RustdlJson.PropHierJson.class); checkVersion(c.schema_version); return c;
    }
    static RustdlJson.IndividualsJson parseIndividuals(String json) {
        RustdlJson.IndividualsJson c = fromJson(json, RustdlJson.IndividualsJson.class); checkVersion(c.schema_version); return c;
    }
    static RustdlJson.PropertyValuesJson parsePropertyValues(String json) {
        RustdlJson.PropertyValuesJson c = fromJson(json, RustdlJson.PropertyValuesJson.class); checkVersion(c.schema_version); return c;
    }
```
> `run(subcommand, ofn, timeoutSec)` already builds `[bin, subcommand, "--json", ofn]` and is fail-closed; reuse it verbatim. If `run` is currently `private`, keep it — these methods are in the same class. (`fromJson`/`checkVersion` are the existing helpers.)

- [ ] **Step 3: Write the 4 fixtures** under `protege/src/test/resources/json/`:

`disjoint.json`:
```json
{ "schema_version": 1, "incomplete": false,
  "disjoint_classes": [["http://ex/#A", "http://ex/#B"]],
  "disjoint_object_properties": [["http://ex/#p", "http://ex/#q"]],
  "disjoint_data_properties": [] }
```
`prophier.json`:
```json
{ "schema_version": 1, "incomplete": false,
  "object_properties": { "equivalent_groups": [["http://ex/#p","http://ex/#p2"]],
                         "direct_subsumptions": [["http://ex/#p","http://ex/#r"]] },
  "data_properties": { "equivalent_groups": [], "direct_subsumptions": [["http://ex/#d","http://ex/#e"]] } }
```
`individuals.json`:
```json
{ "schema_version": 1, "incomplete": true,
  "same_groups": [["http://ex/#a","http://ex/#b"]],
  "different_pairs": [["http://ex/#a","http://ex/#c"]] }
```
`propvalues.json`:
```json
{ "schema_version": 1, "incomplete": false,
  "object_property_values": [["http://ex/#a","http://ex/#p","http://ex/#b"]],
  "data_property_values": [["http://ex/#a","http://ex/#d","5","http://www.w3.org/2001/XMLSchema#integer"]] }
```

- [ ] **Step 4: Add parse tests to `RustdlProcessTest.java`** (mirror the existing `parsesClassify` style): one test per subcommand asserting a representative field, plus one asserting `parseDisjoint("{\"schema_version\":2}")` throws `IllegalStateException`.

- [ ] **Step 5: Run** `mvn -f protege/pom.xml test -Dtest=RustdlProcessTest` → PASS. **Commit.**

---

### Task 2: Property-hierarchy queries (object + data)

**Files:** `RustdlReasoner.java` (modify), `RustdlReasonerTest.java` (test).

**Interfaces:**
- Consumes: `RustdlJson.PropHierJson` (Task 1), the cache field `propHierResult` (Task 6 adds precompute; here add the field + a `forTest` injector).
- Produces: cache-backed `getSub/SuperObjectProperties`, `getEquivalentObjectProperties`, `getSub/SuperDataProperties`, `getEquivalentDataProperties`, and correct top/bottom property nodes.

- [ ] **Step 1: Add the cache field + index builder.** Add `private RustdlJson.PropHierJson propHierResult;` and, mirroring the class-hierarchy `rebuildIndices`, build four maps from `propHierResult.object_properties` / `.data_properties`: `objDirectSupers/objDirectSubs` keyed by `OWLObjectProperty`, `dataDirectSupers/dataDirectSubs` keyed by `OWLDataProperty`, plus equivalence-node maps (`objEquivByIri`, `dataEquivByIri`). Reuse the exact walk pattern from `walkNodes` — write generic helpers `walkObjProps(start, edges, direct)` → `Set<Node<OWLObjectPropertyExpression>>` and `walkDataProps(...)`. Build these in a `rebuildPropHierIndices()` called from wherever `propHierResult` is set (precompute in Task 6; `forTest` here). Add a `ensurePropHier()` lazy-precompute like `ensureClassified()`.

- [ ] **Step 2: Replace the empty property-hierarchy methods.** Full implementations (top/bottom follow OWLAPI: top object property = `owl:topObjectProperty`, bottom = `owl:bottomObjectProperty`; a property with no named super is a direct sub of top; a property with no named sub is a direct super of bottom — MIRROR the class Thing/Nothing logic you already have, adapted to properties). Provide:

```java
@Override public NodeSet<OWLObjectPropertyExpression> getSubObjectProperties(OWLObjectPropertyExpression pe, boolean direct) {
    if (pe.isAnonymous()) return new OWLObjectPropertyNodeSet();
    ensurePropHier();
    return walkObjProps(pe.asOWLObjectProperty(), objDirectSubs, direct);   // + bottom-node handling per contract
}
@Override public NodeSet<OWLObjectPropertyExpression> getSuperObjectProperties(OWLObjectPropertyExpression pe, boolean direct) { /* symmetric, objDirectSupers, + top node */ }
@Override public Node<OWLObjectPropertyExpression> getEquivalentObjectProperties(OWLObjectPropertyExpression pe) { /* objEquivByIri lookup, else singleton */ }
@Override public NodeSet<OWLDataProperty> getSubDataProperties(OWLDataProperty pe, boolean direct) { /* dataDirectSubs */ }
@Override public NodeSet<OWLDataProperty> getSuperDataProperties(OWLDataProperty pe, boolean direct) { /* dataDirectSupers */ }
@Override public Node<OWLDataProperty> getEquivalentDataProperties(OWLDataProperty pe) { /* dataEquivByIri */ }
```
Implement the top/bottom property-node conventions exactly as the class hierarchy does (see your `getSuperClasses`/`getSubClasses`): `getSuperObjectProperties(p,true)` with no named super → `{topObjectProperty}` node; `getSuperObjectProperties(p,false)` includes the top node; `getSubObjectProperties(p,false)` includes the bottom node; likewise data. `throwIfInconsistent()` first (an inconsistent KB throws, matching the class methods). Anonymous `pe` → empty.

- [ ] **Step 3: Add a `forTest` overload** that injects `PropHierJson` (extend the existing `forTest` or add `forTestPropHier`) so the test can exercise mapping without a subprocess.

- [ ] **Step 4: Tests** (`RustdlReasonerTest.java`): inject `prophier.json`-shaped data; assert `getSuperObjectProperties(p, true)` contains `r`; `getEquivalentObjectProperties(p)` contains `p2`; `getSubDataProperties(e, true)` contains `d`; and the top/bottom property nodes appear at the frontier. Run `-Dtest=RustdlReasonerTest`. **Commit.**

---

### Task 3: Disjoint classes + object/data properties

**Files:** `RustdlReasoner.java`, `RustdlReasonerTest.java`.

**Interfaces:** consumes `RustdlJson.DisjointJson` via `disjointResult` cache; produces `getDisjointClasses(ce)`, `getDisjointObjectProperties(pe)`, `getDisjointDataProperties(pe)`.

- [ ] **Step 1:** Add `private RustdlJson.DisjointJson disjointResult;` + `ensureDisjoint()`. Build a symmetric adjacency from `disjoint_classes` (each `[X,Y]` ⇒ Y ∈ disjointOf(X) and X ∈ disjointOf(Y)), and likewise `disjointObjOf` / `disjointDataOf` from the property lists.

- [ ] **Step 2: Replace** `getDisjointClasses(OWLClassExpression ce)`:
```java
@Override public NodeSet<OWLClass> getDisjointClasses(OWLClassExpression ce) {
    if (ce.isAnonymous()) return new OWLClassNodeSet();
    ensureDisjoint(); throwIfInconsistent();
    Set<Node<OWLClass>> nodes = new HashSet<>();
    for (OWLClass d : disjointOf.getOrDefault(ce.asOWLClass(), Collections.emptySet())) {
        nodes.add(equivNodeOf(d));   // group disjoint classes into their equivalence nodes
    }
    return new OWLClassNodeSet(nodes);
}
```
(Note: OWLReasoner's `getDisjointClasses` should also include subclasses of the disjoint classes; for v1 report the directly-entailed disjoint named classes — a sound under-approximation. Document this.) Then `getDisjointObjectProperties(OWLObjectPropertyExpression pe)` and `getDisjointDataProperties(OWLDataPropertyExpression pe)` analogously (return `OWLObjectPropertyNodeSet`/`OWLDataPropertyNodeSet`; anonymous → empty).

- [ ] **Step 3: Tests:** inject `disjoint.json`; assert `getDisjointClasses(A)` contains `B`; `getDisjointObjectProperties(p)` contains `q`; `getDisjointDataProperties` empty. **Commit.**

---

### Task 4: Same / different individuals

**Files:** `RustdlReasoner.java`, `RustdlReasonerTest.java`.

**Interfaces:** consumes `RustdlJson.IndividualsJson` via `individualsResult`; produces `getSameIndividuals(ind)`, `getDifferentIndividuals(ind)`.

- [ ] **Step 1:** Add `private RustdlJson.IndividualsJson individualsResult;` + `ensureIndividuals()`. Build `sameGroupByIri` (each `same_groups` entry → an `OWLNamedIndividualNode` shared by its members) and `differentOf` symmetric adjacency from `different_pairs`.

- [ ] **Step 2: Replace:**
```java
@Override public Node<OWLNamedIndividual> getSameIndividuals(OWLNamedIndividual ind) {
    ensureIndividuals(); throwIfInconsistent();
    Node<OWLNamedIndividual> n = sameGroupByIri.get(ind.getIRI().toString());
    return n != null ? n : new OWLNamedIndividualNode(ind);   // contract: the node always contains ind itself
}
@Override public NodeSet<OWLNamedIndividual> getDifferentIndividuals(OWLNamedIndividual ind) {
    ensureIndividuals(); throwIfInconsistent();
    Set<Node<OWLNamedIndividual>> nodes = new HashSet<>();
    for (OWLNamedIndividual d : differentOf.getOrDefault(ind.getIRI().toString(), Collections.emptySet()))
        nodes.add(new OWLNamedIndividualNode(d));
    return new OWLNamedIndividualNodeSet(nodes);
}
```
(Store `differentOf` keyed by IRI string → `Set<OWLNamedIndividual>`.)

- [ ] **Step 3: Tests:** inject `individuals.json` (note `incomplete:true`); assert `getSameIndividuals(a)` contains `b`; `getDifferentIndividuals(a)` contains `c`; and that precompute logged the incomplete warning (Task 6 wires the log; here just assert the mapping). **Commit.**

---

### Task 5: Object / data property values

**Files:** `RustdlReasoner.java`, `RustdlReasonerTest.java`.

**Interfaces:** consumes `RustdlJson.PropertyValuesJson` via `propValuesResult`; produces `getObjectPropertyValues(ind, pe)`, `getDataPropertyValues(ind, pe)`.

- [ ] **Step 1:** Add `private RustdlJson.PropertyValuesJson propValuesResult;` + `ensurePropValues()`. No index needed — filter the triple/quad lists on query.

- [ ] **Step 2: Replace:**
```java
@Override public NodeSet<OWLNamedIndividual> getObjectPropertyValues(OWLNamedIndividual ind, OWLObjectPropertyExpression pe) {
    if (pe.isAnonymous()) return new OWLNamedIndividualNodeSet();
    ensurePropValues(); throwIfInconsistent();
    String s = ind.getIRI().toString(), p = pe.asOWLObjectProperty().getIRI().toString();
    Set<Node<OWLNamedIndividual>> nodes = new HashSet<>();
    for (List<String> t : orEmpty(propValuesResult.object_property_values)) {
        if (t.get(0).equals(s) && t.get(1).equals(p))
            nodes.add(new OWLNamedIndividualNode(df.getOWLNamedIndividual(IRI.create(t.get(2)))));
    }
    return new OWLNamedIndividualNodeSet(nodes);
}
@Override public Set<OWLLiteral> getDataPropertyValues(OWLNamedIndividual ind, OWLDataProperty pe) {
    ensurePropValues(); throwIfInconsistent();
    String s = ind.getIRI().toString(), p = pe.getIRI().toString();
    Set<OWLLiteral> out = new HashSet<>();
    for (List<String> q : orEmpty(propValuesResult.data_property_values)) {
        if (q.get(0).equals(s) && q.get(1).equals(p))
            out.add(df.getOWLLiteral(q.get(2), df.getOWLDatatype(IRI.create(q.get(3)))));   // lexical + datatype
    }
    return out;
}
```

- [ ] **Step 3: Tests:** inject `propvalues.json`; assert `getObjectPropertyValues(a,p)` contains `b`; `getDataPropertyValues(a,d)` contains the literal `"5"^^xsd:integer`. **Commit.**

---

### Task 6: Precompute wiring + incomplete warnings + smoke IT + docs

**Files:** `RustdlReasoner.java`, `RustdlSmokeIT.java`, `docs/protege-plugin.md`.

- [ ] **Step 1: Advertise all 9 InferenceTypes:**
```java
@Override public Set<InferenceType> getPrecomputableInferenceTypes() {
    return EnumSet.of(InferenceType.CLASS_HIERARCHY, InferenceType.CLASS_ASSERTIONS,
        InferenceType.OBJECT_PROPERTY_HIERARCHY, InferenceType.DATA_PROPERTY_HIERARCHY,
        InferenceType.DISJOINT_CLASSES, InferenceType.SAME_INDIVIDUAL,
        InferenceType.DIFFERENT_INDIVIDUALS, InferenceType.OBJECT_PROPERTY_ASSERTIONS,
        InferenceType.DATA_PROPERTY_ASSERTIONS);
}
```
`isPrecomputed(type)` returns `cache != null` for each mapped type (property-hierarchy for both OBJECT/DATA_PROPERTY_HIERARCHY; individuals for both SAME/DIFFERENT_INDIVIDUALS; property-values for both OBJECT/DATA_PROPERTY_ASSERTIONS), true otherwise.

- [ ] **Step 2: Extend `precomputeInferences`** — after the classify/realize block, in the SAME cycle (shared temp `ofn`), for each requested type run its subprocess once if its cache is null, then `rebuild*Indices()`. Each result: if `result.incomplete`, `LOG.warning("rustdl reports an INCOMPLETE <query> result; sound under-approximation (may miss, never wrong).")`. Wrap failures as `ReasonerInternalException`. Add the corresponding `ensureX()` lazy triggers (each calls `precomputeInferences(THE_TYPE)`). In `handleChanges`, null all new caches + clear all new indices (BUFFERING invalidation).

- [ ] **Step 3: Extend `RustdlSmokeIT`** — on a tiny ontology with two disjoint classes + a sub-property + two same individuals + one property assertion, run the reasoner through the factory, `precomputeInferences(all 9)`, and assert one representative answer from each new family is non-empty (real binary). Keep it `assumeTrue`-gated on a reachable binary.

- [ ] **Step 4: Update `docs/protege-plugin.md`** "What it computes": move property hierarchies, disjointness, same/different individuals, and property values from "returns empty" to "computed"; leave only complex-class-expression queries (#48) as the remaining empty stub.

- [ ] **Step 5:** `mvn -f protege/pom.xml clean package` green; `mvn -f protege/pom.xml test -Dtest=RustdlSmokeIT -Drustdl.bin="$PWD/target/release/rustdl"` runs (build a binary first) and passes. **Commit.**

## Release (after merge — controller)

Tag **v0.4.3** (patch): bump `Cargo.toml` workspace version 0.4.2→0.4.3 (+ the 6 internal dep reqs), `cargo update -w`, CHANGELOG `[0.4.3]` (PR #50 inferred query surface now surfaced in the Protégé plugin; + the #50 review fixes), commit, tag, push. The release workflow rebuilds the plugin jar at 0.4.3 with the new query surface + refreshes `update.properties`.

## Self-Review

- **Spec coverage:** #44 property hierarchy → Task 2; #45 property values → Task 5; #46 same/different → Task 4; #47 disjointness → Task 3; precompute/InferenceType/incomplete/BUFFERING → Task 6; JSON bridge → Task 1. ✔
- **Type consistency:** POJO field names match `json_out.rs` (`disjoint_classes`, `object_properties`/`data_properties` with `equivalent_groups`/`direct_subsumptions`, `same_groups`/`different_pairs`, `object_property_values`/`data_property_values`); `InferenceType` values are the 9 OWLAPI 4.5.29 constants; property-hierarchy reuses the class-hierarchy walk shape (same `direct_subsumptions`/`equivalent_groups`).
- **Placeholder scan:** the query-method bodies with `/* … */` in Tasks 2–3 reference the EXISTING class-hierarchy top/bottom logic already in `RustdlReasoner` — the implementer must mirror that concrete code (it is present in the file), not invent it; every other block is complete literal code.
