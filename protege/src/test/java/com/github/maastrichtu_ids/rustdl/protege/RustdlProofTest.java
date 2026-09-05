package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.liveontologies.puli.Inference;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.IRI;
import org.semanticweb.owlapi.model.OWLAxiom;
import org.semanticweb.owlapi.model.OWLClass;
import org.semanticweb.owlapi.model.OWLDataFactory;
import org.semanticweb.owlapi.model.OWLOntology;
import org.semanticweb.owlapi.model.OWLOntologyManager;
import org.semanticweb.owlapi.model.OWLSubClassOfAxiom;
import org.semanticweb.owl.explanation.api.ExplanationException;

import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.Collection;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

/**
 * {@link RustdlProof} converts a canned {@code prove --json} result (the fixtures under
 * {@code src/test/resources/json}) into a puli {@link org.liveontologies.puli.Proof} whose
 * {@code getInferences(conclusion)} answers not just the root but every nested premise too.
 */
public class RustdlProofTest {

    private static final OWLDataFactory DF = OWLManager.getOWLDataFactory();

    private static OWLClass cls(String localName) {
        return DF.getOWLClass(IRI.create("http://ex/#" + localName));
    }

    /**
     * A source ontology containing exactly the axioms the JSON fixtures cite as leaves.
     *
     * #56: `fromProveJson` now verifies every CITED LEAF axiom against the source, so a test
     * that passed an empty ontology would make every one of these tests fail — the source is
     * part of the fixture now, not incidental setup.
     */
    private static OWLOntology sourceWith(OWLAxiom... axioms) throws Exception {
        OWLOntologyManager manager = OWLManager.createOWLOntologyManager();
        OWLOntology ontology = manager.createOntology();
        for (OWLAxiom axiom : axioms) {
            manager.addAxiom(ontology, axiom);
        }
        return ontology;
    }

    /** The two told axioms `prove.json` cites. */
    private static OWLOntology proveJsonSource() throws Exception {
        return sourceWith(
            DF.getOWLSubClassOfAxiom(cls("A"), cls("B")),
            DF.getOWLSubClassOfAxiom(cls("B"), cls("C")));
    }

    private String fixture(String name) throws Exception {
        return new String(Files.readAllBytes(
            Paths.get(getClass().getResource("/json/" + name).toURI())));
    }

    @Test
    public void stepProofRootInferenceHasCorrectRuleAndPremises() throws Exception {
        RustdlJson.ProveJson json = RustdlProcess.parseProve(fixture("prove.json"));
        OWLSubClassOfAxiom goal = DF.getOWLSubClassOfAxiom(cls("A"), cls("C"));

        RustdlProof proof = RustdlProof.fromProveJson(json, goal, proveJsonSource());

        Collection<? extends Inference<OWLAxiom>> rootInferences = proof.getInferences(goal);
        assertEquals(1, rootInferences.size());
        Inference<OWLAxiom> root = rootInferences.iterator().next();
        assertEquals("SubsumerTransitivity(fwd)", root.getName());
        assertEquals(goal, root.getConclusion());

        // The root's own JSON node cited no axioms (a pure transitivity step), so its premises
        // are exactly the two child nodes' conclusions.
        OWLSubClassOfAxiom ab = DF.getOWLSubClassOfAxiom(cls("A"), cls("B"));
        OWLSubClassOfAxiom bc = DF.getOWLSubClassOfAxiom(cls("B"), cls("C"));
        assertEquals(2, root.getPremises().size());
        assertTrue(root.getPremises().contains(ab));
        assertTrue(root.getPremises().contains(bc));
    }

    @Test
    public void nestedPremiseInferenceIsReachableAndCitesItsAxiom() throws Exception {
        RustdlJson.ProveJson json = RustdlProcess.parseProve(fixture("prove.json"));
        OWLSubClassOfAxiom goal = DF.getOWLSubClassOfAxiom(cls("A"), cls("C"));
        RustdlProof proof = RustdlProof.fromProveJson(json, goal, proveJsonSource());

        OWLSubClassOfAxiom ab = DF.getOWLSubClassOfAxiom(cls("A"), cls("B"));
        Collection<? extends Inference<OWLAxiom>> abInferences = proof.getInferences(ab);
        // ab is BOTH the "ToldSubsumer" step's conclusion AND that same step's own cited axiom
        // (a leaf node with no child premises, only a directly-cited axiom identical to what it
        // proves) -- both got produce()d keyed by the same (structurally-equal) axiom, so both
        // inferences are reachable from one getInferences(ab) call.
        assertEquals(2, abInferences.size());

        Inference<OWLAxiom> toldSubsumer = null;
        Inference<OWLAxiom> asserted = null;
        for (Inference<OWLAxiom> inference : abInferences) {
            if ("ToldSubsumer".equals(inference.getName())) {
                toldSubsumer = inference;
            } else if ("asserted".equals(inference.getName())) {
                asserted = inference;
            }
        }
        assertTrue("expected a ToldSubsumer inference for ab", toldSubsumer != null);
        assertTrue("expected an 'asserted' leaf inference for the cited axiom", asserted != null);

        assertEquals(ab, toldSubsumer.getConclusion());
        // The ToldSubsumer step cited its own axiom (SubClassOf(:A :B)) as its one premise.
        assertEquals(1, toldSubsumer.getPremises().size());
        assertEquals(ab, toldSubsumer.getPremises().get(0));

        assertEquals(ab, asserted.getConclusion());
        assertTrue(asserted.getPremises().isEmpty());
    }

