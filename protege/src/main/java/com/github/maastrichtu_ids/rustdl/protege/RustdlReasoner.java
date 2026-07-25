package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owlapi.model.*;
import org.semanticweb.owlapi.model.parameters.ChangeApplied;
import org.semanticweb.owlapi.reasoner.*;
import org.semanticweb.owlapi.reasoner.impl.*;
import org.semanticweb.owlapi.util.Version;

import java.nio.file.*;
import java.util.*;
import java.util.logging.Logger;

public class RustdlReasoner extends OWLReasonerBase {
    private static final Logger LOG = Logger.getLogger(RustdlReasoner.class.getName());

    private final OWLDataFactory df;
    private final long timeoutSec;
    private final long pairTimeoutMs;

    // Cache, populated by precompute (or injected in tests).
    private RustdlJson.ClassifyJson classifyResult;
    private RustdlJson.RealizeJson realizeResult;
    private RustdlJson.PropHierJson propHierResult;
    private RustdlJson.DisjointJson disjointResult;
    private RustdlJson.IndividualsJson individualsResult;
    private RustdlJson.PropertyValuesJson propValuesResult;

    // Derived indices built from classifyResult.
    private final Map<String, Node<OWLClass>> equivNodeByIri = new HashMap<>();   // iri -> its equiv-class node
    private final Map<OWLClass, Set<OWLClass>> directSupers = new HashMap<>();
    private final Map<OWLClass, Set<OWLClass>> directSubs = new HashMap<>();
    private final Set<OWLClass> unsatisfiable = new HashSet<>();
    private final Set<OWLClass> allNamed = new HashSet<>();      // named classes minus Thing/Nothing
    private final Set<OWLClass> topChildren = new HashSet<>();   // satisfiable classes with no named super
    private final Set<OWLClass> bottomLeaves = new HashSet<>();  // satisfiable classes with no named sub

    // Derived indices built from propHierResult.object_properties.
    private final Map<String, Node<OWLObjectPropertyExpression>> objEquivByIri = new HashMap<>();
    private final Map<OWLObjectProperty, Set<OWLObjectProperty>> objDirectSupers = new HashMap<>();
    private final Map<OWLObjectProperty, Set<OWLObjectProperty>> objDirectSubs = new HashMap<>();
    private final Set<OWLObjectProperty> objAllNamed = new HashSet<>();     // named obj props minus top/bottom
    private final Set<OWLObjectProperty> objTopChildren = new HashSet<>();  // props with no named super
    private final Set<OWLObjectProperty> objBottomLeaves = new HashSet<>(); // props with no named sub

    // Derived indices built from propHierResult.data_properties.
    private final Map<String, Node<OWLDataProperty>> dataEquivByIri = new HashMap<>();
    private final Map<OWLDataProperty, Set<OWLDataProperty>> dataDirectSupers = new HashMap<>();
    private final Map<OWLDataProperty, Set<OWLDataProperty>> dataDirectSubs = new HashMap<>();
    private final Set<OWLDataProperty> dataAllNamed = new HashSet<>();     // named data props minus top/bottom
    private final Set<OWLDataProperty> dataTopChildren = new HashSet<>();  // props with no named super
    private final Set<OWLDataProperty> dataBottomLeaves = new HashSet<>(); // props with no named sub

    // Derived (symmetric) adjacency built from disjointResult.
    private final Map<OWLClass, Set<OWLClass>> disjointOf = new HashMap<>();
    private final Map<OWLObjectProperty, Set<OWLObjectProperty>> disjointObjOf = new HashMap<>();
    private final Map<OWLDataProperty, Set<OWLDataProperty>> disjointDataOf = new HashMap<>();

    // Derived indices built from individualsResult.
    private final Map<String, Node<OWLNamedIndividual>> sameGroupByIri = new HashMap<>();
    private final Map<String, Set<OWLNamedIndividual>> differentOf = new HashMap<>();

    RustdlReasoner(OWLOntology rootOntology, OWLReasonerConfiguration config, BufferingMode mode) {
        super(rootOntology, config, mode);
        this.df = rootOntology.getOWLOntologyManager().getOWLDataFactory();
        this.timeoutSec = resolveTimeout(config);
        this.pairTimeoutMs = resolvePairTimeoutMs();
    }

    /** Test seam: inject canned results, skip the subprocess. */
    static RustdlReasoner forTest(OWLOntology o, RustdlJson.ClassifyJson c, RustdlJson.RealizeJson r) {
        SimpleConfiguration cfg = new SimpleConfiguration();
        RustdlReasoner reasoner = new RustdlReasoner(o, cfg, BufferingMode.BUFFERING);
        reasoner.classifyResult = c;
        reasoner.realizeResult = r;
        reasoner.rebuildIndices();
        return reasoner;
    }

    /** Test seam: as {@link #forTest(OWLOntology, RustdlJson.ClassifyJson, RustdlJson.RealizeJson)}, plus an injected property-hierarchy result. */
    static RustdlReasoner forTest(OWLOntology o, RustdlJson.ClassifyJson c, RustdlJson.RealizeJson r, RustdlJson.PropHierJson p) {
        RustdlReasoner reasoner = forTest(o, c, r);
        reasoner.propHierResult = p;
        reasoner.rebuildPropHierIndices();
        return reasoner;
    }

    /** Test seam: as {@link #forTest(OWLOntology, RustdlJson.ClassifyJson, RustdlJson.RealizeJson)}, plus an injected disjointness result. */
    static RustdlReasoner forTest(OWLOntology o, RustdlJson.ClassifyJson c, RustdlJson.RealizeJson r, RustdlJson.DisjointJson d) {
        RustdlReasoner reasoner = forTest(o, c, r);
        reasoner.disjointResult = d;
        reasoner.rebuildDisjointIndices();
        return reasoner;
    }

