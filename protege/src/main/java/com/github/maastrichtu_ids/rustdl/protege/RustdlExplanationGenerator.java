package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owl.explanation.api.Explanation;
import org.semanticweb.owl.explanation.api.ExplanationException;
import org.semanticweb.owl.explanation.api.ExplanationGenerator;
import org.semanticweb.owl.explanation.api.ExplanationGeneratorInterruptedException;
import org.semanticweb.owl.explanation.api.ExplanationProgressMonitor;
import org.semanticweb.owl.explanation.api.UnsupportedEntailmentException;
import org.semanticweb.owlapi.model.OWLAxiom;
import org.semanticweb.owlapi.model.OWLClass;
import org.semanticweb.owlapi.model.OWLClassAssertionAxiom;
import org.semanticweb.owlapi.model.OWLClassExpression;
import org.semanticweb.owlapi.model.OWLDisjointClassesAxiom;
import org.semanticweb.owlapi.model.OWLEquivalentClassesAxiom;
import org.semanticweb.owlapi.model.OWLIndividual;
import org.semanticweb.owlapi.model.OWLNamedIndividual;
import org.semanticweb.owlapi.model.OWLObjectPropertyAssertionAxiom;
import org.semanticweb.owlapi.model.OWLObjectPropertyExpression;
import org.semanticweb.owlapi.model.OWLOntology;
import org.semanticweb.owlapi.model.OWLSameIndividualAxiom;
import org.semanticweb.owlapi.model.OWLSubClassOfAxiom;
import org.semanticweb.owlapi.model.OWLSubObjectPropertyOfAxiom;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;
import java.util.logging.Logger;

/**
 * OWL Explanation API adapter over {@code rustdl justify --json}.
 *
 * <p>Supported entailments: named-class {@link OWLSubClassOfAxiom} (incl. the
 * {@code owl:Nothing}-superclass unsatisfiability shorthand and the
 * {@code owl:Thing SubClassOf owl:Nothing} inconsistency marker), named-class
 * {@link OWLEquivalentClassesAxiom}/{@link OWLDisjointClassesAxiom} (exactly two named
 * operands), {@link OWLClassAssertionAxiom}, non-negated {@link OWLObjectPropertyAssertionAxiom},
 * named-property {@link OWLSubObjectPropertyOfAxiom}, and (exactly two named individuals)
 * {@link OWLSameIndividualAxiom}. Anything else throws {@link UnsupportedEntailmentException}.
 * Every returned {@link Explanation} contains only axioms verified present in the source
 * ontology's imports closure ({@link RustdlOfn#verifiedAgainst}) — rustdl cannot fabricate
 * an axiom into a displayed justification. That check is fail-hard: a justification containing
 * even one axiom absent from the source is rejected in full (never partially surfaced), and —
 * mirroring km's actual behavior — the resulting {@link ExplanationException} aborts the whole
 * {@link #getExplanations} call, not just that one justification.</p>
 */
public final class RustdlExplanationGenerator implements ExplanationGenerator<OWLAxiom> {
    private static final Logger LOG = Logger.getLogger(RustdlExplanationGenerator.class.getName());

    private final OWLOntology ontology;
    private final ExplanationProgressMonitor<OWLAxiom> progressMonitor;
    private final RustdlExplainConfiguration configuration;
    private final boolean laconic;

    RustdlExplanationGenerator(
            OWLOntology ontology,
            ExplanationProgressMonitor<OWLAxiom> progressMonitor,
            RustdlExplainConfiguration configuration,
            boolean laconic) {
        this.ontology = ontology;
        this.progressMonitor = progressMonitor;
        this.configuration = configuration;
        this.laconic = laconic;
    }

    @Override
    public Set<Explanation<OWLAxiom>> getExplanations(OWLAxiom entailment) throws ExplanationException {
        Result result = generateBounded(entailment, configuration.getMaxJustifications());
        if (!result.enumerationComplete) {
            throw new ExplanationException(
                "rustdl did not exhaust all justifications within the configured cap ("
                    + configuration.getMaxJustifications() + "); use getExplanations(entailment, limit) "
                    + "or raise rustdl.explain.max.justifications/RUSTDL_EXPLAIN_MAX_JUSTIFICATIONS");
        }
        return result.explanations;
    }

