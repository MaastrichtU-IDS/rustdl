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
}