    /** Test seam: as {@link #forTest(OWLOntology, RustdlJson.ClassifyJson, RustdlJson.RealizeJson)}, plus an injected individuals result. */
    static RustdlReasoner forTest(OWLOntology o, RustdlJson.ClassifyJson c, RustdlJson.RealizeJson r, RustdlJson.IndividualsJson i) {
        RustdlReasoner reasoner = forTest(o, c, r);
        reasoner.individualsResult = i;
        reasoner.rebuildIndividualsIndices();
        return reasoner;
    }

    /** Test seam: as {@link #forTest(OWLOntology, RustdlJson.ClassifyJson, RustdlJson.RealizeJson)}, plus an injected property-values result. */
    static RustdlReasoner forTest(OWLOntology o, RustdlJson.ClassifyJson c, RustdlJson.RealizeJson r, RustdlJson.PropertyValuesJson v) {
        RustdlReasoner reasoner = forTest(o, c, r);
        reasoner.propValuesResult = v;
        return reasoner;
    }

    private static long resolveTimeout(OWLReasonerConfiguration config) {
        String p = System.getProperty("rustdl.timeout.seconds");
        if (p == null || p.isEmpty()) p = System.getenv("RUSTDL_TIMEOUT_SECONDS");
        if (p != null && !p.isEmpty()) try { return Long.parseLong(p); } catch (NumberFormatException ignored) {}
        return 600L;
    }

    private static long resolvePairTimeoutMs() {
        String p = System.getProperty("rustdl.pair.timeout.ms");
        if (p == null || p.isEmpty()) p = System.getenv("RUSTDL_PAIR_TIMEOUT_MS");
        if (p != null && !p.isEmpty()) try { return Long.parseLong(p); } catch (NumberFormatException ignored) {}
        return 10000L;
    }

    // ---- identity ----
    @Override public String getReasonerName() { return "rustdl"; }
    @Override public Version getReasonerVersion() { return new Version(0, 0, 0, 0); }

    // ---- precompute / buffering ----
    @Override public Set<InferenceType> getPrecomputableInferenceTypes() {
        return EnumSet.of(InferenceType.CLASS_HIERARCHY, InferenceType.CLASS_ASSERTIONS);
    }
    @Override public boolean isPrecomputed(InferenceType type) {
        if (type == InferenceType.CLASS_HIERARCHY) return classifyResult != null;
        if (type == InferenceType.CLASS_ASSERTIONS) return realizeResult != null;
        return true; // unsupported types are trivially "precomputed" (empty)
    }
    @Override public void precomputeInferences(InferenceType... types) {
        Set<InferenceType> req = new HashSet<>(Arrays.asList(types));
        boolean wantHierarchy = req.contains(InferenceType.CLASS_HIERARCHY);
        boolean wantAssertions = req.contains(InferenceType.CLASS_ASSERTIONS);
        if (!wantHierarchy && !wantAssertions) return;
        Path ofn = null;
        try {
            ofn = Files.createTempFile("rustdl-", ".ofn");
            FlattenedOntology.writeOfn(getRootOntology(), ofn);
            // Classify runs whenever EITHER type is requested: realize's consistency
            // gate below needs classifyResult, and rustdl's realize errors on an
            // inconsistent KB, so we must know consistency before ever calling it.
            if ((wantHierarchy || wantAssertions) && classifyResult == null) {
                classifyResult = RustdlProcess.classify(ofn, timeoutSec, pairTimeoutMs);
                if (classifyResult.incomplete) {
                    LOG.warning("rustdl reports an INCOMPLETE classification (some class pairs timed out); "
                        + "the hierarchy is a sound under-approximation.");
                }
                rebuildIndices();
            }
            if (wantAssertions && realizeResult == null
                    && classifyResult != null && classifyResult.consistent
                    && !getRootOntology().getIndividualsInSignature(true).isEmpty()) {
                realizeResult = RustdlProcess.realize(ofn, timeoutSec);
            }
        } catch (Exception e) {
            throw new ReasonerInternalException("rustdl precompute failed: " + e.getMessage(), e);
        } finally {
            if (ofn != null) try { Files.deleteIfExists(ofn); } catch (Exception ignored) {}
        }
    }

    /** Ensure classify ran (lazy precompute for a supported query issued before precomputeInferences). */
    private void ensureClassified() {
        if (classifyResult == null) precomputeInferences(InferenceType.CLASS_HIERARCHY);
    }
    private void ensureRealized() {
        if (realizeResult == null) precomputeInferences(InferenceType.CLASS_ASSERTIONS);
    }
    /** Ensure the property hierarchy ran (lazy precompute; wiring into precomputeInferences is a later task). */
    private void ensurePropHier() {
        if (propHierResult == null) precomputeInferences(InferenceType.OBJECT_PROPERTY_HIERARCHY);
    }
    /** Ensure disjointness ran (lazy precompute; the subprocess wiring into precomputeInferences is a later task). */
    private void ensureDisjoint() {
        if (disjointResult == null) precomputeInferences(InferenceType.DISJOINT_CLASSES);
    }
    /** Ensure same/different individuals ran (lazy precompute; the subprocess wiring into precomputeInferences is a later task). */
    private void ensureIndividuals() {
        if (individualsResult == null) precomputeInferences(InferenceType.SAME_INDIVIDUAL);
    }
    /** Ensure object/data property values ran (lazy precompute; the subprocess wiring into precomputeInferences is a later task). */
    private void ensurePropValues() {
        if (propValuesResult == null) precomputeInferences(InferenceType.OBJECT_PROPERTY_ASSERTIONS);
    }

