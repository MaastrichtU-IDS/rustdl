#!/usr/bin/env bash
# Build the ORE FP-sweep working set: for each ore_ont stem in $LIST,
#   - copy files/<stem>.owl -> $INPUT/<stem>.ofn   (rustdl reads OWL functional)
#   - ROBOT convert -> .owx, Konclude classify -> $ORACLE/<stem>-classified.owx
# Usage: LIST=/tmp/ore_sroiq.txt bash scripts/ore-oracle-setup.sh
set -uo pipefail
POOL=~/data/ore-run/pool_sample
INPUT=~/data/ore-run/input
ORACLE=~/data/ore-run/oracle
WORK=~/data/ore-run/owx
ROBOT_IMG="${ROBOT_IMAGE:-obolibrary/robot:latest}"
KON_IMG="konclude/konclude:latest"
LIST="${LIST:-/tmp/ore_sroiq.txt}"
mkdir -p "$INPUT" "$ORACLE" "$WORK"

n=0; ok=0; convfail=0; konfail=0
while read stem; do
  stem="${stem%.owl}"
  n=$((n+1))
  src="$POOL/files/$stem.owl"
  [ -f "$src" ] || { echo "MISSING $stem"; continue; }
  cp "$src" "$INPUT/$stem.ofn"
  # already have oracle? skip
  if [ -f "$ORACLE/$stem-classified.owx" ]; then ok=$((ok+1)); continue; fi
  # .owl(functional) -> .owx for Konclude
  if ! docker run --rm -v "$POOL/files:/in" -v "$WORK:/out" -w /in "$ROBOT_IMG" \
        robot convert --input "$stem.owl" --format owx --output "/out/$stem.owx" >/dev/null 2>&1; then
    echo "CONVFAIL $stem"; convfail=$((convfail+1)); continue
  fi
  # Konclude classification -> oracle
  if docker run --rm -v "$WORK:/w" -v "$ORACLE:/o" -w /w "$KON_IMG" \
        classification -w AUTO -i "$stem.owx" -o "/o/$stem-classified.owx" >/dev/null 2>&1 \
        && [ -f "$ORACLE/$stem-classified.owx" ]; then
    ok=$((ok+1))
  else
    echo "KONFAIL $stem"; konfail=$((konfail+1))
  fi
  [ $((n%25)) -eq 0 ] && echo "  ...$n done (ok=$ok convfail=$convfail konfail=$konfail) $(date +%H:%M:%S)"
done < "$LIST"
echo "=== setup done: n=$n ok=$ok convfail=$convfail konfail=$konfail ==="
echo "inputs:  $(ls $INPUT/*.ofn 2>/dev/null | wc -l)"
echo "oracles: $(ls $ORACLE/*-classified.owx 2>/dev/null | wc -l)"
