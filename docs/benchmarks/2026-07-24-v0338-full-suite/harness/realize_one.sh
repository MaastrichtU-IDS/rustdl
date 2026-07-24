#!/bin/bash
# Per-ont time + peak-RSS for the v0.3.38 realize perf sweep.
# RAYON=1 (single-thread, comparable to prior sweeps). Output discarded to
# /dev/null but the hierarchy PRINT still runs (part of the classify cost).
# VmHWM poll (survives SIGKILL, unlike `time -v`). 48GB memcap + 120s time cap.
# Prints: ont <TAB> wall_s <TAB> peakRSS_MB <TAB> status  (ok/exitN/TIMEOUT120/MEMCAP48G/crash)
O="$1"; BIN=/data/dumontier/rustdl/target/release/rustdl
CAP_KB=50331648   # 48 GB
base=$(basename "$O" .owl)
t0=$(date +%s)
RAYON_NUM_THREADS=1 "$BIN" realize "$O" >/dev/null 2>&1 &
PID=$!; peak=0; status=ok; secs=0
while kill -0 "$PID" 2>/dev/null; do
  hwm=$(awk '/VmHWM/{print $2}' /proc/$PID/status 2>/dev/null)
  rss=$(awk '/VmRSS/{print $2}' /proc/$PID/status 2>/dev/null)
  [ -n "$hwm" ] && peak=$hwm
  secs=$(( $(date +%s) - t0 ))
  if [ -n "$rss" ] && [ "$rss" -gt "$CAP_KB" ]; then kill -9 "$PID" 2>/dev/null; status=MEMCAP48G; break; fi
  if [ "$secs" -gt 120 ]; then kill -9 "$PID" 2>/dev/null; status=TIMEOUT120; break; fi
  sleep 1
done
wait "$PID" 2>/dev/null; ec=$?
wall=$(( $(date +%s) - t0 ))
if [ "$status" = ok ] && [ "$ec" -ne 0 ]; then status="exit$ec"; fi
printf "%s\t%s\t%s\t%s\n" "$base" "$wall" "$((peak/1024))" "$status"
