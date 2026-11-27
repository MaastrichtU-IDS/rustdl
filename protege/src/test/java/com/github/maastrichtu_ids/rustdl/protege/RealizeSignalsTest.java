package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.*;
import org.semanticweb.owlapi.reasoner.InferenceType;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.nio.file.attribute.PosixFilePermissions;
import java.util.*;
import java.util.logging.Handler;
import java.util.logging.LogRecord;
import java.util.logging.Logger;
import static org.junit.Assert.*;
import static org.junit.Assume.assumeFalse;

/**
 * #616: realize's remaining signals, classify's {@code wedge_abox_blind} and
 * {@code no_verdict_pairs}, and rustdl's exit 3 after a {@code NoVerdict} JSON.
 *
 * <p>Every fixture below is real CLI output of rustdl at main 09e0d5af, run with every other
 * {@code RUSTDL_*} variable unset. Each fixture's inputs and settings, as {@code rustdl
 * <command> --json <input>}:
 * <ul>
 * <li>{@code realize_direct_unverified.json}: realize on #583's 10-pigeon, 9-hole
 *     pigeonhole ({@code realize_direct_types_cause_cli.rs}), with
 *     {@code RUSTDL_REALIZE_PAIR_TIMEOUT_MS=1}. Exit 0; {@code incomplete},
 *     {@code witness_prune_may_drop} (branched) and {@code direct_types_unverified}.</li>
 * <li>{@code realize_witness_branched.json}: realize on
 *     {@code crates/owl-dl-cli/tests/fixtures/witness_prune/union.ofn}, with
 *     {@code RUSTDL_SEMANTIC_BRANCHING=0 RUSTDL_SAT_LOOKAHEAD=0}. Exit 0.</li>
 * <li>{@code realize_depth_capped.json}: realize on #521's 280-union ∀-fixture
 *     ({@code instance_depth_cut_cli.rs} {@code Fixture::forall}), with
 *     {@code RUSTDL_REALIZE_PAIR_TIMEOUT_MS=60000}. Exit 0.</li>
 * <li>{@code realize_no_verdict.json} / {@code realize_no_verdict_direct.json}: realize on
 *     {@code noverdict_cli.rs}'s {@code el_abox} fixtures (a stalled instance probe; a stalled
 *     #388 check), with {@code RUSTDL_HYPERTABLEAU=0 RUSTDL_WEDGE_CONSISTENCY=0
 *     RUSTDL_TABLEAU_SATURATE_ITERS=1 RUSTDL_REALIZE_SATURATION=0
 *     RUSTDL_REALIZE_PAIR_TIMEOUT_MS=20000}. Exit 3.</li>
 * <li>{@code classify_no_verdict.json}: {@code classify --pair-timeout-ms 20000} on
 *     {@code noverdict_cli.rs}'s {@code SRC}, with the first three of those settings. Exit 3;
 *     {@code no_verdict_pairs: 3}.</li>
 * <li>{@code realize_dropped.json}: realize on a HasKey ontology with one individual. Exit 0.</li>
 * <li>{@code classify_abox_blind.json}: classify on
 *     {@code crates/owl-dl-cli/tests/fixtures/json/wedge_abox_blind_p1.ofn}. Exit 0.</li>
 * </ul>
 */
public class RealizeSignalsTest {
    private static final Map<String, String> NO_ENV = Collections.emptyMap();

    private static String fixture(String name) throws Exception {
        return new String(Files.readAllBytes(
            Paths.get(RealizeSignalsTest.class.getResource("/json/" + name).toURI())),
            StandardCharsets.UTF_8);
    }
    private static RustdlJson.RealizeJson realize(String name) throws Exception {
        return RustdlProcess.parseRealize(fixture(name));
    }
    private static List<String> warnings(String name) throws Exception {
        return RustdlReasoner.realizeWarnings(realize(name), NO_ENV);
    }
    private static void assertOne(List<String> w, String phrase) {
        assertEquals(w.toString(), 1, w.size());
        assertTrue(w.get(0), w.get(0).contains(phrase));
    }

    // ---- realizeWarnings ------------------------------------------------------------------

