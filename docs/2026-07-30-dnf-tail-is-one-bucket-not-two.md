# The DNF tail is ONE mechanism, not two buckets

**Date:** 2026-07-30
**Status:** Measurement finding. Corrects the Bucket A / Bucket B taxonomy recorded on 2026-06-22.
**Method:** re-measured on current `main` (post fod-restricted-scan, match-plan precompute,
incremental `horn_fixpoint`, and the 2026-07-30 DKey work). No new instrumentation needed — the
existing `RUSTDL_TRACE_RSS` phase probes and the `# wall breakdown ms:` banner sufficed.

## What the old taxonomy said

The 2026-06-22 characterisation split the 13-ontology DNF tail into:

- **Bucket A — per-pair-bound (8):** finish when each pair is capped at 5 ms, so the cost is hard
  individual subsumption pairs (disjunctive branching).
- **Bucket B — label-cache-build-bound (5):** `5438 5548 7499 7712 10080`. *"Still DNF even at
  pair=5ms ⟹ cost is OUTSIDE the per-pair loop."*

That framing drove five weeks of work and a standing recommendation to treat Bucket B as the better
DNF target because its blocker was undiagnosed.

## What is actually true

**1. `ore_ont_7499` is not Bucket B. It is per-pair-bound.**

| config | result |
|---|---|
| default | DNF @180 s |
| `--pair-timeout-ms 5` | **COMPLETE in 28.73 s**, 5,109 classes, `label_cache_build=1,317 ms` |
| `RUSTDL_LABEL_HEURISTIC=0` | DNF @180 s |

Its label-cache build is 1.3 s of a 28.7 s run — nowhere near dominant. Capping the per-pair budget
rescues it, which is the definition of Bucket A. Either it was misclassified or the intervening work
moved it.

**2. The other four do stall in the label-cache build — but removing the build does not help.**

`RUSTDL_TRACE_RSS` phase probes, default config, 100 s budget: `10080` and `5438` both emit `entry`,
`after_saturate`, `before_prepared`, `after_prepared` and **never reach `after_label_cache`**. So the
stall is inside the per-class label-cache build. RSS at that point is small — 0.07 GB and 0.19 GB —
so it is **compute-bound, not memory-bound**. Conversion is not implicated either: `tbox-stats`
completes in ≤1 s for all four.

With `RUSTDL_LABEL_HEURISTIC=0`, the build is replaced by `vec![NoVerdict; n]` and skipped instantly —
both then reach `after_label_cache` and stall in the **tier walk** instead:

| ontology | tier-walk progress in ~100 s |
|---|---|
| `ore_ont_5438` | reaches `pair=500` at 2.18 GB — roughly **5 pairs/sec** |
| `ore_ont_10080` | **zero** pair probes, i.e. fewer than 100 pairs — **>1 s per subsumption test** |

