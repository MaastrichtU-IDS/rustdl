package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.*;
import org.semanticweb.owlapi.reasoner.*;
import java.util.*;
import static org.junit.Assert.*;

public class RustdlReasonerTest {
    private final OWLOntologyManager m = OWLManager.createOWLOntologyManager();
    private final OWLDataFactory df = m.getOWLDataFactory();
    private OWLClass cls(String i) { return df.getOWLClass(IRI.create("http://ex/#" + i)); }

    private RustdlReasoner reasoner() throws Exception {
        // A fresh manager per call: some test methods (e.g. types()) call reasoner()
        // more than once, and OWLOntologyManager#createOntology throws
        // OWLOntologyAlreadyExistsException on a second createOntology with the same IRI.
        OWLOntology o = OWLManager.createOWLOntologyManager().createOntology(IRI.create("http://ex/"));
        RustdlJson.ClassifyJson c = new RustdlJson.ClassifyJson();
        c.schema_version = 1; c.consistent = true; c.incomplete = false;
        c.unsatisfiable = new ArrayList<>();
        c.equivalent_groups = Arrays.asList(Arrays.asList("http://ex/#A", "http://ex/#B"));
        c.direct_subsumptions = Arrays.asList(Arrays.asList("http://ex/#A", "http://ex/#C"));
        RustdlJson.RealizeJson r = new RustdlJson.RealizeJson();
        r.schema_version = 1;
        RustdlJson.IndividualJson ind = new RustdlJson.IndividualJson();
        ind.iri = "http://ex/#i"; ind.types = Arrays.asList("http://ex/#A", "http://ex/#C");
        ind.direct_types = Arrays.asList("http://ex/#A");
        r.individuals = Arrays.asList(ind);
        return RustdlReasoner.forTest(o, c, r);
    }

    @Test public void consistent() throws Exception { assertTrue(reasoner().isConsistent()); }

    @Test public void equivalentClasses() throws Exception {
        Node<OWLClass> eq = reasoner().getEquivalentClasses(cls("A"));
        assertTrue(eq.contains(cls("B")));
    }

    @Test public void directSuperClasses() throws Exception {
        NodeSet<OWLClass> supers = reasoner().getSuperClasses(cls("A"), true);
        assertTrue(supers.containsEntity(cls("C")));
    }

    @Test public void directSubClasses() throws Exception {
        NodeSet<OWLClass> subs = reasoner().getSubClasses(cls("C"), true);
        assertTrue(subs.containsEntity(cls("A")));
    }

    @Test public void types() throws Exception {
        OWLNamedIndividual i = df.getOWLNamedIndividual(IRI.create("http://ex/#i"));
        assertTrue(reasoner().getTypes(i, true).containsEntity(cls("A")));
        assertTrue(reasoner().getTypes(i, false).containsEntity(cls("C")));
    }

    @Test public void instances() throws Exception {
        NodeSet<OWLNamedIndividual> insts = reasoner().getInstances(cls("A"), false);
        assertTrue(insts.containsEntity(df.getOWLNamedIndividual(IRI.create("http://ex/#i"))));
    }

    @Test(expected = UnsupportedOperationException.class)
    public void complexSatisfiabilityThrows() throws Exception {
        reasoner().isSatisfiable(df.getOWLObjectIntersectionOf(cls("A"), cls("C")));
    }

    @Test public void unsupportedReturnsEmpty() throws Exception {
        assertTrue(reasoner().getObjectPropertyValues(
            df.getOWLNamedIndividual(IRI.create("http://ex/#i")),
            df.getOWLObjectProperty(IRI.create("http://ex/#p"))).isEmpty());
    }

