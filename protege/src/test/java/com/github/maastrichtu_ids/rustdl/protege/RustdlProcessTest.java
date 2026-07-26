package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import java.io.IOException;
import java.nio.file.*;
import java.util.Arrays;
import java.util.Locale;
import static org.junit.Assert.*;
import static org.junit.Assume.assumeFalse;

public class RustdlProcessTest {
    private String fixture(String name) throws Exception {
        return new String(Files.readAllBytes(
            Paths.get(getClass().getResource("/json/" + name).toURI())));
    }

    @Test public void parsesClassify() throws Exception {
        RustdlJson.ClassifyJson c = RustdlProcess.parseClassify(fixture("classify.json"));
        assertEquals(1, c.schema_version);
        assertTrue(c.consistent);
        assertFalse(c.incomplete);
        assertTrue(c.unsatisfiable.isEmpty());
        assertEquals("http://ex/#A", c.equivalent_groups.get(0).get(0));
        assertEquals("http://ex/#C", c.direct_subsumptions.get(0).get(1));
    }

    @Test public void parsesUnsatAndIncomplete() throws Exception {
        RustdlJson.ClassifyJson c = RustdlProcess.parseClassify(fixture("classify_unsat.json"));
        assertTrue(c.incomplete);
        assertEquals("http://ex/#Bad", c.unsatisfiable.get(0));
    }

    @Test public void parsesConsistent() throws Exception {
        RustdlJson.ConsistentJson c = RustdlProcess.parseConsistent(fixture("inconsistent.json"));
        assertFalse(c.consistent);
    }

    @Test public void parsesRealize() throws Exception {
        RustdlJson.RealizeJson r = RustdlProcess.parseRealize(fixture("realize.json"));
        assertEquals("http://ex/#i", r.individuals.get(0).iri);
        assertEquals(2, r.individuals.get(0).types.size());
        assertEquals("http://ex/#A", r.individuals.get(0).direct_types.get(0));
    }

    @Test(expected = IllegalStateException.class)
    public void rejectsWrongSchemaVersion() {
        RustdlProcess.parseClassify("{ \"schema_version\": 2, \"consistent\": true }");
    }

    @Test public void parsesDisjoint() throws Exception {
        RustdlJson.DisjointJson d = RustdlProcess.parseDisjoint(fixture("disjoint.json"));
        assertEquals(1, d.schema_version);
        assertFalse(d.incomplete);
        assertEquals("http://ex/#A", d.disjoint_classes.get(0).get(0));
        assertEquals("http://ex/#q", d.disjoint_object_properties.get(0).get(1));
        assertTrue(d.disjoint_data_properties.isEmpty());
    }

    @Test public void parsesPropertyHierarchy() throws Exception {
        RustdlJson.PropHierJson p = RustdlProcess.parsePropHier(fixture("prophier.json"));
        assertEquals(1, p.schema_version);
        assertFalse(p.incomplete);
        assertEquals("http://ex/#p2", p.object_properties.equivalent_groups.get(0).get(1));
        assertEquals("http://ex/#r", p.object_properties.direct_subsumptions.get(0).get(1));
        assertTrue(p.data_properties.equivalent_groups.isEmpty());
        assertEquals("http://ex/#e", p.data_properties.direct_subsumptions.get(0).get(1));
    }

    @Test public void parsesIndividuals() throws Exception {
        RustdlJson.IndividualsJson i = RustdlProcess.parseIndividuals(fixture("individuals.json"));
        assertEquals(1, i.schema_version);
        assertTrue(i.incomplete);
        assertEquals("http://ex/#b", i.same_groups.get(0).get(1));
        assertEquals("http://ex/#c", i.different_pairs.get(0).get(1));
    }

    @Test public void parsesPropertyValues() throws Exception {
        RustdlJson.PropertyValuesJson v = RustdlProcess.parsePropertyValues(fixture("propvalues.json"));
        assertEquals(1, v.schema_version);
        assertFalse(v.incomplete);
        assertEquals("http://ex/#b", v.object_property_values.get(0).get(2));
        assertEquals("http://www.w3.org/2001/XMLSchema#integer", v.data_property_values.get(0).get(3));
    }

    @Test(expected = IllegalStateException.class)
    public void rejectsWrongSchemaVersionDisjoint() {
        RustdlProcess.parseDisjoint("{\"schema_version\":2}");
    }

    @Test public void parsesJustify() throws Exception {
        RustdlJson.JustifyJson j = RustdlProcess.parseJustify(fixture("justify.json"));
        assertEquals(1, j.schema_version);
        assertEquals("entailed", j.status);
        assertTrue(j.enumeration_complete);
        assertTrue(j.minimal);
        assertFalse(j.laconic);
        assertEquals(1, j.justifications.size());
        assertTrue(j.justifications.get(0).ofn.contains("SubClassOf(:A :B)"));
    }

