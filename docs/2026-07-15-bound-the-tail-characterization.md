# Bound-the-tail characterization: why hard-tail onts "hang"

2026-07-15. Read-only measurement (no engine change), branch `feat/bound-the-tail`.
Question: do pathological ORE onts (e.g. `ore_ont_10080`, 2.6 GB / multi-minute in the
SP2 sweep) HANG past the deadline (a plumbing bug) or terminate-but-incomplete?

## Measured (rustdl classify, --pair-timeout-ms 250)

| ont | config | wall (real) | exit | verdicts | peak RSS |
|---|---|---|---|---|---|
| ore_ont_10019 | `AGGREGATE_DEADLINE_MS=20000` | 20.49 s | 0 (sound-incomplete) | many | — |
| ore_ont_10080 | `AGGREGATE_DEADLINE_MS=20000` | **20.77 s** | 0 (sound-incomplete) | 3855 | 2.8 GB |
| ore_ont_10080 | `AGGREGATE_DEADLINE_MS=8000` (TRACE) | ~8 s | 0 | — | — |
| ore_ont_10080 | **per-pair 250ms ONLY, no aggregate** | **45.00 s (full external cap, unbounded)** | killed | 0 | — |

## Conclusion

- **The aggregate deadline (`RUSTDL_AGGREGATE_DEADLINE_MS`) IS honored** — it bounds even
  `ore_ont_10080` cleanly (terminates, sound-incomplete). Trace shows it checked 3468×.
- **The per-pair timeout (`--pair-timeout-ms`) alone does NOT bound the run** on `ore_ont_10080`
  (ran the full 45 s external cap, 0 verdicts). This is the "hang": a single pathological pair's
  evaluation runs unbounded because the per-pair deadline is not checked in some inner loop
  (matches the prior note: "`classify_top_down_with_timeout`'s per-pair deadline is NOT honored on
  all paths"; the wedge is depth-cap + DIV_WINDOW bounded → returns `Stalled`, so the unbounded
  path is the non-wedge one — the `search.rs` raw-tableau fallback classify falls through to for
  `NoVerdict` pairs).
- The bounding *mechanism already exists* (aggregate deadline); it is **opt-in** (bare classify was
  deliberately kept unbounded-as-before). So "bound-the-tail" is NOT a new interrupt build.
- Separate concern (out of scope here): peak RSS 2.8 GB on `ore_ont_10080` — a memory blowup, not a
  termination issue.

## Fix options (the fork)

- **(ii) Fix the per-pair root [recommended]:** honor `--pair-timeout-ms` in the inner loop that
  runs unbounded (the `search.rs` raw-tableau fallback), so setting the flag actually bounds each
  pair (and the run). **No default-behavior change** — only affects runs that already set the flag;
  bare classify stays unbounded-complete. Kills the hang; principled. Needs pinpointing the exact
  un-checked loop.
- **(i) Default an aggregate bound:** make classify bounded by default (default/derive the aggregate
  deadline). Simple, but changes the deliberately-preserved "unbounded bare classify" behavior — a
  user-facing default-behavior decision.
- (Could do both: fix (ii) + a modest default backstop.)
