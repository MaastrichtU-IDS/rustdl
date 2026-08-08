# `label_cache_build` is unbounded and dominates 12 of the 13 non-search-bound tail ontologies

**Date:** 2026-08-07 · **Status:** localised, **NOT fixed** — the exact site is not yet found
**Severity:** DNF (no classification at all) on 12 measured ORE ontologies

## The cluster

Of the 39 Set-A tail members Konclude classifies in under 1 s, 13 still DNF even with
per-pair search capped to ~zero (`--pair-timeout-ms 1`), i.e. per-pair search is **not** their
cost. Phase breakdown, single-threaded, forced to emit via `--global-timeout-ms 30000`:

| dominant phase | n |
|---|---:|
| **`label_cache_build`** | **12** (84–100% of wall) |
| `prepare` | 1 (`ore_ont_5368`, the known 18.6 M-axiom DKey case) |

`tier_walk ≈ 0` throughout — **the pair loop never starts**, which is exactly why a per-pair
budget cannot help this cluster. Class counts span 309 → 8,025, so it is not simply "big
ontologies".

## The defect: the phase is not bounded by anything

On `ore_ont_16056` (**309 classes**, 1,425 concept rules) `label_cache_build` is ~34.4 s and is
**invariant** to every knob that should bound or cheapen it:

| configuration | `label_cache_build` |
|---|---:|
| default | 34,392 ms |
| `RUSTDL_LABEL_CACHE_TIMEOUT_MS=1` | 34,356 ms |
| `RUSTDL_LABEL_CACHE_TIMEOUT_MS=10` | 34,352 ms |
| `RUSTDL_CLASSIFY_LABELS_AMORTIZE=0` | 34,377 ms |
| `RUSTDL_CLASSIFY_LABELS_AMORTIZE=1` | 34,347 ms |
| `RUSTDL_CLASSIFY_BACKFOLD=0` | 34,355 ms |
| `RUSTDL_ADAPTIVE_BUDGET=0` | 34,302 ms |
| `RUSTDL_HYPER_INCREMENTAL_FIXPOINT=0` | 35,287 ms |
| `RUSTDL_ITERATIVE_DEEPENING=0` | 34,311 ms |
| `RUSTDL_MAX_NODES=2000` | 34,377 ms |
| `RUSTDL_SAT_SEED=0` | 38,732 ms |

**±0.3% across ten configurations.** With 309 classes and a 1 ms per-class budget the
deadline-bounded *search* can account for at most ~309 ms, so ~34 s is elsewhere.

**It also overruns the global deadline:** under `--global-timeout-ms 25000`,
`label_cache_build` still reports **34.4 s**. The per-class early-return
(`classify.rs:2300`) makes classes return instantly once the global deadline passes, so ≥9 s
ran *inside a single call* past a deadline that call was given.

## What is ruled out — and what that leaves

The deadline **is** plumbed correctly: `classify.rs:2288-2310` computes `cache_ms`, wraps it as
`per_class_cache_dur`, takes `effective_deadline(global_deadline, …)`, and passes it to
`classify_labels`, which hands it to `engine.decide_with_deadline(...)` (`lib.rs:~4110`). So
the *search* is bounded.

Ruled out by measurement: the search itself (deadline-insensitive), engine construction
(`RUSTDL_LABEL_AMORTIZE_MARK` **proves** the amortized path is taken — `engaged` vs
`full-rebuild` — and it changes nothing here), backfold, sat-seed, adaptive budget,
incremental fixpoint, iterative deepening, and the node cap.

That leaves **unguarded work inside `classify_labels` outside `decide_with_deadline`** — the
`extras` seed loops before it, and `satisfiability_labels` / label extraction after it. Both
are per class and neither consults a deadline.

## Contrast that localises the bug

`RUSTDL_LABEL_CACHE_TIMEOUT_MS` **does** work on `ore_ont_15010`: 103.98 s → 5.64 s for
byte-identical output (`docs/known-limitations/label-cache-budget-starved-by-small-pair-timeout.md`).
Same knob, honoured there, ignored here. Whatever differs between those two paths is the
bug's signature, and `15010`/`16056` are a ready-made discriminating pair.

## Why this is a good target despite being unfixed

