package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.semanticweb.owl.explanation.api.Explanation;
import org.semanticweb.owl.explanation.api.NullExplanationProgressMonitor;
import org.semanticweb.owl.explanation.api.UnsupportedEntailmentException;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.IRI;
import org.semanticweb.owlapi.model.OWLAxiom;
import org.semanticweb.owlapi.model.OWLClass;
import org.semanticweb.owlapi.model.OWLDataFactory;
import org.semanticweb.owlapi.model.OWLDataProperty;
import org.semanticweb.owlapi.model.OWLNamedIndividual;
import org.semanticweb.owlapi.model.OWLObjectProperty;
import org.semanticweb.owlapi.model.OWLOntology;
import org.semanticweb.owlapi.model.OWLOntologyManager;
import org.semanticweb.owlapi.model.OWLSubClassOfAxiom;

import java.util.Arrays;
import java.util.List;
import java.util.Set;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

/**
 * Covers the entailment→{@code justify} query-argument mapping ({@link
 * RustdlExplanationGenerator#queryArguments}, including the unsupported-entailment surface) and
 * the CANNED-report materialization pipeline ({@link RustdlExplanationGenerator#materialize}),
 * exercised entirely through the test seam — no rustdl subprocess is spawned.
 */
public class RustdlExplanationGeneratorTest {
    private final OWLOntologyManager m = OWLManager.createOWLOntologyManager();
    private final OWLDataFactory df = m.getOWLDataFactory();

    private OWLClass cls(String local) { return df.getOWLClass(IRI.create("http://ex/#" + local)); }
    private OWLNamedIndividual ind(String local) { return df.getOWLNamedIndividual(IRI.create("http://ex/#" + local)); }
    private OWLObjectProperty objProp(String local) { return df.getOWLObjectProperty(IRI.create("http://ex/#" + local)); }

    // --- queryArguments mapping -------------------------------------------------------------

    @Test public void subClassOfMapsToSubclassQuery() {
        List<String> q = RustdlExplanationGenerator.queryArguments(
            df.getOWLSubClassOfAxiom(cls("A"), cls("B")));
        assertEquals(Arrays.asList("subclass", "http://ex/#A", "http://ex/#B"), q);
    }

    @Test public void subClassOfNothingMapsToUnsatQuery() {
        List<String> q = RustdlExplanationGenerator.queryArguments(
            df.getOWLSubClassOfAxiom(cls("A"), df.getOWLNothing()));
        assertEquals(Arrays.asList("unsat", "http://ex/#A"), q);
    }

    @Test public void thingSubClassOfNothingMapsToInconsistentQuery() {
        List<String> q = RustdlExplanationGenerator.queryArguments(
            df.getOWLSubClassOfAxiom(df.getOWLThing(), df.getOWLNothing()));
        assertEquals(Arrays.asList("inconsistent"), q);
    }

    @Test public void equivalentClassesMapsToEquivalentQuerySortedByIri() {
        List<String> q = RustdlExplanationGenerator.queryArguments(
            df.getOWLEquivalentClassesAxiom(cls("Z"), cls("A")));
        assertEquals(Arrays.asList("equivalent", "http://ex/#A", "http://ex/#Z"), q);
    }

    @Test public void disjointClassesMapsToDisjointQuery() {
        List<String> q = RustdlExplanationGenerator.queryArguments(
            df.getOWLDisjointClassesAxiom(cls("A"), cls("B")));
        assertEquals(Arrays.asList("disjoint", "http://ex/#A", "http://ex/#B"), q);
    }

    @Test public void classAssertionMapsToInstanceQuery() {
        List<String> q = RustdlExplanationGenerator.queryArguments(
            df.getOWLClassAssertionAxiom(cls("A"), ind("i")));
        assertEquals(Arrays.asList("instance", "http://ex/#i", "http://ex/#A"), q);
    }

    @Test public void objectPropertyAssertionMapsToPropertyQuery() {
        List<String> q = RustdlExplanationGenerator.queryArguments(
            df.getOWLObjectPropertyAssertionAxiom(objProp("p"), ind("i"), ind("j")));
        assertEquals(Arrays.asList("property", "http://ex/#i", "http://ex/#p", "http://ex/#j"), q);
    }

    @Test public void subObjectPropertyOfMapsToSubpropertyQuery() {
        List<String> q = RustdlExplanationGenerator.queryArguments(
            df.getOWLSubObjectPropertyOfAxiom(objProp("p"), objProp("q")));
        assertEquals(Arrays.asList("subproperty", "http://ex/#p", "http://ex/#q"), q);
    }

    @Test public void sameIndividualMapsToSameQuerySortedByIri() {
        List<String> q = RustdlExplanationGenerator.queryArguments(
            df.getOWLSameIndividualAxiom(ind("j"), ind("i")));
        assertEquals(Arrays.asList("same", "http://ex/#i", "http://ex/#j"), q);
    }

