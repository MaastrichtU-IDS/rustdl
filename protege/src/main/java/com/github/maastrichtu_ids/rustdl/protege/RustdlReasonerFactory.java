package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owlapi.reasoner.*;
import org.semanticweb.owlapi.model.OWLOntology;

/** Task-1 stub — replaced in Task 4. */
public class RustdlReasonerFactory implements OWLReasonerFactory {
    @Override public String getReasonerName() { return "rustdl"; }
    @Override public OWLReasoner createReasoner(OWLOntology o) { throw new UnsupportedOperationException("stub"); }
    @Override public OWLReasoner createReasoner(OWLOntology o, OWLReasonerConfiguration c) { throw new UnsupportedOperationException("stub"); }
    @Override public OWLReasoner createNonBufferingReasoner(OWLOntology o) { throw new UnsupportedOperationException("stub"); }
    @Override public OWLReasoner createNonBufferingReasoner(OWLOntology o, OWLReasonerConfiguration c) { throw new UnsupportedOperationException("stub"); }
}