    @Override protected void handleChanges(Set<OWLAxiom> added, Set<OWLAxiom> removed) {
        // BUFFERING: an edit invalidates the cache; next query re-runs the subprocess.
        classifyResult = null;
        realizeResult = null;
        propHierResult = null;
        disjointResult = null;
        individualsResult = null;
        propValuesResult = null;
        equivNodeByIri.clear(); directSupers.clear(); directSubs.clear(); unsatisfiable.clear();
        allNamed.clear(); topChildren.clear(); bottomLeaves.clear();
        objEquivByIri.clear(); objDirectSupers.clear(); objDirectSubs.clear();
        objAllNamed.clear(); objTopChildren.clear(); objBottomLeaves.clear();
        dataEquivByIri.clear(); dataDirectSupers.clear(); dataDirectSubs.clear();
        dataAllNamed.clear(); dataTopChildren.clear(); dataBottomLeaves.clear();
        disjointOf.clear(); disjointObjOf.clear(); disjointDataOf.clear();
        sameGroupByIri.clear(); differentOf.clear();
    }

    // ---- index building from classifyResult ----
    private void rebuildIndices() {
        equivNodeByIri.clear(); directSupers.clear(); directSubs.clear(); unsatisfiable.clear();
        allNamed.clear(); topChildren.clear(); bottomLeaves.clear();
        if (classifyResult == null) return;
        for (String iri : orEmpty(classifyResult.unsatisfiable)) unsatisfiable.add(clazz(iri));
        // equivalence nodes
        for (List<String> group : orEmpty(classifyResult.equivalent_groups)) {
            Set<OWLClass> members = new HashSet<>();
            for (String iri : group) members.add(clazz(iri));
            Node<OWLClass> node = new OWLClassNode(members);
            for (OWLClass c : members) equivNodeByIri.put(c.getIRI().toString(), node);
        }
        // direct subsumption edges
        for (List<String> edge : orEmpty(classifyResult.direct_subsumptions)) {
            OWLClass sub = clazz(edge.get(0)), sup = clazz(edge.get(1));
            if (unsatisfiable.contains(sub) || unsatisfiable.contains(sup)) continue; // degenerate ⊥⊑ edge, not a Hasse edge
            directSupers.computeIfAbsent(sub, k -> new HashSet<>()).add(sup);
            directSubs.computeIfAbsent(sup, k -> new HashSet<>()).add(sub);
        }
        // collect every named class mentioned anywhere (signature + JSON), minus Thing/Nothing
        allNamed.addAll(getRootOntology().getClassesInSignature(org.semanticweb.owlapi.model.parameters.Imports.INCLUDED));
        for (String iri : orEmpty(classifyResult.unsatisfiable)) allNamed.add(clazz(iri));
        for (List<String> g : orEmpty(classifyResult.equivalent_groups)) for (String iri : g) allNamed.add(clazz(iri));
        for (List<String> e : orEmpty(classifyResult.direct_subsumptions)) { allNamed.add(clazz(e.get(0))); allNamed.add(clazz(e.get(1))); }
        allNamed.remove(df.getOWLThing());
        allNamed.remove(df.getOWLNothing());
        for (OWLClass c : allNamed) {
            if (unsatisfiable.contains(c)) continue;                 // unsat ≡ Nothing, not a real hierarchy node
            if (directSupers.getOrDefault(c, java.util.Collections.emptySet()).isEmpty()) topChildren.add(c);
            if (directSubs.getOrDefault(c, java.util.Collections.emptySet()).isEmpty()) bottomLeaves.add(c);
        }
    }
    private OWLClass clazz(String iri) { return df.getOWLClass(IRI.create(iri)); }
    private static <T> List<T> orEmpty(List<T> l) { return l == null ? Collections.emptyList() : l; }

    // ---- index building from propHierResult ----
    private void rebuildPropHierIndices() {
        objEquivByIri.clear(); objDirectSupers.clear(); objDirectSubs.clear();
        objAllNamed.clear(); objTopChildren.clear(); objBottomLeaves.clear();
        dataEquivByIri.clear(); dataDirectSupers.clear(); dataDirectSubs.clear();
        dataAllNamed.clear(); dataTopChildren.clear(); dataBottomLeaves.clear();
        if (propHierResult == null) return;
        RustdlJson.PropHierSide obj = propHierResult.object_properties;
        if (obj != null) {
            for (List<String> group : orEmpty(obj.equivalent_groups)) {
                Set<OWLObjectPropertyExpression> members = new HashSet<>();
                for (String iri : group) members.add(objProp(iri));
                Node<OWLObjectPropertyExpression> node = new OWLObjectPropertyNode(members);
                for (OWLObjectPropertyExpression m : members) objEquivByIri.put(m.asOWLObjectProperty().getIRI().toString(), node);
            }
            for (List<String> edge : orEmpty(obj.direct_subsumptions)) {
                OWLObjectProperty sub = objProp(edge.get(0)), sup = objProp(edge.get(1));
                objDirectSupers.computeIfAbsent(sub, k -> new HashSet<>()).add(sup);
                objDirectSubs.computeIfAbsent(sup, k -> new HashSet<>()).add(sub);
            }
            objAllNamed.addAll(getRootOntology().getObjectPropertiesInSignature(org.semanticweb.owlapi.model.parameters.Imports.INCLUDED));
            for (List<String> g : orEmpty(obj.equivalent_groups)) for (String iri : g) objAllNamed.add(objProp(iri));
            for (List<String> e : orEmpty(obj.direct_subsumptions)) { objAllNamed.add(objProp(e.get(0))); objAllNamed.add(objProp(e.get(1))); }
            objAllNamed.remove(df.getOWLTopObjectProperty());
            objAllNamed.remove(df.getOWLBottomObjectProperty());
            for (OWLObjectProperty p : objAllNamed) {
                if (objDirectSupers.getOrDefault(p, Collections.emptySet()).isEmpty()) objTopChildren.add(p);
                if (objDirectSubs.getOrDefault(p, Collections.emptySet()).isEmpty()) objBottomLeaves.add(p);
            }
        }
        RustdlJson.PropHierSide data = propHierResult.data_properties;
        if (data != null) {
            for (List<String> group : orEmpty(data.equivalent_groups)) {
                Set<OWLDataProperty> members = new HashSet<>();
                for (String iri : group) members.add(dataProp(iri));
                Node<OWLDataProperty> node = new OWLDataPropertyNode(members);
                for (OWLDataProperty m : members) dataEquivByIri.put(m.getIRI().toString(), node);
            }
            for (List<String> edge : orEmpty(data.direct_subsumptions)) {
                OWLDataProperty sub = dataProp(edge.get(0)), sup = dataProp(edge.get(1));
                dataDirectSupers.computeIfAbsent(sub, k -> new HashSet<>()).add(sup);
                dataDirectSubs.computeIfAbsent(sup, k -> new HashSet<>()).add(sub);
            }
            dataAllNamed.addAll(getRootOntology().getDataPropertiesInSignature(org.semanticweb.owlapi.model.parameters.Imports.INCLUDED));
            for (List<String> g : orEmpty(data.equivalent_groups)) for (String iri : g) dataAllNamed.add(dataProp(iri));
            for (List<String> e : orEmpty(data.direct_subsumptions)) { dataAllNamed.add(dataProp(e.get(0))); dataAllNamed.add(dataProp(e.get(1))); }
            dataAllNamed.remove(df.getOWLTopDataProperty());
            dataAllNamed.remove(df.getOWLBottomDataProperty());
            for (OWLDataProperty p : dataAllNamed) {
                if (dataDirectSupers.getOrDefault(p, Collections.emptySet()).isEmpty()) dataTopChildren.add(p);
                if (dataDirectSubs.getOrDefault(p, Collections.emptySet()).isEmpty()) dataBottomLeaves.add(p);
            }
        }
    }
    private OWLObjectProperty objProp(String iri) { return df.getOWLObjectProperty(IRI.create(iri)); }
    private OWLDataProperty dataProp(String iri) { return df.getOWLDataProperty(IRI.create(iri)); }