    @Test public void supportsEntailmentAgreesWithQueryArguments() {
        assertTrue(RustdlExplanationGenerator.supportsEntailment(
            df.getOWLSubClassOfAxiom(cls("A"), cls("B"))));
        assertFalse(RustdlExplanationGenerator.supportsEntailment(
            df.getOWLDeclarationAxiom(cls("A"))));
    }

    @Test(expected = UnsupportedEntailmentException.class)
    public void anonymousSubClassIsUnsupported() {
        RustdlExplanationGenerator.queryArguments(
            df.getOWLSubClassOfAxiom(df.getOWLObjectIntersectionOf(cls("A"), cls("B")), cls("C")));
    }

    @Test(expected = UnsupportedEntailmentException.class)
    public void equivalentClassesWithThreeOperandsIsUnsupported() {
        RustdlExplanationGenerator.queryArguments(
            df.getOWLEquivalentClassesAxiom(cls("A"), cls("B"), cls("C")));
    }

    @Test(expected = UnsupportedEntailmentException.class)
    public void dataPropertyAssertionIsUnsupported() {
        OWLDataProperty dp = df.getOWLDataProperty(IRI.create("http://ex/#dp"));
        RustdlExplanationGenerator.queryArguments(
            df.getOWLDataPropertyAssertionAxiom(dp, ind("i"), df.getOWLLiteral("v")));
    }

    @Test(expected = UnsupportedEntailmentException.class)
    public void declarationAxiomIsUnsupported() {
        RustdlExplanationGenerator.queryArguments(df.getOWLDeclarationAxiom(cls("A")));
    }

    // --- materialize (canned report, no subprocess) -----------------------------------------

    private RustdlExplanationGenerator generator(OWLOntology ontology) {
        return new RustdlExplanationGenerator(
            ontology, new NullExplanationProgressMonitor<OWLAxiom>(),
            new RustdlExplainConfiguration(600L, 8), false);
    }

    private RustdlExplanationGenerator laconicGenerator(OWLOntology ontology) {
        return new RustdlExplanationGenerator(
            ontology, new NullExplanationProgressMonitor<OWLAxiom>(),
            new RustdlExplainConfiguration(600L, 8), true);
    }

    private RustdlJson.JustificationJson justification(String ofn) {
        RustdlJson.JustificationJson j = new RustdlJson.JustificationJson();
        j.ofn = ofn;
        return j;
    }

    @Test public void materializeRejectsWholeJustificationContainingFabricatedAxiom() throws Exception {
        OWLOntology o = m.createOntology(IRI.create("http://ex/onto1/"));
        OWLSubClassOfAxiom ab = df.getOWLSubClassOfAxiom(cls("A"), cls("B"));
        OWLSubClassOfAxiom bc = df.getOWLSubClassOfAxiom(cls("B"), cls("C"));
        m.addAxiom(o, ab);
        m.addAxiom(o, bc);

        // The canned justify report: genuine A⊑B, B⊑C PLUS a fabricated X⊑Y not in the source.
        RustdlJson.JustifyJson report = new RustdlJson.JustifyJson();
        report.schema_version = 1;
        report.status = "entailed";
        report.enumeration_complete = true;
        report.minimal = true;
        report.laconic = false;
        report.justifications = Arrays.asList(justification(
            "Prefix(:=<http://ex/#>)\nOntology(\n"
                + "  SubClassOf(:A :B)\n  SubClassOf(:B :C)\n  SubClassOf(:X :Y)\n)\n"));

        OWLAxiom entailment = df.getOWLSubClassOfAxiom(cls("A"), cls("C"));
        // A justification containing even one non-source axiom must be REJECTED WHOLE -- no
        // Explanation is produced from it at all (never a silently-thinned genuine subset,
        // which could be genuine-but-insufficient to actually entail the target).
        try {
            generator(o).materialize(entailment, report);
            fail("expected ExplanationException rejecting the fabricated justification");
        } catch (org.semanticweb.owl.explanation.api.ExplanationException expected) {
            // correct: whole-justification reject, mirroring km's actual behavior.
        }
    }

