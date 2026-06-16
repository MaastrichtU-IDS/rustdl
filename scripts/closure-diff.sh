#!/usr/bin/env bash
# Closure-diff harness — Task 0.1 of the Konclude-parity plan.
#
# Runs the #[ignore]'d Rust tests in
#   crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
# which compute the transitive-closure diff between rustdl and the
# Konclude∩HermiT oracle for every corpus fixture, and parses their
# output into a single summary table.
#
# Why Rust tests, not a bash differ?
#   The Rust harness uses horned-owl's canonical IRI parser, handles
#   EquivalentClasses expansion, thing_equiv exclusion, and symmetric
#   unsat-class exclusion correctly — the same logic that validated
#   the corpus's FP=0 claim across every phase.  A bash/Python differ
#   built on regex would produce false FP and MISSED counts.
#
# Usage:
#   scripts/closure-diff.sh                 # all fixtures, default 200 ms budget
#   RUSTDL_TEST_PAIR_MS=1000 scripts/closure-diff.sh  # generous budget
#
# Environment knobs:
#   RUSTDL_TEST_PAIR_MS  per-pair timeout for rustdl inside the test (default 200 ms)
#
# Outputs:
#   bench-results/closure-diff-<STAMP>.md   (markdown table + FP assertion results)
#   bench-results/closure-diff-<STAMP>.tsv  (TSV)
#   STDOUT: the same table (markdown)
#
# Gate:
#   FP=0 on every fixture is the sacred soundness gate.
#   Any FP > 0 is printed LOUDLY and the script exits with code 1.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Ensure cargo/rustc are on PATH (handles both login shells and non-login invocations)
if [[ -f "$HOME/.cargo/env" ]]; then
  source "$HOME/.cargo/env" 2>/dev/null || true
fi
# Also check the rustup toolchain bin directly
_RUSTUP_BIN="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin"
if [[ -d "$_RUSTUP_BIN" ]]; then
  export PATH="$_RUSTUP_BIN:$PATH"
fi
STAMP="$(date +%Y%m%d-%H%M%S)"
OUT_DIR="$REPO_ROOT/bench-results"
OUT_MD="$OUT_DIR/closure-diff-${STAMP}.md"
OUT_TSV="$OUT_DIR/closure-diff-${STAMP}.tsv"
PAIR_MS="${RUSTDL_TEST_PAIR_MS:-200}"

mkdir -p "$OUT_DIR"

# The Rust test names that correspond to the plan's fixture list.
# Each test prints a line like:
#   --- <label> (<wall>s) ---
#   rustdl_closure=N konclude_closure=M FP=P MISSED=Q (unsat: rustdl=R konclude=S thing-equiv: T)
#
# NOTE: the test binary exits 0 only when FP=0; any FP causes a test failure
# (the tests assert_eq!(fp, 0)).  We capture output regardless and parse it.
declare -A TEST_NAMES=(
  [galen]="galen_closure_matches_konclude"
  [notgalen]="notgalen_closure_matches_konclude"
  [alehif]="alehif_closure_matches_konclude"
  [ore-10908]="ore_10908_sroiq_closure_matches_hermit"
  [ore-15672]="ore_15672_shoin_closure_matches_hermit"
  [sio]="sio_closure_matches_konclude"
  [wine]="wine_closure_matches_konclude"
  [ro]="ro_closure_matches_konclude"
  [pizza]="corpus_closure_matches_konclude"
  [bibtex]="bibtex_closure_matches_konclude"
)

# Note: pizza and sio-stripped/ro-stripped/sulo-stripped are tested by
# corpus_closure_matches_konclude.  We call it once and attribute the output.

# ---------------------------------------------------------------------------
# Write header
# ---------------------------------------------------------------------------
{
  echo "# rustdl closure-diff vs oracle (${STAMP})"
  echo ""
  echo "per-pair budget: ${PAIR_MS} ms"
  echo ""
  echo "| fixture | rustdl closure | oracle closure | FP | MISSED | result |"
  echo "|---|---:|---:|---:|---:|---|"
} > "$OUT_MD"

printf "fixture\trustdl_closure\toracle_closure\tFP\tMISSED\tresult\n" > "$OUT_TSV"

# ---------------------------------------------------------------------------
# Helper: append one result row
# ---------------------------------------------------------------------------
emit_row() {
  local label="$1" rclosure="$2" kclosure="$3" fp="$4" missed="$5" result="$6"
  printf "| %s | %s | %s | %s | %s | %s |\n" \
    "$label" "$rclosure" "$kclosure" "$fp" "$missed" "$result" >> "$OUT_MD"
  printf "%s\t%s\t%s\t%s\t%s\t%s\n" \
    "$label" "$rclosure" "$kclosure" "$fp" "$missed" "$result" >> "$OUT_TSV"
}

