# Hard-coded constant audit — which others have the `HYPER_WEDGE_DEPTH = 256` disease?

**Date:** 2026-08-03 · **Base:** `feat/cb-alch-taming` worktree @ `d567319`, rustdl 0.4.12
· **Investigation only — no production default was changed, and no fix was built.**

The v0.4.12 release shipped because `HYPER_WEDGE_DEPTH = 256` was **wrong in both
directions at once** — `ore_ont_10407` needs 319 (at 256 it does 4.4× more work than
completing), `ore_ont_2182` needs ≤7 (at 8 it is faster *and* more complete). This
document asks the same question of the eight remaining candidates, and answers it by
**making each constant overridable and varying it on an ontology where it provably
binds**, never by reasoning about it.

---

## 0. Method, binaries, and what is NOT established

### 0a. Binaries

Both pinned immediately after the build that produced them.

| path | sha256 | what |
|---|---|---|
| `…/scratchpad/bin/rustdl-AUDIT-BASE-d567319` | `93e7709c59560cad36cbb68861675cb0a4b78fd0dce59b651621f3855eab8fa3` | unmodified `d567319`, control |
| `…/scratchpad/bin/rustdl-AUDIT-INSTR2` | `7628541f921a1ef32706385bca4bd9afd8521f96a7dc99810c16d677fc7d338e` | + audit counters, env overrides, watchdog |

Built with
`PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
RUSTUP_TOOLCHAIN=stable cargo build --release` (a bare `cargo` is not on `PATH` here).

**Every arm of every A/B below comes from `rustdl-AUDIT-INSTR2`, switched by an env
var** — never from two builds, so a stale-binary mix-up cannot produce a delta.

**The instrumented binary is behaviour-identical to the control with no override set**:
`classify` output byte-identical on `pizza` (325 lines), `sio` (1626), `bibtex` (22),
after stripping the four telemetry banners CLAUDE.md already records as
wall-clock-dependent.

### 0b. Instrumentation (TEMPORARY — reverted in the same commit as this document)

A new `crates/owl-dl-tableau/src/audit.rs` carrying (a) process-global `AtomicU64`
counters and (b) `OnceLock` env overrides that **fall back to the compiled-in
constant**, so no default moves. Hook sites:

| constant | override env | binding counter |
|---|---|---|
| `MAX_BODY_VARS` (hyper.rs:46) | `RUSTDL_AUDIT_MAX_BODY_VARS` | `mbv_reject` (clause bodies refused), `mbv_max_seen` |
| `FIXPOINT_ITERS` (hyper.rs:51) | `RUSTDL_AUDIT_FIXPOINT_ITERS` | `fp_exhaust` (cap hit), `fp_max_steps` |
| `DIV_WINDOW` (hyper.rs:2834) | `RUSTDL_AUDIT_DIV_WINDOW` | `div_windows`, `div_fired` |
| `MAX_SEARCH_DEPTH` (lib.rs:4642) | `RUSTDL_AUDIT_MAX_SEARCH_DEPTH` | `search_depth0`, `search_min_remain` |
| `ID_SHALLOW_BUDGET_DIVISOR` (lib.rs:1629) | `RUSTDL_AUDIT_ID_DIVISOR` | — |

The other four candidates already have production env overrides
(`RUSTDL_ID_SHALLOW_MS`, `RUSTDL_CLASSIFY_INCONSISTENCY_MS`,
`RUSTDL_LABEL_CACHE_TIMEOUT_MS`, `RUSTDL_CONSISTENCY_FALLBACK_MS`, `RUSTDL_MAX_NODES`)
and needed no patch.

**The one instrumentation subtlety that mattered.** `search()` conflated two exits in
one predicate — `if max_depth == 0 || ctx.check_deadline()` — both returning
`DepthLimit`. Counting `DepthLimit`s would therefore have read a deadline cut as a
depth-cap hit. The two are split (`search_depth0` vs `search_deadline0`), and the split
is exactly what produces the §1 result: at `--pair-timeout-ms 50` `ore_ont_10019` shows
`search_depth0=0, search_deadline0=23536` (cap irrelevant), while unbounded it shows
`search_depth0=1999` (cap binding). A merged counter would have called both "binds".

A watchdog thread (`RUSTDL_AUDIT_EVERY_MS`) re-prints the counters periodically so a run
killed by `timeout` — SIGTERM, no unwinding, no `Drop` — still leaves a reading. Without
it every DNF, i.e. every interesting target, would have reported nothing.

### 0c. Host discipline and the honest caveat

Every probe under `( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1 timeout N … )`,
run serially within each battery.

