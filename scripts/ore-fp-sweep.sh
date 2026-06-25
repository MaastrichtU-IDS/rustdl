#!/usr/bin/env bash
# Per-ontology, wall-capped ORE FP sweep. One subprocess per ontology so a
# hard SROIQ ontology can be killed without stalling the corpus.
INPUT=/Users/micheldumontier/data/ore-run/input
ORACLE=/Users/micheldumontier/data/ore-run/oracle
TESTBIN="$1"                     # compiled konclude_closure_diff test binary
CAP="${WALL_CAP:-120}"           # per-ontology wall cap (s)
export RUSTDL_TEST_PAIR_MS="${RUSTDL_TEST_PAIR_MS:-200}"
RESULTS=/tmp/ore_fp_results.tsv
: > "$RESULTS"
n=0; fp_total=0; fp_onts=""; killed=0; nooracle=0
for inp in "$INPUT"/*.ofn; do
  stem=$(basename "$inp" .ofn)
  orc="$ORACLE/$stem-classified.owx"
  [ -f "$orc" ] || { nooracle=$((nooracle+1)); continue; }
  n=$((n+1))
  ORE_ONE_INPUT="$inp" ORE_ONE_ORACLE="$orc" \
    "$TESTBIN" --ignored --nocapture --exact ore_one_closure_matches_oracle \
    > /tmp/ore_one.out 2>/dev/null &
  pid=$!
  ( sleep "$CAP"; kill -9 $pid 2>/dev/null ) 2>/dev/null & wpid=$!
  wait $pid 2>/dev/null; rc=$?
  kill -9 $wpid 2>/dev/null
  if [ $rc -eq 137 ] || [ $rc -eq 9 ]; then
    echo -e "$stem\tTIMEOUT(${CAP}s)" >> "$RESULTS"; killed=$((killed+1))
  else
    line=$(grep -m1 '^RESULT' /tmp/ore_one.out)
    if [ -n "$line" ]; then
      echo "$line" | cut -f2- >> "$RESULTS"
      fp=$(echo "$line" | grep -oE 'FP=[0-9]+' | cut -d= -f2)
      if [ "${fp:-0}" -gt 0 ]; then fp_total=$((fp_total+fp)); fp_onts="$fp_onts $stem(FP=$fp)"; fi
    else
      echo -e "$stem\tPANIC/parse-error" >> "$RESULTS"
    fi
  fi
  [ $((n%20)) -eq 0 ] && echo "[progress] $n done, FP_total=$fp_total, timeouts=$killed ($(date +%H:%M:%S))"
done
echo "===================================================="
echo "ORE SROIQ FP sweep complete: $n ontologies diffed"
echo "  no-oracle skipped: $nooracle   timeouts: $killed"
echo "  TOTAL FALSE POSITIVES: $fp_total"
[ -n "$fp_onts" ] && echo "  FP ontologies:$fp_onts"
echo "  results: $RESULTS"
echo "  fragment-level FP counts:"; awk -F'\t' '/FP=/{print}' "$RESULTS" | grep -oE 'FP=[0-9]+' | sort | uniq -c