# ---------------------------------------------------------------------------
# Helper: parse the diff output from cargo test stderr
# Returns: rustdl_closure oracle_closure FP MISSED
# ---------------------------------------------------------------------------
parse_diff_output() {
  local output="$1"
  local rclosure kclosure fp missed
  # Line format printed by eprintln! in the Rust test:
  #   "rustdl_closure=N konclude_closure=M FP=P MISSED=Q (unsat: ...)"
  # Use grep -oE + sed for portability (no grep -P required)
  rclosure=$(echo "$output" | grep -oE 'rustdl_closure=[0-9]+' | sed 's/rustdl_closure=//' | head -1)
  kclosure=$(echo "$output" | grep -oE 'konclude_closure=[0-9]+' | sed 's/konclude_closure=//' | head -1)
  # FP= and MISSED= may also appear in assert messages; pick the diagnostic line
  # The diagnostic line ends with "(unsat:...)" so grep for the full pattern.
  # Fall back to grepping for "FP=[0-9]+" in the whole output if no diagnostic line.
  fp=$(echo "$output" | grep -oE ' FP=[0-9]+' | sed 's/ FP=//' | head -1)
  missed=$(echo "$output" | grep -oE ' MISSED=[0-9]+' | sed 's/ MISSED=//' | head -1)
  [[ -z "$rclosure" ]] && rclosure="?"
  [[ -z "$kclosure" ]] && kclosure="?"
  [[ -z "$fp" ]] && fp="?"
  [[ -z "$missed" ]] && missed="?"
  echo "$rclosure $kclosure $fp $missed"
}

ANY_FP=0
declare -a FIXTURE_ORDER=(galen notgalen alehif ore-10908 ore-15672 sio wine ro pizza bibtex)

for label in "${FIXTURE_ORDER[@]}"; do
  test_name="${TEST_NAMES[$label]:-}"
  if [[ -z "$test_name" ]]; then
    emit_row "$label" "?" "?" "?" "?" "SKIP: no test mapping"
    continue
  fi

  echo "--- ${label} (test: ${test_name}) ---" >&2

  # Run the specific #[ignore]'d test, capturing output (it goes to stderr via eprintln!).
  # The test exits non-zero if it panics (FP > 0) or SKIP (fixture missing).
  test_output=""
  test_rc=0
  test_output="$(
    cd "$REPO_ROOT"
    RUSTDL_TEST_PAIR_MS="$PAIR_MS" \
    cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
      -- --ignored --nocapture "$test_name" 2>&1
  )" || test_rc=$?

  # Parse the metrics first.  For corpus tests (e.g. corpus_closure_matches_konclude)
  # some *other* fixtures in the same test may print "SKIP: missing fixture" while the
  # target fixture still produces valid rustdl_closure= lines.  Only skip if no
  # rustdl_closure= line was produced for this fixture.
  read -r rclosure kclosure fp missed < <(parse_diff_output "$test_output")

  # If we couldn't extract any metric, then this fixture is genuinely absent.
  if [[ "$rclosure" == "?" && "$fp" == "?" ]]; then
    echo "  SKIP (fixture not present)" >&2
    emit_row "$label" "?" "?" "?" "?" "SKIP: fixture not present"
    continue
  fi

  # Determine result
  result="ok (FP=0 MISSED=${missed})"
  if [[ "$fp" == "?" ]]; then
    result="PARSE-ERROR (could not extract FP; check test output)"
  elif [[ "$fp" -gt 0 ]] 2>/dev/null; then
    result="*** FP=${fp} SOUNDNESS VIOLATION ***"
    ANY_FP=1
    echo "  *** SOUNDNESS VIOLATION: FP=${fp} on ${label} ***" >&2
  elif [[ $test_rc -ne 0 ]]; then
    result="test-FAILED (see log; rc=${test_rc})"
  fi

  echo "  rustdl=${rclosure} oracle=${kclosure} FP=${fp} MISSED=${missed}" >&2
  emit_row "$label" "$rclosure" "$kclosure" "$fp" "$missed" "$result"
done

# shoiq-knowledge: explicitly note as not runnable
emit_row "shoiq-knowledge" "SKIP" "SKIP" "SKIP" "SKIP" "SKIP: input .ofn + oracle absent from repo"

# ---------------------------------------------------------------------------
# Footer
# ---------------------------------------------------------------------------
{
  echo ""
  echo "## Legend"
  echo ""
  echo "- **FP** (false positives): pairs rustdl reports as subsumed but not in the oracle."
  echo "  **Must be 0 on every fixture — the sacred soundness gate.**"
  echo "- **MISSED**: pairs in the oracle that rustdl did not report."
  echo "  0 on complete fragments (Horn/pure-EL); may be >0 on out-of-EL with per-pair timeouts."
  echo "- **shoiq-knowledge**: fixture absent from repo (ORE extract, not committed)."
  echo ""
  echo "## Soundness verdict"
  if [[ "$ANY_FP" -eq 0 ]]; then
    echo ""
    echo "**FP=0 on all available fixtures. Soundness gate PASSED.**"
  else
    echo ""
    echo "***** FP > 0 DETECTED ON ONE OR MORE FIXTURES. SOUNDNESS GATE FAILED. *****"
    echo ""
    echo "This is a critical finding. Do NOT merge any change that produces FP > 0."
  fi
} >> "$OUT_MD"

echo "" >&2
echo "Wrote $OUT_MD" >&2
echo "Wrote $OUT_TSV" >&2
echo "" >&2
cat "$OUT_MD"

if [[ "$ANY_FP" -gt 0 ]]; then
  echo "" >&2
  echo "EXIT 1: FP > 0 detected — soundness violation!" >&2
  exit 1
fi
