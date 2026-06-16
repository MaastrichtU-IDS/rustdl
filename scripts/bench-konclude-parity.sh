#!/usr/bin/env bash
# Standing benchmark harness — Task 0.1 of the Konclude-parity plan.
#
# For each corpus fixture runs:
#   1. rustdl classify  single-threaded (RAYON_NUM_THREADS=1)
#   2. rustdl classify  parallel (all cores)
#   3. native Konclude  via docker (konclude/konclude:latest)
#
# and emits a markdown table with:
#   fixture | #cls | rustdl frag | rustdl wall(1T) | rustdl wall(MT) |
#   Konclude wall | Konclude reason-ms | ratio(1T/Konclude) | timed-out | note
#
# Inputs are .ofn (OWL Functional Syntax).  Fixtures lacking a pre-built
# .owx for Konclude are converted on the fly via ROBOT (obolibrary/robot:v1.9.6).
#
# Usage:
#   scripts/bench-konclude-parity.sh                 # defaults
#   PAIR_MS=200 WALL_CAP=180 scripts/bench-konclude-parity.sh
#   SKIP_KONCLUDE=1 scripts/bench-konclude-parity.sh  # rustdl-only
#
# Environment knobs:
#   PAIR_MS      per-pair tableau timeout in ms  (default 200; wine uses min(PAIR_MS,25))
#   WALL_CAP     outer wall-cap per rustdl run   (default 300 s)
#   SKIP_KONCLUDE  set non-empty to skip Konclude timing (useful on no-docker hosts)
#   REPS         number of timing repetitions    (default 1; median taken if >1)
#
# Notes on fixture coverage:
#   shoiq-knowledge — input .ofn and oracle .owx both absent from repo;
#                     SKIP, noted in the table.
#   pizza           — input present, oracle absent; Konclude is run live and
#                     the output is saved to ontologies/real/konclude-input/
#                     pizza-classified.owx for future use.
#
# The script writes:
#   bench-results/konclude-parity-<STAMP>.md   (markdown table, appended row-by-row)
#   bench-results/konclude-parity-<STAMP>.tsv  (TSV, for downstream processing)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Ensure cargo/rustc are on PATH (handles both login shells and non-login invocations)
if [[ -f "$HOME/.cargo/env" ]]; then
  source "$HOME/.cargo/env" 2>/dev/null || true
fi
_RUSTUP_BIN="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin"
if [[ -d "$_RUSTUP_BIN" ]]; then
  export PATH="$_RUSTUP_BIN:$PATH"
fi
BIN="$REPO_ROOT/target/release/rustdl"
PAIR_MS="${PAIR_MS:-200}"
WALL_CAP="${WALL_CAP:-300}"
REPS="${REPS:-1}"
SKIP_KONCLUDE="${SKIP_KONCLUDE:-}"
STAMP="$(date +%Y%m%d-%H%M%S)"
EXT="$REPO_ROOT/ontologies/external"
REAL="$REPO_ROOT/ontologies/real"
ORACLE_REAL="$REPO_ROOT/ontologies/real/konclude-input"
KONCLUDE_WORK="/tmp/bench-konclude-work-$$"
ROBOT_IMAGE="${ROBOT_IMAGE:-obolibrary/robot:v1.9.6}"

OUT_DIR="$REPO_ROOT/bench-results"
OUT_MD="$OUT_DIR/konclude-parity-${STAMP}.md"
OUT_TSV="$OUT_DIR/konclude-parity-${STAMP}.tsv"

mkdir -p "$OUT_DIR" "$KONCLUDE_WORK" "$ORACLE_REAL"

cleanup() { rm -rf "$KONCLUDE_WORK"; }
trap cleanup EXIT

if [[ ! -x "$BIN" ]]; then
  echo "ERROR: rustdl binary not found at $BIN" >&2
  echo "  Run: cargo build --workspace --release" >&2
  exit 2
fi