    @Test public void materializeReturnsExplanationForFullyGenuineJustification() throws Exception {
        OWLOntology o = m.createOntology(IRI.create("http://ex/onto1b/"));
        OWLSubClassOfAxiom ab = df.getOWLSubClassOfAxiom(cls("A"), cls("B"));
        OWLSubClassOfAxiom bc = df.getOWLSubClassOfAxiom(cls("B"), cls("C"));
        m.addAxiom(o, ab);
        m.addAxiom(o, bc);

        RustdlJson.JustifyJson report = new RustdlJson.JustifyJson();
        report.schema_version = 1;
        report.status = "entailed";
        report.enumeration_complete = true;
        report.minimal = true;
        report.laconic = false;
        report.justifications = Arrays.asList(justification(
            "Prefix(:=<http://ex/#>)\nOntology(\n  SubClassOf(:A :B)\n  SubClassOf(:B :C)\n)\n"));

        OWLAxiom entailment = df.getOWLSubClassOfAxiom(cls("A"), cls("C"));
        Set<Explanation<OWLAxiom>> explanations = generator(o).materialize(entailment, report);

        assertEquals(1, explanations.size());
        Explanation<OWLAxiom> explanation = explanations.iterator().next();
        assertEquals(entailment, explanation.getEntailment());
        assertEquals(2, explanation.getAxioms().size());
        assertTrue(explanation.getAxioms().contains(ab));
        assertTrue(explanation.getAxioms().contains(bc));
    }

    @Test public void materializeAbortsWholeBatchWhenOneOfSeveralJustificationsIsFabricated()
            throws Exception {
        // Mirrors km's actual choice: materialize() doesn't catch the fail-hard check per
        // justification, so ANY offending justification aborts the ENTIRE call -- including
        // other, fully-genuine justifications in the same report/batch.
        OWLOntology o = m.createOntology(IRI.create("http://ex/onto1c/"));
        OWLSubClassOfAxiom ab = df.getOWLSubClassOfAxiom(cls("A"), cls("B"));
        OWLSubClassOfAxiom bc = df.getOWLSubClassOfAxiom(cls("B"), cls("C"));
        m.addAxiom(o, ab);
        m.addAxiom(o, bc);

        RustdlJson.JustifyJson report = new RustdlJson.JustifyJson();
        report.schema_version = 1;
        report.status = "entailed";
        report.enumeration_complete = true;
        report.minimal = true;
        report.laconic = false;
        report.justifications = Arrays.asList(
            justification("Prefix(:=<http://ex/#>)\nOntology(\n  SubClassOf(:A :B)\n  SubClassOf(:B :C)\n)\n"),
            justification("Prefix(:=<http://ex/#>)\nOntology(\n  SubClassOf(:X :Y)\n)\n"));

        OWLAxiom entailment = df.getOWLSubClassOfAxiom(cls("A"), cls("C"));
        try {
            generator(o).materialize(entailment, report);
            fail("expected ExplanationException aborting the whole batch");
        } catch (org.semanticweb.owl.explanation.api.ExplanationException expected) {
            // correct.
        }
    }

    @Test public void materializeReturnsEmptySetForNotEntailed() throws Exception {
        OWLOntology o = m.createOntology(IRI.create("http://ex/onto2/"));
        RustdlJson.JustifyJson report = new RustdlJson.JustifyJson();
        report.schema_version = 1;
        report.status = "not-entailed";
        report.enumeration_complete = true;
        report.minimal = true;
        report.laconic = false;
        report.justifications = java.util.Collections.emptyList();

        OWLAxiom entailment = df.getOWLSubClassOfAxiom(cls("A"), cls("C"));
        Set<Explanation<OWLAxiom>> explanations = generator(o).materialize(entailment, report);
        assertTrue(explanations.isEmpty());
    }

    @Test public void materializeSupportsMultipleJustifications() throws Exception {
        OWLOntology o = m.createOntology(IRI.create("http://ex/onto3/"));
        OWLSubClassOfAxiom ab = df.getOWLSubClassOfAxiom(cls("A"), cls("B"));
        OWLSubClassOfAxiom bc = df.getOWLSubClassOfAxiom(cls("B"), cls("C"));
        OWLSubClassOfAxiom ad = df.getOWLSubClassOfAxiom(cls("A"), cls("D"));
        OWLSubClassOfAxiom dc = df.getOWLSubClassOfAxiom(cls("D"), cls("C"));
        m.addAxiom(o, ab);
        m.addAxiom(o, bc);
        m.addAxiom(o, ad);
        m.addAxiom(o, dc);

        RustdlJson.JustifyJson report = new RustdlJson.JustifyJson();
        report.schema_version = 1;
        report.status = "entailed";
        report.enumeration_complete = true;
        report.minimal = true;
        report.laconic = false;
        report.justifications = Arrays.asList(
            justification("Prefix(:=<http://ex/#>)\nOntology(\n  SubClassOf(:A :B)\n  SubClassOf(:B :C)\n)\n"),
            justification("Prefix(:=<http://ex/#>)\nOntology(\n  SubClassOf(:A :D)\n  SubClassOf(:D :C)\n)\n"));

        OWLAxiom entailment = df.getOWLSubClassOfAxiom(cls("A"), cls("C"));
        Set<Explanation<OWLAxiom>> explanations = generator(o).materialize(entailment, report);
        assertEquals(2, explanations.size());
    }

