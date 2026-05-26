#!/usr/bin/env bash
# Re-runnable comparison of rustdl classify modes across the real
# ontology corpus. Produces a markdown table on stdout and a
# tab-separated file on disk, both timestamped.
#
# Usage:
#   scripts/bench-rustdl-modes.sh            # reasonable defaults
#   REPS=5 PAIR_TIMEOUT_MS=200 scripts/bench-rustdl-modes.sh
#   WALL_CAP_S=600 scripts/bench-rustdl-modes.sh   # outer kill switch
#
# The corpus is taken from ontologies/real/; missing files are
# silently skipped so the script works on partial checkouts.
#
# Modes exercised:
#   default     — rustdl classify (top-down + tableau, unbounded)
#   bounded     — rustdl classify --pair-timeout-ms <PAIR_TIMEOUT_MS>
#   sat-only    — rustdl classify --saturation-only
#
# Honest caveats (mirroring docs/perf-2026-05-24-new-server.md §8):
#   - bounded is a sound under-approximation: pairs whose tableau
#     verdict doesn't finish within PAIR_TIMEOUT_MS default to
#     "not subsumed."
#   - sat-only is a sound under-approximation: skips every tableau
#     probe; subsumptions that need tableau reasoning are missed.
#     On mostly-EL workloads (SIO, SULO) the loss is < 0.2 %.
#   - default may DNF on SROIQ-heavy inputs (pizza without
#     --pair-timeout-ms, etc.); WALL_CAP_S kills runs over the cap.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORPUS="$REPO_ROOT/ontologies/real"
BIN="$REPO_ROOT/target/release/rustdl"
REPS="${REPS:-3}"
PAIR_TIMEOUT_MS="${PAIR_TIMEOUT_MS:-200}"
WALL_CAP_S="${WALL_CAP_S:-300}"
STAMP="$(date +%Y%m%d-%H%M%S)"
OUT_TSV="$REPO_ROOT/bench-results/rustdl-modes-${STAMP}.tsv"
OUT_MD="$REPO_ROOT/bench-results/rustdl-modes-${STAMP}.md"

mkdir -p "$REPO_ROOT/bench-results"

if [[ ! -x "$BIN" ]]; then
  echo "rustdl binary missing: $BIN" >&2
  echo "Run: cargo build --release" >&2
  exit 2
fi

# Single-rep timer: prints wall seconds (3 decimals) on stdout, or
# "DNF" if WALL_CAP_S elapses. Stderr/stdout from the classifier
# are discarded.
time_one() {
  local file="$1"; shift
  local out
  out="$(timeout "${WALL_CAP_S}s" /usr/bin/time -f "%e" "$BIN" classify "$@" "$file" 2>&1 >/dev/null | tail -1)" || true
  if [[ "$out" =~ ^[0-9]+\.[0-9]+$ ]]; then
    echo "$out"
  else
    echo "DNF"
  fi
}

# Median of REPS reps; "DNF" if any rep DNFs (conservative).
median_reps() {
  local file="$1"; shift
  local -a samples=()
  local s
  for ((i = 0; i < REPS; i++)); do
    s="$(time_one "$file" "$@")"
    if [[ "$s" == "DNF" ]]; then
      echo "DNF"
      return
    fi
    samples+=("$s")
  done
  # Sort numerically and pick the middle.
  IFS=$'\n' samples=($(sort -n <<<"${samples[*]}"))
  unset IFS
  echo "${samples[REPS / 2]}"
}

# Ontologies to test, in display order. Comment lines (#) skipped.
ONTOLOGIES=(
  "sulo-stripped"
  "family-stripped"
  "pizza"
  "ro-stripped"
  "sio-stripped"
  "go-basic"
)

# Markdown header.
{
  echo "# rustdl classify modes — ${STAMP}"
  echo
  echo "Reps: ${REPS}  per-pair timeout (bounded): ${PAIR_TIMEOUT_MS} ms  wall cap: ${WALL_CAP_S}s"
  echo "Median of ${REPS} reps; \"DNF\" means at least one rep exceeded the wall cap."
  echo
  echo "| ontology | default (s) | bounded (s) | sat-only (s) |"
  echo "|---|---|---|---|"
} > "$OUT_MD"

# TSV header.
{
  printf "ontology\tdefault\tbounded\tsat-only\n"
} > "$OUT_TSV"

for name in "${ONTOLOGIES[@]}"; do
  file="$CORPUS/$name.ofn"
  if [[ ! -f "$file" ]]; then
    continue
  fi
  echo "Timing $name ..." >&2
  default="$(median_reps "$file")"
  bounded="$(median_reps "$file" --pair-timeout-ms "$PAIR_TIMEOUT_MS")"
  sat="$(median_reps "$file" --saturation-only)"
  printf "| %s | %s | %s | %s |\n" "$name" "$default" "$bounded" "$sat" >> "$OUT_MD"
  printf "%s\t%s\t%s\t%s\n" "$name" "$default" "$bounded" "$sat" >> "$OUT_TSV"
done

cat "$OUT_MD"
echo >&2
echo "Wrote $OUT_MD" >&2
echo "Wrote $OUT_TSV" >&2
