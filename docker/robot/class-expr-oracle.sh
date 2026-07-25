#!/usr/bin/env bash
# Produce a reference materialization of inferred SubClassOf / ClassAssertion
# axioms using ROBOT's embedded HermiT (sound + complete), in the OWL/XML
# shape the complex-class-expression oracle harness consumes
# (crates/owl-dl-reasoner/tests/class_expr_oracle.rs).
#
# Usage: class-expr-oracle.sh <input.ofn> <output.owx>
#
# This is the complex-class-expression analog of classify-oracle.sh /
# disjoint-oracle.sh: rustdl's `class_expression_*` queries (issue #48) reduce
# an anonymous class expression to a fresh named PROBE class via
# `EquivalentClasses(<probe> CE)`, so the oracle input must already contain
# that probe axiom (the caller appends it to the fixture before invoking this
# script — see the regenerate command in class_expr_oracle.rs). The output
# then contains HermiT-inferred `SubClassOf(<probe> ...)` (entailment check)
# and `ClassAssertion(<probe> <ind>)` (instances check) axioms over the probe.
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

# `reason` runs HermiT and adds inferred axioms; ask for the SubClass +
# ClassAssertion generators and emit OWL/XML so horned-owl's read_owx() can
# parse it. `--include-indirect true` is required: ROBOT's default
# InferredClassAssertionAxiomGenerator/InferredSubClassAxiomGenerator only
# assert the MOST SPECIFIC (direct) type/superclass per individual/class, so
# without it a probe class equivalent to a non-most-specific union (e.g. `x`'s
# direct type `A` is more specific than the probe `A⊔B`) never gets a
# `ClassAssertion(probe, x)` axiom even though it is fully entailed. ROBOT
# exits non-zero on an inconsistent ontology — surface that rather than
# silently producing nothing.
docker run --rm \
    -v "$TMPDIR:/work" \
    -w /work \
    "$ROBOT_IMAGE" \
    robot reason \
        --reasoner hermit \
        --axiom-generators "SubClass ClassAssertion" \
        --include-indirect true \
        --input in.ofn \
        --output out.owx

cp "$TMPDIR/out.owx" "$OUTPUT"
echo "wrote $OUTPUT" >&2
