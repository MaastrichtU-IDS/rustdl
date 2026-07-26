package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.IRI;
import org.semanticweb.owlapi.model.OWLAxiom;
import org.semanticweb.owlapi.model.OWLClass;
import org.semanticweb.owlapi.model.OWLDataFactory;
import org.semanticweb.owlapi.model.OWLOntology;
import org.semanticweb.owlapi.model.OWLOntologyManager;
import org.semanticweb.owlapi.model.OWLSubClassOfAxiom;

import java.util.Set;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

/**
 * {@link RustdlOfn} round-trips the self-contained OFN ontology DOCUMENT rustdl emits per
 * justification (Prefix(...)/Ontology(...) — see the real fixture captured in
 * {@code src/test/resources/json/justify.json}), and enforces the anti-fabrication guard: an
 * axiom absent from the source ontology's imports closure must never survive into a displayed
 * {@code Explanation}.
 */
public class RustdlOfnTest {

    @Test
    public void parsesRealJustificationDocument() {
        String ofn = "Prefix(:=<http://ex/#>)\n"
            + "Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n"
            + "Ontology(\n"
            + "  SubClassOf(:A :B)\n"
            + "  SubClassOf(:B :C)\n"
            + ")\n";
        Set<OWLAxiom> axioms = RustdlOfn.parse(ofn);
        assertEquals(2, axioms.size());
        OWLDataFactory df = OWLManager.getOWLDataFactory();
        OWLClass a = df.getOWLClass(IRI.create("http://ex/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex/#B"));
        OWLClass c = df.getOWLClass(IRI.create("http://ex/#C"));
        assertTrue(axioms.contains(df.getOWLSubClassOfAxiom(a, b)));
        assertTrue(axioms.contains(df.getOWLSubClassOfAxiom(b, c)));
    }

    @Test(expected = IllegalStateException.class)
    public void rejectsUnparseableDocument() {
        RustdlOfn.parse("not an ofn document at all {{{");
    }

    @Test(expected = IllegalArgumentException.class)
    public void rejectsEmptyDocument() {
        RustdlOfn.parse("");
    }

    @Test
    public void verifiedAgainstDropsFabricatedAxiom() throws Exception {
        OWLOntologyManager manager = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = manager.getOWLDataFactory();
        OWLClass a = df.getOWLClass(IRI.create("http://ex/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex/#B"));
        OWLClass x = df.getOWLClass(IRI.create("http://ex/#X"));
        OWLClass y = df.getOWLClass(IRI.create("http://ex/#Y"));
        OWLSubClassOfAxiom genuine = df.getOWLSubClassOfAxiom(a, b);
        OWLSubClassOfAxiom fabricated = df.getOWLSubClassOfAxiom(x, y);

        OWLOntology source = manager.createOntology(IRI.create("http://ex/"));
        manager.addAxiom(source, genuine);
        // NOTE: `fabricated` is deliberately never added to `source`.

        Set<OWLAxiom> parsed = new java.util.LinkedHashSet<>();
        parsed.add(genuine);
        parsed.add(fabricated);

        Set<OWLAxiom> verified = RustdlOfn.verifiedAgainst(parsed, source);
        assertEquals(1, verified.size());
        assertTrue(verified.contains(genuine));
        assertFalse(verified.contains(fabricated));
    }

    @Test
    public void verifiedAgainstKeepsEveryGenuineAxiom() throws Exception {
        OWLOntologyManager manager = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = manager.getOWLDataFactory();
        OWLClass a = df.getOWLClass(IRI.create("http://ex/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex/#B"));
        OWLClass c = df.getOWLClass(IRI.create("http://ex/#C"));
        OWLSubClassOfAxiom ab = df.getOWLSubClassOfAxiom(a, b);
        OWLSubClassOfAxiom bc = df.getOWLSubClassOfAxiom(b, c);

        OWLOntology source = manager.createOntology(IRI.create("http://ex/"));
        manager.addAxiom(source, ab);
        manager.addAxiom(source, bc);

        Set<OWLAxiom> parsed = new java.util.LinkedHashSet<>();
        parsed.add(ab);
        parsed.add(bc);

        Set<OWLAxiom> verified = RustdlOfn.verifiedAgainst(parsed, source);
        assertEquals(2, verified.size());
    }
}
