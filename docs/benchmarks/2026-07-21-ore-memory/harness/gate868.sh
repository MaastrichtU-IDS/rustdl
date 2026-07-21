#!/bin/bash
# D4 gate: full `rustdl classify` on ore_ont_868 (pure-EL, 981151 classes).
# NOT --saturation-only — emits the hierarchy so direct_subsumers /
# equivalent_classes accessors run end-to-end. 150GB cap, 20min cap.
# Prints a phase timeline (wall marks) + final peak RSS + status + output size.
BIN=/data/dumontier/rustdl/target/release/rustdl
O=/data/dumontier/ore-run/work/sym/ore_ont_868.ofn
OUT=/mnt/um-share-drive/dumontier/rustdl-scratch/gate868.out
CAP_KB=157286400   # 150 GB
t0=$(date +%s)
"$BIN" classify "$O" >"$OUT" 2>/mnt/um-share-drive/dumontier/rustdl-scratch/gate868.err &
PID=$!; peak=0; status=ok; secs=0
while kill -0 "$PID" 2>/dev/null; do
  hwm=$(awk '/VmHWM/{print $2}' /proc/$PID/status 2>/dev/null)
  rss=$(awk '/VmRSS/{print $2}' /proc/$PID/status 2>/dev/null)
  [ -n "$hwm" ] && peak=$hwm
  now=$(date +%s); secs=$((now-t0))
  # log every 10s
  if [ $((secs % 10)) -eq 0 ]; then
    echo "t=${secs}s rss=$((rss/1024))MB peak=$((peak/1024))MB"
  fi
  if [ -n "$rss" ] && [ "$rss" -gt "$CAP_KB" ]; then kill -9 "$PID" 2>/dev/null; status=MEMCAP150G; break; fi
  if [ "$secs" -gt 1200 ]; then kill -9 "$PID" 2>/dev/null; status=TIMEOUT1200; break; fi
  sleep 2
done
wait "$PID" 2>/dev/null; ec=$?
t1=$(date +%s)
if [ "$status" = ok ] && [ "$ec" -ne 0 ]; then status="exit$ec"; fi
echo "=== DONE status=$status exit=$ec wall=$((t1-t0))s peak=$((peak/1024))MB ==="
echo "output lines: $(wc -l <"$OUT" 2>/dev/null)  bytes: $(wc -c <"$OUT" 2>/dev/null)"
echo "stderr tail:"; tail -5 /mnt/um-share-drive/dumontier/rustdl-scratch/gate868.err 2>/dev/null