# ---------------------------------------------------------------------------
# Helper: time a single rustdl run; echoes "<wall_s> <frag> <timed_out> <classes>"
# ---------------------------------------------------------------------------
time_rustdl() {
  local file="$1"; shift          # ontology .ofn path
  local extra_args=("$@")         # additional flags (e.g. --pair-timeout-ms N)

  local t0 t1 wall_ns wall_s
  local stdout stderr_tmp frag timed_out classes incomplete_flag

  stderr_tmp="$(mktemp)"
  t0=$(date +%s%N)
  local rc=0
  stdout="$(timeout "${WALL_CAP}s" "$BIN" classify "${extra_args[@]}" "$file" 2>"$stderr_tmp")" || rc=$?
  t1=$(date +%s%N)
  rm -f "$stderr_tmp"

  if [[ $rc -eq 124 ]]; then
    echo "DNF DNF 0 0"
    return
  fi

  wall_ns=$(( t1 - t0 ))
  wall_s=$(echo "scale=3; $wall_ns / 1000000000" | bc)

  frag=$(printf '%s\n' "$stdout" | grep '^# fragment:' | sed 's/# fragment: //' | awk '{print $1}' || echo "unknown")
  [[ -z "$frag" ]] && frag="unknown"

  timed_out=$(printf '%s\n' "$stdout" | grep '^# timed-out pairs:' | awk '{print $4}' || echo "0")
  [[ -z "$timed_out" ]] && timed_out="0"

  classes=$(printf '%s\n' "$stdout" | grep '^# classes:' | awk '{print $3}' || echo "?")
  [[ -z "$classes" ]] && classes="?"

  echo "$wall_s $frag $timed_out $classes"
}

# ---------------------------------------------------------------------------
# Helper: median of REPS calls; "DNF" if any DNFs
# ---------------------------------------------------------------------------
median_rustdl() {
  local file="$1"; shift
  local extra_args=("$@")
  local walls=() frag="?" timed_out="?" classes="?"
  local result
  for (( i=0; i<REPS; i++ )); do
    result="$(time_rustdl "$file" "${extra_args[@]}")"
    local w f t c
    read -r w f t c <<< "$result"
    if [[ "$w" == "DNF" ]]; then
      echo "DNF $f $t $c"
      return
    fi
    walls+=("$w")
    frag="$f"; timed_out="$t"; classes="$c"
  done
  # Sort and pick middle
  IFS=$'\n' sorted=($(sort -n <<<"${walls[*]}")); unset IFS
  local med="${sorted[REPS / 2]}"
  echo "$med $frag $timed_out $classes"
}

# ---------------------------------------------------------------------------
# Helper: run Konclude via docker and return "<total_wall_s> <reason_ms>"
# Requires .owx input; if not present converts from .ofn via ROBOT.
# ---------------------------------------------------------------------------
run_konclude() {
  local label="$1"
  local ofn_path="$2"
  local owx_out="$3"    # where Konclude writes its classified output

  if [[ -n "$SKIP_KONCLUDE" ]]; then
    echo "SKIP SKIP"
    return
  fi

  # Convert .ofn → .owx for Konclude input unless we already have one
  local owx_in="$KONCLUDE_WORK/${label}.owx"
  if [[ ! -f "$owx_in" ]]; then
    cp "$ofn_path" "$KONCLUDE_WORK/${label}.ofn"
    docker run --rm \
      -v "$KONCLUDE_WORK:/work" -w /work \
      "$ROBOT_IMAGE" \
      robot convert --input "${label}.ofn" --format owx --output "${label}.owx" \
      >/dev/null 2>&1 || { echo "CONV-FAIL CONV-FAIL"; return; }
  fi

  # Run Konclude
  local kout="$KONCLUDE_WORK/${label}-classified.owx"
  local t0 t1 wall_ns wall_s reason_ms
  t0=$(date +%s%N)
  local kout_log
  kout_log="$(docker run --rm \
    -v "$KONCLUDE_WORK:/work" -w /work \
    konclude/konclude:latest \
    classification -w AUTO -i "${label}.owx" -o "${label}-classified.owx" 2>&1)" || true
  t1=$(date +%s%N)
  wall_ns=$(( t1 - t0 ))
  wall_s=$(echo "scale=3; $wall_ns / 1000000000" | bc)

  reason_ms=$(echo "$kout_log" | grep 'Finished class classification in' | grep -oE '[0-9]+ ms' | grep -oE '[0-9]+' || echo "?")
  [[ -z "$reason_ms" ]] && reason_ms="?"

  # Copy Konclude output to oracle dir if requested
  if [[ -n "$owx_out" && -f "$kout" ]]; then
    cp "$kout" "$owx_out"
  fi

  echo "$wall_s $reason_ms"
}