    // --- Critical regression: laconic must bypass the literal source-membership guard ---------

    /**
     * Regression test for the Critical bug: a laconic justification is a WEAKENED FRAGMENT of a
     * source axiom, genuinely absent (as such) from the source ontology by design -- source has
     * {@code SubClassOf(:A, ObjectIntersectionOf(:B,:C))}, the laconic-weakened justification for
     * {@code SubClassOf(:A,:B)} is just {@code SubClassOf(:A,:B)} itself, which the source never
     * literally contains. Before the fix, {@code materialize} unconditionally ran the literal
     * {@code verifiedAgainst} guard even on the laconic path, so this canned report threw
     * {@code ExplanationException} ("possible fabrication") instead of returning an Explanation.
     * After the fix, the laconic generator must return a non-empty Explanation containing exactly
     * the weakened fragment, without throwing.
     */
    @Test public void laconicMaterializeAcceptsGenuinelyWeakenedAxiomNotLiterallyInSource()
            throws Exception {
        OWLOntology o = m.createOntology(IRI.create("http://ex/onto5/"));
        OWLSubClassOfAxiom sourceAxiom = df.getOWLSubClassOfAxiom(
            cls("A"), df.getOWLObjectIntersectionOf(cls("B"), cls("C")));
        m.addAxiom(o, sourceAxiom);

        RustdlJson.JustifyJson report = new RustdlJson.JustifyJson();
        report.schema_version = 1;
        report.status = "entailed";
        report.enumeration_complete = true;
        report.minimal = true;
        report.laconic = true;
        // The weakened fragment SubClassOf(:A :B) is NOT literally in the source ontology (the
        // source only has SubClassOf(:A, ObjectIntersectionOf(:B :C))) -- entailed by it, not
        // equal to it.
        report.justifications = Arrays.asList(justification(
            "Prefix(:=<http://ex/#>)\nOntology(\n  SubClassOf(:A :B)\n)\n"));

        OWLAxiom entailment = df.getOWLSubClassOfAxiom(cls("A"), cls("B"));
        Set<Explanation<OWLAxiom>> explanations = laconicGenerator(o).materialize(entailment, report);

        assertEquals(1, explanations.size());
        Explanation<OWLAxiom> explanation = explanations.iterator().next();
        assertEquals(entailment, explanation.getEntailment());
        assertEquals(1, explanation.getAxioms().size());
        assertTrue(explanation.getAxioms().contains(
            df.getOWLSubClassOfAxiom(cls("A"), cls("B"))));
    }

    /**
     * The non-laconic (minimal) generator must keep rejecting the SAME weakened-fragment
     * justification whole -- literal source-membership verification is the correct anti-
     * fabrication defense for minimal justifications and must stay fail-hard even though the
     * laconic generator now accepts an analogous fragment.
     */
    @Test public void nonLaconicMaterializeStillRejectsAxiomAbsentFromSource() throws Exception {
        OWLOntology o = m.createOntology(IRI.create("http://ex/onto6/"));
        OWLSubClassOfAxiom sourceAxiom = df.getOWLSubClassOfAxiom(
            cls("A"), df.getOWLObjectIntersectionOf(cls("B"), cls("C")));
        m.addAxiom(o, sourceAxiom);

        RustdlJson.JustifyJson report = new RustdlJson.JustifyJson();
        report.schema_version = 1;
        report.status = "entailed";
        report.enumeration_complete = true;
        report.minimal = true;
        report.laconic = false;
        report.justifications = Arrays.asList(justification(
            "Prefix(:=<http://ex/#>)\nOntology(\n  SubClassOf(:A :B)\n)\n"));

        OWLAxiom entailment = df.getOWLSubClassOfAxiom(cls("A"), cls("B"));
        try {
            generator(o).materialize(entailment, report);
            fail("expected ExplanationException: SubClassOf(:A :B) is not literally a source "
                + "axiom, and the non-laconic path must keep the fail-hard literal guard");
        } catch (org.semanticweb.owl.explanation.api.ExplanationException expected) {
            // correct.
        }
    }

    @Test public void getExplanationsWithZeroLimitStillValidatesEntailmentSurface() throws Exception {
        OWLOntology o = m.createOntology(IRI.create("http://ex/onto4/"));
        try {
            generator(o).getExplanations(df.getOWLDeclarationAxiom(cls("A")), 5);
            fail("expected UnsupportedEntailmentException");
        } catch (UnsupportedEntailmentException expected) {
            // correct: an unsupported query must fail even before touching the limit.
        }
        assertTrue(generator(o).getExplanations(
            df.getOWLSubClassOfAxiom(cls("A"), cls("B")), 0).isEmpty());
    }
}