**3. Therefore the Bucket A / Bucket B distinction is not a mechanism difference.** Both buckets are
the same cost — per-class / per-pair wedge satisfiability on dense disjunctive SROIQ — and the label
cache merely relocates it. The 2026-06-22 note itself contained the answer ("the label cache only
MOVES the wedge-sat cost between build and pairs") but the taxonomy built on top of it treated the
two locations as two problems.

Concretely: "still DNF at pair=5 ms" does **not** show the cost is outside the per-pair loop. It shows
the *label-cache build* is unbounded by `--pair-timeout-ms`, which is by design (Phase 8 deliberately
decoupled the cache-build deadline from the per-pair budget). The bucket was an artefact of which
budget each phase honours.

## Consequences

**The DNF tail is a single target.** After today's DKey work the tail is 12: `5964 6485 8273 8666
13545` + `5438 5548 7499 7712 10080` + `4410 5368`. Every one of them reduces to *wedge satisfiability
per class/pair is too slow*, differing only in which phase exhausts its budget first.

**One hypothesis was eliminated today by measurement.** `ore_ont_5548` lost 54% of its disjointness
axioms and half its RSS to the collapse/broadcast split (541,575 → 250,546; 949 MB → 506 MB) and still
DNFs. So the cost is not axiom volume.

**Two candidate levers remain, and they should be judged as one target, not two:**

1. **Share class-independent work across per-class wedge calls — UNTESTED.** The build is
   `(0..n).into_par_iter().map(|i| prepared.classify_labels(class_id, deadline))`: every class
   constructs its own context and derives Horn consequences from scratch (~3,533 fresh contexts on
   `10080`). If the class-independent portion of that derivation dominates a single call, sharing it
   saves `(n−1)×` that portion — and it attacks the cost in *both* locations, since the tier walk pays
   the same per-pair wedge cost. Note `RUSTDL_HYPER_INCREMENTAL_FIXPOINT` (2026-07-14) already does
   this *within* one solve (across branches); doing it *across classes* is the untried step, and is
   the same idea as the parked build-once-classify-many architecture.
   **Caveat that must be measured first:** `10080` spends >1 s on a *single* subsumption test, so the
   per-call cost is already large. Sharing only wins if the shared prefix is a large fraction of one
   call — measure that fraction before building anything.
2. **Clash-driven search / CDCL** — repeatedly NO-GO'd, no cheap entry.

## Method notes

- The `# wall breakdown ms: label_cache_build=… tier_walk=…` banner only prints on a **completed**
  classify, so it is useless on a DNF. The `RUSTDL_TRACE_RSS` phase probes are the right tool there —
  the last marker emitted localises the stall.
- Two self-inflicted measurement errors while doing this, both worth remembering: piping probe output
  through `tail` lost everything when the outer timeout killed the loop (`tail` buffers to EOF), and
  a 4-ontology × 120 s loop under a 10-minute cap silently truncated. Write per-ontology output to
  files and read the files.

---

## Follow-up (2026-07-31): where the label-cache build's time actually goes

Two hypotheses were tested against the four build-stalling ontologies. **The first was refuted by its
own prototype; the second is an unmeasured lead with a fix pattern already in-tree.**

### REFUTED — "the wall-clock deadline is not enforced inside `horn_fixpoint`"

The structural claim is true: `solve()` checks `self.deadline` once on entry (`hyper.rs:2826`) and then
calls `horn_fixpoint(FIXPOINT_ITERS)` with `FIXPOINT_ITERS = 100_000`, whose drain loop has no clock
check. But adding a sampled in-loop check **changed nothing** — patched and unpatched both DNF at
200 s, like-for-like.

Why it cannot be the explanation, from a throwaway diagnostic: every `horn_fixpoint` entry reports
`deadline_set=true incremental=true`. In incremental mode each call drains only the delta its own
decision pushed, so a single drain is small — `steps` rarely reaches the 1024-event sampling interval,
and `solve` already checks the deadline per entry. The deadline IS propagated and IS consulted. The
time is spent **before** any check.

Two of my own errors are worth recording, because both produced uninterpretable data that looked like
evidence:
1. the first "verification" run dropped `RUSTDL_LABEL_CACHE_TIMEOUT_MS`, changing two variables at
   once — without it the adaptive per-class budget scales to `n × per_pair` clamped to [1 s, 30 s],
   which over thousands of classes exceeds 200 s *by design*;
2. the prototype sampled every 1024 events, which in incremental mode is almost never reached — a fix
   that could not fire, then measured.

Also confirmed while chasing this: `RUSTDL_LABEL_CACHE_TIMEOUT_MS` is **not** clamped
(`adaptive_label_cache_ms` returns the override verbatim, `lib.rs:1918`), so a small value really is
honoured. That eliminated the other mundane explanation.

### LEAD (unmeasured) — the per-class clause-`Vec` clone

`PreparedOntology::classify_labels` begins (`lib.rs:2894`):

```rust
let mut clauses = self.clauses.clone();
clauses.push(DlClause { body: vec![Atom::Class(self.fresh_q, X)], head: vec![Atom::Class(c, X)] });
```

That deep-clones the whole clause vector — every `DlClause`'s `body`/`head` `Vec` — **once per class**,
and it happens before any deadline is consulted. Scale on the stalling four:

| ontology | concept_rules |
|---|---|
| `ore_ont_5548` | **541,575** |
| `ore_ont_10080` | 28,407 |
| `ore_ont_5438` | 26,529 |
| `ore_ont_7712` | 20,421 |

Multiplied by thousands of classes this is a very large amount of pre-deadline allocation, which would
explain: the budget not binding, the stall localising inside the build, small/transient RSS, and the
cost reappearing in the tier walk when the build is disabled.

**This is the unfixed sibling of a documented fix.** CLAUDE.md's v0.3.39 entry describes amortizing
exactly this cost for the *per-pair* oracle (`decide_with_stats` "cloned the full clause vector +
rebuilt the whole index on every decided pair", 13,772 × ~34.6k clauses on `ore_ont_1508`). That work
amortized the `ClauseIndexes` **rebuild** via a shared `Arc` + per-pair delta — but the clause-`Vec`
clone itself survives at `lib.rs:1217`, `:1327` and `:2894`. Crucially the engine **already**
branch-routes `clause(ci)`/`match_plan(ci)` between a base slice and per-pair extras, so the mechanism
needed to avoid the clone exists; `classify_labels` simply does not use it.

**Status: LEAD, NOT RESULT.** What is established by reading source: the clone exists, runs per class,
precedes every deadline check, and duplicates a cost already fixed elsewhere. What is **not**
established: that it dominates. Next step is to apply the v0.3.39 routing to `classify_labels` and
measure — cheap, because the machinery is already there, and verdict-safe because passing the same
clauses by a different route cannot change what is derived.

### LEAD REFUTED (2026-07-31) — the per-class clause clone is 0.55–6.3%, not dominant

Measured with a throwaway timer around the clone in `HyperCache::classify_labels`, **after**
verifying the marker was actually in the binary (`strings target/release/rustdl | grep`). 60 s window,
32 cores, `RUSTDL_LABEL_CACHE_TIMEOUT_MS=20 --pair-timeout-ms 5`:

| ontology | clones/60 s | clone CPU | nclauses | share of 1.92M CPU-ms | ms/clone |
|---|---|---|---|---|---|
| `ore_ont_5548` | 4,650 | 120,762 ms | 252,249 | **6.29%** | 25.97 |
| `ore_ont_5438` | 9,700 | 25,251 ms | 31,139 | 1.32% | 2.60 |
| `ore_ont_7712` | 4,200 | 12,172 ms | 24,918 | 0.63% | 2.90 |
| `ore_ont_10080` | 3,500 | 10,541 ms | 31,232 | **0.55%** | 3.01 |

**So the clone is not why these DNF.** It remains a genuine, avoidable waste (and still the unfixed
sibling of v0.3.39) but fixing it would buy at most ~6% on the worst case and under 1.5% on three of
four. Not a DNF lever.

**Two claims made earlier from a DEAD instrument are withdrawn.** The previous attempt instrumented
`sat_only_with_stats` — one of NINE `clauses.clone()` sites — by pattern-matching; that function is
unused, so the compiler removed the diagnostic and every run printed nothing. Reading that silence as
data produced "fewer than 250 classes in 60 s" and "~400× budget overrun". Both false: 3,500–9,700
classes are processed per 60 s.

**What IS confirmed: a ~10–27× per-class budget overrun.** At 1.92M CPU-ms per 60 s window,
`5548` spends ~413 CPU-ms/class, `10080` ~549, `7712` ~457, `5438` ~198 — all against a 20 ms
deadline. Only ~6% of that is the clone, so ~94% is per-class `HyperEngine` construction, seeding and
solve: work that is either pre-deadline or not deadline-checked. (Treat as upper bounds — the wall
includes prepare and not all 32 cores are necessarily on the build.)

**Net: the original 2026-06-22 conclusion stands** — this is per-class wedge *work volume*, not a
single hotspot. Three candidate hotspots have now been eliminated by measurement rather than
argument: axiom volume (the collapse/broadcast split halved 5548's axioms, still DNF), deadline
enforcement inside `horn_fixpoint` (in-loop check changed nothing), and the clause clone (≤6.3%).
Anyone attacking this next should assume the cost is distributed across per-class setup+solve and
target the *number* of per-class wedge calls or their shared structure — not another hotspot hunt.

**Instrumentation rule this cost three rounds to learn:** anchor a diagnostic on text unique to the
target function (verify with a count), and **confirm the marker is in the built binary before
interpreting any measurement**. Silence from an absent instrument is indistinguishable from silence
from a slow program.