    // ---- index building from disjointResult ----
    /** Symmetric adjacency: each rustdl `[X,Y]` pair means Y is disjoint with X AND X is disjoint with Y. */
    private void rebuildDisjointIndices() {
        disjointOf.clear(); disjointObjOf.clear(); disjointDataOf.clear();
        if (disjointResult == null) return;
        for (List<String> pair : orEmpty(disjointResult.disjoint_classes)) {
            OWLClass a = clazz(pair.get(0)), b = clazz(pair.get(1));
            disjointOf.computeIfAbsent(a, k -> new HashSet<>()).add(b);
            disjointOf.computeIfAbsent(b, k -> new HashSet<>()).add(a);
        }
        for (List<String> pair : orEmpty(disjointResult.disjoint_object_properties)) {
            OWLObjectProperty a = objProp(pair.get(0)), b = objProp(pair.get(1));
            disjointObjOf.computeIfAbsent(a, k -> new HashSet<>()).add(b);
            disjointObjOf.computeIfAbsent(b, k -> new HashSet<>()).add(a);
        }
        for (List<String> pair : orEmpty(disjointResult.disjoint_data_properties)) {
            OWLDataProperty a = dataProp(pair.get(0)), b = dataProp(pair.get(1));
            disjointDataOf.computeIfAbsent(a, k -> new HashSet<>()).add(b);
            disjointDataOf.computeIfAbsent(b, k -> new HashSet<>()).add(a);
        }
    }

    // ---- index building from individualsResult ----
    /**
     * Each `same_groups` entry becomes ONE shared {@link OWLNamedIndividualNode} over its
     * members, keyed by every member's IRI string. `differentOf` is a symmetric adjacency
     * built from `different_pairs` (each `[X,Y]` ⇒ Y ∈ differentOf(X) and X ∈ differentOf(Y)).
     */
    private void rebuildIndividualsIndices() {
        sameGroupByIri.clear(); differentOf.clear();
        if (individualsResult == null) return;
        for (List<String> group : orEmpty(individualsResult.same_groups)) {
            Set<OWLNamedIndividual> members = new HashSet<>();
            for (String iri : group) members.add(namedIndividual(iri));
            Node<OWLNamedIndividual> node = new OWLNamedIndividualNode(members);
            for (OWLNamedIndividual i : members) sameGroupByIri.put(i.getIRI().toString(), node);
        }
        for (List<String> pair : orEmpty(individualsResult.different_pairs)) {
            OWLNamedIndividual a = namedIndividual(pair.get(0)), b = namedIndividual(pair.get(1));
            differentOf.computeIfAbsent(a.getIRI().toString(), k -> new HashSet<>()).add(b);
            differentOf.computeIfAbsent(b.getIRI().toString(), k -> new HashSet<>()).add(a);
        }
    }
    private OWLNamedIndividual namedIndividual(String iri) { return df.getOWLNamedIndividual(IRI.create(iri)); }

