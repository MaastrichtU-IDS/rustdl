package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owlapi.model.OWLOntology;
import org.semanticweb.owlapi.reasoner.*;

public class RustdlReasonerFactory implements OWLReasonerFactory {
    @Override public String getReasonerName() { return "rustdl"; }
    @Override public OWLReasoner createReasoner(OWLOntology o) {
        return new RustdlReasoner(o, new SimpleConfiguration(), BufferingMode.BUFFERING);
    }
    @Override public OWLReasoner createReasoner(OWLOntology o, OWLReasonerConfiguration c) {
        return new RustdlReasoner(o, c, BufferingMode.BUFFERING);
    }
    @Override public OWLReasoner createNonBufferingReasoner(OWLOntology o) {
        return new RustdlReasoner(o, new SimpleConfiguration(), BufferingMode.NON_BUFFERING);
    }
    @Override public OWLReasoner createNonBufferingReasoner(OWLOntology o, OWLReasonerConfiguration c) {
        return new RustdlReasoner(o, c, BufferingMode.NON_BUFFERING);
    }
}