    private RustdlReasoner chain() throws Exception {
        OWLOntology o = OWLManager.createOWLOntologyManager().createOntology(IRI.create("http://ex/chain"));
        RustdlJson.ClassifyJson c = new RustdlJson.ClassifyJson();
        c.schema_version = 1; c.consistent = true; c.incomplete = false;
        c.unsatisfiable = new java.util.ArrayList<>(); c.equivalent_groups = new java.util.ArrayList<>();
        c.direct_subsumptions = java.util.Arrays.asList(
            java.util.Arrays.asList("http://ex/#A", "http://ex/#B"),
            java.util.Arrays.asList("http://ex/#B", "http://ex/#C"));
        RustdlJson.RealizeJson r = new RustdlJson.RealizeJson(); r.schema_version = 1; r.individuals = new java.util.ArrayList<>();
        return RustdlReasoner.forTest(o, c, r);
    }
    private RustdlReasoner inconsistentReasoner() throws Exception {
        OWLOntology o = OWLManager.createOWLOntologyManager().createOntology(IRI.create("http://ex/inc"));
        RustdlJson.ClassifyJson c = new RustdlJson.ClassifyJson();
        c.schema_version = 1; c.consistent = false;
        c.unsatisfiable = new java.util.ArrayList<>(); c.equivalent_groups = new java.util.ArrayList<>(); c.direct_subsumptions = new java.util.ArrayList<>();
        RustdlJson.RealizeJson r = new RustdlJson.RealizeJson(); r.schema_version = 1; r.individuals = new java.util.ArrayList<>();
        return RustdlReasoner.forTest(o, c, r);
    }

    /**
     * Same as inconsistentReasoner() but with a non-null, populated realizeResult:
     * proves getTypes/getInstances throw InconsistentOntologyException from the
     * consistency gate BEFORE ever reading realizeResult (i.e. the fix reorders
     * ensureClassified/throwIfInconsistent ahead of ensureRealized).
     */
    private RustdlReasoner inconsistentReasonerWithRealize() throws Exception {
        OWLOntology o = OWLManager.createOWLOntologyManager().createOntology(IRI.create("http://ex/inc2"));
        RustdlJson.ClassifyJson c = new RustdlJson.ClassifyJson();
        c.schema_version = 1; c.consistent = false;
        c.unsatisfiable = new java.util.ArrayList<>(); c.equivalent_groups = new java.util.ArrayList<>(); c.direct_subsumptions = new java.util.ArrayList<>();
        RustdlJson.RealizeJson r = new RustdlJson.RealizeJson();
        r.schema_version = 1;
        RustdlJson.IndividualJson ind = new RustdlJson.IndividualJson();
        ind.iri = "http://ex/#i"; ind.types = Arrays.asList("http://ex/#A");
        ind.direct_types = Arrays.asList("http://ex/#A");
        r.individuals = Arrays.asList(ind);
        return RustdlReasoner.forTest(o, c, r);
    }

    @Test(expected = org.semanticweb.owlapi.reasoner.InconsistentOntologyException.class)
    public void inconsistentThrowsOnTypesBeforeRealize() throws Exception {
        OWLNamedIndividual i = df.getOWLNamedIndividual(IRI.create("http://ex/#i"));
        inconsistentReasonerWithRealize().getTypes(i, false);
    }

    @Test(expected = org.semanticweb.owlapi.reasoner.InconsistentOntologyException.class)
    public void inconsistentThrowsOnInstancesBeforeRealize() throws Exception {
        inconsistentReasonerWithRealize().getInstances(cls("A"), false);
    }