    private Node<OWLObjectPropertyExpression> objEquivNodeOf(OWLObjectProperty p) {
        Node<OWLObjectPropertyExpression> n = objEquivByIri.get(p.getIRI().toString());
        return n != null ? n : new OWLObjectPropertyNode(p);
    }
    private Node<OWLDataProperty> dataEquivNodeOf(OWLDataProperty p) {
        Node<OWLDataProperty> n = dataEquivByIri.get(p.getIRI().toString());
        return n != null ? n : new OWLDataPropertyNode(p);
    }
    private Node<OWLObjectPropertyExpression> objTopNode() { return new OWLObjectPropertyNode(df.getOWLTopObjectProperty()); }
    private Node<OWLObjectPropertyExpression> objBottomNode() { return new OWLObjectPropertyNode(df.getOWLBottomObjectProperty()); }
    private Node<OWLDataProperty> dataTopNode() { return new OWLDataPropertyNode(df.getOWLTopDataProperty()); }
    private Node<OWLDataProperty> dataBottomNode() { return new OWLDataPropertyNode(df.getOWLBottomDataProperty()); }
    private Set<Node<OWLObjectPropertyExpression>> objNodesOf(java.util.Collection<OWLObjectProperty> ps) {
        Set<Node<OWLObjectPropertyExpression>> s = new HashSet<>();
        for (OWLObjectProperty p : ps) s.add(objEquivNodeOf(p));
        return s;
    }
    private Set<Node<OWLDataProperty>> dataNodesOf(java.util.Collection<OWLDataProperty> ps) {
        Set<Node<OWLDataProperty>> s = new HashSet<>();
        for (OWLDataProperty p : ps) s.add(dataEquivNodeOf(p));
        return s;
    }
    /** named ancestors/descendants of `start` as equivalence nodes (direct or transitive). Mirrors walkNodes. */
    private Set<Node<OWLObjectPropertyExpression>> walkObjProps(OWLObjectProperty start, Map<OWLObjectProperty, Set<OWLObjectProperty>> edges, boolean direct) {
        Set<OWLObjectProperty> reached = new HashSet<>();
        Deque<OWLObjectProperty> stack = new ArrayDeque<>(edges.getOrDefault(start, java.util.Collections.emptySet()));
        while (!stack.isEmpty()) {
            OWLObjectProperty p = stack.pop();
            if (!reached.add(p)) continue;
            if (!direct) stack.addAll(edges.getOrDefault(p, java.util.Collections.emptySet()));
        }
        return objNodesOf(reached);
    }
    /** named ancestors/descendants of `start` as equivalence nodes (direct or transitive). Mirrors walkNodes. */
    private Set<Node<OWLDataProperty>> walkDataProps(OWLDataProperty start, Map<OWLDataProperty, Set<OWLDataProperty>> edges, boolean direct) {
        Set<OWLDataProperty> reached = new HashSet<>();
        Deque<OWLDataProperty> stack = new ArrayDeque<>(edges.getOrDefault(start, java.util.Collections.emptySet()));
        while (!stack.isEmpty()) {
            OWLDataProperty p = stack.pop();
            if (!reached.add(p)) continue;
            if (!direct) stack.addAll(edges.getOrDefault(p, java.util.Collections.emptySet()));
        }
        return dataNodesOf(reached);
    }

    private Node<OWLClass> equivNodeOf(OWLClass c) {
        if (unsatisfiable.contains(c) || c.isOWLNothing()) return bottomNode();
        Node<OWLClass> n = equivNodeByIri.get(c.getIRI().toString());
        return n != null ? n : new OWLClassNode(c);
    }

    private void throwIfInconsistent() {
        if (!isConsistent()) throw new InconsistentOntologyException();
    }
    private Node<OWLClass> topNode() { return new OWLClassNode(df.getOWLThing()); }
    private Node<OWLClass> bottomNode() {
        Set<OWLClass> all = new HashSet<>(unsatisfiable); all.add(df.getOWLNothing()); return new OWLClassNode(all);
    }
    private Set<Node<OWLClass>> nodesOf(java.util.Collection<OWLClass> cs) {
        Set<Node<OWLClass>> s = new HashSet<>();
        for (OWLClass c : cs) s.add(equivNodeOf(c));
        return s;
    }
    /** named ancestors/descendants of `start` as equivalence nodes (direct or transitive). */
    private Set<Node<OWLClass>> walkNodes(OWLClass start, Map<OWLClass, Set<OWLClass>> edges, boolean direct) {
        Set<OWLClass> reached = new HashSet<>();
        Deque<OWLClass> stack = new ArrayDeque<>(edges.getOrDefault(start, java.util.Collections.emptySet()));
        while (!stack.isEmpty()) {
            OWLClass c = stack.pop();
            if (!reached.add(c)) continue;
            if (!direct) stack.addAll(edges.getOrDefault(c, java.util.Collections.emptySet()));
        }
        return nodesOf(reached);
    }

    // ---- consistency / satisfiability ----
    @Override public boolean isConsistent() {
        ensureClassified();
        return classifyResult.consistent;
    }
    @Override public boolean isSatisfiable(OWLClassExpression ce) {
        if (ce.isAnonymous()) throw new UnsupportedOperationException("rustdl answers satisfiability only for named classes");
        ensureClassified(); throwIfInconsistent();
        OWLClass c = ce.asOWLClass();
        if (c.isOWLNothing()) return false;
        if (c.isOWLThing()) return true;
        return !unsatisfiable.contains(c);
    }

    @Override public Node<OWLClass> getUnsatisfiableClasses() { ensureClassified(); return bottomNode(); }

    // ---- class hierarchy ----
    @Override public Node<OWLClass> getTopClassNode() { return topNode(); }
    @Override public Node<OWLClass> getBottomClassNode() { ensureClassified(); return bottomNode(); }

    @Override public Node<OWLClass> getEquivalentClasses(OWLClassExpression ce) {
        if (ce.isAnonymous()) return new OWLClassNode();
        ensureClassified(); throwIfInconsistent();
        OWLClass c = ce.asOWLClass();
        if (c.isOWLThing()) return topNode();
        if (c.isOWLNothing() || unsatisfiable.contains(c)) return bottomNode();
        return equivNodeOf(c);
    }