**The host was NOT idle.** A second agent held ~7 cores throughout
(`phase_attribution` at 601% CPU, a `rustdl-scan-instr` at ~97%), and the population
census ran on one `nice -15` core alongside the dose–response battery. Consequences,
stated rather than hidden:

* **Walls are comparable within a battery** (same host, same minute, interleaved arms)
  but should not be compared to numbers in other documents.
* **A DNF at a wall cap is a floor, not a verdict.** Where an arm reads `dnf @150 s`,
  the honest claim is "did not finish in 150 s under load", and the *contrast* with an
  arm that completed in 60 s on the same host in the same battery is what carries the
  finding.
* **Counters are load-independent** for `mbv_reject` / `fp_exhaust` / `search_depth0`
  as **binding predicates** (`> 0` or not). Their magnitudes on a timeout-killed run are
  "as far as it got", and are not quoted as totals.

### 0d. What this document does NOT establish

* **No addressable-set measurement.** The headline recovery is **six ontologies plus
  four healthy controls**; the ORE-wide two-arm sweep that gated v0.4.12 was not run and
  is the first thing any spec must do — the precedent (`ore_ont_13991`) is that a
  full-corpus sweep catches a regression that instance-level evidence misses.
* **The census is a binding census, not a recovery census.** It says where a constant is
  *reached*, not where changing it *helps*; those are different sets, and §5 is the case
  where they diverge completely.
* **`MAX_BODY_VARS` could not be closed.** Every ontology it was seen to bind on is a
  DNF, so the closure it corrupts could not be compared to an oracle (§6).
* Nothing here is a fix. The deliverable is a ranked shortlist and falsifiable
  predictions.

---

## 1. FP-safety, derived from the code before any measurement

The brief's warning is correct and the candidates split into two classes, which the
audit had to establish by reading the consumers rather than by analogy.

**Class 1 — early-termination caps. FP-safe in both directions, by construction.**
`MAX_SEARCH_DEPTH`, `FIXPOINT_ITERS`, `DIV_WINDOW`, `RUSTDL_MAX_NODES`,
`ID_SHALLOW_BUDGET_MS`, `ID_SHALLOW_BUDGET_DIVISOR`, and the three `DEFAULT_MS`
timeouts. Each converts "ran out" into a **non-verdict**, never into `Unsat`:

* `search()` returns `SearchVerdict::DepthLimit` at `max_depth == 0`; in
  `search::branch` a `DepthLimit` from any child sets `depth_limited = true` and the
  frame returns `DepthLimit` **instead of** `Unsat`, which `decide` maps to
  `Err(NoVerdict)` / `Ok(None)`.
* `horn_fixpoint` returns `HyperResult::Stalled` on iteration exhaustion, and `solve`
  propagates `Stalled` without branching.
* `is_diverging` firing returns `Stalled`.
* `node_cap_exceeded` yields `SaturationResult::NodeCapped` → `SearchVerdict::NodeCap`
  → `Ok(None)`.

Lowering any of them can only lose entailments (a MISS); raising any of them can only
add entailments, and by the verdict-monotonicity argument already documented in-tree at
`HyperCache::decide_iterative_deepening` (`lib.rs:3309`) an added entailment is one the
same DFS would have reached with more budget. **So every Class-1 constant is FP-safe to
make adaptive**, and the correctness gate for a change is a superset check, not
byte-identity.

**Class 2 — a structural completeness cap. NOT a budget; the FP direction is live.**
`MAX_BODY_VARS` is different and must not be treated like the others. It sits in
`eval_order`, and exceeding it makes `match_body` return `None`, which all three
consumers (`hyper.rs:3196`, `:3252`, `:3945`) handle by **silently skipping the clause**
(`continue` / `FireOutcome::NoChange`). So the cap does not bound work — it *deletes
inferences*. Its two directions are therefore asymmetric:

* **Lowering** it is trivially FP-safe (fewer clauses fire ⇒ strictly more MISS).
* **Raising** it makes previously-dead clauses fire, i.e. it **adds** derived facts and
  hence `Unsat` verdicts. That is the FP direction. It is *believed* sound — firing a
  clause whose body genuinely matches is ordinary hyperresolution, and the same
  `match_body` machinery verifies the match — but nothing about the cap itself
  guarantees it, and the scratch buffers it is sized against (`SmallVec<[Var; 8]>`)
  spill to the heap rather than misbehave. **Any change here needs a
  Konclude ∪ HermiT adjudication, not a superset check.**

## 2. Population binding census

One pass over **368 ontologies** (all 174 still-DNF on v0.4.12 plus a deterministic
every-9th stride of the completers), `classify` at the **default operating mode** (no
per-pair budget — the mode in which the caps can bind at all), 20 s wall cap, watchdog
dump. Counters only.

