#!/usr/bin/env bash
# Produce a reference materialization of inferred OBJECT (and data) PROPERTY
# ASSERTIONS over named individuals using ROBOT's embedded HermiT (sound +
# complete), in the OWL/XML shape the materialize-oracle harness consumes
# (crates/owl-dl-reasoner/tests/materialize_oracle.rs).
#
# Usage: property-oracle.sh <input.ofn> <output.owx>
#
# The output contains the asserted axioms plus HermiT-inferred
# `(Object|Data)PropertyAssertion` axioms between NAMED individuals — the same
# scope as `materialize_object_property_assertions` (no anonymous witnesses).
# This is the external ground-truth oracle for `materialize` completeness, the
# property-assertion analog of classify-oracle.sh (which does subclass /
# equivalentclass). Konclude cannot serve here: its interface emits the
# classification hierarchy and individual realization (types), not inferred
# property assertions.
#
# Pinning: defaults to obolibrary/robot:v1.9.6; override with ROBOT_IMAGE.

set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "Usage: $0 <input.ofn> <output.owx>" >&2
    exit 2
fi

INPUT="$1"
OUTPUT="$2"

if [[ ! -f "$INPUT" ]]; then
    echo "input not found: $INPUT" >&2
    exit 2
fi

rm -f "$OUTPUT"

ROBOT_IMAGE="${ROBOT_IMAGE:-obolibrary/robot:v1.9.6}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cp "$INPUT" "$TMPDIR/in.ofn"

# `reason` runs HermiT and adds inferred axioms; ask for the property-assertion
# generator and emit OWL/XML so horned-owl's read_owx() can parse it. HermiT's
# InferredPropertyAssertionGenerator emits assertions between NAMED individuals
# only (matching materialize's scope). ROBOT exits non-zero on an inconsistent
# ontology — surface that rather than silently producing nothing.
docker run --rm \
    -v "$TMPDIR:/work" \
    -w /work \
    "$ROBOT_IMAGE" \
    robot reason \
        --reasoner hermit \
        --axiom-generators "PropertyAssertion" \
        --input in.ofn \
        --output out.owx

cp "$TMPDIR/out.owx" "$OUTPUT"
echo "wrote $OUTPUT" >&2
