package com.github.maastrichtu_ids.rustdl.protege;

import com.google.gson.Gson;
import com.google.gson.JsonSyntaxException;

import java.io.IOException;
import java.io.InputStream;
import java.io.ByteArrayOutputStream;
import java.nio.file.Path;
import java.util.concurrent.TimeUnit;

/** Spawns `rustdl <subcmd> --json <ofn>`, enforces a timeout, parses stdout. Fail-closed. */
public final class RustdlProcess {
    private static final Gson GSON = new Gson();
    private static final int SCHEMA_VERSION = 1;
    private RustdlProcess() {}

    public static RustdlJson.ClassifyJson classify(Path ofn, long timeoutSec) throws IOException {
        return parseClassify(run("classify", ofn, timeoutSec));
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
        ProcessBuilder pb = new ProcessBuilder(
            bin.toString(), subcommand, "--json", ofn.toString());
        pb.redirectErrorStream(false);
        Process proc = pb.start();
        String out = readAll(proc.getInputStream());
        String err = readAll(proc.getErrorStream());
        boolean finished;
        try {
            finished = proc.waitFor(timeoutSec, TimeUnit.SECONDS);
        } catch (InterruptedException e) {
            proc.destroyForcibly();
            Thread.currentThread().interrupt();
            throw new IOException("rustdl " + subcommand + " interrupted", e);
        }
        if (!finished) {
            proc.destroyForcibly();
            throw new IOException("rustdl " + subcommand + " timed out after " + timeoutSec + "s");
        }
        int code = proc.exitValue();
        if (code != 0) {
            throw new IOException("rustdl " + subcommand + " exited " + code + ": " + err.trim());
        }
        return out;
    }

    private static String readAll(InputStream in) throws IOException {
        ByteArrayOutputStream bos = new ByteArrayOutputStream();
        byte[] buf = new byte[8192];
        int n;
        while ((n = in.read(buf)) != -1) bos.write(buf, 0, n);
        return bos.toString("UTF-8");
    }
}
