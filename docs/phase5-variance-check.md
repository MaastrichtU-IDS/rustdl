# Phase 5 T3a — variance check: ABORTED (machine contention)

Attempted 2026-06-02. Goal: settle whether the +6.5% GALEN wall regression
documented in `docs/phase2d-2c-redux-results.md` is real or a single-sample
noise artifact (same shape as the Anonymous-349 closure-realization
anomaly), by running GALEN closure-diff 3× at `aab6d03` (pre-Phase-2d
baseline) and 3× at `34a2b62` (post-Phase-2d+2c-redux), via worktree
isolation, standalone.

## Why aborted

The host has had two long-running python processes pinning ~3000% CPU
between them for ~4 days (etime 3-20:37:31). At measurement time:

```
load average: 94.19, 93.84, 85.61
PID       %CPU     ELAPSED     COMMAND
3326299   1470     3-20:37:31  python
3324545   1446     3-20:38:48  python
```

GALEN closure-diff under this contention exceeded the 25-min per-run cap
(`timeout 1500` SIGTERMed run 1) and would not produce a stable wall
reading on any subsequent run. The pre-2d baseline at this commit is
nominally ~13 min standalone; under current load runs would inflate ~2×
and saturate the cap.

Running the remaining 5 iterations under this contention would produce
variance that completely swamps the +6.5% signal we're trying to
characterize. Committing those numbers as "the variance check" would be
worse than no measurement.

## What was learned (incidentally)

- Worktree-based runs need `ontologies/external/` symlinked from the main
  worktree — `git worktree add` doesn't carry gitignored fixtures. Without
  the symlink, `path::exists()` returns false and `galen_closure_matches_konclude`
  exits via its SKIP branch in 20 ms.
- Earlier subagent attempt had the CWD-doesn't-persist-across-Bash-calls
  bug; first run ran via main-worktree's binary (wrong commit), runs 2/3
  hit SKIP. Lesson: single-bash multi-cd patterns must `set` CWD inside
  each subshell or use `--manifest-path`.

## Recommended re-attempt

When the machine is quiet (`uptime` shows load < ~5):

1. Create worktrees + symlink `ontologies` (or skip worktrees and just
   checkout each commit serially on the main tree; CWD-stable and the
   gitignored ontologies survive a `git checkout`).
2. Bump `timeout 1500` to `timeout 2400` (40 min) so a slow-but-completing
   run records a wall time instead of SIGTERMing.
3. Use `--manifest-path` on `cargo test` to avoid CWD-resolution surprises.

## Cross-references

- Phase 5 T2 walltime probe ruled out the saturator as the regression
  source: `docs/phase5-walltime-probe.md` (saturate = 0.99 s of GALEN's
  ~802 s wall; Phase 2d propagation = 47 ms combined).
- The +6.5% measurement being investigated:
  `docs/phase2d-2c-redux-results.md`.
- Anonymous-349 single-sample concurrency-artifact precedent:
  `docs/phase2e-notgalen-diagnosis.md` Addendum.