> **The operating mode is load-bearing, and getting it wrong would have produced four
> false nulls.** An earlier pass at `--pair-timeout-ms 5` reported
> `search_depth0 = 0` on *every* ontology, because the per-pair deadline always fired
> before the depth cap could. Read naively that is "`MAX_SEARCH_DEPTH` never binds" —
> the exact shape of the failure the brief warns about. The split
> `search_depth0`/`search_deadline0` counter is what made the difference visible.

| constant | binds on | evidence counter | note |
|---|---|---|---|
| `DIV_WINDOW = 500` | **47 / 368** | `div_fired > 0` | the most widely binding of the four |
| `MAX_BODY_VARS = 8` | **23 / 368** | `mbv_reject > 0` | several onts sit at `mbv_max_seen = 9` — one variable over |
| `MAX_SEARCH_DEPTH = 256` | **27 / 368** | `search_depth0 > 0` | but see the denominator below |
| `FIXPOINT_ITERS = 100_000` | **11 / 368** | `fp_exhaust > 0`, `fp_max_steps = 100001` | **not the null a prior investigation assumed** |

**The `MAX_SEARCH_DEPTH` denominator is the finding, not the numerator.** The main
tableau is reached at all on only 33 of these ontologies — on the rest
`search_entries = 0`, so the cap is *inert*, and a naive "binds on k/N" reads as small.
Restricted to the ontologies where the tableau actually runs, the cap binds on
**27 of 33 — 82%**, and `search_min_remain` (headroom left at the shallowest frame)
is **0** on every one of them. Whenever the main tableau runs on this population, it
consumes the entire 256-level budget.

**`FIXPOINT_ITERS` is not structurally unreachable.** `fp_max_steps` reads exactly
`100001` — the exhaust value — on 11 / 368, against a median far below it elsewhere
(`ore_ont_1028`, the largest non-exhausting value found, peaks at 19 985). The earlier
characterisation of this constant as "structurally true" is refuted by its own counter.


## 3. Two of the three `DEFAULT_MS` candidates are not what the brief's line numbers say

Establishing what a constant governs came before measuring it, and for this family that
changed the target.

**`lib.rs:2480 DEFAULT_MS = 1000` (`label_cache_timeout_ms`) is DEAD CODE.** The
function is `pub` and documented, but `grep -rn "label_cache_timeout" crates/` returns
**exactly one hit — its own definition.** It has no caller anywhere in the workspace. It
was superseded on 2026-06-25 by `adaptive_label_cache_ms`, which is already the adaptive
lever this audit would otherwise have recommended: `n × per_pair` clamped to
`[LABEL_CACHE_FLOOR_MS = 50, LABEL_CACHE_CEILING_MS = 30_000]`, with
`RUSTDL_LABEL_CACHE_TIMEOUT_MS` still honoured as an override. So the candidate as named
is a **null by unreachability**, and varying it cannot do anything.

The live constant in that family is **`LABEL_CACHE_CEILING_MS = 30_000`**, and its
binding behaviour is decidable by reading `adaptive_label_cache_ms`: with **no**
`--pair-timeout-ms` (the default mode) `per_pair` is `None`, so `base = 30_000` and the
clamp returns **exactly 30 000 ms for every ontology with n ≥ 1**. The per-class label
budget in the default mode is therefore a **fixed 30 s**, `n` classes deep — not
adaptive at all, precisely the pattern this audit is looking for. It is measured in §F
via the surviving env override.

**`lib.rs:4030 DEFAULT_MS = 10_000` (`consistency_fallback_ms`) cannot bind on
`classify`.** Its three call sites (`lib.rs:4923`, `:4971`, `:4983`) are all inside the
`is_consistent` wedge-fallthrough. `classify`'s own inconsistency pre-check is a
different function with a different budget (`classify_inconsistency_budget_ms`). So it
is out of scope for the DNF population this audit targets, and is reported as
**not-applicable rather than measured** — no ontology in the DNF set reaches it.

**`lib.rs:2377 DEFAULT_MS = 3000` (`classify_inconsistency_budget_ms`) is the one live
member of the family on the classify path**, and is already the *fix* for a
v0.4.8 regression of exactly this kind (four ontologies went `ok → dnf` because the
pre-check was unbounded). It is measured in §K.

## 4. `MAX_SEARCH_DEPTH = 256` — the headline. Binds, and is too LARGE.

`crates/owl-dl-reasoner/src/lib.rs:4642`, used at exactly one call site (`:6831`), the
**deadline-bounded** tableau path — which is the classify per-pair path. Iterative
deepening covers the *wedge* cap only; this is the sibling engine's cap and it is
untouched by v0.4.12.

### 4a. Dose–response on `ore_ont_10019`

