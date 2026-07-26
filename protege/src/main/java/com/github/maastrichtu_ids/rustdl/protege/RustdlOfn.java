package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owl.explanation.api.ExplanationException;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.formats.FunctionalSyntaxDocumentFormat;
import org.semanticweb.owlapi.io.StringDocumentSource;
import org.semanticweb.owlapi.model.IRI;
import org.semanticweb.owlapi.model.OWLAxiom;
import org.semanticweb.owlapi.model.OWLOntology;
import org.semanticweb.owlapi.model.OWLOntologyCreationException;
import org.semanticweb.owlapi.model.OWLOntologyManager;
import org.semanticweb.owlapi.model.parameters.AxiomAnnotations;
import org.semanticweb.owlapi.model.parameters.Imports;

import java.util.LinkedHashSet;
import java.util.Set;
import java.util.concurrent.atomic.AtomicLong;
import java.util.logging.Logger;

/**
 * Parses the self-contained OWL Functional Syntax ontology DOCUMENTS emitted per-justification
 * in {@code rustdl justify --json}'s {@code justifications[].ofn} field, and enforces the
 * anti-fabrication guard that every axiom surfaced to the user genuinely occurs in the source
 * ontology (mirrors km's {@code materialize}).
 */
final class RustdlOfn {
    private static final Logger LOG = Logger.getLogger(RustdlOfn.class.getName());
    private static final AtomicLong COUNTER = new AtomicLong();

    private RustdlOfn() {}

    /**
     * Loads {@code ofnDocument} — a full {@code Prefix(...)}/{@code Ontology(...)} OWL
     * Functional Syntax document, NOT a bare axiom fragment — as a fresh, throwaway ontology
     * and returns its own axioms (excluding imports, which a rustdl justification document
     * never has).
     */
    static Set<OWLAxiom> parse(String ofnDocument) {
        if (ofnDocument == null || ofnDocument.isEmpty()) {
            throw new IllegalArgumentException("rustdl returned an empty justification document");
        }
        try {
            OWLOntologyManager manager = OWLManager.createOWLOntologyManager();
            IRI documentIri = IRI.create(
                "urn:rustdl-justification:" + COUNTER.incrementAndGet());
            StringDocumentSource source = new StringDocumentSource(
                ofnDocument, documentIri, new FunctionalSyntaxDocumentFormat(), null);
            OWLOntology parsed = manager.loadOntologyFromOntologyDocument(source);
            return new LinkedHashSet<>(parsed.getAxioms(Imports.EXCLUDED));
        } catch (OWLOntologyCreationException error) {
            throw new IllegalStateException(
                "rustdl returned an unparseable justification document: " + ofnDocument, error);
        }
    }

    /**
     * Parses {@code ofnDocument} (a self-contained OFN ontology document — see {@link #parse})
     * and returns its single LOGICAL axiom (via {@link OWLAxiom#isLogicalAxiom()}, which excludes
     * declarations/annotations), for {@code rustdl prove --json}'s per-proof-node
     * {@code conclusion} and {@code axioms[]} documents (each documented, in
     * {@code docs/json-schema.md}, to contain exactly one logical axiom). Fails loudly — rather
     * than silently picking one — if the document does not contain EXACTLY one: an ambiguous
     * proof node must never be silently resolved into a possibly-wrong axiom.
     */
    static OWLAxiom singleLogicalAxiom(String ofnDocument) {
        Set<OWLAxiom> parsed = parse(ofnDocument);
        OWLAxiom found = null;
        int count = 0;
        for (OWLAxiom axiom : parsed) {
            if (axiom.isLogicalAxiom()) {
                found = axiom;
                count++;
            }
        }
        if (count != 1) {
            throw new IllegalStateException(
                "expected exactly one logical axiom in rustdl proof document, found " + count
                    + ": " + ofnDocument);
        }
        return found;
    }

    /**
     * Fail-hard anti-fabrication guard (mirrors km's {@code KMExplanationGenerator.materialize}):
     * every axiom in {@code parsed} must genuinely occur in {@code source}'s imports closure. If
     * ANY axiom does not, the WHOLE justification is rejected — logging a warning identifying the
     * offending axiom, then throwing {@link ExplanationException} — rather than silently dropping
     * just that axiom and surfacing the surviving subset. A partial surviving subset can be
     * genuine-but-INSUFFICIENT to actually entail the target, which would silently produce a
     * misleading "Explanation"; under correct rustdl operation this is inert (rustdl's
     * {@code justify} only ever returns genuine source axioms), so this is a soundness safety
     * net, not a normal code path. Because the caller ({@code
     * RustdlExplanationGenerator#materialize}) does not catch this per-justification, the thrown
     * exception also aborts every OTHER justification in the same batch — km's own choice (its
     * equivalent check throws unguarded from inside the per-justification loop too), which this
     * mirrors rather than the softer "skip just this one" alternative.
     */
    static Set<OWLAxiom> verifiedAgainst(Set<OWLAxiom> parsed, OWLOntology source) {
        for (OWLAxiom axiom : parsed) {
            if (!source.containsAxiom(axiom, Imports.INCLUDED, AxiomAnnotations.IGNORE_AXIOM_ANNOTATIONS)) {
                LOG.warning("Rejecting rustdl justification: axiom not found in the source "
                    + "ontology's imports closure (possible fabrication): " + axiom);
                throw new ExplanationException(
                    "rustdl justification contained an axiom not present in the source ontology "
                        + "(possible fabrication): " + axiom);
            }
        }
        return parsed;
    }
}