- **Addressable set of 12**, versus the 1–2 of every other lever examined this week.
- **The fix shape is sound by construction.** The label cache is an *accelerator*: it prunes
  96–100% of pairs. A partial cache means less pruning — slower per pair, never a wrong
  answer. So bounding the phase cannot cost soundness, only time.
- The bound is missing, not mis-tuned, which avoids the `n × F` trade that killed the
  floor-raise idea in the sibling document.

## LOCALISED to a single line, by instrumentation (2026-08-07)

A temporary construct/solve split inside `classify_labels` (since reverted) settles it:

```
[lc] calls=5 construct=0ms solve=17158ms
[lc] calls=6 construct=0ms solve=17158ms
[lc] calls=7 construct=0ms solve=34342ms
```

- **`construct = 0 ms`.** Engine construction is not the cost, confirming the amortization
  result independently.
- **`classify_labels` is called only 7 times** on a 309-class ontology — the rest are skipped
  by the global-deadline early return. So this is not "many small calls".
- **Individual `solve` calls take ~17 s each** (calls 1–5 total 17,158 ms; call 7 alone adds
  ~17,184 ms) against a **1 ms** label-cache deadline.

**The cause is `hyper.rs:2899-2903`: `fn solve` checks the deadline exactly once, at entry,
and that is the ONLY runtime deadline check in the whole hyper engine.** A `solve` frame whose
work stays inside its own body therefore never re-consults it.

### A fix was attempted at the obvious place and did NOT work — recorded so it is not retried

Adding a strided deadline check inside `horn_fixpoint`'s worklist drain (mirroring the
saturator's shipped `DEADLINE_CHECK_STRIDE = 4096`) left `label_cache_build` at
**34,298 ms vs 34,392 ms** — unchanged — and the ontologies still DNF. Verified the patch was
in the binary and force-rebuilt before concluding.

**So the 17 s is inside a single event, not spread across worklist events.** That excludes the
drain loop and points at `process_event` (`:2188`) and below it `match_body` (`:4104`) /
`enumerate_matches` (`:4135`) — the non-Horn fire loop already documented as ~25% of self
time. The change was reverted; the tree is clean.

### Why the remaining fix needs care rather than another quick patch

`enumerate_matches` and `match_body` take `&self` and *return matches*. Bailing out of them
early drops matches, and a dropped match must surface as **`Stalled`** — not as a silent
`Sat`. A silent `Sat` would still be sound in the FP sense (fewer derived facts ⇒ fewer
clashes ⇒ a MISS) but it would be **silently incomplete without the `incomplete` flag**, which
is precisely the failure mode this codebase treats as worse than a DNF. So the fix is a
plumbing change through the match path, not a one-line stride.

## FIXED behind `RUSTDL_HYPER_MATCH_DEADLINE` (default OFF) — 2026-08-07

The design below was implemented as specified. **The deadline now binds:**

| `RUSTDL_HYPER_MATCH_DEADLINE` | `label_cache_build` on `ore_ont_16056` (309 classes, 1 ms budget) |
|---|---:|
| 0 (off) | **83,201 ms** |
| **1 (on)** | **102 ms** |

**~800×**, and 102 ms is what a 1 ms per-class budget over 309 classes should cost — i.e. the
phase is finally bounded by the budget it was given, which no configuration could achieve before.

**A recovery attributable to the flag** (identical bounds on both arms; only the flag differs):

| ontology | flag OFF | flag ON |
|---|---|---|
| `ore_ont_16056` | dnf @150 s | **ok / 485 rows / 16.9 s** |
| `ore_ont_6134` | dnf | dnf |

### Cluster measurement: 1 of 12 — my prediction was WRONG

Pre-registered before the run: *"3–8 of 12 recover"*, with the rule *"≥6 ⇒ the aggregate-bound
work is justified and urgent; 2–5 ⇒ keep the flag OFF; ≤1 ⇒ the fix is a correctness repair
only and the cluster needs a different mechanism."* Pinned sha `74180aca60d1bd8f`, propagation
proven on the discriminating input first (83,293 ms vs 105 ms), both arms with
`LABEL_CACHE_TIMEOUT_MS=50` and `--pair-timeout-ms 1`, threads=1, 150 s cap.