    @Test public void transitiveSuperClasses() throws Exception {
        NodeSet<OWLClass> s = chain().getSuperClasses(cls("A"), false);
        assertTrue(s.containsEntity(cls("B"))); assertTrue(s.containsEntity(cls("C")));
        assertTrue("Thing must be an ancestor", s.containsEntity(df.getOWLThing()));
    }
    @Test public void transitiveSubClasses() throws Exception {
        NodeSet<OWLClass> s = chain().getSubClasses(cls("C"), false);
        assertTrue(s.containsEntity(cls("A"))); assertTrue(s.containsEntity(cls("B")));
        assertTrue("Nothing must be a descendant", s.containsEntity(df.getOWLNothing()));
    }
    @Test public void topLevelClassIsDirectSubOfThing() throws Exception {
        assertTrue(chain().getSubClasses(df.getOWLThing(), true).containsEntity(cls("C")));
        assertTrue(chain().getSuperClasses(cls("C"), true).containsEntity(df.getOWLThing()));
    }
    @Test public void leafDirectSubIsNothing() throws Exception {
        assertTrue(chain().getSubClasses(cls("A"), true).containsEntity(df.getOWLNothing()));
    }
    @Test public void nothingIsNotSatisfiable() throws Exception {
        assertFalse(reasoner().isSatisfiable(df.getOWLNothing()));
        assertTrue(reasoner().isSatisfiable(df.getOWLThing()));
    }
    @Test(expected = org.semanticweb.owlapi.reasoner.InconsistentOntologyException.class)
    public void inconsistentThrowsOnSuperClasses() throws Exception {
        inconsistentReasoner().getSuperClasses(cls("A"), false);
    }
    @Test(expected = org.semanticweb.owlapi.reasoner.InconsistentOntologyException.class)
    public void inconsistentThrowsOnInstances() throws Exception {
        inconsistentReasoner().getInstances(cls("A"), false);
    }
    @Test public void handleChangesInvalidatesCache() throws Exception {
        RustdlReasoner reasoner = chain();
        assertTrue(reasoner.isPrecomputed(org.semanticweb.owlapi.reasoner.InferenceType.CLASS_HIERARCHY));
        OWLOntology root = reasoner.getRootOntology();
        root.getOWLOntologyManager().addAxiom(root, df.getOWLSubClassOfAxiom(cls("X"), cls("Y")));
        reasoner.flush();
        assertFalse(reasoner.isPrecomputed(org.semanticweb.owlapi.reasoner.InferenceType.CLASS_HIERARCHY));
    }

    private RustdlReasoner withUnsat() throws Exception {
        // A ⊑ B (both satisfiable); U unsatisfiable. rustdl emits a phantom [U,A] ⊥⊑ edge.
        OWLOntology o = OWLManager.createOWLOntologyManager().createOntology(IRI.create("http://ex/unsat"));
        RustdlJson.ClassifyJson c = new RustdlJson.ClassifyJson();
        c.schema_version = 1; c.consistent = true; c.incomplete = false;
        c.unsatisfiable = java.util.Arrays.asList("http://ex/#U");
        c.equivalent_groups = new java.util.ArrayList<>();
        c.direct_subsumptions = java.util.Arrays.asList(
            java.util.Arrays.asList("http://ex/#A", "http://ex/#B"),   // genuine Hasse edge
            java.util.Arrays.asList("http://ex/#U", "http://ex/#A"));  // phantom ⊥⊑ edge
        RustdlJson.RealizeJson r = new RustdlJson.RealizeJson(); r.schema_version = 1; r.individuals = new java.util.ArrayList<>();
        return RustdlReasoner.forTest(o, c, r);
    }

    private OWLObjectProperty objProp(String i) { return df.getOWLObjectProperty(IRI.create("http://ex/#" + i)); }
    private OWLDataProperty dataProp(String i) { return df.getOWLDataProperty(IRI.create("http://ex/#" + i)); }

    /** Loads the shared prophier.json fixture: object p ⊑ r, p ≡ p2; data d ⊑ e. */
    private RustdlReasoner propHierReasoner() throws Exception {
        OWLOntology o = OWLManager.createOWLOntologyManager().createOntology(IRI.create("http://ex/prophier"));
        RustdlJson.ClassifyJson c = new RustdlJson.ClassifyJson();
        c.schema_version = 1; c.consistent = true; c.incomplete = false;
        c.unsatisfiable = new ArrayList<>(); c.equivalent_groups = new ArrayList<>(); c.direct_subsumptions = new ArrayList<>();
        RustdlJson.RealizeJson r = new RustdlJson.RealizeJson(); r.schema_version = 1; r.individuals = new ArrayList<>();
        String json = new String(java.nio.file.Files.readAllBytes(
            java.nio.file.Paths.get(getClass().getResource("/json/prophier.json").toURI())));
        RustdlJson.PropHierJson p = RustdlProcess.parsePropHier(json);
        return RustdlReasoner.forTest(o, c, r, p);
    }