47 classes, 182 concept rules, 0.01 GB peak RSS, **Konclude 0.06 s** — CLAUDE.md's own
"most extreme peer ratio in the corpus, and it is wall-only", profiled at **84.6% main
tableau**. `classify`, no per-pair budget, 150 s cap:

| cap | wall | rc | `direct` rows | `search_depth0` | deepest level actually reached |
|---|---|---|---|---|---|
| **8** | **60.62 s / 61.36 s** (two runs) | **0** | **59** | 60 786 | 8 — saturated |
| 32 | dnf @150 s | 124 | 0 | 760 742 | 32 — saturated |
| 128 | dnf @150 s | 124 | 0 | 43 360 | 128 — saturated |
| **256 (shipped)** | **dnf @150 s** | 124 | 0 | 3 024 – 4 324 | 256 — saturated |
| 512 | dnf @150 s | 124 | 0 | **0** | **459** |
| 2048 | dnf @150 s | 124 | 0 | **0** | **460** |

Three things this table establishes that a single point could not:

1. **The cap binds**, and the counter says so directly rather than by inference:
   `search_depth0 > 0` at 8/32/128/256 and **exactly 0** at 512 and 2048.
2. **The ontology's genuine requirement is ~460**, read off independently at two
   different caps (512 → 459, 2048 → 460). So 256 is *below* the requirement — and
   raising it to 512 or 2048, which removes the cap entirely, **still DNFs**. The
   "needs more depth" reading is refuted by its own arm.
3. **Only the 36×-smaller cap completes.** Two runs, 60.62 s and 61.36 s, with
   *identical* counters (`search_depth0=60786`, `eval_order_calls=3959`) — deterministic,
   not a lucky scheduling.

This is the `ore_ont_2182` half of the v0.4.12 defect, transplanted to the main tableau:
256 is high enough that a capped branch cannot conclude, so the driver re-descends
through sibling disjuncts, and low enough that it never reaches the 460 the proof would
need. It is wrong in both directions **at the same cap value**, which is exactly what
made a fixed `HYPER_WEDGE_DEPTH` indefensible.

### 4b. It is not one ontology

Same A/B (depth 8 vs the shipped 256), unbounded, 90 s cap, same battery:

| ontology | depth 8 | depth 256 (shipped) | reading |
|---|---|---|---|
| `ore_ont_3250` | **7.80 s, 75 rows** | dnf @90 s | **recovered, 11.5× under the cap** |
| `ore_ont_8666` | **10.15 s, 68 rows** | dnf @90 s | **recovered** |
| `ore_ont_6485` | dnf @90 s | dnf @90 s | no effect — cap binds (`depth0` 75 562 / 90 988) but is not the bottleneck |
| `ore_ont_3281` | dnf @90 s | dnf @90 s | **lowering HURTS**: `search_entries` 10 280 866, `search_depth0` 7 863 837 — two orders of magnitude more re-descent than any other row |

**`ore_ont_3281` is the control that makes this a "both directions" finding rather than
"lower is better".** At depth 8 it does 10.3 M `search` entries and hits the cap 7.9 M
times — the capped-branch re-descent the v0.4.12 write-up describes, now caused by a cap
that is too *small*. `10019` wants 8, `3281` does not. **No single fixed value is right,
which is the disease.**

A second round, targeted at the ontologies the **census** proved the cap binds on
(rather than at ones picked by size), adds the strongest evidence of all — two
ontologies that **already complete** and get ~14× faster with **byte-identical row
counts**:

| ontology | depth 8 | depth 256 (shipped) | `direct` rows | reading |
|---|---|---|---|---|
| `ore_ont_13545` | **3.11 s** | 46.30 s | **2 482 both** | **14.9×, verdict-identical** |
| `ore_ont_2826` | **0.54 s** | 7.29 s | **197 both** | **13.5×, verdict-identical** |
| `ore_ont_10807` | dnf @90 s | dnf @90 s | 0 | no effect (`search_depth0` 39 923) |

`13545` and `2826` are both in v0.4.12's own 16-ontology iterative-deepening recovery
set, at 46.35 s and 7.29 s — i.e. **the wedge-side fix left ~14× of main-tableau depth
cost on the table for them**, and this audit's arms reproduce their post-v0.4.12 walls
to two decimal places (46.30 / 7.29) before removing it.

**Tally over the eight ontologies tried at depth 8 vs 256:** 3 DNFs recovered
(`10019`, `3250`, `8666`), **2 completers ~14× faster with identical answers**
(`13545`, `2826`), 2 unaffected (`6485`, `10807`), 1 actively harmed (`3281`).


### 4c. Lowering the cap is inert on the healthy population

The brief asks for at least one currently-fast ontology per constant. Depth 8 vs the
shipped 256, unbounded, same battery:

