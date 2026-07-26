package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.liveontologies.puli.Inference;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.IRI;
import org.semanticweb.owlapi.model.OWLAxiom;
import org.semanticweb.owlapi.model.OWLClass;
import org.semanticweb.owlapi.model.OWLDataFactory;
import org.semanticweb.owlapi.model.OWLSubClassOfAxiom;

import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.Collection;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

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

    private String fixture(String name) throws Exception {
        return new String(Files.readAllBytes(
            Paths.get(getClass().getResource("/json/" + name).toURI())));
    }

    @Test
    public void stepProofRootInferenceHasCorrectRuleAndPremises() throws Exception {
        RustdlJson.ProveJson json = RustdlProcess.parseProve(fixture("prove.json"));
        OWLSubClassOfAxiom goal = DF.getOWLSubClassOfAxiom(cls("A"), cls("C"));

        RustdlProof proof = RustdlProof.fromProveJson(json, goal);

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
        RustdlProof proof = RustdlProof.fromProveJson(json, goal);

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

        RustdlProof proof = RustdlProof.fromProveJson(json, goal);

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

    @Test
    public void unreachableConclusionHasNoInferences() throws Exception {
        RustdlJson.ProveJson json = RustdlProcess.parseProve(fixture("prove.json"));
        OWLSubClassOfAxiom goal = DF.getOWLSubClassOfAxiom(cls("A"), cls("C"));
        RustdlProof proof = RustdlProof.fromProveJson(json, goal);

        OWLSubClassOfAxiom unrelated = DF.getOWLSubClassOfAxiom(cls("Unrelated1"), cls("Unrelated2"));
        assertTrue(proof.getInferences(unrelated).isEmpty());
    }
}