    @Test(expected = IllegalStateException.class)
    public void rejectsWrongSchemaVersionJustify() {
        RustdlProcess.parseJustify("{\"schema_version\":2}");
    }

    @Test public void parsesProveWithStepProof() throws Exception {
        RustdlJson.ProveJson p = RustdlProcess.parseProve(fixture("prove.json"));
        assertEquals(1, p.schema_version);
        assertTrue(p.entailed);
        assertTrue(p.has_proof);
        assertNull(p.justification_fallback);
        assertNotNull(p.proof);
        assertEquals("SubsumerTransitivity(fwd)", p.proof.rule);
        assertTrue(p.proof.conclusion.contains("SubClassOf(:A :C)"));
        assertTrue(p.proof.axioms.isEmpty());
        assertEquals(2, p.proof.premises.size());
        assertEquals("ToldSubsumer", p.proof.premises.get(0).rule);
        assertTrue(p.proof.premises.get(0).conclusion.contains("SubClassOf(:A :B)"));
        assertEquals(1, p.proof.premises.get(0).axioms.size());
        assertTrue(p.proof.premises.get(1).conclusion.contains("SubClassOf(:B :C)"));
    }

    @Test public void parsesProveFallback() throws Exception {
        RustdlJson.ProveJson p = RustdlProcess.parseProve(fixture("prove_fallback.json"));
        assertTrue(p.entailed);
        assertFalse(p.has_proof);
        assertNull(p.proof);
        assertTrue(p.justification_fallback.contains("SubClassOf(:X :Y)"));
        assertTrue(p.justification_fallback.contains("SubClassOf(:Y :Z)"));
    }

    @Test(expected = IllegalStateException.class)
    public void rejectsWrongSchemaVersionProve() {
        RustdlProcess.parseProve("{\"schema_version\":2}");
    }

    @Test
    public void proveCommandShape() throws Exception {
        String prev = System.getProperty("rustdl.bin");
        Path fakeBin = Paths.get(getClass().getResource("/json/classify.json").toURI());
        System.setProperty("rustdl.bin", fakeBin.toString());
        try {
            java.util.List<String> cmd = RustdlProcess.buildProveCommand(
                Paths.get("ont.ofn"), "http://ex/#A", "http://ex/#C");
            assertEquals(Arrays.asList(
                fakeBin.toString(), "prove", "--json", "--", "ont.ofn",
                "http://ex/#A", "http://ex/#C"), cmd);
        } finally {
            if (prev == null) System.clearProperty("rustdl.bin"); else System.setProperty("rustdl.bin", prev);
        }
    }

    @Test
    public void justifyCommandShapeNonLaconic() throws Exception {
        String prev = System.getProperty("rustdl.bin");
        Path fakeBin = Paths.get(getClass().getResource("/json/classify.json").toURI());
        System.setProperty("rustdl.bin", fakeBin.toString());
        try {
            java.util.List<String> cmd = RustdlProcess.buildJustifyCommand(
                Paths.get("ont.ofn"), false, 8,
                Arrays.asList("subclass", "http://ex/#A", "http://ex/#C"));
            assertEquals(Arrays.asList(
                fakeBin.toString(), "justify", "--all", "--json", "--max", "8", "--",
                "ont.ofn", "subclass", "http://ex/#A", "http://ex/#C"), cmd);
        } finally {
            if (prev == null) System.clearProperty("rustdl.bin"); else System.setProperty("rustdl.bin", prev);
        }
    }

    @Test
    public void justifyCommandShapeLaconicIncludesFlag() throws Exception {
        String prev = System.getProperty("rustdl.bin");
        Path fakeBin = Paths.get(getClass().getResource("/json/classify.json").toURI());
        System.setProperty("rustdl.bin", fakeBin.toString());
        try {
            java.util.List<String> cmd = RustdlProcess.buildJustifyCommand(
                Paths.get("ont.ofn"), true, 3, Arrays.asList("inconsistent"));
            assertEquals(Arrays.asList(
                fakeBin.toString(), "justify", "--all", "--json", "--laconic", "--max", "3", "--",
                "ont.ofn", "inconsistent"), cmd);
        } finally {
            if (prev == null) System.clearProperty("rustdl.bin"); else System.setProperty("rustdl.bin", prev);
        }
    }

    private static boolean isWindows() {
        return System.getProperty("os.name", "").toLowerCase(Locale.ROOT).startsWith("windows");
    }

    @Test
    public void timeoutKillsHangingProcess() throws Exception {
        assumeFalse(isWindows());
        long start = System.currentTimeMillis();
        try {
            RustdlProcess.runCommand(Arrays.asList("sh", "-c", "sleep 30"), "test", 1);
            fail("expected IOException for a timed-out process");
        } catch (IOException e) {
            assertTrue("message should mention timeout: " + e.getMessage(),
                e.getMessage().contains("timed out"));
        }
        long elapsed = System.currentTimeMillis() - start;
        assertTrue("expected the timeout to fire well under 30s, took " + elapsed + "ms",
            elapsed < 10_000);
    }