| ontology | depth 8 | depth 256 | `direct` rows |
|---|---|---|---|
| `ore_ont_16800` | 15.65 s | 26.78 s | **6 689 both** |
| `ore_ont_2232` | 22.33 s | 22.40 s | **4 128 both** |
| `ore_ont_3164` | 2.76 s | 2.58 s | **90 both** |
| `pizza` | 0.26 s | 0.26 s | **309 both** |

**Verdict-identical on all four.** The mechanism is visible in the census: 335 of the
368 ontologies never enter the main tableau at all (`search_entries = 0`), so the cap is
structurally inert for them. `16800`'s 26.78 → 15.65 s is *not* claimed as a win —
CLAUDE.md's own iterative-deepening write-up records `16800` at a 16.65–22.74 s
run-to-run band, so a single pair of runs cannot resolve it.

### 4d. The recovered closure is sound, and the cost is exactly one pair

`ore_ont_10019` at depth 8, adjudicated against a native Konclude v0.7.0 oracle with
`owl-reasoner-harness/scripts/normalise.py`:

| arm | closure | FP | MISSED (of Konclude's 162) |
|---|---|---|---|
| **depth 8, unbounded** | **158** | **0** | **4** |
| depth 256, `--pair-timeout-ms 50` | 159 | **0** | 3 |
| **depth 256, unbounded (the shipped default)** | **0 — DNF** | 0 | **162** |

**This is the number that decides the shape of the fix.** Lowering the cap is a strict,
sound improvement over the shipped default (158 sound pairs against zero) but it is
**not free** — it loses exactly one pair (`KetoneGroup ⊑ CarbonylGroup`) relative to the
budgeted arm. A globally lower fixed cap would therefore trade one defect for another,
which is precisely the argument that produced iterative deepening rather than
`HYPER_WEDGE_DEPTH = 8`.

## 5. `DIV_WINDOW = 500` — binds most widely of all four

`hyper.rs:2834`, the adaptive-budget divergence early-cut: over a window of 500
branches, if ~all failed (`restores/branches ≥ 98%`) *at saturated depth*, return
`Stalled`. CLAUDE.md already records the knob's shape — *"lower `DIV_WINDOW` gains more,
each step gated by a fresh corpus MISSED net"* — i.e. it is a known
completeness-vs-wall dial that was never made adaptive.

The census shows it firing on the largest number of ontologies of any candidate, and
firing *hard*: `ore_ont_10517` cuts 225 096 windows, `ore_ont_8666` 241 674 of 242 755
(99.6%). A cut rate that near 100% means the window is not discriminating — it is
functioning as an unconditional early exit.

**Dose–response, and it is a null in both directions.** `ore_ont_10019` unbounded,
150 s, five windows:

| `DIV_WINDOW` | wall | rows | `div_windows` | `div_fired` |
|---|---|---|---|---|
| 50 | dnf @150 s | 0 | 71 919 | 37 016 |
| 125 | dnf @150 s | 0 | 32 035 | 16 834 |
| **500 (shipped)** | dnf @150 s | 0 | 12 951 | 9 078 |
| 2 000 | dnf @150 s | 0 | 11 812 | 10 918 |
| 20 000 | dnf @150 s | 0 | 2 538 | 2 479 |

The counter moves monotonically — the knob is provably live — and **the outcome never
changes.** Repeated on the two ontologies the census says it dominates
(`ore_ont_10517`, 225 096 fires; `ore_ont_8666`, 99.6% fire rate) at 50 / 500 /
100 000 000 (effectively disabled): **all six arms dnf @90 s.**

**Verdict: binds very widely, and changes nothing.** Disabling the cut entirely does not
rescue an ontology it is cutting, so `is_diverging` is correctly identifying searches
that were not going to converge. This is the cleanest null in the audit, and it is worth
recording precisely because CLAUDE.md flags the constant as a tuning dial: *tuning it
down buys wall on already-completing work, not recoveries.*

## 6. `MAX_BODY_VARS = 8` — binds, but it is a silent-MISS cap, not a wall cap

`hyper.rs:46`. See §1 Class 2: exceeding it makes `match_body` return `None` and all
three consumers skip the clause. The census finds it rejecting bodies on a real
population, and the striking detail is the *margin*: the ontologies that trip it sit at
`mbv_max_seen = 9` — **one variable over the cap**.

**Dose–response on `ore_ont_10140`** (unbounded, 90 s), the first binder found:

| `MAX_BODY_VARS` | `mbv_reject` | `mbv_max_seen` | `fp_max_steps` | outcome |
|---|---|---|---|---|
| 4 | 16 | 5 | 7 899 | dnf @90 s |
| **8 (shipped)** | **6** | **9** | 7 899 | dnf @90 s |
| 16 | **0** | **12** | **905** | dnf @90 s |
| 64 | 0 | 12 | 905 | dnf @90 s |

**The ontology's actual requirement is 12 variables — 1.5× the shipped cap.** At 8 the
engine permanently discards 6 clause bodies, and the change is not cosmetic: the
fixpoint it then computes is different (`fp_max_steps` 7 899 vs 905). Repeated on two
more census binders (`ore_ont_11629`, `ore_ont_3575`) at 8/16/64: **all six arms
dnf @90 s.**

**Verdict: binds, and is provably too small on real ontologies — but no wall effect was
observed, and none should be expected**, because this is not a budget (§1 Class 2). The
defect it causes is a **silent MISS** that no timing measurement can surface. Every
ontology on which it was seen to bind is already a DNF, so the closure it is corrupting
could not be compared to an oracle here. **That is the gap to close, and it needs a
completing binder, not a faster one.**

## 7. `FIXPOINT_ITERS = 100_000`

`hyper.rs:51`. A prior investigation called this "structurally true" — unreachable
because anywhere-blocking bounds the graph. **The census refutes that**: `fp_max_steps`
reads exactly `100001` (the exhaust value) on **11 / 368** ontologies, all 11 in the
DNF-174 set.

**But the interesting arm is the opposite one.** `ore_ont_2232` — a *healthy*, completing
ontology — unbounded, 90 s:

| `FIXPOINT_ITERS` | outcome |
|---|---|
| 10 000 | **dnf @90 s** |
| **100 000 (shipped)** | **23.27 s, 4 128 rows** |
| 10 000 000 | 22.24 s, 4 128 rows |

So the cap is **load-bearing at its current value**: one order of magnitude lower and a
23-second ontology stops finishing. Raising it is inert there, and inert on the binders
too — `ore_ont_11629` and `ore_ont_12432` at 10 000 / 100 000 / 100 000 000 are
**dnf @90 s in all six arms**.

The `1028` arms are the positive control that the override is actually wired:
`fp_exhaust` moves 0 → 74 → 5 958 as the cap drops 100 000 → 10 000 → 1 000.

**Verdict: binds (11/368), is NOT the null it was recorded as, but no value tried in
either direction changes an outcome for the better.** It is correctly sized, and the
evidence says leave it alone.

## 8. The remaining candidates

### `RUSTDL_MAX_NODES` (default 50 000) — clean null, with a positive control

`ore_ont_10019`, unbounded, 90 s:

| setting | outcome |
|---|---|
| 50 000 (default) | dnf @90 s |
| **0 (cap disabled)** | **dnf @90 s — indistinguishable** |
| 1 (forced) | 57.00 s, 58 rows |

Default and *disabled* are the same run, so the cap **never binds** at its default. The
`=1` arm is the control that proves the knob is live and can change an outcome, which is
what separates "never binds" from "my override did nothing". This reproduces the
`2026-08-02-nominal-blocking-rootcause.md` finding ("`RUSTDL_MAX_NODES` is never hit —
the graph stays under 200 nodes") on a different ontology.

### `ID_SHALLOW_BUDGET_DIVISOR = 4` — clean null

`wine --pair-timeout-ms 25` (CLAUDE.md's own documented budgeted mode, and the only mode
in which the divisor has any effect at all), 150 s:

| divisor | wall | rows |
|---|---|---|
| 1 | 4.61 s | 197 |
| 2 | 4.60 s | 197 |
| **4 (shipped)** | **4.61 s** | **197** |
| 16 | 4.77 s | 197 |

Flat to within noise across a 16× range, verdict-identical. **Never worth making
adaptive.**

### `ID_SHALLOW_BUDGET_MS = 5` — already adaptive, but sitting near a cliff

`ore_ont_13991` — the ontology whose regression the waste-shutoff exists to fix — 120 s:

| `RUSTDL_ID_SHALLOW_MS` | outcome |
|---|---|
| 1 | 33.88 s, 2 558 rows |
| **5 (shipped)** | **34.31 s, 2 558 rows** |
| 50 | **dnf @120 s** |

The 1-vs-5 flatness is the shutoff working as designed (the pre-shutoff document
recorded 5 ms as a DNF and 1 ms as 90.31 s on this same ontology; that gap is now
closed). But **10× upward is a DNF**, so the constant has roughly one order of magnitude
of headroom and no more. Not a lever; a fragility note.

### `classify_inconsistency_budget_ms = 3000` — binds, and changes the ANSWER

`family.ofn`, 120 s. This is the only candidate where varying the constant changes what
the reasoner *says*, not how long it takes:

| budget | wall | `direct` rows | meaning |
|---|---|---|---|
| 300 ms | 1.12 s | **118** | pre-check timed out ⇒ inconsistency **MISSED** |
| **3 000 ms (shipped)** | 2.71 s | **0** | inconsistent ⇒ every class unsat (correct) |
| 30 000 ms | 2.70 s | 0 | same |

This reproduces CLAUDE.md's warning exactly (*"`family.ofn` needs ~2.0 s … anything under
~2.5 s silently re-breaks family"*). The constant is already the *fix* for a v0.4.8
regression, and it is correctly sized — but it sits **~1.5× above its own cliff,
calibrated on one ontology**. A larger `family`-shaped ABox would fall off it silently.
Worth a note, not a lever.

### `label_cache_timeout_ms = 1000` — dead code (§3); `LABEL_CACHE_CEILING_MS = 30_000` — untested-null

The live ceiling was probed on `ore_ont_10109` (a label-cache-heavy DNF: `fp_calls`
1.1 M, `search_entries` 0) at 100 / 1 000 / 5 000 / 30 000 / 0-unbounded ms:
**all five arms dnf @90 s, no dose–response at all.** Reported as a null **on that
target**, with the caveat that a single non-responding ontology is weaker evidence than
the `MAX_NODES` default-vs-disabled control, because no positive control was run for
this knob.

### `consistency_fallback_ms = 10_000` — not applicable (§3)


## 9. Ranked shortlist

"Binds" = a counter proves the constant is reached on a real ontology, at the default
operating mode. "Effect" columns are outcome changes, not counter changes.

| # | constant | binds? | target | halving | doubling / raising | FP-safe to adapt? | predicted win |
|---|---|---|---|---|---|---|---|
| **1** | **`MAX_SEARCH_DEPTH = 256`** (`lib.rs:4642`) | **YES — 27/368; 82% of the 33 onts that reach the main tableau at all, and `search_min_remain = 0` on every one** | `10019`, `3250`, `8666`, `13545`, `2826`, `3281` | **→8: 3 DNFs recovered** (`10019` 60.6 s/59 rows, `3250` 7.8 s/75, `8666` 10.2 s/68) **and 2 completers 14× faster with identical answers** (`13545` 46.30→3.11 s, 2482 rows both; `2826` 7.29→0.54 s, 197 both) | **→512/2048: cap stops binding (`depth0=0`, true need 459–460) and `10019` still DNFs.** Inert on the healthy population (`16800`/`2232`/`3164`/`pizza` verdict-identical) | **YES, both directions** (§1 Class 1). Adjudicated: depth 8 on `10019` is **FP=0** vs Konclude, 158/162 | **Iterative deepening on the main tableau, same shape as v0.4.12's on the wedge.** Predict ≥5 DNF→ok on a 1 920-ont sweep and ≥10× on the `13545`/`2826` class, at 0 lost pairs on an unbounded run. Falsified by: any `ok → dnf`, or any closure that shrinks |
| **2** | `MAX_BODY_VARS = 8` (`hyper.rs:46`) | **YES — 23/368**, 22 of them in the DNF-174 | `10140` (needs **12**), `11629`, `3575` | →4: rejects 16 bodies, still DNF | →16: **rejects 0**, `mbv_max_seen` 12, fixpoint materially different (`fp_max_steps` 7899→905); outcome unchanged on all 3 tried | **NO — asymmetric.** Raising it ADDS entailments (§1 Class 2); needs a Konclude ∪ HermiT adjudication, not a superset check | **A silent-MISS fix, not a perf fix.** Predict: raising to ≥16 changes the closure on some completing ontology. **Untested** — every binder found is a DNF, so no closure could be compared. First step is to find a *completing* binder |
| **3** | `classify_inconsistency_budget_ms = 3000` (`lib.rs:2377`) | **YES**, and it is the only candidate that changes the **answer** | `family.ofn` | →300 ms: **118 direct rows — the inconsistency is MISSED** | →30 000 ms: identical to default | YES (a timeout ⇒ no verdict) | **No lever — a fragility note.** Correct today but ~1.5× above its own cliff, calibrated on ONE ontology. Predict: a larger `family`-shaped ABox falls off it silently |
| **4** | `FIXPOINT_ITERS = 100_000` (`hyper.rs:51`) | **YES — 11/368** (`fp_max_steps = 100001`). **The prior "structurally true" reading is refuted** | `11629`, `12432`, `13276`, `2232` | **→10 000: `ore_ont_2232` goes 23.27 s → DNF.** The cap is load-bearing at its value | →10⁷: inert everywhere tried, including on its own binders (6/6 dnf) | YES, both directions | **Leave it.** Correctly sized. Predict: no sweep-visible change from any value in [10⁵, 10⁷] |
| **5** | `DIV_WINDOW = 500` (`hyper.rs:2834`) | **YES — 47/368**, the widest of all four; fires on 99.6% of windows on `8666` | `10019`, `10517`, `8666` | →50: `div_fired` 9 078→37 016, **outcome unchanged** | →20 000 and →disabled: `div_fired` 9 078→2 479→(off), **outcome unchanged, 6/6 dnf** | YES, both directions | **Null.** Disabling the cut rescues nothing it cuts, so `is_diverging` is identifying genuinely non-converging searches. Tuning it buys wall on already-completing work, not recoveries |
| **6** | `ID_SHALLOW_BUDGET_MS = 5` (`lib.rs:1621`) | already adaptive (waste shutoff) | `13991` | →1 ms: 33.88 s (was a 90.31 s pathology pre-shutoff — the shutoff fixed it) | **→50 ms: DNF @120 s** | YES | **No lever.** ~1 order of magnitude of headroom upward and no more |
| **7** | `RUSTDL_MAX_NODES = 50_000` | **NO — null with a positive control** | `10019` | =1 (forced): 57.00 s / 58 rows — proves the knob is live | **=0 (disabled) is indistinguishable from the default** ⇒ never binds | YES | **Closed.** Reproduces the `nominal-blocking-rootcause` null on a new ontology |
| **8** | `ID_SHALLOW_BUDGET_DIVISOR = 4` (`lib.rs:1629`) | **NO** | `wine --pair-timeout-ms 25` | →1: 4.61 s / 197 | →16: 4.77 s / 197 | YES | **Closed.** Flat and verdict-identical across a 16× range |
| **9** | `label_cache_timeout_ms = 1000` (`lib.rs:2480`) | **NO — dead code**, zero callers (§3) | — | — | — | n/a | **Closed by unreachability.** Its live successor `LABEL_CACHE_CEILING_MS = 30_000` showed no dose–response on `10109` (5 arms, all dnf) — a weaker null, no positive control |
| **10** | `consistency_fallback_ms = 10_000` (`lib.rs:4030`) | **n/a** — all 3 call sites are inside `is_consistent`, unreachable from `classify` (§3) | — | — | — | n/a | **Out of scope** for the DNF population |

### The single recommendation

**Pursue #1, `MAX_SEARCH_DEPTH`.** It is the only candidate that (a) binds on 82% of the
population where it is reachable at all, (b) produced **outcome changes in both
directions** — three DNFs recovered at depth 8, one ontology (`3281`) made two orders of
magnitude worse by the same value — and (c) has the shipped value provably in the
pathological middle on `ore_ont_10019`: 256 both *saturates* (`search_depth0 > 0`) and
sits *below* the ontology's real requirement of ~460, while only 8 completes and 512
completes nothing. That is the same simultaneous both-directions wrongness that made
`HYPER_WEDGE_DEPTH` indefensible, in the sibling engine that iterative deepening does not
yet cover. Everything else is either a null (#5, #7, #8, #9, #10), correctly sized (#3,
#4, #6), or a completeness question that needs different evidence entirely (#2).

## 10. What a spec must do first, and how each claim here can be falsified

1. **Run the ORE-wide two-arm sweep before anything else.** The v0.4.12 arc contains the
   precedent twice over: the `RUSTDL_CLASSIFY_INCONSISTENCY` flip was justified on 12
   ontologies reading −1.5% and a full sweep later found four `ok → dnf`; the first
   iterative-deepening build passed every instance gate and the sweep found
   `ore_ont_13991`. Nothing in this document is a population measurement.
2. **The recoveries here are instance evidence and are quoted as such.** Each is a
   `dnf @cap` versus a completion **in the same battery on the same host**, not against
   a number from another document.
3. **Falsifiers, stated in advance.** Each ranked row carries a prediction that a sweep
   can refute — a recovery count with a named mechanism, or a null that one
   counter-example overturns.

## 11. Reproducing this

The instrumentation is **reverted in the same commit that adds this document** — the
repository is left with no audit knob and no changed default. To re-run any of it,
re-apply the five hook sites listed in §0b (each is a two-line change wrapping the
constant in `audit::<name>(CONST)` plus one counter bump) and rebuild. The counter
semantics that matter:

* `search_depth0` must be counted **separately** from the deadline exit, or the
  `MAX_SEARCH_DEPTH` result inverts (§0b).
* The watchdog (`RUSTDL_AUDIT_EVERY_MS`) is not optional: every target of interest is a
  DNF, and `timeout`'s SIGTERM skips `Drop`.
* Probe at the **default operating mode** (no `--pair-timeout-ms`). A per-pair budget
  masks the depth cap entirely (§2).

Raw data: `dose-results.txt`, `dose2-results.txt`, `dose4-results.txt`,
`census-merged.tsv` in the session scratchpad.
