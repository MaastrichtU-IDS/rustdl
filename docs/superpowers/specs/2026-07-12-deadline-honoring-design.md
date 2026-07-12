# Deadline-enforcement diagnosis — `classify_top_down_with_timeout` hangs on `ore_ont_10080`

Date: 2026-07-12 · branch `feat/deadline-honoring` · reproduced in-process under `gtimeout`.

## TL;DR

The hang is **not** a loop that fails to poll a deadline. Every individual unit
of work (each wedge probe, each tableau search, each `horn_fixpoint` call) *does*
honor a deadline. The bug is that **`classify_top_down_with_timeout` establishes
no global/aggregate wall-clock bound** (`global_deadline = None`), and three
phases of `classify_top_down_internal` scale without any aggregate limit on a
hard ontology:

1. **Label-cache build** — per-class deadline is decoupled from `per_pair` and
   clamped to a **30 s ceiling**; hundreds of classes each burn up to ~30 s.
   (deadline *set but far too large*, and only capped if a `global_deadline` exists.)
2. **Tier walk** (`find_direct_parents_top_down`) — each probe honors `per_pair`,
   but its only aggregate guard is the `global_deadline` short-circuit at
   `classify.rs:2059`, which is **inactive when `global_deadline` is `None`**.
3. **Defined-sup sweep** (`classify.rs:~1633`) — honors `per_pair` per probe, but
   has **no aggregate guard at all** (it constructs a fresh wedge engine per
   candidate even after any deadline has passed). This is the one genuinely
   *missing* poll.

On `ore_ont_10080` (n = 3533 classes) phase 1 alone never finished in 60 s of
wall; the full run exceeds 40 min. `ore_ont_10019` finishes @25 ms because it has
far fewer hard classes/pairs — the per-pair contract is *technically* honored on
both; 10080 simply has enough hard work that the missing aggregate bound makes it
effectively unbounded.

## Reproduction (confirmed)

```
BIN=$(ls target/release/deps/konclude_closure_diff-* | grep -v '\.d$' | head -1)
gtimeout 40 env ORE_ONE_INPUT=$HOME/data/ore-run/input/ore_ont_10080.ofn \
  ORE_ONE_ORACLE=$HOME/data/ore-run/oracle/ore_ont_10080-classified.owx \
  RUSTDL_TEST_PAIR_MS=25 "$BIN" ore_one_closure_matches_oracle --ignored --nocapture --exact
# → killed at 40 s (EXIT 124). Only one stderr line ("VALUE_TYPE_DISJOINT …",
#   emitted during PreparedOntology build) then silence → hang is post-prepare,
#   inside classify_top_down_internal.
```

Test = `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs::ore_one_closure_matches_oracle`,
which calls `diff_corpus_ontology → classify_top_down_with_timeout(&onto, per_pair)`.

## Deadline plumbing (file:line, grounded)

Two deadline sources, combined per probe by `effective_deadline`
(`classify.rs:1166`): `min(global_deadline, now + per_pair)`.

- Entry points (`reasoner/src/classify.rs`):
  - `classify_top_down_with_timeout` (`1125`) → `classify_top_down_internal(internal, Some(per_pair), None)` — **global_deadline is always `None` here** (root of the bug).
  - `classify_with_budget` (`489`) / `classify_with_global_deadline` (`466`) thread a real `global_deadline` — these *are* bounded.
- `classify_top_down_internal` (`1179`) runs, in order:
  1. `saturate()` (`1195`) — no deadline, but monotone/bounded (terminates; not the hang).
  2. Label-cache build (`1265`–`1309`): per-class `prepared.classify_labels(class_id, deadline)` where `deadline = effective_deadline(global_deadline, per_class_cache_dur)` and `per_class_cache_dur = adaptive_label_cache_ms(...)` (`1281`).
  3. Unsat probes (`1313`): `decide_with_deadline(effective_deadline(global, per_pair), …)`.
  4. Tier walk (`1474`): `find_direct_parents_top_down` → `subsumes_via_tableau`.
  5. **Defined-sup sweep** (`1633`): per-candidate `subsumes_via_tableau`.
  6. Defined-SUB sweep (`1804`, closure-only, cheap) + backfold (`1858`) + entailment matrix.
