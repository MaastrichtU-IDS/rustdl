# OWLAPI-bench

Multi-ontology classify harness for HermiT + ELK, designed to amortize
JVM and OWLAPI startup across many measurements so the wall-clock
delta isolates reasoning cost rather than process startup.

## How to run

The harness compiles inside a JDK image, pulling the OWLAPI / HermiT /
ELK classpath out of `robot.jar` (which ROBOT bundles all of them).

```bash
# Extract robot.jar from the ROBOT image once.
mkdir -p /tmp/owlapi-bench
docker run --rm -v /tmp/owlapi-bench:/out obolibrary/robot:v1.9.6 \
    cp /tools/robot.jar /out/robot.jar
cp docker/owlapi-bench/Bench.java /tmp/owlapi-bench/

# Drop your .ofn ontologies into /tmp/owlapi-bench/ then run:
docker run --rm -v /tmp/owlapi-bench:/work -w /work eclipse-temurin:17-jdk bash -c '
    javac -cp robot.jar Bench.java &&
    java -cp .:robot.jar Bench file1.ofn file2.ofn ...
'
```

Prints per-(reasoner, file) mean / min / max over 5 measured
iterations (after one warmup) of just the
`precomputeInferences(CLASS_HIERARCHY)` call. ELK's `getReasonerName()`
returns null in the version bundled with current ROBOT, so it shows
up as `null` in the output — it's still ELK; we use
`org.semanticweb.elk.owlapi.ElkReasonerFactory` directly.

## What this is for

Fair comparison against rustdl. Use `owl-dl-bench compare-whelk` for
the in-process Rust vs Rust comparison; use this harness for
Rust vs JVM. JVM startup is paid once via the warmup iteration plus
the static cost of one `java` invocation, so the per-call number is
a meaningful estimate of reasoning time.