    @Test public void directSuperObjectProperty() throws Exception {
        NodeSet<OWLObjectPropertyExpression> supers = propHierReasoner().getSuperObjectProperties(objProp("p"), true);
        assertTrue(supers.containsEntity(objProp("r")));
    }

    @Test public void equivalentObjectProperties() throws Exception {
        Node<OWLObjectPropertyExpression> eq = propHierReasoner().getEquivalentObjectProperties(objProp("p"));
        assertTrue(eq.contains(objProp("p2")));
    }

    @Test public void directSubDataProperty() throws Exception {
        NodeSet<OWLDataProperty> subs = propHierReasoner().getSubDataProperties(dataProp("e"), true);
        assertTrue(subs.containsEntity(dataProp("d")));
    }

    @Test public void topLevelObjectPropertyIsDirectSubOfTop() throws Exception {
        // r has no named super in the fixture, so its direct super frontier is owl:topObjectProperty.
        NodeSet<OWLObjectPropertyExpression> supers = propHierReasoner().getSuperObjectProperties(objProp("r"), true);
        assertTrue(supers.containsEntity(df.getOWLTopObjectProperty()));
    }

    @Test public void leafObjectPropertyDirectSubIsBottom() throws Exception {
        // p has no named sub in the fixture, so its direct sub frontier is owl:bottomObjectProperty.
        NodeSet<OWLObjectPropertyExpression> subs = propHierReasoner().getSubObjectProperties(objProp("p"), true);
        assertTrue(subs.containsEntity(df.getOWLBottomObjectProperty()));
    }

    @Test public void topLevelDataPropertyIsDirectSubOfTop() throws Exception {
        // e has no named super in the fixture, so its direct super frontier is owl:topDataProperty.
        NodeSet<OWLDataProperty> supers = propHierReasoner().getSuperDataProperties(dataProp("e"), true);
        assertTrue(supers.containsEntity(df.getOWLTopDataProperty()));
    }

    @Test public void leafDataPropertyDirectSubIsBottom() throws Exception {
        // d has no named sub in the fixture, so its direct sub frontier is owl:bottomDataProperty.
        NodeSet<OWLDataProperty> subs = propHierReasoner().getSubDataProperties(dataProp("d"), true);
        assertTrue(subs.containsEntity(df.getOWLBottomDataProperty()));
    }

    /** Loads the shared disjoint.json fixture: classes A/B disjoint, object properties p/q disjoint, no disjoint data properties. */
    private RustdlReasoner disjointReasoner() throws Exception {
        OWLOntology o = OWLManager.createOWLOntologyManager().createOntology(IRI.create("http://ex/disjoint"));
        RustdlJson.ClassifyJson c = new RustdlJson.ClassifyJson();
        c.schema_version = 1; c.consistent = true; c.incomplete = false;
        c.unsatisfiable = new ArrayList<>(); c.equivalent_groups = new ArrayList<>(); c.direct_subsumptions = new ArrayList<>();
        RustdlJson.RealizeJson r = new RustdlJson.RealizeJson(); r.schema_version = 1; r.individuals = new ArrayList<>();
        String json = new String(java.nio.file.Files.readAllBytes(
            java.nio.file.Paths.get(getClass().getResource("/json/disjoint.json").toURI())));
        RustdlJson.DisjointJson d = RustdlProcess.parseDisjoint(json);
        return RustdlReasoner.forTest(o, c, r, d);
    }

    @Test public void disjointClasses() throws Exception {
        NodeSet<OWLClass> disjoint = disjointReasoner().getDisjointClasses(cls("A"));
        assertTrue(disjoint.containsEntity(cls("B")));
    }

    @Test public void disjointObjectProperties() throws Exception {
        NodeSet<OWLObjectPropertyExpression> disjoint = disjointReasoner().getDisjointObjectProperties(objProp("p"));
        assertTrue(disjoint.containsEntity(objProp("q")));
    }

