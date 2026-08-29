#!/usr/bin/env bash
# Inertness: on ontologies where rustdl is believed complete, verify-el must
# return Verified. Population is ORE, NOT the curated corpus: only 1 of 15
# curated files is pure-EL (go-basic.ofn) and at 51,967 classes it exceeds
# max_elements, so a curated-corpus sweep produces ZERO Verified verdicts and
# passes on an empty set.
set -uo pipefail
POOL=${POOL:-/data/dumontier/ore-run/pool_sample/files}
BIN=${BIN:-./target/release/rustdl}
for o in 13204 3263 11274 4918 2672 10742 2022 3102 5115 4570 \
         3919 13752 12161 4733 16114 5487 13902 11739 16687 14879; do
  f="$POOL/ore_ont_$o.owl"
  [ -f "$f" ] || { echo "MISSING ore_ont_$o"; continue; }
  RAYON_NUM_THREADS=1 timeout 300 "$BIN" verify-el "$f" >/dev/null 2>&1
  printf "ore_ont_%-8s exit=%s\n" "$o" "$?"
done
