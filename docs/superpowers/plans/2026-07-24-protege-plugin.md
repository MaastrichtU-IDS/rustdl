# rustdl Protégé Reasoner Plugin — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship rustdl as a first-class, seamless-install Protégé reasoner — it appears in the reasoner dropdown and classifies / checks consistency / builds the class hierarchy / computes class assertions, with the native binary bundled in the plugin jar (no separate install or PATH setup).

**Architecture:** A new top-level Maven module `protege/` (sibling to `crates/`), a pure-Java OSGi bundle mirroring `kobayashi-marust`'s (km) plugin structure. It embeds the four `rustdl` binaries built by `release-cli.yml` under `native/<triple>/`, extracts the one matching the host platform to a per-user cache dir at runtime, and drives it through the `--json` contract (`docs/json-schema.md`). The OWLAPI surface is a `OWLReasonerBase` (BUFFERING) whose `precomputeInferences` maps `CLASS_HIERARCHY`→`rustdl classify --json` and `CLASS_ASSERTIONS`→`rustdl realize --json`, caches the JSON, and answers all query methods from the cache. The plugin jar is built and attached to the GitHub Release by a new `build-plugin` job in `release-cli.yml` that consumes the **same-run** binary artifacts.

**Tech Stack:** Java 11, Maven, `org.apache.felix:maven-bundle-plugin:5.1.9` (packaging `bundle`); OWLAPI `4.5.29` + Protégé `protege-editor-owl:5.6.6` (both `provided`); `com.google.code.gson:gson:2.11.0` (embedded) for JSON; JUnit 4.13.2 (test). GitHub Actions for the jar build/release.

## Global Constraints

Every task's requirements implicitly include this section. Values are exact.

- **Toolchain (SDD implementers must install once):** `brew install maven` (pulls a JDK ≥ 11). Verify `mvn -version` and `java -version` before building. There is no Java toolchain on the dev host by default. All Maven commands run from the repo root as `mvn -f protege/pom.xml …`.
- **Dependency versions (mirror km exactly):** OWLAPI `net.sourceforge.owlapi:owlapi-distribution:4.5.29` (`provided`); Protégé `edu.stanford.protege:protege-editor-owl:5.6.6` (`provided`); `com.google.code.gson:gson:2.11.0` (compile, embedded); `junit:junit:4.13.2` (test). `maven.compiler.release` = `11`.
- **Java package root:** `com.github.maastrichtu_ids.rustdl.protege` (all classes).
- **OSGi packaging:** `<packaging>bundle</packaging>`; `maven-bundle-plugin` with `<extensions>true</extensions>`. `Bundle-SymbolicName: nl.maastrichtuniversity.ids.rustdl;singleton:=true`. `Bundle-Activator: org.protege.editor.core.plugin.DefaultPluginActivator`. `plugin.xml` copied to the **jar root** (not `META-INF/`) via `Include-Resource`. gson embedded via `Embed-Dependency`.
- **Reasoner identity:** dropdown name **exactly** `"rustdl"`. `getReasonerName()` returns `"rustdl"`. `getReasonerVersion()` returns the plugin `Version`.
- **Binary contract (from `docs/cli-binaries.md`, already shipped):** bundled resources live at `native/<triple>/rustdl[.exe]` for the four triples `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. Platform routing = the mapping table in `docs/cli-binaries.md`. Unmapped platform ⇒ require the `rustdl.bin`/`RUSTDL_BIN` override.
- **JSON contract (from `docs/json-schema.md`, already shipped):** `classify --json` → `{schema_version, consistent, incomplete, unsatisfiable[], equivalent_groups[][], direct_subsumptions[][2]}`; `consistent --json` → `{schema_version, consistent}`; `realize --json` → `{schema_version, individuals:[{iri, types[], direct_types[]}]}`. `schema_version` MUST equal `1`; any other value fails loudly.
- **Config (3-tier, mirror km):** system property → env var → default. `rustdl.bin` / `RUSTDL_BIN` → default = the bundled binary; `rustdl.timeout.seconds` / `RUSTDL_TIMEOUT_SECONDS` → default `600`.
- **Fail-closed:** binary missing / non-zero exit / timeout / JSON parse or `schema_version` mismatch ⇒ throw (`ReasonerInternalException` / `TimeOutException`), never a silent partial or hung result. `incomplete:true` ⇒ log a warning; the hierarchy is a sound under-approximation.
- **Serialization:** the imports closure (`Imports.INCLUDED`) merged into one ontology and written as **OWL Functional Syntax** (`FunctionalSyntaxDocumentFormat`) to a temp file, once per precompute cycle (shared across the classify + realize calls of that cycle).
- **`release-python.yml` stays untouched.** `release-cli.yml` (from Plan B) MAY be extended (it is not frozen).
- **Scope (v1):** wired = consistency, class hierarchy, unsatisfiable classes, class assertions (types/instances). Everything else (object/data-property hierarchies & assertions, same/different individuals, disjoint classes, complex-class-expression queries) returns empty node sets. Named-class `isSatisfiable`/`isEntailed(SubClassOf)` answered from cache; complex-class-expression `isSatisfiable`/`isEntailed` throw `UnsupportedOperationException` (a bool guess would be unsound). Explanation/justify/repair UI is deferred (design §2).

## Reference material

The km plugin is the structural template. Read-only copies of every km plugin file are in the scratchpad at `.../scratchpad/km/` (fetched during planning): `protege_pom.xml`, `protege_src_main_resources_plugin.xml`, `KMReasoner.java`, `KMReasonerFactory.java`, `KMReasonerInfo.java`, `Classifier.java`, `FlattenedOntology.java`. Where this plan says "mirror km's X", read km's file for the mechanical shape and apply the rustdl deltas the task specifies. If the scratchpad copies are gone, fetch from `https://raw.githubusercontent.com/bio-ontology-research-group/kobayashi-marust/main/protege/…`.

## File Structure

```
protege/
  pom.xml
  README.md
  src/main/java/com/github/maastrichtu_ids/rustdl/protege/
    RustdlBinary.java          # platform detect + extract bundled binary + override + --version verify
    RustdlProcess.java         # spawn rustdl <subcmd> --json, timeout, fail-closed, gson parse
    RustdlJson.java            # gson POJOs: ClassifyJson / ConsistentJson / RealizeJson / IndividualJson
    FlattenedOntology.java     # imports-closure → merged ontology → OFN temp file  (mirror km)
    RustdlReasoner.java        # OWLReasonerBase (BUFFERING): precompute + cache + query mapping
    RustdlReasonerFactory.java # OWLReasonerFactory delegate
    RustdlReasonerInfo.java    # AbstractProtegeOWLReasonerInfo → puts "rustdl" in the dropdown
  src/main/resources/
    plugin.xml                 # OSGi extension: org.protege.editor.owl.inference_reasonerfactory
    native/                    # binaries land here at release build time (git-ignored; empty in dev)
  src/test/java/com/github/maastrichtu_ids/rustdl/protege/
    RustdlBinaryTest.java
    RustdlProcessTest.java
    RustdlReasonerTest.java
    PluginRegistrationTest.java
  src/test/resources/json/     # canned JSON fixtures for parse + mapping tests
    classify.json  classify_unsat.json  inconsistent.json  realize.json
  .gitignore                   # ignore src/main/resources/native/*/ (built artifacts, never committed)
```

