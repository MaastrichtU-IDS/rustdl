#!/usr/bin/env bash
# Saturation-only fragment pre-pass: label each candidate ontology
# PureEl / Horn / out-of-EL so the FP sweep can target the SROIQ subset.
POOL=/Users/micheldumontier/data/ore-run/pool_sample
RUSTDL=/Users/micheldumontier/code/rustdl/target/release/rustdl
LIST=/tmp/ore_u5.txt
OUT=/tmp/ore_fragments.tsv
: > "$OUT"
n=0
while read -r f; do
  n=$((n+1))
  # per-file watchdog: kill saturation-only after 30s (treat as TIMEOUT)
  "$RUSTDL" classify --saturation-only "$POOL/files/$f" >/tmp/ore_frag_one.txt 2>/dev/null &
  pid=$!
  ( sleep 30; kill -9 $pid 2>/dev/null ) 2>/dev/null &
  wpid=$!
  wait $pid 2>/dev/null; rc=$?
  kill -9 $wpid 2>/dev/null
  if [ $rc -eq 137 ] || [ $rc -eq 9 ]; then
    frag="TIMEOUT"
  else
    frag=$(grep -m1 "# fragment:" /tmp/ore_frag_one.txt | sed 's/# fragment: //' | cut -c1-22)
    [ -z "$frag" ] && frag="ERROR/empty"
  fi
  printf '%s\t%s\n' "$f" "$frag" >> "$OUT"
  [ $((n % 50)) -eq 0 ] && echo "  ...$n/$(wc -l < $LIST) $(date +%H:%M:%S)"
done < "$LIST"
echo "=== fragment distribution ==="
cut -f2 "$OUT" | sort | uniq -c | sort -rn
grep "out-of-EL" "$OUT" | cut -f1 > /tmp/ore_sroiq.txt
echo "=== out-of-EL (SROIQ) selected: $(wc -l < /tmp/ore_sroiq.txt) ==="
