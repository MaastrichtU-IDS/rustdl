BIN=/data/dumontier/rustdl/target/release/rustdl
O=/data/dumontier/ore-run/pool_sample/files/ore_ont_9347.owl
CAP_KB=104857600  # 100GB
t0=$(date +%s)
RAYON_NUM_THREADS=1 "$BIN" classify "$O" >/tmp/probe9347.out 2>/tmp/probe9347.err &
PID=$!; peak=0; status=ok
while kill -0 "$PID" 2>/dev/null; do
  hwm=$(awk '/VmHWM/{print $2}' /proc/$PID/status 2>/dev/null)
  rss=$(awk '/VmRSS/{print $2}' /proc/$PID/status 2>/dev/null)
  [ -n "$hwm" ] && peak=$hwm
  secs=$(( $(date +%s) - t0 ))
  [ $((secs % 15)) -eq 0 ] && echo "t=${secs}s rss=$((rss/1024))MB peak=$((peak/1024))MB"
  if [ -n "$rss" ] && [ "$rss" -gt "$CAP_KB" ]; then kill -9 "$PID"; status=MEMCAP100G; break; fi
  if [ "$secs" -gt 360 ]; then kill -9 "$PID"; status=TIMEOUT360; break; fi
  sleep 3
done
wait "$PID" 2>/dev/null; ec=$?
echo "=== DONE status=$status exit=$ec wall=$(($(date +%s)-t0))s peak=$((peak/1024))MB lines=$(wc -l </tmp/probe9347.out)"
