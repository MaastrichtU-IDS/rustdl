package com.github.maastrichtu_ids.rustdl.protege;

import org.liveontologies.protege.explanation.proof.service.ProofService;
import org.liveontologies.puli.DynamicProof;
import org.liveontologies.puli.DynamicProof.ChangeListener;
import org.liveontologies.puli.Inference;
import org.semanticweb.owlapi.model.OWLAxiom;
import org.semanticweb.owlapi.model.OWLClassExpression;
import org.semanticweb.owlapi.model.OWLOntology;
import org.semanticweb.owlapi.model.OWLSubClassOfAxiom;
import org.semanticweb.owlapi.reasoner.UnsupportedEntailmentTypeException;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Collection;

/**
 * Exposes {@code rustdl prove --json}'s step-level EL proof trees (falling back to the
 * axiom-level justification for SROIQ-only entailments) through the liveontologies
 * {@code org.liveontologies.protege.explanation.proof.service} extension point, mirroring ELK's
 * {@code ElkProofService}.
 *
 * <p>Unlike ELK — whose reasoner instance lives in the JVM and can incrementally notify this
 * service of changes — {@code rustdl} is an external process rerun per query, so
 * {@link #getProof(OWLAxiom)} always serializes the CURRENT active ontology and spawns a fresh
 * {@code rustdl prove --json} at call time; the returned {@link DynamicProof} is a minimal
 * recompute-on-demand snapshot (no incremental change tracking, no listener notification after
 * construction) rather than ELK's reactive {@code DynamicOwlProof} — acceptable for v1 per the
 * task brief. A subsequent ontology edit is picked up the next time Protégé calls
 * {@link #getProof(OWLAxiom)} again (e.g. re-opening the Explanation dialog), not by this
 * instance updating itself in place.</p>
 */
public final class RustdlProofService extends ProofService {

    /** Overridable via {@code -Drustdl.prove.timeout.seconds} (default matches the reasoner's own
     * per-query default order of magnitude; prove is a single bounded subprocess call, not a
     * long-running classify). */
    private static final long TIMEOUT_SECONDS =
        Long.getLong("rustdl.prove.timeout.seconds", 60L);

    @Override
    public void initialise() throws Exception {
        // No state to set up: getProof (re)computes fresh via a fresh subprocess run every time,
        // so there is nothing to prime here (contrast ELK's ElkProofService, which caches the
        // live ElkReasoner instance and listens for reasoner/ontology-change events).
    }

    @Override
    public void dispose() {
        // Nothing held across calls.
    }

    @Override
    public boolean hasProof(OWLAxiom entailment) {
        return isSupported(entailment);
    }

    @Override
    public DynamicProof<Inference<? extends OWLAxiom>> getProof(OWLAxiom entailment)
            throws UnsupportedEntailmentTypeException {
        if (!isSupported(entailment)) {
            throw new UnsupportedEntailmentTypeException(entailment);
        }
        return new RecomputingProof(entailment);
    }

    @Override
    public Inference<? extends OWLAxiom> getExample(Inference<? extends OWLAxiom> inference) {
        // No canned "worked example" library for rustdl's rules (ELK substitutes a
        // representative example inference for its own rule names here); nothing to substitute,
        // so return the inference itself unchanged.
        return inference;
    }

    /**
     * Exact entailment surface: a {@code SubClassOf} between two NAMED class expressions — the
     * only shape {@code rustdl prove --json <ofn> <sub> <sup>} accepts (it takes two literal
     * IRIs, not the richer query-keyword grammar {@code justify} supports). Deliberately not
     * extended beyond this (YAGNI): no unsat/inconsistent/instance/property-assertion proof
     * queries, since {@code prove} itself has no such forms to run.
     */
    private static boolean isSupported(OWLAxiom entailment) {
        if (!(entailment instanceof OWLSubClassOfAxiom)) {
            return false;
        }
        OWLSubClassOfAxiom subClassOf = (OWLSubClassOfAxiom) entailment;
        OWLClassExpression sub = subClassOf.getSubClass();
        OWLClassExpression sup = subClassOf.getSuperClass();
        return !sub.isAnonymous() && !sup.isAnonymous();
    }