- Per-probe path `subsumes_via_tableau` (`2147`): `hyper_decide(sub,sup,effective_deadline(...))` (`2227`), then main tableau `decide_with_deadline(effective_deadline(...))` (`2307`).
- Wedge deadline: `HyperCache::classify_labels/decide → HyperEngine::decide_with_deadline` (`hyper.rs:1730`) sets `self.deadline`; `solve` (`hyper.rs:2163`) polls it at the top of every frame; `horn_fixpoint` (`hyper.rs:1526`) is capped by `FIXPOINT_ITERS = 100_000` steps (`hyper.rs:51`).
- Main tableau deadline: `decide` (`lib.rs:4874`) calls `ctx.set_deadline` (`lib.rs:4900`); `search` polls `ctx.check_deadline()` at the top of every call (`search.rs:97`); `saturate` polls it in-loop (`saturate.rs:84,93,111,151`); `check_deadline` (`lib.rs:535`) is a straight `now >= deadline`.

## Which loops were investigated and ruled out

Confirmed by temporary instrumentation (now reverted):

- **`horn_fixpoint` (hyper.rs:1526)** does *not* poll `self.deadline` (only the
  100 k step cap). A temp deadline-poll every 4096 steps + a heartbeat every
  16384 steps **never fired** across the whole run → no single `horn_fixpoint`
  call runs long; each stays well under 16 k steps. Not the hang.
- **Main tableau `search`/`branch`**: with `RUSTDL_TRACE=1` the tail is thousands
  of `# trace search STOP … deadline=true` — i.e. `check_deadline()` **is** firing
  and the per-probe search *does* return `DepthLimit`. Disambiguated: **33 359 / 33 359
  stops were `deadline=true`, zero were depth-zero.** Per-probe deadline honored.
- **Per-probe wall**: temp timers on both `hyper_decide` and `decide_with_deadline`
  in `subsumes_via_tableau` — **zero probes exceeded 200 ms** (8× the 25 ms budget).
  So no single pair ignores `per_pair`.
- **`partition_rec` / `solve_at_most`** (hyper.rs:2696/2727): recurse via `solve`,
  which polls the deadline at its top → bounded. Not the hang.

## Where the time actually goes (measured on 10080, n = 3533)

- **Phase 1 — label-cache build**: `adaptive_label_cache_ms(3533, 25 ms, None)` =
  `clamp(3533 × 25 ms, 50 ms, 30_000 ms)` = **30 000 ms per class**
  (`lib.rs:1802`, ceiling `LABEL_CACHE_CEILING_MS = 30_000` at `lib.rs:1794`).
  118+ classes each took 1–~25 s (many at the 30 s cap); the build never completed
  in 110 s. Each class *does* honor its 30 s deadline (`solve` polls it) — the
  deadline is simply **1200× the 25 ms `per_pair` the caller asked for**, with no
  aggregate cap. This is the dominant cost and the *default-config* hang.
- **Phase 2 — tier walk**: with the label cache force-capped to 25 ms
  (`RUSTDL_LABEL_CACHE_TIMEOUT_MS=25`, builds in 5.3 s) the walk then stalls at
  tier ~7/44. Each probe honors 25 ms, but with the label cache mostly `NoVerdict`
  every candidate falls through to a real (deadline-cut) probe; the *count* of
  probes is bounded only by the hierarchy walk (≈ n² worst case) and by the
  `global_deadline` short-circuit at `classify.rs:2059` — which is a no-op here.
- **Phase 3 — defined-sup sweep** (`classify.rs:1633`, 489 `sweep_sups` on 10080):
  even after supplying a global deadline (so phases 1–2 became bounded), the run
  still ran > 150 s here. The sweep loop has **no** `global_deadline` short-circuit;
  it calls `subsumes_via_tableau` for every candidate, and each call *constructs a
  fresh `HyperEngine`* (clones the full clause set for a 3533-class ontology) before
  the deadline cuts the search. The engine-construction cost accrues for every
  candidate regardless of the elapsed deadline.

## Set vs polled

- Phases 1 & 2: deadline is **set and correctly polled**, but there is no
  aggregate wall (`global_deadline = None`) and phase 1's per-class budget is
  decoupled from `per_pair` (30 s ceiling). "Deadline present but not
  aggregate-bounded."
