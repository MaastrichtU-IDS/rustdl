package com.github.maastrichtu_ids.rustdl.protege;

import com.google.gson.Gson;
import com.google.gson.JsonSyntaxException;

import java.io.IOException;
import java.io.InputStream;
import java.io.ByteArrayOutputStream;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.List;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.TimeUnit;

/** Spawns `rustdl <subcmd> --json <ofn>`, enforces a timeout, parses stdout. Fail-closed. */
public final class RustdlProcess {
    private static final Gson GSON = new Gson();
    private static final int SCHEMA_VERSION = 1;
    private RustdlProcess() {}

    public static RustdlJson.ClassifyJson classify(Path ofn, long timeoutSec, long pairTimeoutMs) throws IOException {
        return parseClassify(runCommand(buildClassifyCommand(ofn, pairTimeoutMs), "classify", timeoutSec));
    }

    /** Package-visible so tests can assert on the command list without spawning. */
    static List<String> buildClassifyCommand(Path ofn, long pairTimeoutMs) throws IOException {
        java.util.List<String> cmd = new java.util.ArrayList<>(java.util.Arrays.asList(
            RustdlBinary.resolve().toString(), "classify", "--json"));
        if (pairTimeoutMs > 0) { cmd.add("--pair-timeout-ms"); cmd.add(Long.toString(pairTimeoutMs)); }
        cmd.add(ofn.toString());
        return cmd;
    }
    public static RustdlJson.ConsistentJson consistent(Path ofn, long timeoutSec) throws IOException {
        return parseConsistent(run("consistent", ofn, timeoutSec));
    }
    public static RustdlJson.RealizeJson realize(Path ofn, long timeoutSec) throws IOException {
        return parseRealize(run("realize", ofn, timeoutSec));
    }

    static RustdlJson.ClassifyJson parseClassify(String json) {
        RustdlJson.ClassifyJson c = fromJson(json, RustdlJson.ClassifyJson.class);
        checkVersion(c.schema_version);
        return c;
    }
    static RustdlJson.ConsistentJson parseConsistent(String json) {
        RustdlJson.ConsistentJson c = fromJson(json, RustdlJson.ConsistentJson.class);
        checkVersion(c.schema_version);
        return c;
    }
    static RustdlJson.RealizeJson parseRealize(String json) {
        RustdlJson.RealizeJson r = fromJson(json, RustdlJson.RealizeJson.class);
        checkVersion(r.schema_version);
        return r;
    }

    private static <T> T fromJson(String json, Class<T> type) {
        try {
            T v = GSON.fromJson(json, type);
            if (v == null) throw new IllegalStateException("rustdl produced empty JSON");
            return v;
        } catch (JsonSyntaxException e) {
            throw new IllegalStateException("rustdl produced unparseable JSON: " + e.getMessage(), e);
        }
    }
    private static void checkVersion(int v) {
        if (v != SCHEMA_VERSION) {
            throw new IllegalStateException("Unsupported rustdl JSON schema_version " + v
                + " (this plugin supports " + SCHEMA_VERSION + "); upgrade the plugin.");
        }
    }

    private static String run(String subcommand, Path ofn, long timeoutSec) throws IOException {
        Path bin = RustdlBinary.resolve();
        return runCommand(
            Arrays.asList(bin.toString(), subcommand, "--json", ofn.toString()),
            subcommand, timeoutSec);
    }

    /**
     * Runs {@code command}, draining stdout/stderr concurrently on daemon threads so a
     * child that hangs before writing/closing either stream cannot block the JVM, and
     * enforcing {@code timeoutSec} via {@code waitFor} independently of those reads
     * (draining alone cannot observe a timeout — a hung child never triggers EOF).
     */
    static String runCommand(List<String> command, String label, long timeoutSec) throws IOException {
        ProcessBuilder pb = new ProcessBuilder(command);
        pb.redirectErrorStream(false);
        Process proc = pb.start();

        ExecutorService pool = Executors.newFixedThreadPool(2, DAEMON_THREAD_FACTORY);
        try {
            Future<String> outF = pool.submit(() -> readAll(proc.getInputStream()));
            Future<String> errF = pool.submit(() -> readAll(proc.getErrorStream()));

            boolean finished;
            try {
                finished = proc.waitFor(timeoutSec, TimeUnit.SECONDS);
            } catch (InterruptedException e) {
                proc.destroyForcibly();
                Thread.currentThread().interrupt();
                throw new IOException("rustdl " + label + " interrupted", e);
            }
            if (!finished) {
                proc.destroyForcibly();
                throw new IOException("rustdl " + label + " timed out after " + timeoutSec + "s");
            }

            String out;
            String err;
            try {
                out = outF.get();
                err = errF.get();
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                throw new IOException("rustdl " + label + " interrupted while reading output", e);
            } catch (ExecutionException e) {
                throw new IOException("rustdl " + label + " failed reading output: "
                    + e.getCause(), e);
            }

            int code = proc.exitValue();
            if (code != 0) {
                throw new IOException("rustdl " + label + " exited " + code + ": " + err.trim());
            }
            return out;
        } finally {
            pool.shutdownNow();
        }
    }

    private static final ThreadFactory DAEMON_THREAD_FACTORY = r -> {
        Thread t = new Thread(r, "rustdl-stream-drain");
        t.setDaemon(true);
        return t;
    };

    private static String readAll(InputStream in) throws IOException {
        ByteArrayOutputStream bos = new ByteArrayOutputStream();
        byte[] buf = new byte[8192];
        int n;
        while ((n = in.read(buf)) != -1) bos.write(buf, 0, n);
        return bos.toString("UTF-8");
    }
}