| | |
|---|---:|
| **RECOVERED** | **1 of 12** (`ore_ont_16056`, dnf → ok/485 rows/17 s) |
| unchanged | 11 (`12432`, `6910`, `13122`, `14223`, `9944`, `15733`, `6134`, `8441`, `6608`, `10080`, `7174`) |
| REGRESSED | 0 |
| negative control `ore_ont_5368` (`prepare`-bound) | ok/6,100 rows in **both** arms, 125 s vs 117 s — correctly **unaffected** |

**So the pre-registered rule says: this is a correctness repair, not a recovery lever, and the
aggregate-bound follow-up does NOT inherit a justification from it.**

> **CONFIRMED 2026-08-08 — the aggregate bound was built anyway, and the rule was right.**
> `RUSTDL_LABEL_CACHE_TOTAL_MS` (opt-in) bounds the phase exactly (tracks its budget to
> within 18 ms). Under an identical 100 s global budget it redirects **~95 seconds** from
> `label_cache_build` into `tier_walk` and yields **+6 rows out of 2,349 on ONE** of five
> tail members and **zero** on the other four; 0 of 11 recover at default or at
> `--pair-timeout-ms 50`. The intervening justification — profiling showed the median
> per-class overshoot is 0 ms, so "they are aggregate-bound and need an aggregate bound" —
> was quantitative, correctly derived, and still wrong: it asked which phase *consumes* the
> wall without asking what that phase *buys* the phase after it. The label cache is a
> PRUNE, so starving it makes the tier walk probe more, not progress more.
> See `docs/2026-08-08-label-cache-aggregate-bound.md`. The 12 are
**phase-homogeneous but not mechanism-homogeneous** — they all stall in `label_cache_build`, for
at least two different underlying reasons, and only `16056`'s is the unbounded match
enumeration. The negative control behaving correctly is what makes that reading trustworthy
rather than a null result from mis-wired arms.

**Not universal, and the reason matters.** At *default* budgets `16056` still DNFs, because the
default per-class budget is `clamp(n × per_pair, …)` = 30 s × 309 classes. The fix makes a
budget *effective*; it does not make the default budget *sane*. **That is the second, still-open
defect: `label_cache_build` has a per-class bound and no AGGREGATE bound.** Recovery needed the
label cache bounded *and* the pair loop bounded — with only one of the two, the work simply
moves downstream (bounding the cache removes pruning, so the tier walk does more).

### Gates run

- **Flag-OFF byte-identical** to flag-ON on pizza/ro/sulo/bibtex at `--pair-timeout-ms 1000`
  (4/4), so the default path is untouched.
- **FP=0 net, flag ON: 13 VERIFIED, zero `FP>0`/`MISSED>0` rows.**
- **1,586 tests pass**; `fmt` clean; `clippy -D warnings` clean.
- **Not run, so not claimed:** the 1,920-ontology two-arm sweep and a MISSED-net arm. Both are
  required before any default flip. The flag ships OFF for exactly that reason.

### The load-bearing detail, restated

`horn_fixpoint` converts `match_deadline_hit` into `HyperResult::Stalled` **before** it can
return `Sat` — both inside the drain loop and at the final return. Without that, a truncated
enumeration would surface as a trusted `Sat`: FP-safe, but silently incomplete with no
`incomplete` flag. `Unsat` is still returned first, so a clash found *before* truncation
remains a real clash.

## Design as implemented

Signatures checked, so this is buildable as written:

- `enumerate_matches(&self, node, plan, i, binding, out)` — `hyper.rs:4135`, **`&self`**, recursive
  over the match cross-product. This is where the ~17 s sits.
- `match_body(&self, ci, node) -> Option<Vec<Binding>>` — `:4104`.
- `FireOutcome { Clash, Changed, NoChange }` — `:4621`.
- `horn_fixpoint` already returns `HyperResult::Stalled` on `steps > max_iters`, so the
  `Stalled → NoVerdict → incomplete` path exists end to end and needs no new plumbing.
- `hyper.rs` contains **zero** `Cell`/`RefCell` today, so interior mutability is a new pattern
  in this file — call it out in review rather than slipping it in.

**Design:**

1. Add `deadline_hit: std::cell::Cell<bool>` and `match_steps: std::cell::Cell<u64>` to
   `HyperEngine` (needed because `enumerate_matches` takes `&self`).