    @Test public void disjointDataPropertiesEmpty() throws Exception {
        NodeSet<OWLDataProperty> disjoint = disjointReasoner().getDisjointDataProperties(dataProp("d"));
        assertTrue(disjoint.isEmpty());
    }

    /** Loads the shared individuals.json fixture: a same b (incomplete:true); a different c. */
    private RustdlReasoner individualsReasoner() throws Exception {
        OWLOntology o = OWLManager.createOWLOntologyManager().createOntology(IRI.create("http://ex/individuals"));
        RustdlJson.ClassifyJson c = new RustdlJson.ClassifyJson();
        c.schema_version = 1; c.consistent = true; c.incomplete = false;
        c.unsatisfiable = new ArrayList<>(); c.equivalent_groups = new ArrayList<>(); c.direct_subsumptions = new ArrayList<>();
        RustdlJson.RealizeJson r = new RustdlJson.RealizeJson(); r.schema_version = 1; r.individuals = new ArrayList<>();
        String json = new String(java.nio.file.Files.readAllBytes(
            java.nio.file.Paths.get(getClass().getResource("/json/individuals.json").toURI())));
        RustdlJson.IndividualsJson ind = RustdlProcess.parseIndividuals(json);
        return RustdlReasoner.forTest(o, c, r, ind);
    }
    private OWLNamedIndividual ind(String i) { return df.getOWLNamedIndividual(IRI.create("http://ex/#" + i)); }

    @Test public void sameIndividualsContainsGroupMemberAndSelf() throws Exception {
        Node<OWLNamedIndividual> same = individualsReasoner().getSameIndividuals(ind("a"));
        assertTrue(same.contains(ind("b")));
        assertTrue("node must always contain the queried individual itself", same.contains(ind("a")));
    }

    @Test public void sameIndividualsWithNoGroupReturnsSingleton() throws Exception {
        Node<OWLNamedIndividual> same = individualsReasoner().getSameIndividuals(ind("z"));
        assertTrue(same.contains(ind("z")));
        assertEquals(1, same.getSize());
    }

    @Test public void differentIndividualsContainsPairedIndividual() throws Exception {
        NodeSet<OWLNamedIndividual> diff = individualsReasoner().getDifferentIndividuals(ind("a"));
        assertTrue(diff.containsEntity(ind("c")));
    }

    @Test public void unsatEdgeDoesNotCorruptLeaves() throws Exception {
        RustdlReasoner reasoner = withUnsat();
        // A is a genuine leaf: its direct sub is the bottom node (which includes U and owl:Nothing, MERGED into one node)
        NodeSet<OWLClass> aSubs = reasoner.getSubClasses(cls("A"), true);
        assertTrue(aSubs.containsEntity(df.getOWLNothing()));
        assertTrue(aSubs.containsEntity(cls("U")));
        Node<OWLClass> bottom = null;
        for (Node<OWLClass> n : aSubs.getNodes()) if (n.contains(df.getOWLNothing())) bottom = n;
        assertNotNull("A must have the bottom node as a direct sub", bottom);
        assertTrue("U must be MERGED into the bottom node, not a separate singleton", bottom.contains(cls("U")));
        // B's genuine sub is A (the phantom edge must not have polluted anything)
        assertTrue(reasoner.getSubClasses(cls("B"), true).containsEntity(cls("A")));
        // U (unsatisfiable) is equivalent to owl:Nothing and not satisfiable
        assertTrue(reasoner.getEquivalentClasses(cls("U")).contains(df.getOWLNothing()));
        assertFalse(reasoner.isSatisfiable(cls("U")));
        // U's direct superclass is the real leaf frontier {A}, NOT a fallback to Thing
        assertTrue(reasoner.getSuperClasses(cls("U"), true).containsEntity(cls("A")));
        assertFalse(reasoner.getSuperClasses(cls("U"), true).containsEntity(df.getOWLThing()));
    }
}