    @Override
    public Set<Explanation<OWLAxiom>> getExplanations(OWLAxiom entailment, int limit)
            throws ExplanationException {
        // Validate the advertised entailment surface even when the caller asks for zero
        // results: an unsupported query must fail explicitly, never masquerade as a valid
        // query with no explanations.
        queryArguments(entailment);
        if (limit <= 0) {
            return Collections.emptySet();
        }
        return generateBounded(entailment, limit).explanations;
    }

    /** Exact entailment surface advertised to OWLAPI and Protégé callers. */
    public static boolean supportsEntailment(OWLAxiom entailment) {
        try {
            queryArguments(entailment);
            return true;
        } catch (UnsupportedEntailmentException error) {
            return false;
        }
    }

    Result generateBounded(OWLAxiom entailment, int limit) {
        if (limit <= 0) {
            throw new IllegalArgumentException("justification limit must be positive");
        }
        if (progressMonitor.isCancelled()) {
            throw new ExplanationGeneratorInterruptedException();
        }
        List<String> query = queryArguments(entailment);

        Path source = null;
        try {
            source = Files.createTempFile("rustdl-explain-", ".ofn");
            FlattenedOntology.writeOfn(ontology, source);

            RustdlJson.JustifyJson report;
            try {
                // Passing progressMonitor::isCancelled makes the wait for the rustdl subprocess
                // itself responsive to Cancel: RustdlProcess polls it at ~100ms granularity
                // (mirrors km's KMExplanationGenerator.waitFor) instead of blocking until the
                // process exits or the full configured timeout elapses.
                report = RustdlProcess.justify(
                    source, laconic, limit, configuration.getTimeoutSeconds(), query,
                    progressMonitor::isCancelled);
            } catch (RustdlProcess.CancelledException error) {
                throw new ExplanationGeneratorInterruptedException();
            } catch (IOException error) {
                throw new ExplanationException(
                    "rustdl justify failed or timed out: " + error.getMessage(), error);
            }
            validateReport(report, limit);

            if (progressMonitor.isCancelled()) {
                throw new ExplanationGeneratorInterruptedException();
            }

            if (!report.minimal) {
                LOG.warning("rustdl returned a sound but possibly non-minimal (not subset-minimal "
                    + "guaranteed) justification set for " + entailment);
            }

            Set<Explanation<OWLAxiom>> explanations = materialize(entailment, report);
            return new Result(explanations, report.enumeration_complete);
        } catch (ExplanationException error) {
            throw error;
        } catch (Exception error) {
            throw new ExplanationException("Could not generate a rustdl explanation", error);
        } finally {
            delete(source);
        }
    }

    /**
     * Package-visible test seam: applies the verify-then-wrap pipeline {@link #getExplanations}
     * uses internally to a CANNED {@link RustdlJson.JustifyJson} report, without spawning a
     * subprocess. Exercised directly by {@code RustdlExplanationGeneratorTest}.
     */
    Set<Explanation<OWLAxiom>> materialize(OWLAxiom entailment, RustdlJson.JustifyJson report) {
        if ("not-entailed".equals(report.status)) {
            return Collections.emptySet();
        }
        Set<Explanation<OWLAxiom>> explanations = new LinkedHashSet<>();
        for (RustdlJson.JustificationJson justification : report.justifications) {
            if (justification == null || justification.ofn == null) {
                throw new ExplanationException("rustdl returned a null justification document");
            }
            Set<OWLAxiom> parsed = RustdlOfn.parse(justification.ofn);
            // Fail-hard: verifiedAgainst throws ExplanationException on the FIRST axiom it finds
            // absent from the source ontology, rejecting this justification whole rather than
            // returning a partial (possibly insufficient-to-entail) subset. Nothing here catches
            // that per-justification, so it propagates out of this whole materialize() call and
            // aborts every OTHER justification in `report` too -- km's actual whole-attempt-abort
            // choice (see RustdlOfn.verifiedAgainst javadoc), not a per-justification skip.
            Set<OWLAxiom> verified = RustdlOfn.verifiedAgainst(parsed, ontology);
            if (verified.isEmpty()) {
                throw new ExplanationException(
                    "rustdl justification contained no axioms: " + justification.ofn);
            }
            Explanation<OWLAxiom> explanation = new Explanation<>(entailment, verified);
            explanations.add(explanation);
            progressMonitor.foundExplanation(this, explanation, explanations);
            if (progressMonitor.isCancelled()) {
                throw new ExplanationGeneratorInterruptedException();
            }
        }
        return explanations;
    }

