package com.github.maastrichtu_ids.rustdl.protege;

import org.protege.editor.owl.model.inference.AbstractProtegeOWLReasonerInfo;
import org.semanticweb.owlapi.reasoner.BufferingMode;
import org.semanticweb.owlapi.reasoner.OWLReasonerFactory;

/** Places "rustdl" in Protégé's reasoner dropdown. */
public class RustdlReasonerInfo extends AbstractProtegeOWLReasonerInfo {
    private final RustdlReasonerFactory factory = new RustdlReasonerFactory();

    @Override public OWLReasonerFactory getReasonerFactory() { return factory; }
    @Override public BufferingMode getRecommendedBuffering() { return BufferingMode.BUFFERING; }
    @Override public void initialise() { }
    @Override public void dispose() { }
}
