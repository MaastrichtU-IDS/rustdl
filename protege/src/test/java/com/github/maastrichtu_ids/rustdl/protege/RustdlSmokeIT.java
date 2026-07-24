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
}