- Phase 3: the aggregate guard is **missing** — the sweep loop never consults the
  deadline, so it grinds through all `sweep_sups × candidates` even after any
  deadline elapses. "Guard absent."

## Fix — smallest correct shape

Both parts are needed (verified below):

1. **Give the per-pair-only path a global/aggregate bound.** In
   `classify_top_down_internal` (or `classify_top_down_with_timeout`,
   `classify.rs:1125/1179`), when `per_pair` is set and `global_deadline` is
   `None`, synthesize one, e.g. `global_deadline = Some(now + adaptive_label_cache_ms(...))`
   (the "n × per_pair, clamped" aggregate the label cache already computes is the
   right scale) — or bound the label-cache *phase* by a single shared build
   deadline rather than a 30 s-per-class ceiling. This immediately bounds phases 1
   and 2 through the existing `effective_deadline` cap (`1303`) and the tier-walk
   short-circuit (`2059`).
   - Alternatively/additionally, lower phase 1's per-class budget toward `per_pair`
     when `per_pair` is small so the label cache does not run 1200× longer than the
     caller's unit budget. (Guard any change with the galen/wine completeness
     fixtures — the label cache builds full within its budget there.)

2. **Add the missing aggregate guard to the defined-sup sweep loop**
   (`classify.rs`, top of `for &sup in &sweep_sups`, ~line 1633), mirroring
   `find_direct_parents_top_down` at `2059`:

   ```rust
   for &sup in &sweep_sups {
       if global_deadline.is_some_and(|gd| Instant::now() >= gd) {
           continue; // budget exhausted → leave pair undecided (sound MISS)
       }
       …
   }
   ```

   Check is O(1) per `sup` (outer loop, not per inner candidate), so no hot-path
   cost. Optionally also record skipped pairs into `stats.timed_out_pair_ids` for
   the anytime contract (as the tier walk does at `2060`).

No inner-loop `check_deadline()` needs to be *added* — every inner loop already
polls (search/saturate) or is step-capped (`horn_fixpoint`). The fix is purely
about establishing and honoring an **aggregate** deadline.

## Soundness & perf

- **Sound**: every deadline cut yields "not subsumed" — a MISS at worst, never an
  FP. The sweep only ever *adds* subsumptions (`direct_supers[cand].push(sup)`
  when `subsumed`); skipping a candidate can only omit an edge. Cutting the label
  cache early yields `NoVerdict`, which makes the tier walk fall through to the
  (bounded) per-pair probe — no new positives.
- **Fast path untouched**: pure-EL / saturator-complete inputs return before any
  of these phases (`classify.rs:1206`). The sweep guard is O(1)/sup; the global
  synth is one `Instant::now()` at entry. `ore_ont_10019`-style easy inputs finish
  before the aggregate deadline and are unaffected.

## Verification (temp fix, reverted)

With a synthesized global deadline (temp env `RUSTDL_TEMP_GLOBAL_MS`) **plus** the
sweep loop short-circuit:

```
RUSTDL_TEST_PAIR_MS=25 RUSTDL_TEMP_GLOBAL_MS=5000 <bin> ore_one_closure_matches_oracle …
# → EXIT 0 in 6.02 s:
#   RESULT ore_ont_10080  rustdl=32356  konclude=32356  FP=0  MISSED=0
```

10080 now finishes bounded, and (notably) with FP=0/MISSED=0 — the EL closure +
5 s label cache recover the full hierarchy; the aggregate bound only cuts work
that was redundant. The global deadline alone (without the sweep guard) still ran
> 150 s in phase 3, confirming the sweep guard is required.

Suggested regression: assert `classify_top_down_with_timeout(10080, 25 ms)`
returns within a few seconds (DNF/partial permitted), FP=0.

## Cleanup

All temporary instrumentation reverted. `git diff` on tracked source is empty;
`cargo build --release -p owl-dl-cli -p owl-dl-bench` (and the reasoner tests)
compile clean on the reverted tree. Files touched during diagnosis and restored:
`crates/owl-dl-tableau/src/{hyper.rs,search.rs}`,
`crates/owl-dl-reasoner/src/classify.rs`.