2. In `enumerate_matches`' recursion, every `DEADLINE_CHECK_STRIDE` steps test the deadline;
   on expiry set `deadline_hit` and return early, truncating the enumeration.
3. **Do NOT signal via `match_body`'s `None`.** `None` already means "this clause does not
   match", so a deadline-`None` would silently skip a clause that might have derived a clash —
   incompleteness with no `incomplete` flag, which is the failure mode this codebase treats as
   worse than a DNF.
4. Instead, test `deadline_hit` in `horn_fixpoint`'s drain **before** its final
   `HyperResult::Sat` return, and return `Stalled`. That is the load-bearing line: it makes a
   truncated enumeration incapable of surfacing as a trusted `Sat`.

**Soundness argument:** a deadline is a cut. Truncating enumeration derives *fewer* facts ⇒
fewer clashes ⇒ fewer `Unsat` verdicts ⇒ a MISS, never a false positive — *provided* step 4
holds. Without step 4 the change is FP-safe but silently incomplete, which is not acceptable
here.

**Gates:** FP=0 net; the 12-ontology cluster (`label_cache_build` must fall below the
label-cache budget on `ore_ont_16056`); `ore_ont_15010` as the discriminating pair-mate; and a
1,920-ontology two-arm sweep, since this changes engine behaviour on every deadline-bounded
classify.

## PROFILED 2026-08-08: the other 11 are AGGREGATE-bound, not precision-bound

After v0.4.16 made the per-class deadline bind, all 11 remaining members are **still**
`label_cache_build`-dominated (84–100% of wall; class counts 1,682–8,025). Instrumenting
`classify_labels` on `ore_ont_6134` (n=1,682) at a **1 ms** per-class budget:

| | flag ON (stride 4096) | flag OFF |
|---|---|---|
| classes processed in a 20 s window | 237 | 156 |
| mean overshoot vs the 1 ms budget | **85.1 ms** | 127.5 ms |
| median | **0 ms** | 0 ms |
| p90 | 406 ms | 548 ms |
| max | 560 ms | 559 ms |
| `construct_tot` | 1 ms | — |

Three things follow.

1. **v0.4.16 helps but does not bound**: −33% mean overshoot and +52% classes processed
   per window. A ~560 ms max against a 1 ms budget is too large to be stride granularity,
   so **a third unguarded region exists inside `solve`**, beyond the match cross-product.
   Unlike `ore_ont_16056`'s 17 s, it is worth ~85 ms per class here.
2. **Construction is again free** (1 ms total), independently re-confirming that the
   clause-index amortization is not the issue on this cluster.
3. **The dominant term is arithmetic, not precision.** The median overshoot is **0 ms** —
   most classes are instant — and the cost is a tail of 400–560 ms classes. But even at a
   *perfect* 10 ms per class, 1,682–8,025 classes cost **17–80 s**. No achievable per-class
   precision fixes that.

**Conclusion: these 11 need the AGGREGATE bound, and chasing further per-class deadline
precision would not recover them.** That settles the open question of whether the
aggregate-bound work is justified — it is, for this cluster, and the profiling is what
justifies it rather than the single `ore_ont_16056` recovery (which was 1 of 12 and is a
*different* mechanism).

An aggregate bound is also robust to the residual imprecision: it cuts the whole phase, so
it works even while per-class deadlines still overshoot.

## Next step (not attempted here)

Add a deadline check inside the match/fire path (`process_event` → `match_body` /
`enumerate_matches`), propagating exhaustion as `HyperResult::Stalled` so the `incomplete`
signal is preserved. Then re-measure `ore_ont_16056` (`label_cache_build` must fall below the
label-cache budget) and re-run the 12-ontology cluster.

Three traps that cost time here, for whoever continues:
- `perf` is unavailable in this environment (`samply` is present).
- `match engine.decide_with_deadline(HYPER_WEDGE_DEPTH, deadline) {` occurs **twice** in
  `lib.rs` — line 4100 is `classify_labels`, line 4461 is `base_model_types`. A single-anchor
  patch silently hits the wrong site; patch by line number.
- `cargo build` reporting `Finished … in 0.10s` means **nothing was rebuilt**. Verify with
  `strings target/release/rustdl | grep <marker>` or `touch` the file before trusting a
  measurement.

No code changes ship from this investigation; the tree is clean and rebuilt.