    @Test public void aCleanRealizeHasNoWarnings() throws Exception {
        assertTrue(warnings("realize.json").isEmpty());
    }

    @Test public void aWitnessPruneThatMayDropNamesItsReasons() throws Exception {
        List<String> w = warnings("realize_witness_branched.json");
        assertOne(w, "pseudo-model prune answered \"not a type\"");
        assertTrue(w.get(0), w.get(0).contains("type (branched), so"));
        assertTrue(w.get(0), w.get(0).contains("RUSTDL_PSEUDO_MODEL=0"));
    }

    @Test public void aWitnessPruneWithoutReasonsHasNoEmptyParentheses() throws Exception {
        RustdlJson.RealizeJson r = realize("realize_witness_branched.json");
        r.witness_prune_reasons = null;  // older output
        List<String> w = RustdlReasoner.realizeWarnings(r, NO_ENV);
        assertOne(w, "known reason to lack an entailed type, so");
        r.witness_prune_reasons = new ArrayList<>();
        assertOne(RustdlReasoner.realizeWarnings(r, NO_ENV), "known reason to lack an entailed type, so");
    }

    @Test public void aDepthCutNamesTheLimitNotTheBudget() throws Exception {
        List<String> w = warnings("realize_depth_capped.json");
        assertOne(w, "hit the tableau's decision-depth limit");
        assertTrue(w.get(0), w.get(0).contains("RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0 runs the deadline-free probe"));
        assertTrue(w.get(0), w.get(0).contains("deadline off the classification step"));
        assertFalse(w.get(0), w.get(0).contains("INCOMPLETE instance-types"));
    }

    @Test public void theDirectTypesPigeonholeWarnsCutWitnessAndDirectTypes() throws Exception {
        List<String> w = warnings("realize_direct_unverified.json");
        assertEquals(w.toString(), 3, w.size());
        assertTrue(w.get(0), w.get(0).startsWith("rustdl reports an INCOMPLETE instance-types result"));
        assertTrue(w.get(0), w.get(0).contains("RUSTDL_MAX_NODES"));
        assertTrue(w.get(1), w.get(1).contains("pseudo-model prune"));
        assertTrue(w.get(2), w.get(2).contains("two direct types whose subsumption was left undecided"));
        assertTrue(w.get(2), w.get(2).contains("For a deadline or node-cap cut"));
        assertFalse(w.get(2), w.get(2).contains("decision-depth limit"));
    }

    @Test public void aDepthCappedDirectTypesCheckNamesBothRemedies() throws Exception {
        // No small trigger for direct_types_depth_capped exists
        // (realize_direct_types_cause_cli.rs), so the real fixture's field is flipped.
        RustdlJson.RealizeJson r = realize("realize_direct_unverified.json");
        r.direct_types_depth_capped = true;
        String d = RustdlReasoner.realizeWarnings(r, NO_ENV).get(2);
        assertTrue(d, d.contains("At least one check hit the tableau's decision-depth limit"));
        assertTrue(d, d.contains("For a deadline or node-cap cut"));
    }

    @Test public void theDirectTypesRemedyFollowsTheEnvironment() throws Exception {
        RustdlJson.RealizeJson r = realize("realize_direct_unverified.json");
        String off = RustdlReasoner.realizeWarnings(r,
            Collections.singletonMap("RUSTDL_REALIZE_VERIFY_DIRECT", "0")).get(2);
        assertTrue(off, off.contains("The check is off (RUSTDL_REALIZE_VERIFY_DIRECT=0)"));
        assertFalse(off, off.contains("For a deadline"));
        String free = RustdlReasoner.realizeWarnings(r,
            Collections.singletonMap("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0")).get(2);
        assertTrue(free, free.endsWith("every listed type is a genuine type."));
    }

    @Test public void aNoVerdictReplacesTheCutWarning() throws Exception {
        List<String> w = warnings("realize_no_verdict.json");
        assertOne(w, "likely a reasoner bug");
        assertTrue(w.get(0), w.get(0).startsWith(RustdlReasoner.noVerdictNote("realize")));
        assertFalse(w.get(0), w.get(0).contains("RUSTDL_MAX_NODES"));
    }