    @Test
    public void pollingRunCommandAbortsPromptlyWhenCancelled() throws Exception {
        assumeFalse(isWindows());
        long start = System.currentTimeMillis();
        java.util.concurrent.atomic.AtomicInteger polls = new java.util.concurrent.atomic.AtomicInteger();
        try {
            // A predicate that reports cancelled on its very first poll: the polling loop ticks
            // every ~100ms, so this must destroy the still-sleeping process and abort in well
            // under the (deliberately generous, real-timeout-would-mask-the-bug) 30s command
            // timeout and the 30s sleep itself.
            RustdlProcess.runCommand(
                Arrays.asList("sh", "-c", "sleep 30"), "test", 30,
                () -> polls.incrementAndGet() >= 1);
            fail("expected CancelledException when the predicate reports cancelled");
        } catch (RustdlProcess.CancelledException e) {
            assertTrue("message should mention cancellation: " + e.getMessage(),
                e.getMessage().contains("cancelled"));
        }
        long elapsed = System.currentTimeMillis() - start;
        assertTrue("expected cancellation to fire within ~100ms polling granularity, took "
                + elapsed + "ms",
            elapsed < 5_000);
        assertTrue("expected the predicate to actually be polled", polls.get() >= 1);
    }

    @Test
    public void pollingRunCommandBehavesLikeNonPollingVariantWhenNeverCancelled() throws Exception {
        // The plain 3-arg runCommand delegates to the polling 4-arg variant with `() -> false`;
        // confirm that delegation preserves identical success-path behavior for the reasoner
        // path's existing callers (no cancellation predicate involved at all).
        String out = RustdlProcess.runCommand(
            Arrays.asList("sh", "-c", "printf DONE"), "test", 5, () -> false);
        assertEquals("DONE", out);
    }

    @Test
    public void pollingRunCommandStillTimesOutWhenNeverCancelled() throws Exception {
        assumeFalse(isWindows());
        long start = System.currentTimeMillis();
        try {
            RustdlProcess.runCommand(
                Arrays.asList("sh", "-c", "sleep 30"), "test", 1, () -> false);
            fail("expected plain (non-Cancelled) IOException for a timed-out process");
        } catch (RustdlProcess.CancelledException e) {
            fail("timeout must not be reported as CancelledException: " + e.getMessage());
        } catch (IOException e) {
            assertTrue("message should mention timeout: " + e.getMessage(),
                e.getMessage().contains("timed out"));
        }
        long elapsed = System.currentTimeMillis() - start;
        assertTrue("expected the timeout to fire well under 30s, took " + elapsed + "ms",
            elapsed < 10_000);
    }

    @Test
    public void noDeadlockOnLargeStderr() throws Exception {
        assumeFalse(isWindows());
        String out = RustdlProcess.runCommand(
            Arrays.asList("sh", "-c", "yes X | head -c 200000 1>&2; printf DONE"),
            "test", 30);
        assertEquals("DONE", out);
    }

    /**
     * buildClassifyCommand resolves the rustdl binary path via RustdlBinary.resolve(),
     * which (absent a bundled binary in this dev/test jar) requires -Drustdl.bin to
     * point at an existing regular file. Any regular file works for this purely
     * command-list-shape assertion, so point it at a test resource and restore the
     * property afterward so this test doesn't leak state to others.
     */
    @Test
    public void classifyCommandIncludesPairTimeoutWhenPositive() throws Exception {
        String prev = System.getProperty("rustdl.bin");
        Path fakeBin = Paths.get(getClass().getResource("/json/classify.json").toURI());
        System.setProperty("rustdl.bin", fakeBin.toString());
        try {
            java.util.List<String> cmd = RustdlProcess.buildClassifyCommand(Paths.get("ont.ofn"), 10000);
            assertTrue(cmd.contains("--pair-timeout-ms"));
            assertTrue(cmd.contains("10000"));
        } finally {
            if (prev == null) System.clearProperty("rustdl.bin"); else System.setProperty("rustdl.bin", prev);
        }
    }

    @Test
    public void classifyCommandOmitsPairTimeoutWhenZero() throws Exception {
        String prev = System.getProperty("rustdl.bin");
        Path fakeBin = Paths.get(getClass().getResource("/json/classify.json").toURI());
        System.setProperty("rustdl.bin", fakeBin.toString());
        try {
            java.util.List<String> cmd = RustdlProcess.buildClassifyCommand(Paths.get("ont.ofn"), 0);
            assertFalse(cmd.contains("--pair-timeout-ms"));
        } finally {
            if (prev == null) System.clearProperty("rustdl.bin"); else System.setProperty("rustdl.bin", prev);
        }
    }
}