    @Override public NodeSet<OWLClass> getSuperClasses(OWLClassExpression ce, boolean direct) {
        if (ce.isAnonymous()) return new OWLClassNodeSet();
        ensureClassified(); throwIfInconsistent();
        OWLClass c = ce.asOWLClass();
        if (c.isOWLThing()) return new OWLClassNodeSet();
        if (c.isOWLNothing() || unsatisfiable.contains(c)) {
            if (direct) {
                return bottomLeaves.isEmpty() ? new OWLClassNodeSet(topNode()) : new OWLClassNodeSet(nodesOf(bottomLeaves));
            }
            Set<Node<OWLClass>> nodes = new HashSet<>();
            for (OWLClass n : allNamed) if (!unsatisfiable.contains(n)) nodes.add(equivNodeOf(n));
            nodes.add(topNode());
            return new OWLClassNodeSet(nodes);
        }
        if (direct) {
            if (directSupers.getOrDefault(c, java.util.Collections.emptySet()).isEmpty()) return new OWLClassNodeSet(topNode());
            return new OWLClassNodeSet(walkNodes(c, directSupers, true));
        }
        Set<Node<OWLClass>> nodes = walkNodes(c, directSupers, false);
        nodes.add(topNode());
        return new OWLClassNodeSet(nodes);
    }

    @Override public NodeSet<OWLClass> getSubClasses(OWLClassExpression ce, boolean direct) {
        if (ce.isAnonymous()) return new OWLClassNodeSet();
        ensureClassified(); throwIfInconsistent();
        OWLClass c = ce.asOWLClass();
        if (c.isOWLNothing()) return new OWLClassNodeSet();
        if (c.isOWLThing()) {
            if (direct) {
                return topChildren.isEmpty() ? new OWLClassNodeSet(bottomNode()) : new OWLClassNodeSet(nodesOf(topChildren));
            }
            Set<Node<OWLClass>> nodes = new HashSet<>();
            for (OWLClass n : allNamed) if (!unsatisfiable.contains(n)) nodes.add(equivNodeOf(n));
            nodes.add(bottomNode());
            return new OWLClassNodeSet(nodes);
        }
        if (unsatisfiable.contains(c)) return new OWLClassNodeSet();  // unsat ≡ Nothing: no proper subclasses
        if (direct) {
            if (directSubs.getOrDefault(c, java.util.Collections.emptySet()).isEmpty()) return new OWLClassNodeSet(bottomNode());
            return new OWLClassNodeSet(walkNodes(c, directSubs, true));
        }
        Set<Node<OWLClass>> nodes = walkNodes(c, directSubs, false);
        nodes.add(bottomNode());
        return new OWLClassNodeSet(nodes);
    }

    /**
     * v1 reports the directly-entailed disjoint NAMED classes from rustdl's `disjoint_classes`
     * pairs, grouped into their equivalence nodes — a sound under-approximation. (The full
     * OWLReasoner contract also expects subclasses of each disjoint class in the result; that
     * transitive closure over directSubs is not computed here yet.)
     */
    @Override public NodeSet<OWLClass> getDisjointClasses(OWLClassExpression ce) {
        if (ce.isAnonymous()) return new OWLClassNodeSet();
        ensureDisjoint(); throwIfInconsistent();
        Set<Node<OWLClass>> nodes = new HashSet<>();
        for (OWLClass d : disjointOf.getOrDefault(ce.asOWLClass(), Collections.emptySet())) {
            nodes.add(equivNodeOf(d));
        }
        return new OWLClassNodeSet(nodes);
    }

    // ---- individuals ----
    @Override public NodeSet<OWLClass> getTypes(OWLNamedIndividual ind, boolean direct) {
        ensureClassified(); throwIfInconsistent(); ensureRealized();
        if (realizeResult == null) return new OWLClassNodeSet();
        for (RustdlJson.IndividualJson i : orEmpty(realizeResult.individuals)) {
            if (i.iri.equals(ind.getIRI().toString())) {
                List<String> src = direct ? i.direct_types : i.types;
                Set<Node<OWLClass>> nodes = new HashSet<>();
                for (String iri : orEmpty(src)) nodes.add(equivNodeOf(clazz(iri)));
                return new OWLClassNodeSet(nodes);
            }
        }
        return new OWLClassNodeSet();
    }
    @Override public NodeSet<OWLNamedIndividual> getInstances(OWLClassExpression ce, boolean direct) {
        if (ce.isAnonymous()) return new OWLNamedIndividualNodeSet();
        ensureClassified(); throwIfInconsistent(); ensureRealized();
        if (realizeResult == null) return new OWLNamedIndividualNodeSet();
        String target = ce.asOWLClass().getIRI().toString();
        Set<Node<OWLNamedIndividual>> nodes = new HashSet<>();
        for (RustdlJson.IndividualJson i : orEmpty(realizeResult.individuals)) {
            List<String> src = direct ? i.direct_types : i.types;
            if (orEmpty(src).contains(target)) {
                nodes.add(new OWLNamedIndividualNode(df.getOWLNamedIndividual(IRI.create(i.iri))));
            }
        }
        return new OWLNamedIndividualNodeSet(nodes);
    }

    // ---- entailment ----
    @Override public boolean isEntailmentCheckingSupported(AxiomType<?> axiomType) {
        return axiomType == AxiomType.SUBCLASS_OF;
    }
    @Override public boolean isEntailed(OWLAxiom axiom) {
        ensureClassified(); throwIfInconsistent();
        if (axiom instanceof OWLSubClassOfAxiom) {
            OWLSubClassOfAxiom sc = (OWLSubClassOfAxiom) axiom;
            if (sc.getSubClass().isAnonymous() || sc.getSuperClass().isAnonymous()) {
                throw new UnsupportedOperationException("rustdl entails only named SubClassOf");
            }
            if (sc.getSuperClass().asOWLClass().isOWLThing()) return true;
            return getSuperClasses(sc.getSubClass(), false).containsEntity(sc.getSuperClass().asOWLClass())
                || getEquivalentClasses(sc.getSubClass()).contains(sc.getSuperClass().asOWLClass());
        }
        throw new UnsupportedOperationException("rustdl entails only SubClassOf axioms");
    }
    @Override public boolean isEntailed(Set<? extends OWLAxiom> axioms) {
        for (OWLAxiom a : axioms) if (!isEntailed(a)) return false;
        return true;
    }