    @Test public void aNoVerdictDirectTypesCheckGivesNoRemedy() throws Exception {
        List<String> w = warnings("realize_no_verdict_direct.json");
        assertEquals(w.toString(), 2, w.size());
        assertTrue(w.get(0), w.get(0).contains("likely a reasoner bug"));
        assertTrue(w.get(1), w.get(1).endsWith("every listed type is a genuine type."));
    }

    @Test public void droppedAxiomsAreWarnedOnRealize() throws Exception {
        assertOne(warnings("realize_dropped.json"), "left out 1 axiom(s) (HasKey: unsupported axiom (HasKey) ×1)");
    }

    @Test public void theUniversalRoleIsStillWarnedOnRealize() throws Exception {
        assertOne(warnings("realize_universal_role.json"), "rustdl realize: owl:topObjectProperty");
    }

    // ---- classifyWarnings -----------------------------------------------------------------

    @Test public void wedgeAboxBlindIsWarnedOn() throws Exception {
        List<String> w = RustdlReasoner.classifyWarnings(
            RustdlProcess.parseClassify(fixture("classify_abox_blind.json")));
        assertOne(w, "touch a nominal while an ABox fact did not apply");
        assertTrue(w.get(0), w.get(0).contains("RUSTDL_CLASSIFY_WEDGE_ABOX_DEMOTE=1 in Protégé's environment"));
    }

    @Test public void classifyNoVerdictPairsAreWarnedOn() throws Exception {
        List<String> w = RustdlReasoner.classifyWarnings(
            RustdlProcess.parseClassify(fixture("classify_no_verdict.json")));
        String nv = null;
        for (String s : w) if (s.contains("likely a reasoner bug")) nv = s;
        assertNotNull(w.toString(), nv);
        assertTrue(nv, nv.contains(" 3 class pair(s)"));
    }

    @Test public void aCleanClassifyHasNeitherNewWarning() throws Exception {
        String w = RustdlReasoner.classifyWarnings(
            RustdlProcess.parseClassify(fixture("classify.json"))).toString();
        assertFalse(w, w.contains("touch a nominal"));
        assertFalse(w, w.contains("NoVerdict"));
    }

    // ---- exit 3 ---------------------------------------------------------------------------

    private static boolean windows() {
        return System.getProperty("os.name", "").toLowerCase(Locale.ROOT).contains("win");
    }

    /**
     * A fake rustdl: for {@code classify} it prints {@code classifyOut} and exits
     * {@code classifyCode}; for {@code realize}, {@code realizeOut} and {@code realizeCode}.
     */
    private static Path fakeRustdl(String classifyOut, int classifyCode, String realizeOut,
            int realizeCode) throws IOException {
        Path dir = Files.createTempDirectory("rustdl-fake-");
        Path c = dir.resolve("classify.out");
        Path r = dir.resolve("realize.out");
        Files.write(c, classifyOut.getBytes(StandardCharsets.UTF_8));
        Files.write(r, realizeOut.getBytes(StandardCharsets.UTF_8));
        Path bin = dir.resolve("rustdl");
        String script = "#!/bin/sh\n"
            + "case \"$1\" in\n"
            + "  classify) cat '" + c + "'; echo 'stderr of classify' >&2; exit " + classifyCode + ";;\n"
            + "  realize) cat '" + r + "'; echo 'stderr of realize' >&2; exit " + realizeCode + ";;\n"
            + "esac\nexit 64\n";
        Files.write(bin, script.getBytes(StandardCharsets.UTF_8));
        Files.setPosixFilePermissions(bin, PosixFilePermissions.fromString("rwx------"));
        return bin;
    }

    private interface Body { void run() throws Exception; }

    private static void withBin(Path bin, Body body) throws Exception {
        String prev = System.getProperty("rustdl.bin");
        System.setProperty("rustdl.bin", bin.toString());
        try {
            body.run();
        } finally {
            if (prev == null) System.clearProperty("rustdl.bin"); else System.setProperty("rustdl.bin", prev);
        }
    }