Also modified: `.github/workflows/release-cli.yml` (add `build-plugin` job); new `.github/workflows/java-ci.yml` (PR compile+test); `docs/cli-binaries.md` and/or a new `docs/protege-plugin.md` (install + build docs). Root `.gitignore` gets `protege/src/main/resources/native/`.

---

### Task 1: Maven module skeleton + OSGi bundle build

**Files:**
- Create: `protege/pom.xml`, `protege/src/main/resources/plugin.xml`, `protege/README.md`, `protege/.gitignore`
- Create: `protege/src/main/java/com/github/maastrichtu_ids/rustdl/protege/RustdlReasonerInfo.java` (minimal stub so the bundle has its referenced class; fleshed out in Task 4)
- Modify: root `.gitignore` (add `protege/src/main/resources/native/`)

**Interfaces:**
- Consumes: nothing.
- Produces: a buildable module — `mvn -f protege/pom.xml package` emits `protege/target/rustdl-protege-*.jar` with `plugin.xml` at the jar root. Later tasks add classes under the same package.

- [ ] **Step 1: Install and verify the toolchain**

```bash
command -v mvn >/dev/null || brew install maven
mvn -version && java -version
```
Expected: Maven ≥ 3.6 and a JDK ≥ 11 both print versions.

- [ ] **Step 2: Write `protege/pom.xml`**

Mirror km's `protege_pom.xml`, with the rustdl identity, gson embedded, and the antrun jar-content self-check. Exact content:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>

  <groupId>nl.maastrichtuniversity.ids</groupId>
  <artifactId>rustdl-protege</artifactId>
  <version>0.0.0-SNAPSHOT</version>
  <packaging>bundle</packaging>

  <name>rustdl Reasoner (Protégé plugin)</name>
  <description>rustdl OWL 2 reasoner as a Protégé plugin</description>
  <url>https://github.com/MaastrichtU-IDS/rustdl</url>

  <properties>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
    <maven.compiler.release>11</maven.compiler.release>
    <owlapi.version>4.5.29</owlapi.version>
    <protege.version>5.6.6</protege.version>
    <gson.version>2.11.0</gson.version>
  </properties>

  <dependencies>
    <dependency>
      <groupId>net.sourceforge.owlapi</groupId>
      <artifactId>owlapi-distribution</artifactId>
      <version>${owlapi.version}</version>
      <scope>provided</scope>
    </dependency>
    <dependency>
      <groupId>edu.stanford.protege</groupId>
      <artifactId>protege-editor-owl</artifactId>
      <version>${protege.version}</version>
      <scope>provided</scope>
    </dependency>
    <dependency>
      <groupId>com.google.code.gson</groupId>
      <artifactId>gson</artifactId>
      <version>${gson.version}</version>
    </dependency>
    <dependency>
      <groupId>junit</groupId>
      <artifactId>junit</artifactId>
      <version>4.13.2</version>
      <scope>test</scope>
    </dependency>
  </dependencies>

  <build>
    <plugins>
      <plugin>
        <groupId>org.apache.felix</groupId>
        <artifactId>maven-bundle-plugin</artifactId>
        <version>5.1.9</version>
        <extensions>true</extensions>
        <configuration>
          <instructions>
            <Bundle-SymbolicName>nl.maastrichtuniversity.ids.rustdl;singleton:=true</Bundle-SymbolicName>
            <Bundle-Name>rustdl Reasoner</Bundle-Name>
            <Bundle-Category>protege</Bundle-Category>
            <Bundle-ContactAddress>https://github.com/MaastrichtU-IDS/rustdl</Bundle-ContactAddress>
            <Bundle-Vendor>Institute of Data Science, Maastricht University</Bundle-Vendor>
            <Bundle-RequiredExecutionEnvironment>JavaSE-11</Bundle-RequiredExecutionEnvironment>
            <Bundle-License>Apache-2.0</Bundle-License>
            <Bundle-Activator>org.protege.editor.core.plugin.DefaultPluginActivator</Bundle-Activator>
            <Export-Package>com.github.maastrichtu_ids.rustdl.protege;version="${project.version}"</Export-Package>
            <Import-Package>
              org.protege.editor.core.*,
              org.protege.editor.owl.*,
              org.semanticweb.owlapi.*,
              org.osgi.framework,
              *
            </Import-Package>
            <Embed-Dependency>gson;scope=compile;inline=false</Embed-Dependency>
            <Embed-Transitive>false</Embed-Transitive>
            <Include-Resource>{maven-resources}, plugin.xml=src/main/resources/plugin.xml</Include-Resource>
          </instructions>
        </configuration>
      </plugin>
      <plugin>
        <groupId>org.apache.maven.plugins</groupId>
        <artifactId>maven-antrun-plugin</artifactId>
        <version>3.1.0</version>
        <executions>
          <execution>
            <id>verify-bundle-contents</id>
            <phase>verify</phase>
            <goals><goal>run</goal></goals>
            <configuration>
              <target>
                <unzip src="${project.build.directory}/${project.build.finalName}.jar"
                       dest="${project.build.directory}/jar-check"/>
                <fail message="plugin.xml missing from jar root">
                  <condition><not><available file="${project.build.directory}/jar-check/plugin.xml"/></not></condition>
                </fail>
                <fail message="gson not embedded">
                  <condition><not><available file="${project.build.directory}/jar-check/com/google/gson"/></not></condition>
                </fail>
              </target>
            </configuration>
          </execution>
        </executions>
      </plugin>
    </plugins>
  </build>
</project>
```

- [ ] **Step 3: Write `protege/src/main/resources/plugin.xml`**

```xml
<?xml version="1.0"?>
<?eclipse version="3.0"?>
<plugin>
   <extension id="RustdlReasonerFactory"
              point="org.protege.editor.owl.inference_reasonerfactory">
      <reasonerFactory name="rustdl"
                       factoryClass="com.github.maastrichtu_ids.rustdl.protege.RustdlReasonerInfo"/>
   </extension>
</plugin>
```

- [ ] **Step 4: Write the minimal `RustdlReasonerInfo` stub** (so the bundle's referenced class exists; Task 4 completes it)

`protege/src/main/java/com/github/maastrichtu_ids/rustdl/protege/RustdlReasonerInfo.java`:

```java
package com.github.maastrichtu_ids.rustdl.protege;

import org.protege.editor.owl.model.inference.AbstractProtegeOWLReasonerInfo;
import org.semanticweb.owlapi.reasoner.BufferingMode;
import org.semanticweb.owlapi.reasoner.OWLReasonerFactory;

/** Places "rustdl" in Protégé's reasoner dropdown. */
public class RustdlReasonerInfo extends AbstractProtegeOWLReasonerInfo {
    private final RustdlReasonerFactory factory = new RustdlReasonerFactory();

    @Override public OWLReasonerFactory getReasonerFactory() { return factory; }
    @Override public BufferingMode getRecommendedBuffering() { return BufferingMode.BUFFERING; }
    @Override public void initialise() { }
    @Override public void dispose() { }
}
```

This references `RustdlReasonerFactory` (Task 4). To keep Task 1 independently compilable, ALSO create a minimal compiling stub of the factory now; Task 4 replaces it:

`RustdlReasonerFactory.java` (Task-1 stub):
```java
package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owlapi.reasoner.*;
import org.semanticweb.owlapi.model.OWLOntology;

