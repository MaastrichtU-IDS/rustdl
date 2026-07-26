package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.liveontologies.puli.Inference;
import org.semanticweb.owl.explanation.api.Explanation;
import org.semanticweb.owl.explanation.api.ExplanationGenerator;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.*;
import org.semanticweb.owlapi.model.parameters.Imports;
import org.semanticweb.owlapi.reasoner.*;
import static org.junit.Assume.assumeTrue;
import static org.junit.Assert.*;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Collection;
import java.util.Set;

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

    /**
     * End-to-end justification/laconic-justification smoke against a REAL rustdl binary (Task 5,
     * Step 1): a tiny EL ontology ({@code A ⊑ B ⊑ C}, entailing {@code A ⊑ C}) is handed to
     * {@link RustdlExplanationGeneratorFactory#createExplanationGenerator(OWLOntology)}, which
     * spawns {@code rustdl justify --json} as a subprocess and parses its output. Asserts a
     * non-empty {@code Set<Explanation<OWLAxiom>>} from both the non-laconic and laconic
     * factories, and — the anti-fabrication guarantee holding end-to-end through the real
     * binary, not just against canned JSON — that every axiom in every returned explanation is a
     * genuine source axiom of the ontology (mirrors the fail-hard check in
     * {@link RustdlOfn#verifiedAgainst}, exercised here via the real subprocess rather than a
     * test seam).
     */
    @Test public void justifiesEntailmentWithRealBinaryAgainstSourceAxioms() throws Exception {
        assumeTrue("no rustdl binary available",
            RustdlBinary.configuredOverride() != null
            || RustdlBinary.class.getResource("/native") != null);
        OWLOntologyManager m = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = m.getOWLDataFactory();
        OWLOntology o = m.createOntology(IRI.create("http://ex4/"));

        OWLClass a = df.getOWLClass(IRI.create("http://ex4/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex4/#B"));
        OWLClass c = df.getOWLClass(IRI.create("http://ex4/#C"));
        m.addAxiom(o, df.getOWLSubClassOfAxiom(a, b));
        m.addAxiom(o, df.getOWLSubClassOfAxiom(b, c));

        OWLSubClassOfAxiom entailment = df.getOWLSubClassOfAxiom(a, c);
        Set<OWLAxiom> sourceAxioms = o.getAxioms(Imports.INCLUDED);

        ExplanationGenerator<OWLAxiom> generator =
            new RustdlExplanationGeneratorFactory().createExplanationGenerator(o);
        Set<Explanation<OWLAxiom>> explanations = generator.getExplanations(entailment);
        assertFalse("real rustdl binary must return at least one justification",
            explanations.isEmpty());
        for (Explanation<OWLAxiom> explanation : explanations) {
            for (OWLAxiom axiom : explanation.getAxioms()) {
                assertTrue(
                    "explanation axiom must be a genuine source axiom of the ontology "
                        + "(anti-fabrication): " + axiom,
                    sourceAxioms.contains(axiom));
            }
        }

        ExplanationGenerator<OWLAxiom> laconicGenerator =
            new RustdlLaconicExplanationGeneratorFactory().createExplanationGenerator(o);
        Set<Explanation<OWLAxiom>> laconicExplanations = laconicGenerator.getExplanations(entailment);
        assertFalse("real rustdl binary must return at least one laconic justification",
            laconicExplanations.isEmpty());
    }

    /**
     * End-to-end proof-tree smoke against a REAL rustdl binary (Task 5, Step 1): the same tiny EL
     * ontology as above, this time queried via {@code rustdl prove --json} directly through
     * {@link RustdlProcess#prove} and converted with {@link RustdlProof#fromProveJson}.
     *
     * <p><b>Why not {@code RustdlProofService.getProof}:</b> {@code RustdlProofService} extends
     * the liveontologies {@code ProofService} base class, whose {@code getEditorKit()} requires a
     * full {@code org.protege.editor.owl.OWLEditorKit} — wired up only inside a running Protégé
     * workbench ({@code ProofService.setup(OWLEditorKit, ...)}) — which cannot be constructed in
     * a headless JUnit/Failsafe process. {@link RustdlProcess#prove} and {@link RustdlProof} are
     * exactly the two pieces {@code RustdlProofService}'s {@code RecomputingProof.compute} calls
     * internally (see its source), so invoking them directly here still exercises the real binary
     * end-to-end through the proof path — the only piece skipped is the Protégé editor-kit
     * plumbing around it, which has no reasoning logic of its own.</p>
     */
    @Test public void provesEntailmentWithRealBinaryProducingNonTrivialProof() throws Exception {
        assumeTrue("no rustdl binary available",
            RustdlBinary.configuredOverride() != null
            || RustdlBinary.class.getResource("/native") != null);
        OWLOntologyManager m = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = m.getOWLDataFactory();
        OWLOntology o = m.createOntology(IRI.create("http://ex5/"));

        OWLClass a = df.getOWLClass(IRI.create("http://ex5/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex5/#B"));
        OWLClass c = df.getOWLClass(IRI.create("http://ex5/#C"));
        m.addAxiom(o, df.getOWLSubClassOfAxiom(a, b));
        m.addAxiom(o, df.getOWLSubClassOfAxiom(b, c));

        Path ofn = Files.createTempFile("rustdl-smoke-prove-", ".ofn");
        try {
            FlattenedOntology.writeOfn(o, ofn);
            RustdlJson.ProveJson json = RustdlProcess.prove(
                ofn, "http://ex5/#A", "http://ex5/#C", 60L);
            assertTrue("real rustdl binary must report the SubClassOf(A,C) entailment",
                json.entailed);
            assertTrue(
                "real rustdl binary must return a step-level proof for this EL entailment "
                    + "(not just the justification fallback)",
                json.has_proof);

            OWLSubClassOfAxiom goal = df.getOWLSubClassOfAxiom(a, c);
            RustdlProof proof = RustdlProof.fromProveJson(json, goal);

            Collection<? extends Inference<OWLAxiom>> rootInferences = proof.getInferences(goal);
            assertFalse("proof must contain an inference for the root goal conclusion",
                rootInferences.isEmpty());
            Inference<OWLAxiom> root = rootInferences.iterator().next();
            assertEquals(goal, root.getConclusion());
            assertFalse(
                "root inference must be non-trivial (at least one premise, not a bare fact)",
                root.getPremises().isEmpty());
        } finally {
            Files.deleteIfExists(ofn);
        }
    }
}