    /**
     * Serializes the active ontology and runs {@code rustdl prove --json} exactly once, at
     * construction time, then answers every {@link #getInferences(Object)} call from that single
     * snapshot. Mirrors the SHAPE of ELK's {@code ElkProofService.DynamicOwlProof} (an inner
     * {@link DynamicProof}{@code <Inference<? extends OWLAxiom>>} wrapping a delegate whose
     * declared type carries the outer wildcard, {@code DynamicProof<? extends Inference<?
     * extends OWLAxiom>>}, which is what lets a concretely-typed {@code RustdlProof} — a
     * {@code DynamicProof<Inference<OWLAxiom>>} — satisfy the wildcarded return type this
     * class's {@link ProofService#getProof} override must produce) without ELK's reactive
     * reasoner-change listening (see the class-level javadoc).
     */
    private final class RecomputingProof implements DynamicProof<Inference<? extends OWLAxiom>> {

        private final DynamicProof<? extends Inference<? extends OWLAxiom>> proof;

        RecomputingProof(OWLAxiom entailment) throws UnsupportedEntailmentTypeException {
            this.proof = compute(entailment);
        }

        private DynamicProof<? extends Inference<? extends OWLAxiom>> compute(OWLAxiom entailment)
                throws UnsupportedEntailmentTypeException {
            OWLSubClassOfAxiom subClassOf = (OWLSubClassOfAxiom) entailment;
            String sub = subClassOf.getSubClass().asOWLClass().getIRI().toString();
            String sup = subClassOf.getSuperClass().asOWLClass().getIRI().toString();
            OWLOntology ontology = getEditorKit().getOWLModelManager().getActiveOntology();

            Path ofn = null;
            try {
                ofn = Files.createTempFile("rustdl-prove-", ".ofn");
                FlattenedOntology.writeOfn(ontology, ofn);
                RustdlJson.ProveJson json = RustdlProcess.prove(ofn, sub, sup, TIMEOUT_SECONDS);
                if (!json.entailed) {
                    // hasProof() only promises a SUPPORTED query shape, not that the axiom is
                    // actually entailed; Protégé only calls getProof for entailments it already
                    // knows hold (e.g. from the class hierarchy or the Explanation dialog), so
                    // this should not happen in practice -- but if rustdl and the caller ever
                    // disagree, failing loudly here is safer than fabricating an empty proof.
                    throw new UnsupportedEntailmentTypeException(entailment);
                }
                return RustdlProof.fromProveJson(json, entailment, ontology);
            } catch (IOException error) {
                throw new IllegalStateException(
                    "rustdl prove --json failed: " + error.getMessage(), error);
            } catch (org.semanticweb.owlapi.model.OWLOntologyCreationException
                    | org.semanticweb.owlapi.model.OWLOntologyStorageException error) {
                throw new IllegalStateException(
                    "could not serialize the active ontology for rustdl prove: "
                        + error.getMessage(), error);
            } finally {
                if (ofn != null) {
                    try {
                        Files.deleteIfExists(ofn);
                    } catch (IOException ignored) {
                        // Temporary-file cleanup must not mask the proof result.
                    }
                }
            }
        }

        @Override
        public Collection<? extends Inference<? extends OWLAxiom>> getInferences(Object conclusion) {
            return proof.getInferences(conclusion);
        }

        @Override
        public void addListener(ChangeListener listener) {
            // Recompute-on-demand (see class javadoc): this snapshot never mutates after
            // construction, so there is nothing to notify a listener about later. A later
            // ontology edit is picked up by a FRESH getProof() call, not by this instance
            // changing in place.
        }

        @Override
        public void removeListener(ChangeListener listener) {
            // No-op: addListener above never retains one.
        }

        @Override
        public void dispose() {
            // Nothing held beyond the immutable `proof` snapshot.
        }
    }
}
