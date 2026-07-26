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
     * Regression guard for the Critical bug fixed alongside this test: a laconic justification is
     * a WEAKENED FRAGMENT of a source axiom, genuinely absent (as such, verbatim) from the source
     * ontology by design -- {@code materialize} must NOT run the literal source-membership guard
     * ({@link RustdlOfn#verifiedAgainst}) on the laconic path.
     *
     * <p>Ontology: {@code SubClassOf(:A, ObjectIntersectionOf(:B,:C))} only. Querying
     * {@code justify --laconic subclass A B} against a REAL rustdl binary weakens the sole source
     * axiom's RHS conjunction down to {@code SubClassOf(:A,:B)} -- entailed by the source axiom,
     * but not literally equal to (or contained in) it. Before the fix, this genuinely-weakened
     * case aborted the whole request via {@code ExplanationException} ("possible fabrication"),
     * breaking laconic explanations in exactly the case they exist for. After the fix, the laconic
     * generator must return a non-empty {@code Explanation} containing the weakened fragment.</p>
     */
    @Test public void laconicExplanationAcceptsGenuinelyWeakenedAxiomWithRealBinary() throws Exception {
        assumeTrue("no rustdl binary available",
            RustdlBinary.configuredOverride() != null
            || RustdlBinary.class.getResource("/native") != null);
        OWLOntologyManager m = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = m.getOWLDataFactory();
        OWLOntology o = m.createOntology(IRI.create("http://ex7/"));

        OWLClass a = df.getOWLClass(IRI.create("http://ex7/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex7/#B"));
        OWLClass c = df.getOWLClass(IRI.create("http://ex7/#C"));
        // The ONLY source axiom: A subclass-of (B and C). Entails A subclass-of B, but that
        // entailment is nowhere LITERALLY present as its own axiom in the source.
        m.addAxiom(o, df.getOWLSubClassOfAxiom(a, df.getOWLObjectIntersectionOf(b, c)));

        OWLSubClassOfAxiom entailment = df.getOWLSubClassOfAxiom(a, b);
        ExplanationGenerator<OWLAxiom> laconicGenerator =
            new RustdlLaconicExplanationGeneratorFactory().createExplanationGenerator(o);

        Set<Explanation<OWLAxiom>> explanations = laconicGenerator.getExplanations(entailment);
        assertFalse(
            "real rustdl binary's laconic weakening must produce a genuine, non-empty "
                + "Explanation instead of the fabrication guard aborting the whole request",
            explanations.isEmpty());
        boolean containsWeakenedFragment = explanations.stream()
            .anyMatch(exp -> exp.getAxioms().contains(entailment));
        assertTrue(
            "expected at least one explanation to contain the weakened fragment SubClassOf(A,B)",
            containsWeakenedFragment);
    }

    /**
     * Minor hardening #4: round-trips a NON-TRIVIAL axiom shape (here, {@code
     * ObjectSomeValuesFrom}) through the full in-memory-OWLAPI -> OFN -> {@code rustdl justify
     * --json} -> OFN-parse -> {@code containsAxiom} path (the non-laconic/minimal
     * anti-fabrication guard, {@link RustdlOfn#verifiedAgainst}), asserting the genuine axioms are
     * NOT dropped. Guards against a future OFN writer/parser normalization mismatch silently
     * rejecting genuine explanations for anything beyond bare named-class SubClassOf axioms.
     *
     * <p>Ontology: {@code A ⊑ ∃hasParent.B}, {@code B ⊑ C}, {@code ∃hasParent.C ⊑ D}, entailing
     * {@code A ⊑ D} (via {@code B ⊑ C} monotonicity through the existential). The minimal
     * justification for this entailment is exactly those three axioms, each involving
     * {@code ObjectSomeValuesFrom} -- confirmed against the real binary before finalizing this
     * test.</p>
     */
    @Test public void justifyRoundTripsNonTrivialExistentialAxiomWithRealBinary() throws Exception {
        assumeTrue("no rustdl binary available",
            RustdlBinary.configuredOverride() != null
            || RustdlBinary.class.getResource("/native") != null);
        OWLOntologyManager m = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = m.getOWLDataFactory();
        OWLOntology o = m.createOntology(IRI.create("http://ex8/"));

        OWLClass a = df.getOWLClass(IRI.create("http://ex8/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex8/#B"));
        OWLClass c = df.getOWLClass(IRI.create("http://ex8/#C"));
        OWLClass d = df.getOWLClass(IRI.create("http://ex8/#D"));
        OWLObjectProperty hasParent = df.getOWLObjectProperty(IRI.create("http://ex8/#hasParent"));

        OWLSubClassOfAxiom aExistsB = df.getOWLSubClassOfAxiom(
            a, df.getOWLObjectSomeValuesFrom(hasParent, b));
        OWLSubClassOfAxiom bc = df.getOWLSubClassOfAxiom(b, c);
        OWLSubClassOfAxiom existsCD = df.getOWLSubClassOfAxiom(
            df.getOWLObjectSomeValuesFrom(hasParent, c), d);
        m.addAxiom(o, aExistsB);
        m.addAxiom(o, bc);
        m.addAxiom(o, existsCD);

        OWLSubClassOfAxiom entailment = df.getOWLSubClassOfAxiom(a, d);
        ExplanationGenerator<OWLAxiom> generator =
            new RustdlExplanationGeneratorFactory().createExplanationGenerator(o);
        Set<Explanation<OWLAxiom>> explanations = generator.getExplanations(entailment);

        assertFalse("real rustdl binary must return at least one justification for A subclass-of D",
            explanations.isEmpty());
        Explanation<OWLAxiom> explanation = explanations.iterator().next();
        assertTrue(
            "genuine ObjectSomeValuesFrom-bearing axiom A subclass-of ExistsHasParent.B must "
                + "round-trip through OFN-write/rustdl-justify/OFN-parse and NOT be dropped by "
                + "the literal-membership anti-fabrication guard",
            explanation.getAxioms().contains(aExistsB));
        assertTrue(
            "genuine axiom B subclass-of C must round-trip and NOT be dropped",
            explanation.getAxioms().contains(bc));
        assertTrue(
            "genuine ObjectSomeValuesFrom-bearing axiom ExistsHasParent.C subclass-of D must "
                + "round-trip and NOT be dropped",
            explanation.getAxioms().contains(existsCD));
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
