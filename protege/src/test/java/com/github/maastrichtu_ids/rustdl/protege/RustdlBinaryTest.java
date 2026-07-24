package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import static org.junit.Assert.*;

public class RustdlBinaryTest {
    @Test public void routesLinuxX64() {
        assertEquals("x86_64-unknown-linux-musl", RustdlBinary.targetTriple("Linux", "amd64"));
        assertEquals("x86_64-unknown-linux-musl", RustdlBinary.targetTriple("Linux", "x86_64"));
    }
    @Test public void routesLinuxArm() {
        assertEquals("aarch64-unknown-linux-musl", RustdlBinary.targetTriple("Linux", "aarch64"));
        assertEquals("aarch64-unknown-linux-musl", RustdlBinary.targetTriple("Linux", "arm64"));
    }
    @Test public void routesMacArm() {
        assertEquals("aarch64-apple-darwin", RustdlBinary.targetTriple("Mac OS X", "aarch64"));
    }
    @Test public void routesWindowsX64() {
        assertEquals("x86_64-pc-windows-msvc", RustdlBinary.targetTriple("Windows 11", "amd64"));
    }
    @Test public void unmappedPlatformReturnsNull() {
        assertNull(RustdlBinary.targetTriple("Mac OS X", "x86_64")); // Intel mac: use override
        assertNull(RustdlBinary.targetTriple("SunOS", "sparc"));
    }
    @Test public void overrideWins() {
        // A set rustdl.bin system property short-circuits extraction.
        String prev = System.getProperty("rustdl.bin");
        try {
            System.setProperty("rustdl.bin", "/nonexistent/rustdl-override");
            // resolve() returns the override path verbatim WITHOUT --version verification
            // is deferred to the process call; here we assert the path is taken.
            assertEquals("/nonexistent/rustdl-override", RustdlBinary.configuredOverride());
        } finally {
            if (prev == null) System.clearProperty("rustdl.bin"); else System.setProperty("rustdl.bin", prev);
        }
    }
}
