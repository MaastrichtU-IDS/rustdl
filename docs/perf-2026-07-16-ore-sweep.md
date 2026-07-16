# ORE performance sweep — 2026-07-16 (re-measured, new machine)

Full `rustdl classify` performance sweep over the ORE 2015 `pool_sample` corpus.
**Re-measurement on a different machine** — numbers differ from prior docs by design.

## Setup

- **Machine:** 32 cores, 251 GB RAM (235 GB free, idle) — the multi-GB RSS tail is not
  memory-constrained here.
- **Binary:** fresh release build at `ca24083` (HEAD), default config (wedge + label
  heuristic + Horn-shortcircuit + all default-ON flags).
- **Corpus:** `/data/dumontier/ore-run/pool_sample/files/*.owl` — **1920** ontologies,
  20.8 GB total, median 1.19 MB, p90 24 MB, p99 156 MB, max 563 MB.
- **Command:** `rustdl classify <file>` — **default mode** (unbounded per-pair; no
  `--pair-timeout-ms`), **60 s per-file outer wall cap**, 4-way concurrent, 8 rayon
  threads each.
- Raw data: `bench-results/ore-perf-sweep-20260716.tsv`.

## Headline

| outcome | count | of 1920 |
|---|---|---|
| **front-end reject** (anonymous individuals — Phase 7 gap) | 446 | 23 % |
| reached reasoner | 1474 | 77 % |
| — **classified < 60 s** | **1180** | 80 % of reached |
| — **DNF > 60 s / OOM** | **294** | 20 % of reached |

