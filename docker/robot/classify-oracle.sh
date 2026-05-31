#!/usr/bin/env bash
# Produce a reference classification of an OWL ontology using ROBOT's
# embedded HermiT (sound + complete), in the OWL/XML shape the closure-
# diff harness consumes (crates/owl-dl-reasoner/tests/konclude_closure_diff.rs).
#
# Usage: classify-oracle.sh <input.ofn> <output-classified.owx>
#
# The output contains the asserted axioms plus HermiT-inferred direct
# SubClassOf / EquivalentClasses axioms. The harness takes the transitive
# closure, so direct inferred edges are sufficient.
#
# Pinning: defaults to obolibrary/robot:v1.9.6; override with ROBOT_IMAGE.

set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "Usage: $0 <input.ofn> <output-classified.owx>" >&2
    exit 2
fi

INPUT="$1"
OUTPUT="$2"

if [[ ! -f "$INPUT" ]]; then
    echo "input not found: $INPUT" >&2
    exit 2
fi

# Remove any stale output from a prior run so callers can use file existence
# as a reliable success proxy (Phase 0 batch loops, Tasks 3 and 5).
rm -f "$OUTPUT"

ROBOT_IMAGE="${ROBOT_IMAGE:-obolibrary/robot:v1.9.6}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cp "$INPUT" "$TMPDIR/in.ofn"

# `reason` runs HermiT and adds inferred axioms; we ask explicitly for
# the class hierarchy generators and emit OWL/XML so the harness's
# read_owx() can parse it. ROBOT may exit non-zero if the ontology is
# inconsistent — surface that rather than silently producing nothing.
docker run --rm \
    -v "$TMPDIR:/work" \
    -w /work \
    "$ROBOT_IMAGE" \
    robot reason \
        --reasoner hermit \
        --axiom-generators "subclass EquivalentClass" \
        --input in.ofn \
        --output out.owx

cp "$TMPDIR/out.owx" "$OUTPUT"
echo "wrote $OUTPUT" >&2
