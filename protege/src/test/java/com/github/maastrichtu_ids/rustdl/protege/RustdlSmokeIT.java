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

    /**
     * Regression guard for the Critical integration defect: on a genuinely inconsistent
     * ABox ontology (DisjointClasses(A,B) + ClassAssertion(A,x) + ClassAssertion(B,x)),
     * rustdl's new property-hierarchy/disjoint/individuals/property-values subcommands
     * all return Err(Inconsistent) (non-zero exit). Before the fix, precomputeInferences
     * ran those 4 subprocess blocks unconditionally, so the non-zero exit surfaced as an
     * IOException wrapped in ReasonerInternalException — breaking Protégé's inconsistency
     * handling. After the fix, precomputeInferences(<all 9 types>) must complete WITHOUT
     * throwing (the 4 new blocks are skipped once classify reports inconsistent), and a
     * subsequent query (getDisjointClasses) must throw InconsistentOntologyException, not
     * ReasonerInternalException.
     */
    @Test public void precomputeDoesNotThrowOnInconsistentAboxWithRealBinary() throws Exception {
        assumeTrue("no rustdl binary available",
            RustdlBinary.configuredOverride() != null
            || RustdlBinary.class.getResource("/native") != null);
        OWLOntologyManager m = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = m.getOWLDataFactory();
        OWLOntology o = m.createOntology(IRI.create("http://ex3/"));

        OWLClass a = df.getOWLClass(IRI.create("http://ex3/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex3/#B"));
        OWLNamedIndividual x = df.getOWLNamedIndividual(IRI.create("http://ex3/#x"));
        m.addAxiom(o, df.getOWLDisjointClassesAxiom(a, b));
        m.addAxiom(o, df.getOWLClassAssertionAxiom(a, x));
        m.addAxiom(o, df.getOWLClassAssertionAxiom(b, x));

        OWLReasoner r = new RustdlReasonerFactory().createReasoner(o);

        // Confirm rustdl genuinely reports this ABox inconsistent before asserting anything else.
        assertFalse("ontology must be genuinely inconsistent for this regression guard to be meaningful",
            r.isConsistent());

        // The regression guard itself: must throw NOTHING (in particular, no
        // ReasonerInternalException from an unconditional new-subprocess call).
        r.precomputeInferences(
            InferenceType.CLASS_HIERARCHY, InferenceType.CLASS_ASSERTIONS,
            InferenceType.OBJECT_PROPERTY_HIERARCHY, InferenceType.DATA_PROPERTY_HIERARCHY,
            InferenceType.DISJOINT_CLASSES, InferenceType.SAME_INDIVIDUAL,
            InferenceType.DIFFERENT_INDIVIDUALS, InferenceType.OBJECT_PROPERTY_ASSERTIONS,
            InferenceType.DATA_PROPERTY_ASSERTIONS);

        assertFalse(r.isConsistent());
        try {
            r.getDisjointClasses(a);
            fail("expected InconsistentOntologyException");
        } catch (org.semanticweb.owlapi.reasoner.InconsistentOntologyException expected) {
            // correct contract behaviour
        }
    }
}