    // ---- interrupt / precompute misc ----
    @Override public void interrupt() { /* subprocess is bounded by timeout; nothing to interrupt mid-call */ }

    // ---- object/data property hierarchy (cache-backed) ----
    @Override public Node<OWLObjectPropertyExpression> getTopObjectPropertyNode() { return objTopNode(); }
    @Override public Node<OWLObjectPropertyExpression> getBottomObjectPropertyNode() { return objBottomNode(); }

    @Override public Node<OWLObjectPropertyExpression> getEquivalentObjectProperties(OWLObjectPropertyExpression pe) {
        if (pe.isAnonymous()) return new OWLObjectPropertyNode();
        ensurePropHier(); throwIfInconsistent();
        OWLObjectProperty p = pe.asOWLObjectProperty();
        if (p.isOWLTopObjectProperty()) return objTopNode();
        if (p.isOWLBottomObjectProperty()) return objBottomNode();
        return objEquivNodeOf(p);
    }

    @Override public NodeSet<OWLObjectPropertyExpression> getSuperObjectProperties(OWLObjectPropertyExpression pe, boolean direct) {
        if (pe.isAnonymous()) return new OWLObjectPropertyNodeSet();
        ensurePropHier(); throwIfInconsistent();
        OWLObjectProperty p = pe.asOWLObjectProperty();
        if (p.isOWLTopObjectProperty()) return new OWLObjectPropertyNodeSet();
        if (p.isOWLBottomObjectProperty()) {
            if (direct) {
                return objBottomLeaves.isEmpty() ? new OWLObjectPropertyNodeSet(objTopNode()) : new OWLObjectPropertyNodeSet(objNodesOf(objBottomLeaves));
            }
            Set<Node<OWLObjectPropertyExpression>> nodes = new HashSet<>();
            for (OWLObjectProperty n : objAllNamed) nodes.add(objEquivNodeOf(n));
            nodes.add(objTopNode());
            return new OWLObjectPropertyNodeSet(nodes);
        }
        if (direct) {
            if (objDirectSupers.getOrDefault(p, Collections.emptySet()).isEmpty()) return new OWLObjectPropertyNodeSet(objTopNode());
            return new OWLObjectPropertyNodeSet(walkObjProps(p, objDirectSupers, true));
        }
        Set<Node<OWLObjectPropertyExpression>> nodes = walkObjProps(p, objDirectSupers, false);
        nodes.add(objTopNode());
        return new OWLObjectPropertyNodeSet(nodes);
    }

    @Override public NodeSet<OWLObjectPropertyExpression> getSubObjectProperties(OWLObjectPropertyExpression pe, boolean direct) {
        if (pe.isAnonymous()) return new OWLObjectPropertyNodeSet();
        ensurePropHier(); throwIfInconsistent();
        OWLObjectProperty p = pe.asOWLObjectProperty();
        if (p.isOWLBottomObjectProperty()) return new OWLObjectPropertyNodeSet();
        if (p.isOWLTopObjectProperty()) {
            if (direct) {
                return objTopChildren.isEmpty() ? new OWLObjectPropertyNodeSet(objBottomNode()) : new OWLObjectPropertyNodeSet(objNodesOf(objTopChildren));
            }
            Set<Node<OWLObjectPropertyExpression>> nodes = new HashSet<>();
            for (OWLObjectProperty n : objAllNamed) nodes.add(objEquivNodeOf(n));
            nodes.add(objBottomNode());
            return new OWLObjectPropertyNodeSet(nodes);
        }
        if (direct) {
            if (objDirectSubs.getOrDefault(p, Collections.emptySet()).isEmpty()) return new OWLObjectPropertyNodeSet(objBottomNode());
            return new OWLObjectPropertyNodeSet(walkObjProps(p, objDirectSubs, true));
        }
        Set<Node<OWLObjectPropertyExpression>> nodes = walkObjProps(p, objDirectSubs, false);
        nodes.add(objBottomNode());
        return new OWLObjectPropertyNodeSet(nodes);
    }

    /** v1 reports the directly-entailed disjoint NAMED object properties (sound under-approximation; no subproperty closure). */
    @Override public NodeSet<OWLObjectPropertyExpression> getDisjointObjectProperties(OWLObjectPropertyExpression pe) {
        if (pe.isAnonymous()) return new OWLObjectPropertyNodeSet();
        ensureDisjoint(); throwIfInconsistent();
        Set<Node<OWLObjectPropertyExpression>> nodes = new HashSet<>();
        for (OWLObjectProperty d : disjointObjOf.getOrDefault(pe.asOWLObjectProperty(), Collections.emptySet())) {
            nodes.add(objEquivNodeOf(d));
        }
        return new OWLObjectPropertyNodeSet(nodes);
    }
    @Override public Node<OWLObjectPropertyExpression> getInverseObjectProperties(OWLObjectPropertyExpression pe) { return new OWLObjectPropertyNode(pe.getInverseProperty()); }
    @Override public NodeSet<OWLClass> getObjectPropertyDomains(OWLObjectPropertyExpression pe, boolean direct) { return new OWLClassNodeSet(); }
    @Override public NodeSet<OWLClass> getObjectPropertyRanges(OWLObjectPropertyExpression pe, boolean direct) { return new OWLClassNodeSet(); }

    @Override public Node<OWLDataProperty> getTopDataPropertyNode() { return dataTopNode(); }
    @Override public Node<OWLDataProperty> getBottomDataPropertyNode() { return dataBottomNode(); }

    @Override public Node<OWLDataProperty> getEquivalentDataProperties(OWLDataProperty p) {
        ensurePropHier(); throwIfInconsistent();
        if (p.isOWLTopDataProperty()) return dataTopNode();
        if (p.isOWLBottomDataProperty()) return dataBottomNode();
        return dataEquivNodeOf(p);
    }

