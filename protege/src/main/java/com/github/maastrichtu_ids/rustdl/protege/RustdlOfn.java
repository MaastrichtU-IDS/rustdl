package com.github.maastrichtu_ids.rustdl.protege;

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
     * Anti-fabrication guard: drops any axiom in {@code parsed} that is not actually present
     * in {@code source}'s imports closure, logging a warning for each dropped axiom. An
     * {@code Explanation} handed to the caller must contain only genuine source axioms — never
     * an axiom rustdl invented or one that survived only because of a parsing/round-trip quirk.
     */
    static Set<OWLAxiom> verifiedAgainst(Set<OWLAxiom> parsed, OWLOntology source) {
        Set<OWLAxiom> verified = new LinkedHashSet<>();
        for (OWLAxiom axiom : parsed) {
            if (source.containsAxiom(axiom, Imports.INCLUDED, AxiomAnnotations.IGNORE_AXIOM_ANNOTATIONS)) {
                verified.add(axiom);
            } else {
                LOG.warning("Dropping rustdl justification axiom not found in the source "
                    + "ontology (possible fabrication): " + axiom);
            }
        }
        return verified;
    }
}
