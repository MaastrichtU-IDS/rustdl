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