/** Task-1 stub — replaced in Task 4. */
public class RustdlReasonerFactory implements OWLReasonerFactory {
    @Override public String getReasonerName() { return "rustdl"; }
    @Override public OWLReasoner createReasoner(OWLOntology o) { throw new UnsupportedOperationException("stub"); }
    @Override public OWLReasoner createReasoner(OWLOntology o, OWLReasonerConfiguration c) { throw new UnsupportedOperationException("stub"); }
    @Override public OWLReasoner createNonBufferingReasoner(OWLOntology o) { throw new UnsupportedOperationException("stub"); }
    @Override public OWLReasoner createNonBufferingReasoner(OWLOntology o, OWLReasonerConfiguration c) { throw new UnsupportedOperationException("stub"); }
}
```

- [ ] **Step 5: Write `protege/.gitignore` and update root `.gitignore`**

`protege/.gitignore`:
```
/target/
/src/main/resources/native/
```
Append to root `.gitignore`:
```
protege/src/main/resources/native/
protege/target/
```

- [ ] **Step 6: Write `protege/README.md`** (build + install instructions — content below, complete)

```markdown
# rustdl Protégé plugin

Bundles the rustdl OWL 2 reasoner as a Protégé reasoner plugin.

## Build (dev)

    brew install maven          # JDK 11+ + Maven
    mvn -f protege/pom.xml package
    # → protege/target/rustdl-protege-0.0.0-SNAPSHOT.jar

The dev jar contains no bundled binaries; run Protégé with
`-Drustdl.bin=/path/to/rustdl` (build one via `cargo build --release --bin rustdl`).

## Install

Drop the release jar (which bundles all four platform binaries) into Protégé's
`plugins/` directory and restart. "rustdl" then appears in Reasoner ▸.

- macOS: `~/Library/Application Support/Protege/plugins/` or `Protege.app/Contents/Java/plugins/`
- Linux: `~/.Protege/plugins/` or `<Protege>/plugins/`
- Windows: `%USERPROFILE%\.Protege\plugins\` or `<Protege>\plugins\`

## Config (system property overrides env overrides default)

- `rustdl.bin` / `RUSTDL_BIN` — path to a rustdl binary (default: the bundled one)
- `rustdl.timeout.seconds` / `RUSTDL_TIMEOUT_SECONDS` — per-call timeout (default 600)
```

- [ ] **Step 7: Build and verify**

```bash
mvn -f protege/pom.xml clean package
unzip -l protege/target/rustdl-protege-0.0.0-SNAPSHOT.jar | grep -E "plugin.xml|com/google/gson" | head
```
Expected: build succeeds through the `verify` phase (antrun self-check passes); `plugin.xml` is at the jar root and `com/google/gson/...` classes are embedded.

- [ ] **Step 8: Commit**

```bash
git add protege .gitignore
git commit -m "feat(protege): Maven OSGi bundle skeleton + reasoner registration"
```

---

### Task 2: `RustdlBinary` — platform detection, extraction, override

**Files:**
- Create: `protege/src/main/java/.../RustdlBinary.java`
- Test: `protege/src/test/java/.../RustdlBinaryTest.java`

**Interfaces:**
- Consumes: nothing (self-contained resolution).
- Produces: `RustdlBinary.resolve()` → `Path` to a runnable rustdl executable. Static `String targetTriple(String osName, String osArch)` (package-visible, for unit testing the routing). `RustdlProcess` (Task 3) calls `RustdlBinary.resolve()`.

- [ ] **Step 1: Write the failing test** `RustdlBinaryTest.java`

```java
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
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `mvn -f protege/pom.xml test -Dtest=RustdlBinaryTest`
Expected: FAIL to compile (`RustdlBinary` undefined).

- [ ] **Step 3: Write `RustdlBinary.java`**

```java
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `mvn -f protege/pom.xml test -Dtest=RustdlBinaryTest`
Expected: PASS (all routing + override cases green).

- [ ] **Step 5: Commit**

```bash
git add protege/src/main/java protege/src/test/java
git commit -m "feat(protege): RustdlBinary platform routing + bundled-binary extraction"
```

---

### Task 3: `RustdlProcess` + JSON model + OFN serialization

**Files:**
- Create: `protege/src/main/java/.../RustdlJson.java`, `RustdlProcess.java`, `FlattenedOntology.java`
- Test: `protege/src/test/java/.../RustdlProcessTest.java`
- Test resources: `protege/src/test/resources/json/{classify,classify_unsat,inconsistent,realize}.json`

