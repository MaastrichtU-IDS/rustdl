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
# Output: a single line, either "sat" or "unsat" on stdout. ROBOT
# diagnostics on stderr. Exit 0 on success, non-zero only if ROBOT
# could not run at all (image missing, file unreadable, ...).
#
# How it works
# ============
#
# ROBOT's `reason` subcommand runs HermiT (or other reasoners via
# `--reasoner`). When it finds unsatisfiable classes it logs an
# explicit line on stderr:
#
#     ERROR ... There are N unsatisfiable classes in the ontology.
#     ERROR ...     unsatisfiable: <iri>
#
# and exits non-zero. (The log lines come out on **stdout**, not
# stderr, despite the ERROR tag — Java logback config quirk.) We
# capture both streams and search for our test IRI. No unsatisfiable
# line ⇒ satisfiable.
#
# Pinning
# =======
#
# Defaults to `obolibrary/robot:v1.9.6`; override with the ROBOT_IMAGE
# env var. Image is published at
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

# Capture both streams; ROBOT may exit non-zero when classes are
# unsat, so don't let that abort the script.
LOG="$TMPDIR/robot.log"
set +e
docker run --rm \
    -v "$TMPDIR:/work" \
    -w /work \
    "$ROBOT_IMAGE" \
    robot reason \
        --reasoner hermit \
        --input in.ofn \
        --output out.ofn \
        >"$LOG" 2>&1
ROBOT_EXIT=$?
set -e

# Replay ROBOT's log so callers can see it.
cat "$LOG" >&2

# If our test IRI shows up in an "unsatisfiable: <iri>" line, that's
# the verdict.
if grep -qE "unsatisfiable:[[:space:]]+${TEST_IRI}([[:space:]]|$)" "$LOG"; then
    echo "unsat"
    exit 0
fi

# Ontology-level inconsistency: every class is unsatisfiable (no
# model exists). ROBOT prints "The ontology is inconsistent" and
# exits non-zero without listing individual classes.
if grep -qE "The ontology is inconsistent" "$LOG"; then
    echo "unsat"
    exit 0
fi

if [[ "$ROBOT_EXIT" -ne 0 ]]; then
    echo "robot reason failed (exit $ROBOT_EXIT) without flagging $TEST_IRI" >&2
    exit "$ROBOT_EXIT"
fi

echo "sat"
