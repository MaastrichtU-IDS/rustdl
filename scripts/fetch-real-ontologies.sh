#!/usr/bin/env bash
# Fetch the real-world ontology corpus used by the bench harness and
# convert each input into OWL functional syntax (.ofn) so the
# in-process bench binaries — which read OFN only — can consume them.
#
# Files land in `ontologies/real/` (gitignored). Re-running the script
# refreshes both the source download and the converted OFN.
#
# Conversion uses the obolibrary/robot Docker image. Override the tag
# with ROBOT_IMAGE=... if you need to pin a different version.
#
# See `docs/real-ontology-corpus.md` for the source list, sizes, and
# rationale.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$REPO_ROOT/ontologies/real"
ROBOT_IMAGE="${ROBOT_IMAGE:-obolibrary/robot:v1.9.6}"

mkdir -p "$OUT"

# Each entry: <slug>|<source URL>|<downloaded filename>
# The OFN output is always <slug>.ofn.
SOURCES=(
    "sio|https://semanticscience.org/ontology/sio.owl|sio.owl"
    "sulo|https://w3id.org/sulo/sulo.ttl|sulo.ttl"
    "family|https://www.cs.man.ac.uk/~stevensr/ontology/family.rdf.owl|family.rdf.owl"
    "pizza|http://protege.stanford.edu/ontologies/pizza/pizza.owl|pizza.owl"
    "ro|http://purl.obolibrary.org/obo/ro.owl|ro.owl"
    "go-basic|http://purl.obolibrary.org/obo/go/go-basic.obo|go-basic.obo"
)

fetch_one() {
    local slug url src
    slug="${1%%|*}"
    rest="${1#*|}"
    url="${rest%%|*}"
    src="${rest##*|}"

    echo "==> $slug"
    echo "    fetch: $url"
    curl -sS --fail --max-time 300 -L -o "$OUT/$src" "$url"
    local bytes
    bytes=$(stat -c%s "$OUT/$src")
    printf "    saved: ontologies/real/%s (%s bytes)\n" "$src" "$bytes"

    echo "    convert -> $slug.ofn"
    docker run --rm \
        -v "$OUT:/work" -w /work \
        "$ROBOT_IMAGE" \
        robot convert --input "$src" --format ofn --output "$slug.ofn" >/dev/null
    local ofn_bytes
    ofn_bytes=$(stat -c%s "$OUT/$slug.ofn")
    printf "    saved: ontologies/real/%s.ofn (%s bytes)\n" "$slug" "$ofn_bytes"
}

for entry in "${SOURCES[@]}"; do
    fetch_one "$entry"
done

cat <<EOF

Done. Files in $OUT:
$(ls -lh "$OUT")
EOF
