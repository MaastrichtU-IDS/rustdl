package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.semanticweb.owl.explanation.api.ExplanationException;
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
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

/**
 * {@link RustdlOfn} round-trips the self-contained OFN ontology DOCUMENT rustdl emits per
 * justification (Prefix(...)/Ontology(...) — see the real fixture captured in
 * {@code src/test/resources/json/justify.json}), and enforces the FAIL-HARD anti-fabrication
 * guard: an axiom absent from the source ontology's imports closure must reject the WHOLE
 * justification (never survive, partially or otherwise, into a displayed {@code Explanation}).
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
    public void verifiedAgainstRejectsWholeJustificationOnFabricatedAxiom() throws Exception {
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

        // A single fabricated/non-source axiom must reject the ENTIRE justification -- not
        // silently surface the genuine subset, which could be genuine-but-insufficient to
        // actually entail the target (a misleading "Explanation").
        try {
            RustdlOfn.verifiedAgainst(parsed, source);
            fail("expected ExplanationException rejecting the whole justification");
        } catch (ExplanationException expected) {
            assertTrue("exception should identify the offending axiom: " + expected.getMessage(),
                expected.getMessage().contains(fabricated.toString())
                    || expected.getMessage().contains("http://ex/#X"));
        }
    }

    @Test
    public void singleLogicalAxiomExtractsTheOneLogicalAxiom() {
        String ofn = "Prefix(:=<http://ex/#>)\n"
            + "Ontology(\n"
            + "  Declaration(Class(:A))\n"
            + "  Declaration(Class(:B))\n"
            + "  SubClassOf(:A :B)\n"
            + ")\n";
        OWLAxiom axiom = RustdlOfn.singleLogicalAxiom(ofn);
        OWLDataFactory df = OWLManager.getOWLDataFactory();
        OWLClass a = df.getOWLClass(IRI.create("http://ex/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex/#B"));
        assertEquals(df.getOWLSubClassOfAxiom(a, b), axiom);
    }

    @Test(expected = IllegalStateException.class)
    public void singleLogicalAxiomRejectsZeroLogicalAxioms() {
        String ofn = "Prefix(:=<http://ex/#>)\n"
            + "Ontology(\n"
            + "  Declaration(Class(:A))\n"
            + ")\n";
        RustdlOfn.singleLogicalAxiom(ofn);
    }

    @Test(expected = IllegalStateException.class)
    public void singleLogicalAxiomRejectsMoreThanOneLogicalAxiom() {
        String ofn = "Prefix(:=<http://ex/#>)\n"
            + "Ontology(\n"
            + "  SubClassOf(:A :B)\n"
            + "  SubClassOf(:B :C)\n"
            + ")\n";
        RustdlOfn.singleLogicalAxiom(ofn);
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