# ---------------------------------------------------------------------------
# Fixture table:  label | ofn_path | oracle_owx_path | wine_mode | note
# wine_mode=1 means use PAIR_MS=min(25,PAIR_MS) for wine
#
# shoiq-knowledge is noted as SKIP (not in repo).
# ---------------------------------------------------------------------------
declare -a LABELS=(
  galen
  notgalen
  alehif
  ore-10908
  ore-15672
  sio
  wine
  ro
  pizza
  bibtex
)

ofn_path() {
  case "$1" in
    galen)     echo "$EXT/galen.ofn" ;;
    notgalen)  echo "$EXT/notgalen.ofn" ;;
    alehif)    echo "$EXT/alehif-test.ofn" ;;
    ore-10908) echo "$EXT/ore-10908-sroiq.ofn" ;;
    ore-15672) echo "$EXT/ore-15672-shoin.ofn" ;;
    sio)       echo "$REAL/sio.ofn" ;;
    wine)      echo "$REAL/wine.ofn" ;;
    ro)        echo "$REAL/ro.ofn" ;;
    pizza)     echo "$REAL/pizza.ofn" ;;
    bibtex)    echo "$REAL/bibtex.ofn" ;;
    *) echo "" ;;
  esac
}

oracle_path() {
  case "$1" in
    galen)     echo "$EXT/galen-classified.owx" ;;
    notgalen)  echo "$EXT/notgalen-classified.owx" ;;
    alehif)    echo "$EXT/alehif-test-classified.owx" ;;
    ore-10908) echo "$EXT/ore-10908-sroiq-classified.owx" ;;
    ore-15672) echo "$EXT/ore-15672-shoin-classified.owx" ;;
    sio)       echo "$ORACLE_REAL/sio-classified.owx" ;;
    wine)      echo "$ORACLE_REAL/wine-classified.owx" ;;
    ro)        echo "$ORACLE_REAL/ro-classified.owx" ;;
    pizza)     echo "$ORACLE_REAL/pizza-classified.owx" ;;
    bibtex)    echo "$ORACLE_REAL/bibtex-classified.owx" ;;
    *) echo "" ;;
  esac
}

# Wine gets a reduced per-pair timeout to avoid DNF (25ms is documented
# as "7.5x faster, identical hierarchy, MISSED=0 vs HermiT").
wine_pair_ms() {
  local m="$PAIR_MS"
  [[ "$m" -gt 25 ]] && m=25
  echo "$m"
}

# ---------------------------------------------------------------------------
# Write headers (incremental: file is opened once per header, then appended)
# ---------------------------------------------------------------------------
{
  echo "# rustdl vs Konclude — parity benchmark (${STAMP})"
  echo ""
  echo "Host: $(hostname)  CPUs: $(nproc)  Rust: $(rustc --version 2>/dev/null || echo '?')"
  echo "rustdl: $(${BIN} --info 2>&1 | head -1 || echo '?')"
  echo "per-pair timeout: ${PAIR_MS} ms (wine: $(wine_pair_ms) ms)  wall-cap: ${WALL_CAP} s  reps: ${REPS}"
  echo "Konclude: docker konclude/konclude:latest  ROBOT: ${ROBOT_IMAGE}"
  echo ""
  echo "| fixture | #cls | rustdl frag | rustdl 1T | rustdl MT | Konclude wall | Konclude reason-ms | ratio(1T/K) | timed-out | note |"
  echo "|---|---:|---|---:|---:|---:|---:|---:|---:|---|"
} > "$OUT_MD"

printf "fixture\tclasses\tfrag\trustdl_1T_s\trustdl_MT_s\tkonclude_wall_s\tkonclude_reason_ms\tratio_1T\ttimed_out\tnote\n" > "$OUT_TSV"

# ---------------------------------------------------------------------------
# Helper: append one row (called after each fixture so results persist on crash)
# ---------------------------------------------------------------------------
emit_row() {
  local label="$1" classes="$2" frag="$3" wall1="$4" wallmt="$5"
  local kwall="$6" kreason="$7" ratio="$8" timedout="$9" note="${10:-}"

  printf "| %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |\n" \
    "$label" "$classes" "$frag" "${wall1} s" "${wallmt} s" \
    "${kwall}" "${kreason}" "${ratio}" "$timedout" "$note" >> "$OUT_MD"

  printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n" \
    "$label" "$classes" "$frag" "$wall1" "$wallmt" \
    "$kwall" "$kreason" "$ratio" "$timedout" "$note" >> "$OUT_TSV"
}

