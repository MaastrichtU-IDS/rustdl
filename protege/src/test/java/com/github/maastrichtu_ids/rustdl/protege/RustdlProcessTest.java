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
}