    @Override public NodeSet<OWLDataProperty> getSuperDataProperties(OWLDataProperty p, boolean direct) {
        ensurePropHier(); throwIfInconsistent();
        if (p.isOWLTopDataProperty()) return new OWLDataPropertyNodeSet();
        if (p.isOWLBottomDataProperty()) {
            if (direct) {
                return dataBottomLeaves.isEmpty() ? new OWLDataPropertyNodeSet(dataTopNode()) : new OWLDataPropertyNodeSet(dataNodesOf(dataBottomLeaves));
            }
            Set<Node<OWLDataProperty>> nodes = new HashSet<>();
            for (OWLDataProperty n : dataAllNamed) nodes.add(dataEquivNodeOf(n));
            nodes.add(dataTopNode());
            return new OWLDataPropertyNodeSet(nodes);
        }
        if (direct) {
            if (dataDirectSupers.getOrDefault(p, Collections.emptySet()).isEmpty()) return new OWLDataPropertyNodeSet(dataTopNode());
            return new OWLDataPropertyNodeSet(walkDataProps(p, dataDirectSupers, true));
        }
        Set<Node<OWLDataProperty>> nodes = walkDataProps(p, dataDirectSupers, false);
        nodes.add(dataTopNode());
        return new OWLDataPropertyNodeSet(nodes);
    }

    @Override public NodeSet<OWLDataProperty> getSubDataProperties(OWLDataProperty p, boolean direct) {
        ensurePropHier(); throwIfInconsistent();
        if (p.isOWLBottomDataProperty()) return new OWLDataPropertyNodeSet();
        if (p.isOWLTopDataProperty()) {
            if (direct) {
                return dataTopChildren.isEmpty() ? new OWLDataPropertyNodeSet(dataBottomNode()) : new OWLDataPropertyNodeSet(dataNodesOf(dataTopChildren));
            }
            Set<Node<OWLDataProperty>> nodes = new HashSet<>();
            for (OWLDataProperty n : dataAllNamed) nodes.add(dataEquivNodeOf(n));
            nodes.add(dataBottomNode());
            return new OWLDataPropertyNodeSet(nodes);
        }
        if (direct) {
            if (dataDirectSubs.getOrDefault(p, Collections.emptySet()).isEmpty()) return new OWLDataPropertyNodeSet(dataBottomNode());
            return new OWLDataPropertyNodeSet(walkDataProps(p, dataDirectSubs, true));
        }
        Set<Node<OWLDataProperty>> nodes = walkDataProps(p, dataDirectSubs, false);
        nodes.add(dataBottomNode());
        return new OWLDataPropertyNodeSet(nodes);
    }

    /** v1 reports the directly-entailed disjoint NAMED data properties (sound under-approximation; no subproperty closure). */
    @Override public NodeSet<OWLDataProperty> getDisjointDataProperties(OWLDataPropertyExpression pe) {
        if (pe.isAnonymous()) return new OWLDataPropertyNodeSet();
        ensureDisjoint(); throwIfInconsistent();
        Set<Node<OWLDataProperty>> nodes = new HashSet<>();
        for (OWLDataProperty d : disjointDataOf.getOrDefault(pe.asOWLDataProperty(), Collections.emptySet())) {
            nodes.add(dataEquivNodeOf(d));
        }
        return new OWLDataPropertyNodeSet(nodes);
    }
    @Override public NodeSet<OWLClass> getDataPropertyDomains(OWLDataProperty pe, boolean direct) { return new OWLClassNodeSet(); }
    @Override public NodeSet<OWLNamedIndividual> getObjectPropertyValues(OWLNamedIndividual ind, OWLObjectPropertyExpression pe) {
        if (pe.isAnonymous()) return new OWLNamedIndividualNodeSet();
        ensurePropValues(); throwIfInconsistent();
        String s = ind.getIRI().toString(), p = pe.asOWLObjectProperty().getIRI().toString();
        Set<Node<OWLNamedIndividual>> nodes = new HashSet<>();
        for (List<String> t : orEmpty(propValuesResult == null ? null : propValuesResult.object_property_values)) {
            if (t.get(0).equals(s) && t.get(1).equals(p))
                nodes.add(new OWLNamedIndividualNode(df.getOWLNamedIndividual(IRI.create(t.get(2)))));
        }
        return new OWLNamedIndividualNodeSet(nodes);
    }
    @Override public Set<OWLLiteral> getDataPropertyValues(OWLNamedIndividual ind, OWLDataProperty pe) {
        ensurePropValues(); throwIfInconsistent();
        String s = ind.getIRI().toString(), p = pe.getIRI().toString();
        Set<OWLLiteral> out = new HashSet<>();
        for (List<String> q : orEmpty(propValuesResult == null ? null : propValuesResult.data_property_values)) {
            if (q.get(0).equals(s) && q.get(1).equals(p))
                out.add(df.getOWLLiteral(q.get(2), df.getOWLDatatype(IRI.create(q.get(3)))));   // lexical + datatype
        }
        return out;
    }
    @Override public Node<OWLNamedIndividual> getSameIndividuals(OWLNamedIndividual ind) {
        ensureIndividuals(); throwIfInconsistent();
        Node<OWLNamedIndividual> n = sameGroupByIri.get(ind.getIRI().toString());
        return n != null ? n : new OWLNamedIndividualNode(ind);   // contract: the node always contains ind itself
    }
    @Override public NodeSet<OWLNamedIndividual> getDifferentIndividuals(OWLNamedIndividual ind) {
        ensureIndividuals(); throwIfInconsistent();
        Set<Node<OWLNamedIndividual>> nodes = new HashSet<>();
        for (OWLNamedIndividual d : differentOf.getOrDefault(ind.getIRI().toString(), Collections.emptySet())) {
            nodes.add(new OWLNamedIndividualNode(d));
        }
        return new OWLNamedIndividualNodeSet(nodes);
    }
}