# ---------------------------------------------------------------------------
# shoiq-knowledge: emit SKIP row immediately
# ---------------------------------------------------------------------------
emit_row "shoiq-knowledge" "?" "?" "SKIP" "SKIP" "SKIP" "SKIP" "-" "-" \
  "SKIP: input .ofn and oracle .owx absent from repo (ORE extract, not committed)"

# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------
for label in "${LABELS[@]}"; do
  ofn="$(ofn_path "$label")"
  oracle="$(oracle_path "$label")"

  if [[ -z "$ofn" || ! -f "$ofn" ]]; then
    echo "--- ${label}: SKIP (input not found: ${ofn})" >&2
    emit_row "$label" "?" "?" "SKIP" "SKIP" "SKIP" "SKIP" "-" "-" "SKIP: input .ofn missing"
    continue
  fi

  echo "=== ${label} ===" >&2

  # Per-pair timeout: wine gets 25 ms max
  local_pair_ms="$PAIR_MS"
  [[ "$label" == "wine" ]] && local_pair_ms="$(wine_pair_ms)"

  # --- rustdl 1T ---
  echo "  rustdl 1T ..." >&2
  read -r wall1 frag timed_out classes < <(
    RAYON_NUM_THREADS=1 median_rustdl "$ofn" --pair-timeout-ms "$local_pair_ms"
  )

  # --- rustdl MT ---
  echo "  rustdl MT ..." >&2
  read -r wallmt frag_mt timed_out_mt classes_mt < <(
    median_rustdl "$ofn" --pair-timeout-ms "$local_pair_ms"
  )

  # If 1T DNFed, take frag/classes from the MT run (which completes).
  # The MT run uses the same budget so its frag and timed_out are valid.
  if [[ "$wall1" == "DNF" ]]; then
    frag="$frag_mt"
    timed_out="$timed_out_mt"
    classes="$classes_mt"
  fi

  # --- Konclude ---
  # pizza: save output to oracle dir so closure-diff can use it
  kout=""
  [[ "$label" == "pizza" ]] && kout="$ORACLE_REAL/pizza-classified.owx"

  echo "  Konclude ..." >&2
  read -r kwall kreason < <(run_konclude "$label" "$ofn" "$kout")

  # --- Ratio ---
  ratio="-"
  if [[ "$wall1" != "DNF" && "$wall1" != "SKIP" && \
        "$kwall" != "SKIP" && "$kwall" != "CONV-FAIL" && "$kwall" != "?" ]]; then
    ratio=$(echo "scale=1; $wall1 / $kwall" | bc 2>/dev/null || echo "?")
    ratio="${ratio}x"
  fi

  # --- Note ---
  note=""
  [[ "$timed_out" != "0" && "$timed_out" != "?" ]] && note="INCOMPLETE(${timed_out} t/o)"
  [[ "$wall1" == "DNF" ]] && note="DNF"
  [[ "$kwall" == "CONV-FAIL" ]] && note="${note:+${note}; }Konclude conversion failed"

  echo "  -> wall1=${wall1}s wallmt=${wallmt}s kwall=${kwall}s reason=${kreason}ms timed-out=${timed_out}" >&2

  emit_row "$label" "$classes" "$frag" "$wall1" "$wallmt" \
    "${kwall} s" "${kreason} ms" "$ratio" "$timed_out" "$note"
done

# ---------------------------------------------------------------------------
# Footer
# ---------------------------------------------------------------------------
{
  echo ""
  echo "## Notes"
  echo ""
  echo "- **rustdl frag** legend: \`pure-EL\` / \`Horn\` = complete (saturation fast path);"
  echo "  \`out-of-EL\` = hybrid (wedge + tableau), may be incomplete if timed-out > 0."
  echo "- **ratio** = rustdl 1T wall / Konclude total wall (native docker ≈ +0.5 s overhead)."
  echo "- **timed-out** = pairs that exceeded the per-pair budget and defaulted to not-subsumed"
  echo "  (sound: FP=0; may MISS real subsumptions if > 0 — see closure-diff for MISSED count)."
  echo "- **shoiq-knowledge**: fixture absent from repo (ORE extract, not committed)."
  echo "- **wine**: per-pair timeout capped at 25 ms (documented MISSED=0 vs oracle, 7.5× faster)."
  echo ""
  echo "Run \`scripts/closure-diff.sh\` for FP/MISSED counts vs the Konclude∩HermiT oracle."
} >> "$OUT_MD"

echo "" >&2
echo "Wrote $OUT_MD" >&2
echo "Wrote $OUT_TSV" >&2
cat "$OUT_MD"
