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
