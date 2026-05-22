import java.io.File;
import java.util.Arrays;
import java.util.List;

import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.IRI;
import org.semanticweb.owlapi.model.OWLOntology;
import org.semanticweb.owlapi.model.OWLOntologyManager;
import org.semanticweb.owlapi.reasoner.InferenceType;
import org.semanticweb.owlapi.reasoner.OWLReasoner;
import org.semanticweb.owlapi.reasoner.OWLReasonerFactory;

/**
 * Multi-ontology classify harness so JVM + OWLAPI startup is paid
 * once across all measurements. Loads each .ofn file fresh in the
 * same JVM, runs each reasoner on it, and prints per-(reasoner,
 * file) wall-clock time (in milliseconds) for just the
 * `precomputeInferences(CLASS_HIERARCHY)` step — that's the call
 * that does the actual classification, isolated from parsing and
 * I/O.
 */
public final class Bench {
    public static void main(String[] args) throws Exception {
        if (args.length < 1) {
            System.err.println("usage: Bench file1.ofn [file2.ofn ...]");
            System.exit(2);
        }
        List<File> files = Arrays.stream(args).map(File::new).toList();

        OWLReasonerFactory hermit = new org.semanticweb.HermiT.ReasonerFactory();
        OWLReasonerFactory elk = new org.semanticweb.elk.owlapi.ElkReasonerFactory();

        // One warmup pass to warm the JVM JIT.
        for (File f : files) {
            classifyOnce(f, hermit);
            classifyOnce(f, elk);
        }

        // Five measured iterations per (reasoner, file).
        System.out.printf("%-10s  %-32s  %-9s%n", "reasoner", "file", "ms_mean");
        for (OWLReasonerFactory factory : List.of(hermit, elk)) {
            String name = factory.getReasonerName();
            for (File f : files) {
                long total = 0;
                long min = Long.MAX_VALUE;
                long max = 0;
                int iters = 5;
                for (int i = 0; i < iters; i++) {
                    long ms = classifyOnce(f, factory);
                    total += ms;
                    min = Math.min(min, ms);
                    max = Math.max(max, ms);
                }
                System.out.printf(
                    "%-10s  %-32s  mean=%-6d  min=%-6d  max=%-6d%n",
                    name, f.getName(), total / iters, min, max
                );
            }
        }
    }

    private static long classifyOnce(File file, OWLReasonerFactory factory) throws Exception {
        OWLOntologyManager mgr = OWLManager.createOWLOntologyManager();
        OWLOntology onto = mgr.loadOntologyFromOntologyDocument(file);
        OWLReasoner reasoner = factory.createReasoner(onto);
        long start = System.nanoTime();
        reasoner.precomputeInferences(InferenceType.CLASS_HIERARCHY);
        long elapsed = (System.nanoTime() - start) / 1_000_000;
        reasoner.dispose();
        mgr.removeOntology(onto);
        return elapsed;
    }
}
