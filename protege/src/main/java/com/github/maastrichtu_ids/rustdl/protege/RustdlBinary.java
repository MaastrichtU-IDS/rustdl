package com.github.maastrichtu_ids.rustdl.protege;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.*;
import java.nio.file.attribute.PosixFilePermission;
import java.util.EnumSet;
import java.util.Locale;

/**
 * Resolves a runnable rustdl executable: an explicit override
 * (-Drustdl.bin / RUSTDL_BIN) wins; otherwise the platform-matching binary
 * bundled at native/&lt;triple&gt;/rustdl[.exe] is extracted to a per-user cache
 * dir and made executable. Pure resolution — no reasoning.
 */
public final class RustdlBinary {
    private RustdlBinary() {}

    /** Rust target triple for a JVM (os.name, os.arch), or null if unsupported. */
    static String targetTriple(String osName, String osArch) {
        String os = osName.toLowerCase(Locale.ROOT);
        String arch = osArch.toLowerCase(Locale.ROOT);
        boolean x64 = arch.equals("amd64") || arch.equals("x86_64");
        boolean arm64 = arch.equals("aarch64") || arch.equals("arm64");
        if (os.startsWith("linux")) {
            if (x64) return "x86_64-unknown-linux-musl";
            if (arm64) return "aarch64-unknown-linux-musl";
        } else if (os.startsWith("mac") || os.contains("darwin")) {
            if (arm64) return "aarch64-apple-darwin";
        } else if (os.startsWith("windows")) {
            if (x64) return "x86_64-pc-windows-msvc";
        }
        return null;
    }

    /** The configured override path (system property beats env), or null. */
    static String configuredOverride() {
        String p = System.getProperty("rustdl.bin");
        if (p != null && !p.isEmpty()) return p;
        String e = System.getenv("RUSTDL_BIN");
        if (e != null && !e.isEmpty()) return e;
        return null;
    }

    /** Resolve a usable executable path, extracting the bundled binary if needed. */
    static Path resolve() throws IOException {
        String override = configuredOverride();
        if (override != null) {
            Path p = Paths.get(override);
            if (!Files.isRegularFile(p)) {
                throw new IOException("rustdl.bin/RUSTDL_BIN points at a missing file: " + override);
            }
            return p;
        }
        String triple = targetTriple(System.getProperty("os.name", ""), System.getProperty("os.arch", ""));
        if (triple == null) {
            throw new IOException("No bundled rustdl binary for os.name=" + System.getProperty("os.name")
                + " os.arch=" + System.getProperty("os.arch")
                + " — set -Drustdl.bin=/path/to/rustdl (build via `cargo build --release --bin rustdl`).");
        }
        boolean windows = triple.contains("windows");
        String exe = windows ? "rustdl.exe" : "rustdl";
        String resource = "/native/" + triple + "/" + exe;
        Path cacheDir = Paths.get(System.getProperty("user.home"), ".cache", "rustdl-protege", triple);
        Files.createDirectories(cacheDir);
        Path dest = cacheDir.resolve(exe);
        try (InputStream in = RustdlBinary.class.getResourceAsStream(resource)) {
            if (in == null) {
                throw new IOException("Bundled binary not found on classpath: " + resource
                    + " (this dev jar has no binaries — set -Drustdl.bin).");
            }
            Files.copy(in, dest, StandardCopyOption.REPLACE_EXISTING);
        }
        if (!windows) {
            EnumSet<PosixFilePermission> perms = EnumSet.of(
                PosixFilePermission.OWNER_READ, PosixFilePermission.OWNER_WRITE, PosixFilePermission.OWNER_EXECUTE,
                PosixFilePermission.GROUP_READ, PosixFilePermission.GROUP_EXECUTE,
                PosixFilePermission.OTHERS_READ, PosixFilePermission.OTHERS_EXECUTE);
            try { Files.setPosixFilePermissions(dest, perms); } catch (UnsupportedOperationException ignored) { }
        }
        return dest;
    }
}