    private static void validateReport(RustdlJson.JustifyJson report, int requestedLimit) {
        if (report == null) {
            throw new ExplanationException("rustdl returned no justify result");
        }
        if (!"entailed".equals(report.status) && !"not-entailed".equals(report.status)) {
            throw new ExplanationException("Unknown rustdl justify status: " + report.status);
        }
        if ("entailed".equals(report.status)
                && (report.justifications == null || report.justifications.isEmpty())) {
            throw new ExplanationException(
                "rustdl returned an entailed verdict without any justifications");
        }
        if ("not-entailed".equals(report.status)
                && report.justifications != null && !report.justifications.isEmpty()) {
            throw new ExplanationException(
                "rustdl returned a not-entailed verdict together with justifications");
        }
        if (report.justifications != null && report.justifications.size() > requestedLimit) {
            throw new ExplanationException(
                "rustdl returned more justifications than the requested bound");
        }
    }

    /**
     * Maps a supported entailment axiom to {@code rustdl justify} query arguments (full IRIs,
     * per the {@code owl_dl_reasoner::justify::parse_query} grammar). Throws
     * {@link UnsupportedEntailmentException} for anything outside the advertised surface.
     */
    static List<String> queryArguments(OWLAxiom entailment) {
        if (entailment instanceof OWLSubClassOfAxiom) {
            return subClassOfQuery((OWLSubClassOfAxiom) entailment);
        }
        if (entailment instanceof OWLEquivalentClassesAxiom) {
            List<OWLClass> pair = namedPair(
                ((OWLEquivalentClassesAxiom) entailment).getClassExpressions(),
                RustdlExplanationGenerator::namedClassOrNull);
            return args("equivalent", pair.get(0).getIRI().toString(), pair.get(1).getIRI().toString());
        }
        if (entailment instanceof OWLDisjointClassesAxiom) {
            List<OWLClass> pair = namedPair(
                ((OWLDisjointClassesAxiom) entailment).getClassExpressions(),
                RustdlExplanationGenerator::namedClassOrNull);
            return args("disjoint", pair.get(0).getIRI().toString(), pair.get(1).getIRI().toString());
        }
        if (entailment instanceof OWLClassAssertionAxiom) {
            OWLClassAssertionAxiom classAssertion = (OWLClassAssertionAxiom) entailment;
            OWLNamedIndividual individual = namedIndividualOrNull(classAssertion.getIndividual());
            OWLClass type = namedClassOrNull(classAssertion.getClassExpression());
            if (individual == null || type == null) {
                throw unsupported("ClassAssertion requires a named individual and named class");
            }
            return args("instance", individual.getIRI().toString(), type.getIRI().toString());
        }
        if (entailment instanceof OWLObjectPropertyAssertionAxiom) {
            OWLObjectPropertyAssertionAxiom propertyAssertion = (OWLObjectPropertyAssertionAxiom) entailment;
            OWLNamedIndividual subject = namedIndividualOrNull(propertyAssertion.getSubject());
            OWLNamedIndividual object = namedIndividualOrNull(propertyAssertion.getObject());
            OWLObjectPropertyExpression propertyExpression = propertyAssertion.getProperty();
            if (subject == null || object == null || propertyExpression.isAnonymous()) {
                throw unsupported(
                    "ObjectPropertyAssertion requires named subject/object and a named property");
            }
            return args("property",
                subject.getIRI().toString(),
                propertyExpression.asOWLObjectProperty().getIRI().toString(),
                object.getIRI().toString());
        }
        if (entailment instanceof OWLSubObjectPropertyOfAxiom) {
            OWLSubObjectPropertyOfAxiom subProperty = (OWLSubObjectPropertyOfAxiom) entailment;
            OWLObjectPropertyExpression sub = subProperty.getSubProperty();
            OWLObjectPropertyExpression sup = subProperty.getSuperProperty();
            if (sub.isAnonymous() || sup.isAnonymous()) {
                throw unsupported("SubObjectPropertyOf requires named sub- and super-properties");
            }
            return args("subproperty",
                sub.asOWLObjectProperty().getIRI().toString(),
                sup.asOWLObjectProperty().getIRI().toString());
        }
        if (entailment instanceof OWLSameIndividualAxiom) {
            List<OWLNamedIndividual> pair = namedPair(
                ((OWLSameIndividualAxiom) entailment).getIndividuals(),
                RustdlExplanationGenerator::namedIndividualOrNull);
            return args("same", pair.get(0).getIRI().toString(), pair.get(1).getIRI().toString());
        }
        throw unsupported("unsupported entailment type: " + entailment.getClass().getSimpleName());
    }

