#!/usr/bin/env bash
# Generate Konclude classification oracles (.owx) for the closure-diff net.
set -uo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$REPO_ROOT/ontologies/real"
ORACLE="$OUT/konclude-input"
ROBOT_IMG="${ROBOT_IMAGE:-obolibrary/robot:latest}"
KON_IMG="konclude/konclude:latest"
mkdir -p "$ORACLE"

for f in sio sulo ro wine; do
  echo "==> $f"
  # back up the HermiT oracle if present
  [[ -f "$ORACLE/$f-classified.owx" && ! -f "$ORACLE/$f-classified.hermit.owx" ]] \
    && cp "$ORACLE/$f-classified.owx" "$ORACLE/$f-classified.hermit.owx"
  # .ofn -> .owx (Konclude input)
  docker run --rm -v "$OUT:/work" -w /work "$ROBOT_IMG" \
    robot convert --input "$f.ofn" --format owx --output "konclude-input/$f.owx" >/dev/null 2>&1 \
    || { echo "   CONVERT FAILED"; continue; }
  # Konclude classification -> oracle .owx
  log="$(docker run --rm -v "$ORACLE:/work" -w /work "$KON_IMG" \
    classification -w AUTO -i "$f.owx" -o "$f-classified.owx" 2>&1)" || true
  if [[ -f "$ORACLE/$f-classified.owx" ]]; then
    echo "   konclude oracle ok ($(grep -oE 'Finished class classification in [0-9]+ ms' <<<"$log" | head -1))"
  else
    echo "   KONCLUDE FAILED:"; echo "$log" | tail -3
  fi
done
echo "=== oracles ==="
ls -la "$ORACLE"/*-classified.owx 2>/dev/null
