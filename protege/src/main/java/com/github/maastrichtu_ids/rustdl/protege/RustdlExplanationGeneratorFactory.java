package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owl.explanation.api.ExplanationException;
import org.semanticweb.owl.explanation.api.ExplanationGenerator;
import org.semanticweb.owl.explanation.api.ExplanationGeneratorFactory;
import org.semanticweb.owl.explanation.api.ExplanationProgressMonitor;
import org.semanticweb.owl.explanation.api.NullExplanationProgressMonitor;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.OWLAxiom;
import org.semanticweb.owlapi.model.OWLOntology;

import java.util.LinkedHashSet;
import java.util.Set;

/**
 * Discoverable ({@code META-INF/services/org.semanticweb.owl.explanation.api.ExplanationGeneratorFactory})
 * factory for {@link RustdlExplanationGenerator}s, backing Protégé's Explanation ("?") dialog
 * with rustdl's non-laconic {@code justify} justifications.
 */
public class RustdlExplanationGeneratorFactory implements ExplanationGeneratorFactory<OWLAxiom> {

    private final RustdlExplainConfiguration configuration;

    public RustdlExplanationGeneratorFactory() {
        this(RustdlExplainConfiguration.fromSystemProperties());
    }

    public RustdlExplanationGeneratorFactory(RustdlExplainConfiguration configuration) {
        if (configuration == null) {
            throw new NullPointerException("configuration");
        }
        this.configuration = configuration;
    }

    /** Display name for this generator; distinguishes it from the laconic variant. */
    public String getExplanationGeneratorName() {
        return "rustdl";
    }

    /** Whether {@link #createExplanationGenerator} produces laconic (structurally weakened) justifications. */
    boolean isLaconic() {
        return false;
    }

    @Override
    public ExplanationGenerator<OWLAxiom> createExplanationGenerator(OWLOntology ontology) {
        return createExplanationGenerator(ontology, new NullExplanationProgressMonitor<OWLAxiom>());
    }

    @Override
    public ExplanationGenerator<OWLAxiom> createExplanationGenerator(
            OWLOntology ontology, ExplanationProgressMonitor<OWLAxiom> progressMonitor) {
        if (ontology == null) {
            throw new NullPointerException("ontology");
        }
        if (progressMonitor == null) {
            throw new NullPointerException("progressMonitor");
        }
        return new RustdlExplanationGenerator(ontology, progressMonitor, configuration, isLaconic());
    }

    @Override
    public ExplanationGenerator<OWLAxiom> createExplanationGenerator(Set<? extends OWLAxiom> axioms) {
        return createExplanationGenerator(axioms, new NullExplanationProgressMonitor<OWLAxiom>());
    }

    @Override
    public ExplanationGenerator<OWLAxiom> createExplanationGenerator(
            Set<? extends OWLAxiom> axioms, ExplanationProgressMonitor<OWLAxiom> progressMonitor) {
        if (axioms == null) {
            throw new NullPointerException("axioms");
        }
        try {
            Set<OWLAxiom> copied = new LinkedHashSet<>();
            copied.addAll(axioms);
            OWLOntology ontology = OWLManager.createOWLOntologyManager().createOntology(copied);
            return createExplanationGenerator(ontology, progressMonitor);
        } catch (Exception error) {
            throw new ExplanationException(
                "Could not create an ontology for rustdl explanation axioms", error);
        }
    }
}
