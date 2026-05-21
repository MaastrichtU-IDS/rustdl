#!/usr/bin/env bash
# Run HermiT against an OWL fixture via ROBOT and emit a sat/unsat verdict
# for a named test class.
#
# Usage: oracle.sh <fixture.ofn> <test-class-iri>
#
# Example:
#   docker/robot/oracle.sh \
#     crates/owl-dl-bench/fixtures/02_and_not_a_unsat.ofn \
#     http://rustdl.test/Test
#
# Output: a single line, either "sat" or "unsat" on stdout. Diagnostics on
# stderr. Exit 0 on success, non-zero if ROBOT failed.
#
# How it works
# ============
#
# ROBOT's `reason` subcommand runs HermiT (or other reasoners via
# `--reasoner`) and emits inferred axioms. If `:Test` is unsatisfiable,
# the reasoner will infer `SubClassOf(:Test owl:Nothing)` and ROBOT will
# include that in the output. We grep for it.
#
# Pinning
# =======
#
# The default image tag is `v1.9.6`; override with the ROBOT_IMAGE env
# var. The official image is published at
# https://hub.docker.com/r/obolibrary/robot.

set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "Usage: $0 <fixture.ofn> <test-class-iri>" >&2
    exit 2
fi

FIXTURE="$1"
TEST_IRI="$2"

if [[ ! -f "$FIXTURE" ]]; then
    echo "fixture not found: $FIXTURE" >&2
    exit 2
fi

ROBOT_IMAGE="${ROBOT_IMAGE:-obolibrary/robot:v1.9.6}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cp "$FIXTURE" "$TMPDIR/in.ofn"

# Run ROBOT reason with HermiT. The output is the *full* ontology with
# inferred axioms merged in; the `--axiom-generators` flag restricts
# which inference kinds get emitted. SubClass is what we need for
# unsatisfiability detection: every unsat class becomes a subclass of
# owl:Nothing.
docker run --rm \
    -v "$TMPDIR:/work" \
    -w /work \
    "$ROBOT_IMAGE" \
    robot reason \
        --reasoner hermit \
        --axiom-generators "SubClass EquivalentClass" \
        --input in.ofn \
        --output out.ofn >&2

# Inferred output is OWL Functional Syntax. Look for any line asserting
# the test class is a subclass of or equivalent to owl:Nothing. ROBOT
# emits IRIs in full angle-bracket form in functional syntax output, so
# we match against the full IRI.
NOTHING="<http://www.w3.org/2002/07/owl#Nothing>"
TEST="<${TEST_IRI}>"

if grep -E \
    "SubClassOf\(${TEST} ${NOTHING}\)|EquivalentClasses\(${TEST} ${NOTHING}\)|EquivalentClasses\(${NOTHING} ${TEST}\)" \
    "$TMPDIR/out.ofn" >/dev/null 2>&1; then
    echo "unsat"
else
    echo "sat"
fi