    @Test
    public void fallbackBuildsSyntheticJustificationInference() throws Exception {
        RustdlJson.ProveJson json = RustdlProcess.parseProve(fixture("prove_fallback.json"));
        OWLSubClassOfAxiom goal = DF.getOWLSubClassOfAxiom(cls("Sub"), cls("Sup"));

        RustdlProof proof = RustdlProof.fromProveJson(json, goal, sourceWith(
            DF.getOWLSubClassOfAxiom(cls("X"), cls("Y")),
            DF.getOWLSubClassOfAxiom(cls("Y"), cls("Z"))));

        Collection<? extends Inference<OWLAxiom>> inferences = proof.getInferences(goal);
        assertEquals(1, inferences.size());
        Inference<OWLAxiom> inference = inferences.iterator().next();
        assertEquals("justification", inference.getName());
        assertEquals(goal, inference.getConclusion());

        OWLSubClassOfAxiom xy = DF.getOWLSubClassOfAxiom(cls("X"), cls("Y"));
        OWLSubClassOfAxiom yz = DF.getOWLSubClassOfAxiom(cls("Y"), cls("Z"));
        assertEquals(2, inference.getPremises().size());
        assertTrue(inference.getPremises().contains(xy));
        assertTrue(inference.getPremises().contains(yz));
    }

    /**
     * #56 — a genuine proof survives verification unchanged.
     *
     * The companion to {@link #aFabricatedLeafAxiomIsRejected}: without this one, "the guard
     * catches fabrication" would be satisfied by a guard that rejects EVERYTHING. Both cited
     * leaves here are real source axioms, so all four inferences must still be produced.
     */
    @Test
    public void aGenuineProofSurvivesLeafVerification() throws Exception {
        RustdlJson.ProveJson json = RustdlProcess.parseProve(fixture("prove.json"));
        OWLSubClassOfAxiom goal = DF.getOWLSubClassOfAxiom(cls("A"), cls("C"));

        RustdlProof proof = RustdlProof.fromProveJson(json, goal, proveJsonSource());

        assertEquals(1, proof.getInferences(goal).size());
        // Both told leaves survive, each carrying its own "asserted" inference.
        assertEquals(2, proof.getInferences(DF.getOWLSubClassOfAxiom(cls("A"), cls("B"))).size());
        assertEquals(2, proof.getInferences(DF.getOWLSubClassOfAxiom(cls("B"), cls("C"))).size());
    }

    /**
     * #56 — THE ANTI-FABRICATION TEST. A cited leaf axiom absent from the source must reject the
     * whole proof rather than being displayed as the justification for a step.
     *
     * The source here deliberately OMITS `SubClassOf(:B :C)` while `prove.json` cites it, which
     * is exactly the shape a future OFN writer/parser normalization mismatch would produce: a
     * leaf that looks well-formed and is not what the ontology asserts. Before this guard the
     * proof view rendered it verbatim.
     *
     * Fail-hard, matching the justify surface — a partial proof with the offending leaf dropped
     * would still be displayed as an explanation while no longer establishing its conclusion.
     */
    @Test
    public void aFabricatedLeafAxiomIsRejected() throws Exception {
        RustdlJson.ProveJson json = RustdlProcess.parseProve(fixture("prove.json"));
        OWLSubClassOfAxiom goal = DF.getOWLSubClassOfAxiom(cls("A"), cls("C"));
        OWLOntology missingOneLeaf = sourceWith(DF.getOWLSubClassOfAxiom(cls("A"), cls("B")));

        try {
            RustdlProof.fromProveJson(json, goal, missingOneLeaf);
            fail("a cited leaf axiom absent from the source must be rejected as a possible "
                + "fabrication, not rendered into the proof");
        } catch (ExplanationException expected) {
            assertTrue(
                "the message must name the offending axiom and the surface: "
                    + expected.getMessage(),
                expected.getMessage().contains("proof")
                    && expected.getMessage().contains("not present in the source ontology"));
        }
    }

    /**
     * #56 — the `justification_fallback` branch is guarded too.
     *
     * Every axiom on that path IS a literal source axiom (rustdl emits a plain minimal
     * justification there, never a laconic-weakened one), so it is verified wholesale. Missed by
     * a fix that only walked the proof tree.
     */
    @Test
    public void aFabricatedFallbackAxiomIsRejected() throws Exception {
        RustdlJson.ProveJson json = RustdlProcess.parseProve(fixture("prove_fallback.json"));
        OWLSubClassOfAxiom goal = DF.getOWLSubClassOfAxiom(cls("Sub"), cls("Sup"));
        OWLOntology missingOne = sourceWith(DF.getOWLSubClassOfAxiom(cls("X"), cls("Y")));

        try {
            RustdlProof.fromProveJson(json, goal, missingOne);
            fail("a fabricated justification_fallback axiom must be rejected too");
        } catch (ExplanationException expected) {
            assertTrue(expected.getMessage().contains("not present in the source ontology"));
        }
    }

    @Test
    public void unreachableConclusionHasNoInferences() throws Exception {
        RustdlJson.ProveJson json = RustdlProcess.parseProve(fixture("prove.json"));
        OWLSubClassOfAxiom goal = DF.getOWLSubClassOfAxiom(cls("A"), cls("C"));
        RustdlProof proof = RustdlProof.fromProveJson(json, goal, proveJsonSource());

        OWLSubClassOfAxiom unrelated = DF.getOWLSubClassOfAxiom(cls("Unrelated1"), cls("Unrelated2"));
        assertTrue(proof.getInferences(unrelated).isEmpty());
    }
}
