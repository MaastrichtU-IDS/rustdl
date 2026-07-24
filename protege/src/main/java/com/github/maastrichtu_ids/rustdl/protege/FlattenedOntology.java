package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.formats.FunctionalSyntaxDocumentFormat;
import org.semanticweb.owlapi.model.*;
import org.semanticweb.owlapi.model.parameters.Imports;

import java.nio.file.Path;

/** Serialises an ontology's imports closure into one OWL Functional Syntax file. */
public final class FlattenedOntology {
    private FlattenedOntology() {}

    public static void writeOfn(OWLOntology source, Path destination) throws OWLOntologyCreationException, OWLOntologyStorageException {
        OWLOntologyManager manager = OWLManager.createOWLOntologyManager();
        OWLOntology flattened = manager.createOntology(source.getAxioms(Imports.INCLUDED));
        manager.saveOntology(flattened, new FunctionalSyntaxDocumentFormat(),
            IRI.create(destination.toUri()));
    }
}