The 446 ERR1 are **100 % the same known front-end limitation** ("anonymous individuals are
not supported") — a converter gap, not a reasoner failure. They reject fast (median 0.05 s)
and are excluded from the timing denominator.

## Timing (the 1180 that classified)

- **median 0.050 s**, mean 2.72 s, p90 5.89 s, p95 16.5 s, p99 48.0 s, max 59.6 s.
- **836 (71 %) under 0.5 s**; 928 (79 %) under 1 s.

| wall bucket | count |
|---|---|
| < 0.5 s | 836 |
| 0.5–1 s | 92 |
| 1–5 s | 117 |
| 5–15 s | 69 |
| 15–30 s | 30 |
| 30–60 s | 36 |

The bulk of the ORE corpus classifies **essentially instantly**; the cost lives entirely in
a tail.

## How performance scales with size

| size band | reached reasoner | classified | DNF | median (classified) |
|---|---|---|---|---|
| < 1 MB | 823 | 776 (94 %) | 47 | 0.02 s |
| 1–5 MB | 303 | 269 (89 %) | 34 | 0.60 s |
| 5–20 MB | 169 | 94 (56 %) | 75 | 4.48 s |
| 20–50 MB | 121 | 35 (29 %) | 86 | 7.07 s |
| > 50 MB | 58 | 6 (10 %) | 52 | 8.87 s |

DNF rate climbs steeply with size: 6 % (<1 MB) → 90 % (>50 MB). **Two tail components:**

1. **Scale / memory** — big files dominate the DNF tail (52 of 77 files >50 MB DNF). This is
   the known RSS/scale asymptote.
2. **Algorithmic (small-but-hard SROIQ)** — 81 DNF are <5 MB, and the slowest *classified*
   files span sizes (`ore_ont_12174` 2.1 MB → 55.7 s; `ore_ont_13071` 5.2 MB → 58.1 s). These
   are the disjunctive/nominal search-explosion class characterized this session (dense-SROIQ
   tail, dead disj deps).

## Bounded mode (`--pair-timeout-ms 50`) — barely moves the needle

Re-swept all 1920 identically but with a **50 ms per-pair timeout** (sound under-approximation:
a pair whose tableau verdict doesn't finish in 50 ms defaults to "not subsumed").

| mode | classified <60 s | DNF | OK-of-reached |
|---|---|---|---|
| default | 1180 | 294 | 80 % |
| **bounded 50 ms** | **1201** | **273** | **81 %** |

Net: **+21 classified, only 24 files recovered** (KILLED→OK); median wall unchanged (50→60 ms,
noise). **The DNF tail is scale-dominated, not per-pair-search-bound** — a huge ontology has
O(n²) pairs, so `50 ms × millions of pairs` (plus parse + label-cache build + RSS) blows the
60 s cap regardless of the per-pair bound. Per-pair bounding only rescues the *small-hard*
algorithmic slice (~24 files, e.g. `ore_ont_10019`: default DNF → bounded 6.36 s). It does
**not** address the big-file scale tail.

## Konclude head-to-head (234-ontology pilot subset)

The pilot subset (`/data/dumontier/ore-run/pilot`, 234 onts with a pre-converted `canon.owx`;
stratified, includes the SROIQ OutOfFragment cases — i.e. the *harder* end of the corpus).
Konclude run via `docker … konclude/konclude:latest classification -w AUTO`; **`reason_ms`** =
Konclude's own reported classification time (excludes docker startup + parse). rustdl walls
from the sweeps above.

**Who classifies the 234:** rustdl default **212**, rustdl bounded **220**, Konclude **222**.

**Konclude reasoning time:** median **5 ms**, mean 39 ms, p90 72 ms, p99 326 ms, max 1.79 s;
63 % under 10 ms, none over 10 s. (Total docker wall median 0.53 s — startup-floored ~0.6 s.)

**Ratio where both succeed** (`rustdl_wall / konclude_reason_s`; generous to Konclude — rustdl's
wall includes parse, Konclude's `reason_ms` does not):

| comparison | n | median | mean | p90 | max |
|---|---|---|---|---|---|
| rustdl default vs Konclude | 180 | **10×** | 61× | 71× | 4204× |
| rustdl bounded-50 vs Konclude | 188 | **10×** | 60× | 96× | 1697× |

**The qualitative gap — 20 onts rustdl DNFs (>60 s) but Konclude reasons in mean 228 ms**, and
they are mostly *small* (14 of 20 are <5 MB, many <1 MB): `ore_ont_10019` (Konclude 13 ms),
`ore_ont_3250` (8 ms), `ore_ont_6485` (11 ms), `ore_ont_8666` (14 ms)… **rustdl's hardest cases
are Konclude's trivial cases** — the small-hard SROIQ search-explosion class, exactly the
dense-SROIQ tail this session mapped to the (soundness-ruled-out) search-reuse frontier.
(Reverse: 10 onts where Konclude emitted no `reason_ms` — likely inconsistency / empty-class /
a Konclude quirk — that rustdl produced a result for; not counted as rustdl wins.)

**Exemplar `ore_ont_10019`** (this session's running example): default **DNF**, bounded-50
**6.36 s**, Konclude **13 ms** (~490× even after the bounded rescue).

**Reading:** on the easy bulk, rustdl is fast in absolute terms (median tens of ms) and, with
docker startup + parse counted on both sides, roughly comparable to Konclude. Konclude's
advantage is (a) a faster reasoning core on the median and (b) — the real gap — it does
**not** have rustdl's DNF tail: the cases rustdl can't finish, Konclude finishes in
milliseconds. That tail is the SROIQ search-reuse frontier, not a constant-factor deficit.

## Parse vs reasoning split (apples-to-apples with Konclude `reason_ms`)

The ratios above use rustdl's **total wall** (parse + reason) against Konclude's **`reason_ms`**
(reason only) — generous to Konclude. Added an env-gated phase timer (`RUSTDL_TIMING=1` →
`TIMING parse_ms=… classify_ms=…`), separating `parse_ofn` (horned-owl read) from `classify`
(convert/preprocess + reasoning), and re-measured the 234 pilot set (bounded-50, 220 completed).

- **rustdl parse:** median 3.7 ms, mean 12.7 ms, p90 41.5 ms, max 109 ms.
- **rustdl classify:** median 12.7 ms, mean **1962 ms** (tail-dominated), p90 4758 ms, max 48 s.
- **Parse is a real fraction of the easy-ont wall: median 19 %, mean 23 %** — the horned-owl
  read is a meaningful chunk of the sub-second onts (and it is *excluded* from Konclude's
  `reason_ms`, so the wall-based ratio double-counted it against rustdl).

**Fair ratio — rustdl `classify_ms` vs Konclude `reason_ms`, both parse-excluded** (188 onts
where both succeed):

| framing | median | mean | p90 | p99 | max |
|---|---|---|---|---|---|
| total wall / reason_ms (prior) | 10× | 61× | 71× | — | 4204× |
| **classify_ms / reason_ms (fair)** | **4.7×** | 50× | 88× | 809× | 1168× |

**Separating parse halves the median gap (10× → 4.7×)**, and on **11 % of onts (20/188) rustdl's
reasoning is ≤ Konclude's**. Side-by-side on the easy end confirms near-parity: `ore_ont_10056`
classify 1.3 ms vs Konclude 1 ms (1.3×), `ore_ont_10134` 6.7 ms vs 4 ms (1.7×). The large mean
/ tail (p99 809×, max 1168×) is **entirely the hard SROIQ onts** where `classify_ms` runs to
seconds (`ore_ont_10019`: classify 6284 ms vs Konclude 13 ms).

**Refined reading:** rustdl's *reasoning core* is within a small factor of Konclude on the
common case (median ~4.7×, near-parity on the easy end, faster on 11 %); a non-trivial slice of
the apparent wall gap was parsing, not reasoning. The genuine, large gap is confined to the
hard-SROIQ tail (search-reuse frontier) — plus, separately, rustdl's slower parse front-end
(horned-owl) which shows up as ~20 % of the easy-ont wall.

## Stratified by OWL profile (all 1920)

ORE stratifies the corpus into `el` / `dl` / `pure_dl` pools (via per-profile `fileorder.txt`).
EL and pure-DL are disjoint; the DL pool overlaps both. Disjoint assignment with priority
**EL → pure-DL → DL** gives **EL 594, DL 554, pure-DL 772 = 1920**.

**Default `classify`:**

| profile | total | reached reasoner | classified | DNF | anon-reject | median | p90 |
|---|---|---|---|---|---|---|---|
| **EL** | 594 | 594 | **536 (90 %)** | 58 | **0** | 0.05 s | 4.2 s |
| **DL** | 554 | 418 | 342 (82 %) | 76 | 136 | 0.06 s | 5.5 s |
| **pure-DL** | 772 | 462 | **302 (65 %)** | 160 | **310** | 0.06 s | 11.9 s |

A clean monotone gradient with expressiveness:
- **EL is rustdl's strong fragment** — 90 % classified, **zero anonymous-individual rejects**
  (EL ontologies here don't use them), shortest tail (p90 4.2 s).
- **pure-DL is the hard end** — only 65 % classified, **40 % rejected** on the anonymous-
  individuals converter gap, and the longest tail (p90 11.9 s). The median stays ~50 ms across
  *all* profiles (71 % of everything is trivial); the profile difference lives in the **tail and
  the reach**, not the common case.

**Bounded (`--pair-timeout-ms 50`)** barely shifts it, and the small recovery concentrates
where expected — pure-DL: 302 → 318 classified (+16, the small-hard SROIQ slice); EL 536 → 535
and DL 342 → 348 essentially flat.

**vs Konclude, parse-excluded (`classify_ms` / `reason_ms`, 234 pilot subset):**

| profile | n | median | p90 | max | rustdl ≤ Konclude |
|---|---|---|---|---|---|
| **EL** | 56 | **2.0×** | 33× | 179× | **14 (25 %)** |
| **DL** | 59 | 5.7× | 61× | 809× | 5 (8 %) |
| **pure-DL** | 95 | **7.9×** | 225× | 1168× | 4 (4 %) |

The reasoning gap also grows monotonically: on **EL, rustdl's reasoning is within 2× of Konclude
and beats it on 25 % of ontologies**; on pure-DL it is ~8× median with a 1168× tail and wins on
only 4 %. (Konclude reason itself scales the same way: EL median 3 ms / max 36 ms → pure-DL
median 9 ms / max 1219 ms.) **rustdl is competitive-to-winning on EL and loses progressively as
expressiveness rises** — consistent with the EL/Horn-complete, SROIQ-tail-bound characterization.

## Caveats

- **DNF = `KILLED`** conflates the 60 s cap with OOM; with 235 GB free, OOM was rare, so
  `KILLED` ≈ ">60 s". Some DNF files finish given more wall (e.g. `ore_ont_15672`-class) or a
  `--pair-timeout-ms` bound.
- **This is default mode** — the honest out-of-the-box view. A bounded mode
  (`--pair-timeout-ms`) is a sound under-approximation that would convert much of the
  *small-hard* DNF tail into fast "not-subsumed" verdicts (at some completeness cost);
  measuring that mode is a separate sweep.
- Not a head-to-head: native Konclude classifies most of these in ms–seconds and wins across
  the board (see `docs/perf-2026-06-08-konclude-vs-rustdl.md`). This sweep characterizes
  rustdl's own distribution on this machine.

## Takeaway

On the ORE corpus, default `rustdl`: rejects 23 % up front (anonymous-individuals converter
gap), and of what it reasons on, **classifies 80 % within 60 s — 71 % of those instantly
(< 0.5 s), median 50 ms**. The 20 % DNF tail is size-dominated (big-file scale/RSS) plus a
smaller algorithmic core (small-hard SROIQ). This matches the standing characterization:
rustdl is fast on the bulk and EL/Horn fragment; the open frontier is the SROIQ scale tail
(memory) and the search-reuse tail (the reuse-trap, ruled NO-GO on soundness this session) —
not the common case.