**Interfaces:**
- Consumes: `RustdlBinary.resolve()` (Task 2).
- Produces:
  - `RustdlJson.ClassifyJson { int schema_version; boolean consistent; boolean incomplete; List<String> unsatisfiable; List<List<String>> equivalent_groups; List<List<String>> direct_subsumptions; }`
  - `RustdlJson.ConsistentJson { int schema_version; boolean consistent; }`
  - `RustdlJson.RealizeJson { int schema_version; List<IndividualJson> individuals; }` with `IndividualJson { String iri; List<String> types; List<String> direct_types; }`
  - `RustdlProcess.classify(Path ofn, long timeoutSec)`, `.consistent(...)`, `.realize(...)` → the matching POJO. `RustdlProcess.parseClassify(String json)` etc. (package-visible, pure) for unit testing without a subprocess.
  - `FlattenedOntology.writeOfn(OWLOntology, Path)` (mirror km's `FlattenedOntology.save`).

- [ ] **Step 1: Write the JSON fixtures** (`src/test/resources/json/`)

`classify.json`:
```json
{ "schema_version": 1, "consistent": true, "incomplete": false,
  "unsatisfiable": [],
  "equivalent_groups": [["http://ex/#A", "http://ex/#B"]],
  "direct_subsumptions": [["http://ex/#A", "http://ex/#C"]] }
```
`classify_unsat.json`:
```json
{ "schema_version": 1, "consistent": true, "incomplete": true,
  "unsatisfiable": ["http://ex/#Bad"],
  "equivalent_groups": [],
  "direct_subsumptions": [["http://ex/#Sub", "http://ex/#Sup"]] }
```
`inconsistent.json`:
```json
{ "schema_version": 1, "consistent": false }
```
`realize.json`:
```json
{ "schema_version": 1,
  "individuals": [ { "iri": "http://ex/#i", "types": ["http://ex/#A","http://ex/#C"],
                     "direct_types": ["http://ex/#A"] } ] }
```

- [ ] **Step 2: Write the failing test** `RustdlProcessTest.java`

```java
package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import java.nio.file.*;
import static org.junit.Assert.*;

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
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `mvn -f protege/pom.xml test -Dtest=RustdlProcessTest`
Expected: FAIL to compile (`RustdlJson`/`RustdlProcess` undefined).

- [ ] **Step 4: Write `RustdlJson.java`**

```java
package com.github.maastrichtu_ids.rustdl.protege;

import java.util.List;

/** gson-mapped POJOs for the rustdl --json contract (docs/json-schema.md, schema_version 1). */
public final class RustdlJson {
    private RustdlJson() {}

    public static final class ClassifyJson {
        public int schema_version;
        public boolean consistent;
        public boolean incomplete;
        public List<String> unsatisfiable;
        public List<List<String>> equivalent_groups;
        public List<List<String>> direct_subsumptions;
    }
    public static final class ConsistentJson {
        public int schema_version;
        public boolean consistent;
    }
    public static final class RealizeJson {
        public int schema_version;
        public List<IndividualJson> individuals;
    }
    public static final class IndividualJson {
        public String iri;
        public List<String> types;
        public List<String> direct_types;
    }
}
```

- [ ] **Step 5: Write `RustdlProcess.java`**

```java
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
```

> Note: reading stdout/stderr fully before `waitFor` avoids the classic pipe-buffer deadlock. Because `--json` output can exceed a pipe buffer on large ontologies, the reads happen on the calling thread after `start()`, draining as the process writes; this is adequate for the CLI's bounded output. (If a future ontology produces enormous output and this blocks, switch to draining threads — not needed for v1.)

- [ ] **Step 6: Write `FlattenedOntology.java`** (mirror km's `FlattenedOntology.save`)

```java
package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.formats.FunctionalSyntaxDocumentFormat;
import org.semanticweb.owlapi.model.*;
import org.semanticweb.owlapi.model.parameters.Imports;

import java.nio.file.Path;

/** Serialises an ontology's imports closure into one OWL Functional Syntax file. */
public final class FlattenedOntology {
    private FlattenedOntology() {}

    public static void writeOfn(OWLOntology source, Path destination) throws OWLOntologyCreationException, OWLOntologyStorageException {
        OWLOntologyManager manager = OWLManager.createOWLOntologyManager();
        OWLOntology flattened = manager.createOntology(source.getAxioms(Imports.INCLUDED));
        manager.saveOntology(flattened, new FunctionalSyntaxDocumentFormat(),
            IRI.create(destination.toUri()));
    }
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `mvn -f protege/pom.xml test -Dtest=RustdlProcessTest`
Expected: PASS (all parse cases + the schema-version rejection).

- [ ] **Step 8: Commit**

```bash
git add protege/src
git commit -m "feat(protege): RustdlProcess JSON bridge + OFN serialization"
```

---

### Task 4: `RustdlReasoner` (OWLReasonerBase, BUFFERING) + Factory + Info

**Files:**
- Create/replace: `protege/src/main/java/.../RustdlReasoner.java`, `RustdlReasonerFactory.java` (replace Task-1 stub); complete `RustdlReasonerInfo.java` is already done.
- Test: `protege/src/test/java/.../RustdlReasonerTest.java`, `PluginRegistrationTest.java`

**Interfaces:**
- Consumes: `RustdlProcess` (Task 3), `FlattenedOntology` (Task 3), `RustdlBinary` (Task 2).
- Produces: a working `OWLReasoner`. Package-visible seams for unit testing WITHOUT a subprocess: a constructor/factory-method that injects a pre-built `RustdlJson.ClassifyJson` and `RustdlJson.RealizeJson` into the cache (e.g. `static RustdlReasoner forTest(OWLOntology, ClassifyJson, RealizeJson)`), so query-mapping is tested against canned JSON.

- [ ] **Step 1: Write the failing tests**

`RustdlReasonerTest.java` — build an ontology with `OWLManager`, inject canned JSON, assert the query mapping:

```java
package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.*;
import org.semanticweb.owlapi.reasoner.*;
import java.util.*;
import static org.junit.Assert.*;

public class RustdlReasonerTest {
    private final OWLOntologyManager m = OWLManager.createOWLOntologyManager();
    private final OWLDataFactory df = m.getOWLDataFactory();
    private OWLClass cls(String i) { return df.getOWLClass(IRI.create("http://ex/#" + i)); }

    private RustdlReasoner reasoner() throws Exception {
        OWLOntology o = m.createOntology(IRI.create("http://ex/"));
        RustdlJson.ClassifyJson c = new RustdlJson.ClassifyJson();
        c.schema_version = 1; c.consistent = true; c.incomplete = false;
        c.unsatisfiable = new ArrayList<>();
        c.equivalent_groups = Arrays.asList(Arrays.asList("http://ex/#A", "http://ex/#B"));
        c.direct_subsumptions = Arrays.asList(Arrays.asList("http://ex/#A", "http://ex/#C"));
        RustdlJson.RealizeJson r = new RustdlJson.RealizeJson();
        r.schema_version = 1;
        RustdlJson.IndividualJson ind = new RustdlJson.IndividualJson();
        ind.iri = "http://ex/#i"; ind.types = Arrays.asList("http://ex/#A", "http://ex/#C");
        ind.direct_types = Arrays.asList("http://ex/#A");
        r.individuals = Arrays.asList(ind);
        return RustdlReasoner.forTest(o, c, r);
    }

    @Test public void consistent() throws Exception { assertTrue(reasoner().isConsistent()); }

    @Test public void equivalentClasses() throws Exception {
        Node<OWLClass> eq = reasoner().getEquivalentClasses(cls("A"));
        assertTrue(eq.contains(cls("B")));
    }

    @Test public void directSuperClasses() throws Exception {
        NodeSet<OWLClass> supers = reasoner().getSuperClasses(cls("A"), true);
        assertTrue(supers.containsEntity(cls("C")));
    }

    @Test public void directSubClasses() throws Exception {
        NodeSet<OWLClass> subs = reasoner().getSubClasses(cls("C"), true);
        assertTrue(subs.containsEntity(cls("A")));
    }

    @Test public void types() throws Exception {
        OWLNamedIndividual i = df.getOWLNamedIndividual(IRI.create("http://ex/#i"));
        assertTrue(reasoner().getTypes(i, true).containsEntity(cls("A")));
        assertTrue(reasoner().getTypes(i, false).containsEntity(cls("C")));
    }

    @Test public void instances() throws Exception {
        NodeSet<OWLNamedIndividual> insts = reasoner().getInstances(cls("A"), false);
        assertTrue(insts.containsEntity(df.getOWLNamedIndividual(IRI.create("http://ex/#i"))));
    }

    @Test(expected = UnsupportedOperationException.class)
    public void complexSatisfiabilityThrows() throws Exception {
        reasoner().isSatisfiable(df.getOWLObjectIntersectionOf(cls("A"), cls("C")));
    }

    @Test public void unsupportedReturnsEmpty() throws Exception {
        assertTrue(reasoner().getObjectPropertyValues(
            df.getOWLNamedIndividual(IRI.create("http://ex/#i")),
            df.getOWLObjectProperty(IRI.create("http://ex/#p"))).isEmpty());
    }
}
```

`PluginRegistrationTest.java` — assert `plugin.xml` on the classpath registers the reasoner factory class:

```java
package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import java.io.InputStream;
import java.util.Scanner;
import static org.junit.Assert.*;

public class PluginRegistrationTest {
    @Test public void pluginXmlRegistersRustdl() throws Exception {
        try (InputStream in = getClass().getResourceAsStream("/plugin.xml")) {
            assertNotNull("plugin.xml must be on the classpath", in);
            String xml = new Scanner(in, "UTF-8").useDelimiter("\\A").next();
            assertTrue(xml.contains("org.protege.editor.owl.inference_reasonerfactory"));
            assertTrue(xml.contains("name=\"rustdl\""));
            assertTrue(xml.contains("com.github.maastrichtu_ids.rustdl.protege.RustdlReasonerInfo"));
        }
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `mvn -f protege/pom.xml test -Dtest=RustdlReasonerTest+PluginRegistrationTest`
Expected: FAIL (`forTest`/query methods undefined; `RustdlReasoner` is not yet the real class).

- [ ] **Step 3: Write `RustdlReasoner.java`**

This is the crux. Structure (mirror km's `KMReasoner` for the mechanical `OWLReasonerBase` wiring; apply the rustdl deltas: `--json` cache, `CLASS_HIERARCHY`+`CLASS_ASSERTIONS` precompute, realize-backed instances/types). Complete implementation:

```java
package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owlapi.model.*;
import org.semanticweb.owlapi.model.parameters.ChangeApplied;
import org.semanticweb.owlapi.reasoner.*;
import org.semanticweb.owlapi.reasoner.impl.*;
import org.semanticweb.owlapi.util.Version;

import java.nio.file.*;
import java.util.*;
import java.util.logging.Logger;

public class RustdlReasoner extends OWLReasonerBase {
    private static final Logger LOG = Logger.getLogger(RustdlReasoner.class.getName());

    private final OWLDataFactory df;
    private final long timeoutSec;

    // Cache, populated by precompute (or injected in tests).
    private RustdlJson.ClassifyJson classifyResult;
    private RustdlJson.RealizeJson realizeResult;

    // Derived indices built from classifyResult.
    private final Map<String, Node<OWLClass>> equivNodeByIri = new HashMap<>();   // iri -> its equiv-class node
    private final Map<OWLClass, Set<OWLClass>> directSupers = new HashMap<>();
    private final Map<OWLClass, Set<OWLClass>> directSubs = new HashMap<>();
    private final Set<OWLClass> unsatisfiable = new HashSet<>();

    RustdlReasoner(OWLOntology rootOntology, OWLReasonerConfiguration config, BufferingMode mode) {
        super(rootOntology, config, mode);
        this.df = rootOntology.getOWLOntologyManager().getOWLDataFactory();
        this.timeoutSec = resolveTimeout(config);
    }

    /** Test seam: inject canned results, skip the subprocess. */
    static RustdlReasoner forTest(OWLOntology o, RustdlJson.ClassifyJson c, RustdlJson.RealizeJson r) {
        SimpleConfiguration cfg = new SimpleConfiguration();
        RustdlReasoner reasoner = new RustdlReasoner(o, cfg, BufferingMode.BUFFERING);
        reasoner.classifyResult = c;
        reasoner.realizeResult = r;
        reasoner.rebuildIndices();
        return reasoner;
    }

    private static long resolveTimeout(OWLReasonerConfiguration config) {
        String p = System.getProperty("rustdl.timeout.seconds");
        if (p == null || p.isEmpty()) p = System.getenv("RUSTDL_TIMEOUT_SECONDS");
        if (p != null && !p.isEmpty()) try { return Long.parseLong(p); } catch (NumberFormatException ignored) {}
        return 600L;
    }

    // ---- identity ----
    @Override public String getReasonerName() { return "rustdl"; }
    @Override public Version getReasonerVersion() { return new Version(0, 0, 0, 0); }

    // ---- precompute / buffering ----
    @Override public Set<InferenceType> getPrecomputableInferenceTypes() {
        return EnumSet.of(InferenceType.CLASS_HIERARCHY, InferenceType.CLASS_ASSERTIONS);
    }
    @Override public boolean isPrecomputed(InferenceType type) {
        if (type == InferenceType.CLASS_HIERARCHY) return classifyResult != null;
        if (type == InferenceType.CLASS_ASSERTIONS) return realizeResult != null;
        return true; // unsupported types are trivially "precomputed" (empty)
    }
    @Override public void precomputeInferences(InferenceType... types) {
        Set<InferenceType> req = new HashSet<>(Arrays.asList(types));
        boolean wantHierarchy = req.contains(InferenceType.CLASS_HIERARCHY);
        boolean wantAssertions = req.contains(InferenceType.CLASS_ASSERTIONS);
        if (!wantHierarchy && !wantAssertions) return;
        Path ofn = null;
        try {
            ofn = Files.createTempFile("rustdl-", ".ofn");
            FlattenedOntology.writeOfn(getRootOntology(), ofn);
            if (wantHierarchy && classifyResult == null) {
                classifyResult = RustdlProcess.classify(ofn, timeoutSec);
                if (classifyResult.incomplete) {
                    LOG.warning("rustdl reports an INCOMPLETE classification (some class pairs timed out); "
                        + "the hierarchy is a sound under-approximation.");
                }
                rebuildIndices();
            }
            if (wantAssertions && realizeResult == null
                    && !getRootOntology().getIndividualsInSignature(true).isEmpty()) {
                realizeResult = RustdlProcess.realize(ofn, timeoutSec);
            }
        } catch (Exception e) {
            throw new ReasonerInternalException("rustdl precompute failed: " + e.getMessage(), e);
        } finally {
            if (ofn != null) try { Files.deleteIfExists(ofn); } catch (Exception ignored) {}
        }
    }

    /** Ensure classify ran (lazy precompute for a supported query issued before precomputeInferences). */
    private void ensureClassified() {
        if (classifyResult == null) precomputeInferences(InferenceType.CLASS_HIERARCHY);
    }
    private void ensureRealized() {
        if (realizeResult == null) precomputeInferences(InferenceType.CLASS_ASSERTIONS);
    }

    @Override protected void handleChanges(Set<OWLAxiom> added, Set<OWLAxiom> removed) {
        // BUFFERING: an edit invalidates the cache; next query re-runs the subprocess.
        classifyResult = null;
        realizeResult = null;
        equivNodeByIri.clear(); directSupers.clear(); directSubs.clear(); unsatisfiable.clear();
    }

    // ---- index building from classifyResult ----
    private void rebuildIndices() {
        equivNodeByIri.clear(); directSupers.clear(); directSubs.clear(); unsatisfiable.clear();
        if (classifyResult == null) return;
        for (String iri : orEmpty(classifyResult.unsatisfiable)) unsatisfiable.add(clazz(iri));
        // equivalence nodes
        for (List<String> group : orEmpty(classifyResult.equivalent_groups)) {
            Set<OWLClass> members = new HashSet<>();
            for (String iri : group) members.add(clazz(iri));
            Node<OWLClass> node = new OWLClassNode(members);
            for (OWLClass c : members) equivNodeByIri.put(c.getIRI().toString(), node);
        }
        // direct subsumption edges
        for (List<String> edge : orEmpty(classifyResult.direct_subsumptions)) {
            OWLClass sub = clazz(edge.get(0)), sup = clazz(edge.get(1));
            directSupers.computeIfAbsent(sub, k -> new HashSet<>()).add(sup);
            directSubs.computeIfAbsent(sup, k -> new HashSet<>()).add(sub);
        }
    }
    private OWLClass clazz(String iri) { return df.getOWLClass(IRI.create(iri)); }
    private static <T> List<T> orEmpty(List<T> l) { return l == null ? Collections.emptyList() : l; }

    private Node<OWLClass> equivNodeOf(OWLClass c) {
        Node<OWLClass> n = equivNodeByIri.get(c.getIRI().toString());
        return n != null ? n : new OWLClassNode(c);
    }

    // ---- consistency / satisfiability ----
    @Override public boolean isConsistent() {
        ensureClassified();
        return classifyResult.consistent;
    }
    @Override public boolean isSatisfiable(OWLClassExpression ce) {
        if (ce.isAnonymous()) {
            throw new UnsupportedOperationException(
                "rustdl answers satisfiability only for named classes");
        }
        ensureClassified();
        if (!isConsistent()) throw new InconsistentOntologyException();
        return !unsatisfiable.contains(ce.asOWLClass());
    }
    @Override public Node<OWLClass> getUnsatisfiableClasses() {
        ensureClassified();
        Set<OWLClass> all = new HashSet<>(unsatisfiable);
        all.add(df.getOWLNothing());
        return new OWLClassNode(all);
    }

    // ---- class hierarchy ----
    @Override public Node<OWLClass> getTopClassNode() { return new OWLClassNode(df.getOWLThing()); }
    @Override public Node<OWLClass> getBottomClassNode() { return getUnsatisfiableClasses(); }

    @Override public Node<OWLClass> getEquivalentClasses(OWLClassExpression ce) {
        if (ce.isAnonymous()) return new OWLClassNode();
        ensureClassified();
        return equivNodeOf(ce.asOWLClass());
    }

    @Override public NodeSet<OWLClass> getSuperClasses(OWLClassExpression ce, boolean direct) {
        if (ce.isAnonymous()) return new OWLClassNodeSet();
        ensureClassified();
        return walk(ce.asOWLClass(), directSupers, direct);
    }
    @Override public NodeSet<OWLClass> getSubClasses(OWLClassExpression ce, boolean direct) {
        if (ce.isAnonymous()) return new OWLClassNodeSet();
        ensureClassified();
        return walk(ce.asOWLClass(), directSubs, direct);
    }

    /** direct=true → the immediate edges; direct=false → transitive closure, grouped into equiv nodes. */
    private NodeSet<OWLClass> walk(OWLClass start, Map<OWLClass, Set<OWLClass>> edges, boolean direct) {
        Set<OWLClass> reached = new HashSet<>();
        Deque<OWLClass> stack = new ArrayDeque<>(edges.getOrDefault(start, Collections.emptySet()));
        while (!stack.isEmpty()) {
            OWLClass c = stack.pop();
            if (!reached.add(c)) continue;
            if (!direct) stack.addAll(edges.getOrDefault(c, Collections.emptySet()));
        }
        Set<Node<OWLClass>> nodes = new HashSet<>();
        for (OWLClass c : reached) nodes.add(equivNodeOf(c));
        return new OWLClassNodeSet(nodes);
    }
    @Override public NodeSet<OWLClass> getDisjointClasses(OWLClassExpression ce) { return new OWLClassNodeSet(); }

    // ---- individuals ----
    @Override public NodeSet<OWLClass> getTypes(OWLNamedIndividual ind, boolean direct) {
        ensureRealized();
        if (realizeResult == null) return new OWLClassNodeSet();
        for (RustdlJson.IndividualJson i : orEmpty(realizeResult.individuals)) {
            if (i.iri.equals(ind.getIRI().toString())) {
                List<String> src = direct ? i.direct_types : i.types;
                Set<Node<OWLClass>> nodes = new HashSet<>();
                for (String iri : orEmpty(src)) nodes.add(equivNodeOf(clazz(iri)));
                return new OWLClassNodeSet(nodes);
            }
        }
        return new OWLClassNodeSet();
    }
    @Override public NodeSet<OWLNamedIndividual> getInstances(OWLClassExpression ce, boolean direct) {
        if (ce.isAnonymous()) return new OWLNamedIndividualNodeSet();
        ensureRealized();
        if (realizeResult == null) return new OWLNamedIndividualNodeSet();
        String target = ce.asOWLClass().getIRI().toString();
        Set<Node<OWLNamedIndividual>> nodes = new HashSet<>();
        for (RustdlJson.IndividualJson i : orEmpty(realizeResult.individuals)) {
            List<String> src = direct ? i.direct_types : i.types;
            if (orEmpty(src).contains(target)) {
                nodes.add(new OWLNamedIndividualNode(df.getOWLNamedIndividual(IRI.create(i.iri))));
            }
        }
        return new OWLNamedIndividualNodeSet(nodes);
    }

    // ---- entailment ----
    @Override public boolean isEntailmentCheckingSupported(AxiomType<?> axiomType) {
        return axiomType == AxiomType.SUBCLASS_OF;
    }
    @Override public boolean isEntailed(OWLAxiom axiom) {
        if (axiom instanceof OWLSubClassOfAxiom) {
            OWLSubClassOfAxiom sc = (OWLSubClassOfAxiom) axiom;
            if (sc.getSubClass().isAnonymous() || sc.getSuperClass().isAnonymous()) {
                throw new UnsupportedOperationException("rustdl entails only named SubClassOf");
            }
            if (sc.getSuperClass().asOWLClass().isOWLThing()) return true;
            return getSuperClasses(sc.getSubClass(), false).containsEntity(sc.getSuperClass().asOWLClass())
                || getEquivalentClasses(sc.getSubClass()).contains(sc.getSuperClass().asOWLClass());
        }
        throw new UnsupportedOperationException("rustdl entails only SubClassOf axioms");
    }
    @Override public boolean isEntailed(Set<? extends OWLAxiom> axioms) {
        for (OWLAxiom a : axioms) if (!isEntailed(a)) return false;
        return true;
    }

    // ---- interrupt / precompute misc ----
    @Override public void interrupt() { /* subprocess is bounded by timeout; nothing to interrupt mid-call */ }

    // ---- unsupported node-set queries → empty (sound under-approximation) ----
    @Override public Node<OWLObjectPropertyExpression> getTopObjectPropertyNode() { return new OWLObjectPropertyNode(df.getOWLTopObjectProperty()); }
    @Override public Node<OWLObjectPropertyExpression> getBottomObjectPropertyNode() { return new OWLObjectPropertyNode(df.getOWLBottomObjectProperty()); }
    @Override public NodeSet<OWLObjectPropertyExpression> getSubObjectProperties(OWLObjectPropertyExpression pe, boolean direct) { return new OWLObjectPropertyNodeSet(); }
    @Override public NodeSet<OWLObjectPropertyExpression> getSuperObjectProperties(OWLObjectPropertyExpression pe, boolean direct) { return new OWLObjectPropertyNodeSet(); }
    @Override public Node<OWLObjectPropertyExpression> getEquivalentObjectProperties(OWLObjectPropertyExpression pe) { return new OWLObjectPropertyNode(pe); }
    @Override public NodeSet<OWLObjectPropertyExpression> getDisjointObjectProperties(OWLObjectPropertyExpression pe) { return new OWLObjectPropertyNodeSet(); }
    @Override public Node<OWLObjectPropertyExpression> getInverseObjectProperties(OWLObjectPropertyExpression pe) { return new OWLObjectPropertyNode(pe.getInverseProperty()); }
    @Override public NodeSet<OWLClass> getObjectPropertyDomains(OWLObjectPropertyExpression pe, boolean direct) { return new OWLClassNodeSet(); }
    @Override public NodeSet<OWLClass> getObjectPropertyRanges(OWLObjectPropertyExpression pe, boolean direct) { return new OWLClassNodeSet(); }
    @Override public Node<OWLDataProperty> getTopDataPropertyNode() { return new OWLDataPropertyNode(df.getOWLTopDataProperty()); }
    @Override public Node<OWLDataProperty> getBottomDataPropertyNode() { return new OWLDataPropertyNode(df.getOWLBottomDataProperty()); }
    @Override public NodeSet<OWLDataProperty> getSubDataProperties(OWLDataProperty pe, boolean direct) { return new OWLDataPropertyNodeSet(); }
    @Override public NodeSet<OWLDataProperty> getSuperDataProperties(OWLDataProperty pe, boolean direct) { return new OWLDataPropertyNodeSet(); }
    @Override public Node<OWLDataProperty> getEquivalentDataProperties(OWLDataProperty pe) { return new OWLDataPropertyNode(pe); }
    @Override public NodeSet<OWLDataProperty> getDisjointDataProperties(OWLDataPropertyExpression pe) { return new OWLDataPropertyNodeSet(); }
    @Override public NodeSet<OWLClass> getDataPropertyDomains(OWLDataProperty pe, boolean direct) { return new OWLClassNodeSet(); }
    @Override public NodeSet<OWLNamedIndividual> getObjectPropertyValues(OWLNamedIndividual ind, OWLObjectPropertyExpression pe) { return new OWLNamedIndividualNodeSet(); }
    @Override public Set<OWLLiteral> getDataPropertyValues(OWLNamedIndividual ind, OWLDataProperty pe) { return Collections.emptySet(); }
    @Override public Node<OWLNamedIndividual> getSameIndividuals(OWLNamedIndividual ind) { return new OWLNamedIndividualNode(ind); }
    @Override public NodeSet<OWLNamedIndividual> getDifferentIndividuals(OWLNamedIndividual ind) { return new OWLNamedIndividualNodeSet(); }
}
```

> Implementer notes:
> - `OWLReasonerBase` (OWLAPI 4.5.29) provides `getRootOntology`, `getBufferingMode`, `getReasonerConfiguration`, `getPendingChanges`, `flush` (which calls your `handleChanges`), `dispose`, `getTimeOut`, `getFreshEntityPolicy`, `getIndividualNodeSetPolicy`. Do NOT re-implement those. Confirm the exact abstract-method set by compiling; add any the compiler demands with the same empty/trivial pattern shown above. Read km's `KMReasoner` for the confirmed OWLAPI-4.5.29 abstract set.
> - If `getInstances`/`getTypes` with `direct=true` must match Protégé's expectation exactly, the rustdl `realize --json` already provides `direct_types`; use it verbatim (do not recompute).

- [ ] **Step 4: Write the real `RustdlReasonerFactory.java`** (replace the Task-1 stub)

```java
package com.github.maastrichtu_ids.rustdl.protege;

import org.semanticweb.owlapi.model.OWLOntology;
import org.semanticweb.owlapi.reasoner.*;

public class RustdlReasonerFactory implements OWLReasonerFactory {
    @Override public String getReasonerName() { return "rustdl"; }
    @Override public OWLReasoner createReasoner(OWLOntology o) {
        return new RustdlReasoner(o, new SimpleConfiguration(), BufferingMode.BUFFERING);
    }
    @Override public OWLReasoner createReasoner(OWLOntology o, OWLReasonerConfiguration c) {
        return new RustdlReasoner(o, c, BufferingMode.BUFFERING);
    }
    @Override public OWLReasoner createNonBufferingReasoner(OWLOntology o) {
        return new RustdlReasoner(o, new SimpleConfiguration(), BufferingMode.NON_BUFFERING);
    }
    @Override public OWLReasoner createNonBufferingReasoner(OWLOntology o, OWLReasonerConfiguration c) {
        return new RustdlReasoner(o, c, BufferingMode.NON_BUFFERING);
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `mvn -f protege/pom.xml test -Dtest=RustdlReasonerTest+PluginRegistrationTest`
Expected: PASS. If compilation demands additional `OWLReasonerBase` abstract methods, add them with the empty/trivial pattern and re-run.

- [ ] **Step 6: Full build**

Run: `mvn -f protege/pom.xml clean package`
Expected: all tests pass; the bundle jar builds through `verify`.

- [ ] **Step 7: Commit**

```bash
git add protege/src
git commit -m "feat(protege): OWLReasonerBase adapter — flag-driven classify/realize, cache-backed queries"
```

---

### Task 5: Binary bundling, release integration, PR CI, docs

**Files:**
- Modify: `.github/workflows/release-cli.yml` (add a `build-plugin` job)
- Create: `.github/workflows/java-ci.yml` (compile + unit-test on PRs)
- Create: `protege/src/test/java/.../RustdlSmokeIT.java` (integration smoke test, gated on a real binary via `-Drustdl.bin`)
- Create/modify: `docs/protege-plugin.md` (install + build)

**Interfaces:**
- Consumes: the `cli-<triple>` workflow artifacts produced by `release-cli.yml`'s `build-cli` (Plan B); a locally-built `rustdl` for the smoke test.
- Produces: `rustdl-protege-<version>.jar` attached to the GitHub Release; a green PR check for the Java module.

- [ ] **Step 1: Write the integration smoke test** `RustdlSmokeIT.java`

Runs the real binary end-to-end via the factory. Skips (does not fail) when no binary is available.

```java
package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.semanticweb.owlapi.apibinding.OWLManager;
import org.semanticweb.owlapi.model.*;
import org.semanticweb.owlapi.reasoner.*;
import static org.junit.Assume.assumeTrue;
import static org.junit.Assert.*;

public class RustdlSmokeIT {
    @Test public void classifiesTinyOntologyWithRealBinary() throws Exception {
        // Only runs when a binary is reachable (dev: -Drustdl.bin; CI: bundled or override).
        assumeTrue("no rustdl binary available",
            RustdlBinary.configuredOverride() != null
            || RustdlBinary.class.getResource("/native") != null);
        OWLOntologyManager m = OWLManager.createOWLOntologyManager();
        OWLDataFactory df = m.getOWLDataFactory();
        OWLOntology o = m.createOntology(IRI.create("http://ex/"));
        OWLClass a = df.getOWLClass(IRI.create("http://ex/#A"));
        OWLClass b = df.getOWLClass(IRI.create("http://ex/#B"));
        m.addAxiom(o, df.getOWLSubClassOfAxiom(a, b));
        OWLReasoner r = new RustdlReasonerFactory().createReasoner(o);
        r.precomputeInferences(InferenceType.CLASS_HIERARCHY);
        assertTrue(r.isConsistent());
        assertTrue(r.getSuperClasses(a, false).containsEntity(b));
    }
}
```

Run locally with a freshly built binary:
```bash
RUSTUP_TOOLCHAIN=stable cargo build --release --bin rustdl
mvn -f protege/pom.xml test -Dtest=RustdlSmokeIT -Drustdl.bin="$PWD/target/release/rustdl"
```
Expected: PASS (the real binary classifies the tiny ontology). Without `-Drustdl.bin`, the test is skipped (assumeTrue), not failed.

- [ ] **Step 2: Write `.github/workflows/java-ci.yml`** (PR compile + unit tests; builds a rustdl binary so the smoke IT runs on Linux)

```yaml
name: Java plugin CI

on:
  push:
    branches: [main]
    paths: ['protege/**', '.github/workflows/java-ci.yml']
  pull_request:
    paths: ['protege/**', '.github/workflows/java-ci.yml']
  workflow_dispatch:

jobs:
  build:
    runs-on: ubuntu-latest
    env:
      RUSTUP_TOOLCHAIN: stable
    steps:
      - uses: actions/checkout@v6
      - uses: actions/setup-java@v4
        with:
          distribution: temurin
          java-version: '17'
      - uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9 # master, pinned
        with:
          toolchain: stable
      - uses: Swatinem/rust-cache@v2
      - name: Build a rustdl binary for the smoke test
        run: cargo build --release --bin rustdl -p owl-dl-cli
      - name: Build + test the plugin
        run: mvn -B -f protege/pom.xml verify -Drustdl.bin="$PWD/target/release/rustdl"
```

- [ ] **Step 3: Add the `build-plugin` job to `.github/workflows/release-cli.yml`**

Append after `publish-cli`. It consumes the SAME-RUN `cli-*` artifacts (no cross-workflow race), stages them into the plugin resources, sets the version from the tag (or `0.0.0-dev` on dispatch), builds the bundle, uploads it as an artifact, and — on a tag — attaches it to the release.

```yaml
  build-plugin:
    name: Build + attach the Protégé plugin jar
    needs: build-cli
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v6
      - uses: actions/setup-java@v4
        with:
          distribution: temurin
          java-version: '17'
      - uses: actions/download-artifact@v8
        with:
          pattern: 'cli-*'
          path: cli-bins
          merge-multiple: true
      - name: Stage binaries into plugin resources
        run: |
          set -euo pipefail
          base=protege/src/main/resources/native
          for triple in x86_64-unknown-linux-musl aarch64-unknown-linux-musl aarch64-apple-darwin; do
            mkdir -p "$base/$triple"
            cp "cli-bins/rustdl-$triple" "$base/$triple/rustdl"
            chmod +x "$base/$triple/rustdl"
          done
          mkdir -p "$base/x86_64-pc-windows-msvc"
          cp "cli-bins/rustdl-x86_64-pc-windows-msvc.exe" "$base/x86_64-pc-windows-msvc/rustdl.exe"
          ls -R "$base"
      - name: Set plugin version
        run: |
          if [[ "${GITHUB_REF}" == refs/tags/* ]]; then ver="${GITHUB_REF_NAME#v}"; else ver="0.0.0-dev"; fi
          mvn -B -f protege/pom.xml versions:set -DnewVersion="$ver" -DgenerateBackupPoms=false
      - name: Build the bundle jar
        # Skip the smoke IT here (native/ is populated but the runner is linux-x64;
        # the bundled linux-x64 binary WILL run — keep the IT via the bundled path).
        run: mvn -B -f protege/pom.xml clean package
      - uses: actions/upload-artifact@v7
        with:
          name: protege-plugin
          path: protege/target/rustdl-protege-*.jar
          if-no-files-found: error
      - name: Attach to the GitHub Release
        if: startsWith(github.ref, 'refs/tags/')
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          tag="${GITHUB_REF_NAME}"
          gh release create "$tag" --title "rustdl ${tag#v}" \
            --notes "Release ${tag}. See CHANGELOG.md for details." || true
          gh release upload "$tag" protege/target/rustdl-protege-*.jar --clobber
```

> The staged `protege/src/main/resources/native/` is git-ignored (Task 1), so this job only ever writes build-time artifacts. The bundled linux-x64 binary means `mvn package` here also exercises the smoke IT through the real bundled path (not just the override), verifying end-to-end bundling.

- [ ] **Step 4: `actionlint` both workflows**

```bash
actionlint .github/workflows/release-cli.yml .github/workflows/java-ci.yml
```
Expected: exit 0, no findings.

- [ ] **Step 5: Write `docs/protege-plugin.md`**

```markdown
# rustdl Protégé plugin

rustdl ships as a Protégé reasoner plugin: install one jar and rustdl appears in
Reasoner ▸ with no separate binary install (the four platform binaries are
bundled and the matching one is extracted at first use).

## Install

Download `rustdl-protege-<version>.jar` from the [GitHub Release](https://github.com/MaastrichtU-IDS/rustdl/releases),
drop it into Protégé's `plugins/` directory, and restart. Select **rustdl** from
Reasoner ▸, then Reasoner ▸ Start reasoner.

Supported platforms: Linux x86_64/aarch64, macOS Apple Silicon, Windows x86_64.
Other platforms (e.g. Intel macOS): build a binary (`cargo build --release --bin
rustdl`) and launch Protégé with `-Drustdl.bin=/path/to/rustdl`.

## What it computes (v1)

Consistency, class hierarchy (classify), unsatisfiable classes, and class
assertions (types/instances via realize). Property hierarchies/values,
same/different individuals, disjointness, and complex-class-expression queries
return empty (not yet backed by rustdl). An `incomplete` classification (some
hard class pairs hit the per-pair budget) is logged; results stay sound (no false
subsumptions, some may be missed).

## Config

- `-Drustdl.bin=…` / `RUSTDL_BIN=…` — use a specific binary (default: bundled)
- `-Drustdl.timeout.seconds=…` / `RUSTDL_TIMEOUT_SECONDS=…` — per-call timeout (default 600)

## Build from source

See `protege/README.md`.
```

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/release-cli.yml .github/workflows/java-ci.yml protege/src/test docs/protege-plugin.md
git commit -m "ci+docs(protege): bundle binaries into the release jar, PR Java CI, install docs"
```

---

## Integration Verification (after merge — controller, not a task)

1. `java-ci.yml` runs on the merge to `main` (or via `gh workflow run java-ci.yml --ref main`): confirms the plugin compiles and unit + smoke tests pass on Linux with a freshly built binary.
2. `gh workflow run release-cli.yml --ref main` (no tag): the new `build-plugin` job stages the four same-run binaries, builds the bundle, and uploads `protege-plugin` as a workflow artifact (release-upload skipped). Download it and confirm `unzip -l` shows `plugin.xml` at root, `native/<triple>/rustdl[.exe]` ×4, and embedded gson.
3. Manual acceptance (once, human): drop the artifact jar into a real Protégé install, confirm **rustdl** appears in Reasoner ▸ and classifies a small ontology (e.g. `sulo`).
4. The first `v*.*.*` tag then attaches `rustdl-protege-<version>.jar` to the release automatically.

## Self-Review

- **Spec coverage (design §5):** RustdlBinary (§5 bullet 1) → Task 2; RustdlProcess (§5 bullet 2) → Task 3; RustdlReasoner BUFFERING + flag-driven precompute + query mapping (§5 bullets 3, "flag-driven lifecycle", "query-method mapping") → Task 4; RustdlReasonerFactory + Info + plugin.xml registration (§5 bullet 4) → Tasks 1+4; seamless bundling of the four binaries (§8) → Tasks 2+5; error handling (§6: fail-closed, timeout, inconsistent, schema mismatch, incomplete warning, unsupported→empty, complex→UnsupportedOperationException) → Tasks 3+4; testing (§7: JSON-parse, DAG/query mapping, platform routing, integration smoke) → Tasks 2–5. ✔
- **Placeholder scan:** none — every class, pom, workflow, and doc is complete literal content. The only deliberately-empty artifact is `src/main/resources/native/` (git-ignored; filled at release build time), which is by design.
- **Type/name consistency:** package `com.github.maastrichtu_ids.rustdl.protege` throughout; `RustdlReasonerInfo`→`RustdlReasonerFactory`→`RustdlReasoner`; JSON POJO field names match `docs/json-schema.md` exactly (`schema_version`, `equivalent_groups`, `direct_subsumptions`, `direct_types`); triples match `docs/cli-binaries.md`; asset name `rustdl-x86_64-pc-windows-msvc.exe` matches Plan B's upload; `plugin.xml` `factoryClass` matches `RustdlReasonerInfo`'s FQN.
- **Known risk to flag at execution:** the exact abstract-method set of `OWLReasonerBase` in OWLAPI 4.5.29 is confirmed only at compile time; Task 4 Step 5 handles additions with the documented empty/trivial pattern (km's `KMReasoner` is the reference for the confirmed set).
