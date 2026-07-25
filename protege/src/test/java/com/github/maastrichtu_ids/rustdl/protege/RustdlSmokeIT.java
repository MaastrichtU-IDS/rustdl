package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.*;
import org.semanticweb.owlapi.reasoner.*;
import static org.junit.Assume.assumeTrue;
import static org.junit.Assert.*;

public class RustdlSmokeIT {
    @Test public void classifiesTinyOntologyWithRealBinary() throws Exception {
        // Only runs when a binary is reachable (dev: -Drustdl.bin; CI: bundled or override).
        assumeTrue("no rustdl binary available",
            RustdlBinary.configuredOverride() != null
            || RustdlBinary.class.getResource("/native") != null);
        OWLOntologyManager m = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = m.getOWLDataFactory();
        OWLOntology o = m.createOntology(IRI.create("http://ex/"));
        OWLClass a = df.getOWLClass(IRI.create("http://ex/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex/#B"));
        m.addAxiom(o, df.getOWLSubClassOfAxiom(a, b));
        OWLReasoner r = new RustdlReasonerFactory().createReasoner(o);
        r.precomputeInferences(InferenceType.CLASS_HIERARCHY);
        assertTrue(r.isConsistent());
        assertTrue(r.getSuperClasses(a, false).containsEntity(b));
    }

    /**
     * Exercises the full 9-InferenceType query surface (Task 6) against a REAL rustdl
     * binary: two disjoint classes, a sub-object-property, two same individuals, and one
     * object-property assertion. Asserts one representative non-empty answer from EACH new
     * family (disjoint classes, object-property hierarchy, same individuals, object-property
     * values) — proving precomputeInferences genuinely runs the 4 new subprocesses.
     */
    @Test public void precomputesAllNineInferenceTypesWithRealBinary() throws Exception {
        assumeTrue("no rustdl binary available",
            RustdlBinary.configuredOverride() != null
            || RustdlBinary.class.getResource("/native") != null);
        OWLOntologyManager m = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = m.getOWLDataFactory();
        OWLOntology o = m.createOntology(IRI.create("http://ex2/"));

        OWLClass disjointA = df.getOWLClass(IRI.create("http://ex2/#DisjointA"));
        OWLClass disjointB = df.getOWLClass(IRI.create("http://ex2/#DisjointB"));
        m.addAxiom(o, df.getOWLDisjointClassesAxiom(disjointA, disjointB));

        OWLObjectProperty subProp = df.getOWLObjectProperty(IRI.create("http://ex2/#hasSubProp"));
        OWLObjectProperty superProp = df.getOWLObjectProperty(IRI.create("http://ex2/#hasSuperProp"));
        m.addAxiom(o, df.getOWLSubObjectPropertyOfAxiom(subProp, superProp));

        OWLNamedIndividual indA = df.getOWLNamedIndividual(IRI.create("http://ex2/#indA"));
        OWLNamedIndividual indB = df.getOWLNamedIndividual(IRI.create("http://ex2/#indB"));
        m.addAxiom(o, df.getOWLSameIndividualAxiom(indA, indB));

        OWLNamedIndividual subj = df.getOWLNamedIndividual(IRI.create("http://ex2/#subj"));
        OWLNamedIndividual obj = df.getOWLNamedIndividual(IRI.create("http://ex2/#obj"));
        m.addAxiom(o, df.getOWLObjectPropertyAssertionAxiom(subProp, subj, obj));

        OWLReasoner r = new RustdlReasonerFactory().createReasoner(o);
        r.precomputeInferences(
            InferenceType.CLASS_HIERARCHY, InferenceType.CLASS_ASSERTIONS,
            InferenceType.OBJECT_PROPERTY_HIERARCHY, InferenceType.DATA_PROPERTY_HIERARCHY,
            InferenceType.DISJOINT_CLASSES, InferenceType.SAME_INDIVIDUAL,
            InferenceType.DIFFERENT_INDIVIDUALS, InferenceType.OBJECT_PROPERTY_ASSERTIONS,
            InferenceType.DATA_PROPERTY_ASSERTIONS);
        assertTrue(r.isConsistent());

        // disjoint classes
        assertTrue("disjoint classes must be non-empty",
            r.getDisjointClasses(disjointA).containsEntity(disjointB));
        // object-property hierarchy
        assertTrue("object-property hierarchy must be non-empty",
            r.getSuperObjectProperties(subProp, true).containsEntity(superProp));
        // same individuals
        assertTrue("same individuals must be non-empty",
            r.getSameIndividuals(indA).contains(indB));
        // object-property values
        assertTrue("object-property values must be non-empty",
            r.getObjectPropertyValues(subj, subProp).containsEntity(obj));
    }
}
