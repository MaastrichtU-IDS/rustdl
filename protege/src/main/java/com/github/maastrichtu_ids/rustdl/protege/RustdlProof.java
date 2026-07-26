package com.github.maastrichtu_ids.rustdl.protege;

import org.liveontologies.puli.BaseProof;
import org.liveontologies.puli.Inference;
import org.semanticweb.owlapi.model.OWLAxiom;

import java.util.ArrayList;
import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/**
 * A puli {@link org.liveontologies.puli.Proof} over a {@code rustdl prove --json} result,
 * mirroring ELK's {@code ElkOwlProof} (which wraps ELK's own internal evidence into the same
 * puli {@code Proof}/{@code Inference} model). Every {@link RustdlJson.ProofNodeJson} in the
 * tree becomes one {@link Inference}{@code <OWLAxiom>}, {@link BaseProof#produce produced} into
 * this proof keyed by its conclusion axiom — so {@link #getInferences(Object)} (inherited from
 * {@link BaseProof}) answers not just the root conclusion but every intermediate premise in the
 * tree too, which is what lets Protégé's proof view recurse into nested steps.
 *
 * <p><b>Cited source axioms as premises.</b> Each step's {@code axioms} (the source axioms rustdl
 * cited for that rule application — e.g. the told {@code SubClassOf} a {@code ToldSubsumer} step
 * applies) are folded in as ADDITIONAL premises alongside the recursively-built child-node
 * conclusions, and each distinct cited axiom is itself registered as a trivial "asserted" leaf
 * inference (rule name {@code "asserted"}, zero premises) so it appears in the proof as a
 * terminal, given fact rather than an unexplained gap. This mirrors how ELK's own proof DAG
 * represents told axioms as leaf conclusions (via {@code AssertedConclusionInference}) rather
 * than leaving them implicit in the rule name alone — without folding them in, a leaf
 * {@code ToldSubsumer} step would show zero premises and hide the very axiom that justifies it,
 * defeating the point of a step-level proof surface.</p>
 */
final class RustdlProof extends BaseProof<Inference<OWLAxiom>> {

    /** De-duplicates "asserted" leaf inferences across the whole tree (same axiom may be cited by
     * more than one step). */
    private final Set<OWLAxiom> assertedRegistered = new HashSet<>();

    private RustdlProof() {}

    /**
     * Builds the proof for one {@code rustdl prove --json} result.
     *
     * @param json the parsed {@code ProveJson}; must have {@code entailed == true} (the caller —
     *     {@link RustdlProofService} — only builds a proof for an entailed result).
     * @param goal the OWLAxiom the caller asked {@link RustdlProofService#getProof} to explain.
     *     Used directly as the synthetic fallback inference's conclusion when
     *     {@code has_proof == false} (there is no OFN document representing the goal in that
     *     branch — {@code justification_fallback} carries only the justification's axioms, not
     *     the entailment itself). When {@code has_proof == true} the root node's own parsed
     *     conclusion is used instead (expected — not re-checked here — to be structurally equal
     *     to {@code goal}, since it renders the same {@code SUB ⊑ SUP} the query asked about).
     */
    static RustdlProof fromProveJson(RustdlJson.ProveJson json, OWLAxiom goal) {
        if (json == null) {
            throw new IllegalArgumentException("rustdl prove --json returned no result");
        }
        RustdlProof proof = new RustdlProof();
        if (json.has_proof) {
            if (json.proof == null) {
                throw new IllegalStateException(
                    "rustdl prove --json: has_proof=true but proof is null");
            }
            proof.buildNode(json.proof);
        } else {
            if (json.justification_fallback == null) {
                throw new IllegalStateException(
                    "rustdl prove --json: has_proof=false but justification_fallback is null");
            }
            List<OWLAxiom> premises = new ArrayList<>(RustdlOfn.parse(json.justification_fallback));
            proof.produce(new SimpleInference("justification", goal, premises));
        }
        return proof;
    }

    /**
     * Recursively converts {@code node} — and every node in its subtree — into
     * {@link Inference}s produced into this proof, returning {@code node}'s own conclusion axiom
     * so the caller (a parent {@code buildNode} call, or {@link #fromProveJson}) can use it as one
     * of ITS premises.
     */
    private OWLAxiom buildNode(RustdlJson.ProofNodeJson node) {
        // No conclusion de-dup guard needed here today: rustdl's `ProofNodeJson` tree is an OWNED
        // tree (each premise belongs to exactly one parent), not a shared DAG, so a shared
        // sub-conclusion is simply rebuilt (and re-produce()d, harmlessly, into this Proof) once
        // per occurrence rather than being visited once and reused. If a future rustdl change
        // makes `ProofNode` DAG-share premises (so the same node object appears under multiple
        // parents), this recursion would need a visited-conclusion memo to avoid redundant
        // re-processing (still correct, just wasted work) — see the Task 4 review fold-in M4.2.
        OWLAxiom conclusion = RustdlOfn.singleLogicalAxiom(node.conclusion);

        List<OWLAxiom> premises = new ArrayList<>();
        if (node.axioms != null) {
            for (String axiomDoc : node.axioms) {
                OWLAxiom axiom = RustdlOfn.singleLogicalAxiom(axiomDoc);
                premises.add(axiom);
                if (assertedRegistered.add(axiom)) {
                    produce(new SimpleInference("asserted", axiom, Collections.emptyList()));
                }
            }
        }
        if (node.premises != null) {
            for (RustdlJson.ProofNodeJson child : node.premises) {
                premises.add(buildNode(child));
            }
        }

        produce(new SimpleInference(node.rule, conclusion, premises));
        return conclusion;
    }

    /** A trivial {@link Inference}{@code <OWLAxiom>} built directly from already-parsed axioms. */
    private static final class SimpleInference implements Inference<OWLAxiom> {
        private final String name;
        private final OWLAxiom conclusion;
        private final List<OWLAxiom> premises;

        SimpleInference(String name, OWLAxiom conclusion, List<OWLAxiom> premises) {
            this.name = name;
            this.conclusion = conclusion;
            this.premises = Collections.unmodifiableList(new ArrayList<>(premises));
        }

        @Override
        public String getName() {
            return name;
        }

        @Override
        public OWLAxiom getConclusion() {
            return conclusion;
        }

        @Override
        public List<? extends OWLAxiom> getPremises() {
            return premises;
        }

        @Override
        public String toString() {
            return name + premises + " => " + conclusion;
        }
    }
}