    private static void assertExitFailure(Body body, String label, int code) throws Exception {
        try {
            body.run();
            fail("expected rustdl " + label + " exit " + code + " to fail");
        } catch (IOException e) {
            assertTrue(e.getMessage(), e.getMessage().startsWith("rustdl " + label + " exited " + code + ":"));
            assertTrue(e.getMessage(), e.getMessage().contains("stderr of " + label));
        }
    }

    @Test public void realizeAcceptsExit3AfterANoVerdictJson() throws Exception {
        assumeFalse(windows());
        Path ofn = Paths.get("unused.ofn");
        withBin(fakeRustdl("", 0, fixture("realize_no_verdict.json"), 3), () -> {
            RustdlJson.RealizeJson r = RustdlProcess.realize(ofn, 30);
            assertTrue(r.no_verdict);
            assertNotNull(r.individuals);
        });
    }

    @Test public void realizeRejectsExit3WithoutANoVerdictJson() throws Exception {
        assumeFalse(windows());
        Path ofn = Paths.get("unused.ofn");
        // A JSON whose no_verdict is false, the bare `unknown` of main's NoVerdict net,
        // and the no_verdict JSON under another non-zero exit: each fails as before.
        withBin(fakeRustdl("", 0, fixture("realize.json"), 3),
            () -> assertExitFailure(() -> RustdlProcess.realize(ofn, 30), "realize", 3));
        withBin(fakeRustdl("", 0, "unknown\n", 3),
            () -> assertExitFailure(() -> RustdlProcess.realize(ofn, 30), "realize", 3));
        withBin(fakeRustdl("", 0, fixture("realize_no_verdict.json"), 1),
            () -> assertExitFailure(() -> RustdlProcess.realize(ofn, 30), "realize", 1));
    }

    @Test public void classifyAcceptsExit3AfterANoVerdictJsonOnly() throws Exception {
        assumeFalse(windows());
        Path ofn = Paths.get("unused.ofn");
        withBin(fakeRustdl(fixture("classify_no_verdict.json"), 3, "", 0), () -> {
            RustdlJson.ClassifyJson c = RustdlProcess.classify(ofn, 30, 0);
            assertEquals(3, c.no_verdict_pairs);
        });
        withBin(fakeRustdl(fixture("classify.json"), 3, "", 0),
            () -> assertExitFailure(() -> RustdlProcess.classify(ofn, 30, 0), "classify", 3));
        withBin(fakeRustdl("unknown\n", 3, "", 0),
            () -> assertExitFailure(() -> RustdlProcess.classify(ofn, 30, 0), "classify", 3));
    }

    // ---- precomputeInferences -------------------------------------------------------------

    @Test public void precomputeLogsRealizesWarningsAndKeepsANoVerdictResult() throws Exception {
        assumeFalse(windows());
        OWLOntologyManager m = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = m.getOWLDataFactory();
        OWLOntology o = m.createOntology(IRI.create("http://ex/el-abox"));
        OWLNamedIndividual b = df.getOWLNamedIndividual(IRI.create("http://ex/#b"));
        m.addAxiom(o, df.getOWLClassAssertionAxiom(df.getOWLClass(IRI.create("http://ex/#C")), b));

        List<String> logged = new ArrayList<>();
        Handler h = new Handler() {
            @Override public void publish(LogRecord r) { logged.add(r.getMessage()); }
            @Override public void flush() {}
            @Override public void close() {}
        };
        Logger log = Logger.getLogger(RustdlReasoner.class.getName());
        log.addHandler(h);
        try {
            withBin(fakeRustdl(fixture("classify.json"), 0, fixture("realize_no_verdict.json"), 3), () -> {
                RustdlReasoner reasoner =
                    (RustdlReasoner) new RustdlReasonerFactory().createReasoner(o);
                reasoner.precomputeInferences(InferenceType.CLASS_ASSERTIONS);
                assertTrue(reasoner.isPrecomputed(InferenceType.CLASS_ASSERTIONS));
            });
        } finally {
            log.removeHandler(h);
        }
        boolean seen = false;
        for (String s : logged) seen |= s.startsWith(RustdlReasoner.noVerdictNote("realize"));
        assertTrue(logged.toString(), seen);
    }
}
