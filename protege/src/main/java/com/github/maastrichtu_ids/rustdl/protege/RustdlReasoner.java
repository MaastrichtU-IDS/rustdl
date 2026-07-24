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

    // Cache, populated by precompute (or injected in tests).
    private RustdlJson.ClassifyJson classifyResult;
    private RustdlJson.RealizeJson realizeResult;

    // Derived indices built from classifyResult.
    private final Map<String, Node<OWLClass>> equivNodeByIri = new HashMap<>();   // iri -> its equiv-class node
    private final Map<OWLClass, Set<OWLClass>> directSupers = new HashMap<>();
    private final Map<OWLClass, Set<OWLClass>> directSubs = new HashMap<>();
    private final Set<OWLClass> unsatisfiable = new HashSet<>();

    RustdlReasoner(OWLOntology rootOntology, OWLReasonerConfiguration config, BufferingMode mode) {
        super(rootOntology, config, mode);
        this.df = rootOntology.getOWLOntologyManager().getOWLDataFactory();
        this.timeoutSec = resolveTimeout(config);
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

    private static long resolveTimeout(OWLReasonerConfiguration config) {
        String p = System.getProperty("rustdl.timeout.seconds");
        if (p == null || p.isEmpty()) p = System.getenv("RUSTDL_TIMEOUT_SECONDS");
        if (p != null && !p.isEmpty()) try { return Long.parseLong(p); } catch (NumberFormatException ignored) {}
        return 600L;
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
            if (wantHierarchy && classifyResult == null) {
                classifyResult = RustdlProcess.classify(ofn, timeoutSec);
                if (classifyResult.incomplete) {
                    LOG.warning("rustdl reports an INCOMPLETE classification (some class pairs timed out); "
                        + "the hierarchy is a sound under-approximation.");
                }
                rebuildIndices();
            }
            if (wantAssertions && realizeResult == null
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

    @Override protected void handleChanges(Set<OWLAxiom> added, Set<OWLAxiom> removed) {
        // BUFFERING: an edit invalidates the cache; next query re-runs the subprocess.
        classifyResult = null;
        realizeResult = null;
        equivNodeByIri.clear(); directSupers.clear(); directSubs.clear(); unsatisfiable.clear();
    }

    // ---- index building from classifyResult ----
    private void rebuildIndices() {
        equivNodeByIri.clear(); directSupers.clear(); directSubs.clear(); unsatisfiable.clear();
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
            directSupers.computeIfAbsent(sub, k -> new HashSet<>()).add(sup);
            directSubs.computeIfAbsent(sup, k -> new HashSet<>()).add(sub);
        }
    }
    private OWLClass clazz(String iri) { return df.getOWLClass(IRI.create(iri)); }
    private static <T> List<T> orEmpty(List<T> l) { return l == null ? Collections.emptyList() : l; }

    private Node<OWLClass> equivNodeOf(OWLClass c) {
        Node<OWLClass> n = equivNodeByIri.get(c.getIRI().toString());
        return n != null ? n : new OWLClassNode(c);
    }

    // ---- consistency / satisfiability ----
    @Override public boolean isConsistent() {
        ensureClassified();
        return classifyResult.consistent;
    }
    @Override public boolean isSatisfiable(OWLClassExpression ce) {
        if (ce.isAnonymous()) {
            throw new UnsupportedOperationException(
                "rustdl answers satisfiability only for named classes");
        }
        ensureClassified();
        if (!isConsistent()) throw new InconsistentOntologyException();
        return !unsatisfiable.contains(ce.asOWLClass());
    }
    @Override public Node<OWLClass> getUnsatisfiableClasses() {
        ensureClassified();
        Set<OWLClass> all = new HashSet<>(unsatisfiable);
        all.add(df.getOWLNothing());
        return new OWLClassNode(all);
    }

    // ---- class hierarchy ----
    @Override public Node<OWLClass> getTopClassNode() { return new OWLClassNode(df.getOWLThing()); }
    @Override public Node<OWLClass> getBottomClassNode() { return getUnsatisfiableClasses(); }

    @Override public Node<OWLClass> getEquivalentClasses(OWLClassExpression ce) {
        if (ce.isAnonymous()) return new OWLClassNode();
        ensureClassified();
        return equivNodeOf(ce.asOWLClass());
    }

    @Override public NodeSet<OWLClass> getSuperClasses(OWLClassExpression ce, boolean direct) {
        if (ce.isAnonymous()) return new OWLClassNodeSet();
        ensureClassified();
        return walk(ce.asOWLClass(), directSupers, direct);
    }
    @Override public NodeSet<OWLClass> getSubClasses(OWLClassExpression ce, boolean direct) {
        if (ce.isAnonymous()) return new OWLClassNodeSet();
        ensureClassified();
        return walk(ce.asOWLClass(), directSubs, direct);
    }

    /** direct=true → the immediate edges; direct=false → transitive closure, grouped into equiv nodes. */
    private NodeSet<OWLClass> walk(OWLClass start, Map<OWLClass, Set<OWLClass>> edges, boolean direct) {
        Set<OWLClass> reached = new HashSet<>();
        Deque<OWLClass> stack = new ArrayDeque<>(edges.getOrDefault(start, Collections.emptySet()));
        while (!stack.isEmpty()) {
            OWLClass c = stack.pop();
            if (!reached.add(c)) continue;
            if (!direct) stack.addAll(edges.getOrDefault(c, Collections.emptySet()));
        }
        Set<Node<OWLClass>> nodes = new HashSet<>();
        for (OWLClass c : reached) nodes.add(equivNodeOf(c));
        return new OWLClassNodeSet(nodes);
    }
    @Override public NodeSet<OWLClass> getDisjointClasses(OWLClassExpression ce) { return new OWLClassNodeSet(); }

    // ---- individuals ----
    @Override public NodeSet<OWLClass> getTypes(OWLNamedIndividual ind, boolean direct) {
        ensureRealized();
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
        ensureRealized();
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

    // ---- unsupported node-set queries → empty (sound under-approximation) ----
    @Override public Node<OWLObjectPropertyExpression> getTopObjectPropertyNode() { return new OWLObjectPropertyNode(df.getOWLTopObjectProperty()); }
    @Override public Node<OWLObjectPropertyExpression> getBottomObjectPropertyNode() { return new OWLObjectPropertyNode(df.getOWLBottomObjectProperty()); }
    @Override public NodeSet<OWLObjectPropertyExpression> getSubObjectProperties(OWLObjectPropertyExpression pe, boolean direct) { return new OWLObjectPropertyNodeSet(); }
    @Override public NodeSet<OWLObjectPropertyExpression> getSuperObjectProperties(OWLObjectPropertyExpression pe, boolean direct) { return new OWLObjectPropertyNodeSet(); }
    @Override public Node<OWLObjectPropertyExpression> getEquivalentObjectProperties(OWLObjectPropertyExpression pe) { return new OWLObjectPropertyNode(pe); }
    @Override public NodeSet<OWLObjectPropertyExpression> getDisjointObjectProperties(OWLObjectPropertyExpression pe) { return new OWLObjectPropertyNodeSet(); }
    @Override public Node<OWLObjectPropertyExpression> getInverseObjectProperties(OWLObjectPropertyExpression pe) { return new OWLObjectPropertyNode(pe.getInverseProperty()); }
    @Override public NodeSet<OWLClass> getObjectPropertyDomains(OWLObjectPropertyExpression pe, boolean direct) { return new OWLClassNodeSet(); }
    @Override public NodeSet<OWLClass> getObjectPropertyRanges(OWLObjectPropertyExpression pe, boolean direct) { return new OWLClassNodeSet(); }
    @Override public Node<OWLDataProperty> getTopDataPropertyNode() { return new OWLDataPropertyNode(df.getOWLTopDataProperty()); }
    @Override public Node<OWLDataProperty> getBottomDataPropertyNode() { return new OWLDataPropertyNode(df.getOWLBottomDataProperty()); }
    @Override public NodeSet<OWLDataProperty> getSubDataProperties(OWLDataProperty pe, boolean direct) { return new OWLDataPropertyNodeSet(); }
    @Override public NodeSet<OWLDataProperty> getSuperDataProperties(OWLDataProperty pe, boolean direct) { return new OWLDataPropertyNodeSet(); }
    @Override public Node<OWLDataProperty> getEquivalentDataProperties(OWLDataProperty pe) { return new OWLDataPropertyNode(pe); }
    @Override public NodeSet<OWLDataProperty> getDisjointDataProperties(OWLDataPropertyExpression pe) { return new OWLDataPropertyNodeSet(); }
    @Override public NodeSet<OWLClass> getDataPropertyDomains(OWLDataProperty pe, boolean direct) { return new OWLClassNodeSet(); }
    @Override public NodeSet<OWLNamedIndividual> getObjectPropertyValues(OWLNamedIndividual ind, OWLObjectPropertyExpression pe) { return new OWLNamedIndividualNodeSet(); }
    @Override public Set<OWLLiteral> getDataPropertyValues(OWLNamedIndividual ind, OWLDataProperty pe) { return Collections.emptySet(); }
    @Override public Node<OWLNamedIndividual> getSameIndividuals(OWLNamedIndividual ind) { return new OWLNamedIndividualNode(ind); }
    @Override public NodeSet<OWLNamedIndividual> getDifferentIndividuals(OWLNamedIndividual ind) { return new OWLNamedIndividualNodeSet(); }
}
