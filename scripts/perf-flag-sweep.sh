#!/usr/bin/env bash
# Canonical perf sweep: time `rustdl classify` flag-OFF vs flag-ON for a single
# env flag, across the SAME canonical corpus every time (docs/corpus.md).
# ABORTS on any unresolved fixture path — a missing fixture must never become a
# silent 0.00 (see memory: evaluate-innovations-full-corpus).
#
# Usage: scripts/perf-flag-sweep.sh RUSTDL_WEDGE_SEMANTIC_BRANCHING
#        (compares <flag>=0 vs <flag>=1)
set -euo pipefail
FLAG="${1:?usage: perf-flag-sweep.sh <ENV_FLAG_NAME>}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/release/rustdl"
CAP="${WALL_CAP_S:-300}"

# Canonical wedge-exercising perf corpus + EL controls (docs/corpus.md).
declare -a CORPUS=(
  "galen:ontologies/external/galen.ofn"
  "go-basic:ontologies/real/go-basic.ofn"
  "alehif:ontologies/external/alehif-test-classified.owx"
  "ore-10908:ontologies/external/ore-10908-sroiq-classified.owx"
  "ore-15672:ontologies/external/ore-15672-shoin-classified.owx"
  "sio:ontologies/real/sio.ofn"
  "pizza:ontologies/real/pizza.ofn"
  "wine:ontologies/real/wine.ofn"
)

# Fail loud on missing paths BEFORE timing anything.
for entry in "${CORPUS[@]}"; do
  p="$ROOT/${entry#*:}"
  [ -f "$p" ] || { echo "ABORT: missing fixture ${entry%%:*} -> $p" >&2; exit 1; }
done

wall() { # flagval path
  /usr/bin/time -f '%e' env "$FLAG=$1" timeout "${CAP}s" "$BIN" classify "$2" >/dev/null 2>/tmp/.psweep || true
  tail -1 /tmp/.psweep
}

printf "%-12s %-12s %-12s %s\n" "fixture" "${FLAG}=0" "${FLAG}=1" "ratio"
for entry in "${CORPUS[@]}"; do
  name="${entry%%:*}"; p="$ROOT/${entry#*:}"
  off="$(wall 0 "$p")"; on="$(wall 1 "$p")"
  ratio="$(awk -v a="$off" -v b="$on" 'BEGIN{ if(a>0) printf "%.2fx", b/a; else print "?" }')"
  printf "%-12s %-12s %-12s %s\n" "$name" "$off" "$on" "$ratio"
done