    private static List<String> subClassOfQuery(OWLSubClassOfAxiom subClassOf) {
        OWLClassExpression subExpression = subClassOf.getSubClass();
        OWLClassExpression superExpression = subClassOf.getSuperClass();
        if (subExpression.isAnonymous() || superExpression.isAnonymous()) {
            throw unsupported("SubClassOf requires named subclass and superclass expressions");
        }
        OWLClass subClass = subExpression.asOWLClass();
        OWLClass superClass = superExpression.asOWLClass();
        if (subClass.isOWLThing() && superClass.isOWLNothing()) {
            return Collections.singletonList("inconsistent");
        }
        if (superClass.isOWLNothing()) {
            return args("unsat", subClass.getIRI().toString());
        }
        return args("subclass", subClass.getIRI().toString(), superClass.getIRI().toString());
    }

    /**
     * Requires {@code operands} to reduce (via {@code namer}) to exactly two DISTINCT named
     * entities, sorted by IRI so the emitted query is deterministic regardless of the source
     * {@code Set}'s iteration order (equivalence/disjointness/sameness are symmetric, so operand
     * order doesn't change what is being asked). Anything else (an anonymous operand, or more
     * than two operands — an n-ary axiom rustdl's binary query grammar can't express as one
     * query) is out of scope: {@link UnsupportedEntailmentException}.
     */
    private static <T, N extends org.semanticweb.owlapi.model.OWLEntity> List<N> namedPair(
            Set<T> operands, java.util.function.Function<T, N> namer) {
        List<N> named = new ArrayList<>();
        for (T operand : operands) {
            N entity = namer.apply(operand);
            if (entity == null) {
                throw unsupported("requires named operands only");
            }
            named.add(entity);
        }
        if (named.size() != 2) {
            throw unsupported(
                "requires exactly two named operands (got " + named.size() + ")");
        }
        named.sort((a, b) -> a.getIRI().toString().compareTo(b.getIRI().toString()));
        return named;
    }

    private static OWLClass namedClassOrNull(OWLClassExpression expression) {
        return expression.isAnonymous() ? null : expression.asOWLClass();
    }

    private static OWLNamedIndividual namedIndividualOrNull(OWLIndividual individual) {
        return individual.isAnonymous() ? null : individual.asOWLNamedIndividual();
    }

    private static List<String> args(String... parts) {
        List<String> list = new ArrayList<>();
        Collections.addAll(list, parts);
        return list;
    }

    private static UnsupportedEntailmentException unsupported(String message) {
        return new UnsupportedEntailmentException("rustdl explanations: " + message);
    }

    private static void delete(Path path) {
        if (path != null) {
            try {
                Files.deleteIfExists(path);
            } catch (Exception ignored) {
                // Temporary-file cleanup must not mask the explanation result.
            }
        }
    }

    /** Bundles a bounded run's explanations with whether enumeration was complete. */
    static final class Result {
        final Set<Explanation<OWLAxiom>> explanations;
        final boolean enumerationComplete;

        Result(Set<Explanation<OWLAxiom>> explanations, boolean enumerationComplete) {
            this.explanations = Collections.unmodifiableSet(new LinkedHashSet<>(explanations));
            this.enumerationComplete = enumerationComplete;
        }
    }
}
