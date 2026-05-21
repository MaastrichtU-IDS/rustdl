# ROBOT + HermiT oracle

This directory wraps the [ROBOT](http://robot.obolibrary.org/) command-line
tool — which embeds the HermiT OWL reasoner — for use as a differential
testing oracle against rustdl.

## What it does

`oracle.sh <fixture.ofn> <test-class-iri>` runs ROBOT inside the
`obolibrary/robot` Docker image, asks HermiT to compute inferred axioms,
and reports whether the named test class is unsatisfiable (subclass of
`owl:Nothing`) or not.

`run-all.sh` iterates every fixture in
[`crates/owl-dl-bench/fixtures/`](../../crates/owl-dl-bench/fixtures/),
calls `oracle.sh` for each, and compares against `manifest.toml`'s
expected verdict.

## Requirements

- Docker (any modern version). Tested with 29.x.
- Network access on first run to pull the ROBOT image (~600 MB).
- No JVM, no ROBOT install on the host — everything runs in the
  container.

## Usage

Single fixture:

```bash
docker/robot/oracle.sh \
    crates/owl-dl-bench/fixtures/02_and_not_a_unsat.ofn \
    http://rustdl.test/Test
# → unsat
```

Whole suite:

```bash
docker/robot/run-all.sh
# 01_atomic_sat.ofn          expected=sat  oracle=sat  OK
# 02_and_not_a_unsat.ofn     expected=unsat oracle=unsat OK
# …
# summary: 10 passed, 0 failed
```

## Pinning

The script defaults to `obolibrary/robot:v1.9.6`. Override with:

```bash
ROBOT_IMAGE=obolibrary/robot:v1.9.5 docker/robot/oracle.sh ...
```

## How it detects unsatisfiability

ROBOT's `reason` subcommand emits inferred axioms. When `:Test` is
unsatisfiable, every reasoner — including HermiT — infers
`SubClassOf(:Test owl:Nothing)`. The oracle script greps the inferred
output for that axiom (and the `EquivalentClasses` variants ROBOT
sometimes emits instead).

This is a coarse but reliable signal for the pure-ALC fixtures here. If
we add fixtures with multiple test classes or more interesting
classification queries later, this script will need to grow.

## Next step

Wire the same fixture loop into rustdl once the
`owl-dl-reasoner` crate exposes a public `is_satisfiable(ontology,
class)` facade. The bench binary will then run both verdicts on every
fixture and exit non-zero if any disagree — closing the differential
testing loop.
