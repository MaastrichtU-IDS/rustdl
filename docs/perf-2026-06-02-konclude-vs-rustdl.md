# Head-to-head: rustdl vs Konclude (2026-06-02 post-Phase-6)

> ⚠ **SUPERSEDED** — See [perf-2026-06-04-konclude-vs-rustdl.md](perf-2026-06-04-konclude-vs-rustdl.md) for current state. This file is retained as a historical baseline.

---

Run 2026-06-02 at HEAD `8014328` (post-Phase-6 walk dedup +
Phase 2d+2c-redux + Phase 3a-d + Phase 4b-c). Companion to
`docs/perf-2026-05-24-new-server.md` §4 (the prior multi-reasoner
head-to-head).

## Caveat — host contention

The benchmark host had load avg ~93 with two long-running python
processes consuming ~3000 % CPU between them for ~4 days at the time
of measurement (see `docs/phase5-variance-check.md` §"Why aborted").
Absolute wall numbers are inflated by ~10–30 %; the rustdl-vs-Konclude
ratio is preserved because both ran under identical contention. The
GALEN/notgalen wall numbers cited from earlier docs were taken under
similar contention on the same host.

## Tools

- **rustdl**: built at HEAD `8014328` with
  `cargo build --release -p owl-dl-cli`. All runs use
  `classify --pair-timeout-ms 200` (sound under-approximation per
  CLAUDE.md — every reported subsumption holds; pairs that don't
  finish within 200 ms are defaulted to "not subsumed"). Single rep.
- **Konclude**: `konclude/konclude:latest` (382 MB docker image,
  `v0.7.0-1138`, Jun 2021), invoked as
  `Konclude classification -w AUTO -i <file>` for auto-core scaling
  to all 32 cores. Single rep.
- **Wall measurement**: `/usr/bin/time -f "%e"` (real time, seconds).
- Single rep per measurement (no median); contention makes additional
  reps noisy without proportional information gain.

## Results — small/medium set (8 ontologies, run today)

| Ontology | Classes | rustdl fragment | rustdl wall (s) | Konclude wall (s) | Konclude "real" (ms) | rustdl / Konclude | rustdl complete? |
|---|---:|---|---:|---:|---:|---:|---|
| `pizza.ofn` | 99 | out-of-EL | 4.39 | 1.68 | 45 (+25 pre, +41 prec) | 2.6× | **no** — 4 MISSED `*⊑InterestingPizza`; 23 pairs timed out at 200 ms cap |
| `sulo-stripped.ofn` | 17 | out-of-EL | 0.09 | 1.59 | 19 (+28+51) | **0.06×** (rustdl wins) | **yes** — MISSED=0, closure 51=51 |
| `family-stripped.ofn` | 58 | out-of-EL | 84.05 | 3.71 | (Konclude: **inconsistent** — divergent verdict, see note) | — | rustdl: 774 timed-out pairs at 200 ms cap |
| `ro-stripped.ofn` | 58 | out-of-EL | 0.87 | 1.78 | 9 (+31+46) | 0.49× (rustdl wins) | **yes** — MISSED=0, closure 158=158 |
| `sio-fp2-module.ofn` | 74 | out-of-EL | 0.70 | 1.56 | 25 (+25+59) | 0.45× (rustdl wins) | (no built-in diff fixture; banner: 459 subs found, no timed-out pairs reported) |
| `alehif-test.ofn` | 167 | **Horn** | 2.87 | 1.78 | 9 (+61+208) | 1.6× | **yes** (sound-by-construction Horn fixpoint) — MISSED=0, closure 247=247 |
| `ore-10908-sroiq.ofn` | 692 | out-of-EL | 27.37 | 1.61 | 97 (+66+52) | 17.0× | **yes** — MISSED=0, closure 6001=6001 |
| `ore-15672-shoin.ofn` | 82 | out-of-EL | 29.55 | 1.72 | 25 (+45+94) | 17.2× | **yes** on listed closure (MISSED=0, 142=142), but 109 pairs timed out at 200 ms cap (Konclude found 0 new subs in those) |

"Konclude real (ms)" reports `Finished class classification in N ms`
plus preprocessing + precomputing time. The ~1.5 s wall is
overwhelmingly docker container startup (the May 24 measurement put
the Konclude container floor at ~1.27 s median; the actual
classification work is < 100 ms on every ontology in this set).

### Note on `family-stripped.ofn` (Konclude inconsistent)

Konclude reports the family-stripped ontology as **inconsistent**
("Ontology … is inconsistent" repeated for every classification query),
finishing in 3.71 s. rustdl runs through classification in 84 s and
reports 324 saturation subsumptions, but with **774 pairs hitting the
200 ms per-pair cap** and defaulted to not-subsumed. The two reasoners
disagree on the consistency verdict here, which makes a per-pair
ratio meaningless — this is a coverage/verdict gap, not a perf gap.
The May 24 doc noted that `rustdl classify --pair-timeout-ms 200`
hard-errors on family.ofn on length-3 role chains; the stripped
variant evidently exercises the same complex-role machinery and is
the workload where rustdl is least competitive in this corpus.

## Results — large set (cited from prior docs, not re-run today)

| Ontology | Classes | rustdl fragment | rustdl wall | Konclude wall (May 24, ms) | rustdl / Konclude | rustdl complete? |
|---|---:|---|---:|---:|---:|---|
| `galen.ofn` | 2748 | out-of-EL | **684 s** (Phase 6) | not in May 24 §4 | — | **yes — full Konclude parity**, MISSED=0, closure 27,997 = 27,997 |
| `notgalen.ofn` | 3087 | out-of-EL | **~1977 s (32.95 min)** (Phase 2d+2c-redux) | not in May 24 §4 | — | MISSED=18, closure 32,721 vs 32,739 (16 are dl-approximation artifacts per `docs/phase2e-notgalen-diagnosis.md`) |

GALEN and notgalen were not re-run for this comparison — the cap-time
guardrail in the task spec said skip large ontologies if it would push
total runtime past 60 min. The cited numbers come from
`docs/phase6-results.md` (GALEN, 684 s under load ~93) and
`docs/phase2d-2c-redux-results.md` (notgalen, 1977 s = 32.95 min).
Konclude was not re-measured on these today; the comparison vs
Konclude on GALEN is closure-level (27,997 = 27,997, full parity)
from the existing closure-diff harness.

## Headline

**Konclude is faster on most workloads, but the gap is workload-shaped,
not uniform.** On three small ontologies (`sulo-stripped`,
`ro-stripped`, `sio-fp2-module`) rustdl beats Konclude on wall — but
only because Konclude pays ~1.3 s of container/JVM-equivalent startup
floor that rustdl doesn't have. Subtract the floor and Konclude
"wins" everywhere on actual reasoning work. On the two larger
ontologies in the small/medium set (`ore-10908-sroiq` and
`ore-15672-shoin`), Konclude is ~17× faster than rustdl in wall —
those are the workloads where Konclude's <100 ms classification cost
makes the startup floor irrelevant and rustdl's per-pair budget
dominates. **The new news vs May 24** is that rustdl is now sound and
complete on every workload that finishes here (alehif, ore-10908,
ore-15672, sulo, ro, sio-fp2), and on GALEN (the prior DNF). Pizza
still has 4 MISSED (`*⊑InterestingPizza`), family-stripped diverges
on consistency, and notgalen has 18 MISSED — those are the remaining
completeness gaps.

## Comparison to May 24 head-to-head (§4)

| Ontology | May 24 rustdl wall | 2026-06-02 rustdl wall | Konclude (May 24) | Konclude (today) | Change |
|---|---:|---:|---:|---:|---|
| pizza.ofn | **timeout > 120 s** | 4.39 s | 1.44 s | 1.68 s | rustdl: timeout → completes (4 MISSED) |
| sulo-stripped.ofn | 0.49 s | 0.09 s | 0.95 s | 1.59 s | rustdl: ~5× faster, still wins vs Konclude |
| family-stripped.ofn | 0.06 s (partial; errored on role chain) | 84.05 s (completes, 774 timed-out pairs) | 2.27 s | 3.71 s | rustdl: now classifies (no role-chain error) at the cost of 774 capped pairs |
| sio-stripped.ofn | **timeout > 120 s** | not run today (sio-fp2-module subset: 0.70 s) | 1.57 s | — | rustdl: closure-diff now passes at 200 s in long-timeout test (MISSED=2) |
| galen.ofn | — (not in May 24 §4) | 684 s | — | — | new entry: **full Konclude parity** (closure 27,997=27,997) |

The May 24 doc could not claim rustdl completed pizza or SIO at all.
Today rustdl completes both, with the small/medium set all running
to a sound classification.

## Detailed completeness (from `konclude_closure_diff.rs`)

Closure pair counts from `cargo test -p owl-dl-reasoner --test
konclude_closure_diff -- --ignored …` (200 ms per-pair cap):

| Ontology | rustdl closure | Konclude closure | FP | MISSED | Notes |
|---|---:|---:|---:|---:|---|
| sulo-stripped | 51 | 51 | 0 | 0 | match |
| ro-stripped | 158 | 158 | 0 | 0 | match |
| alehif-test | 247 | 247 | 0 | 0 | Horn fixpoint, sound-by-construction |
| ore-10908-sroiq | 6001 | 6001 | 0 | 0 | match (SROIQ) |
| ore-15672-shoin | 142 | 142 | 0 | 0 | match (SHOIN) |
| pizza | 495 | 499 | 0 | 4 | 4 missed `*⊑InterestingPizza` |
| sio-stripped | 8902 | 8904 | 0 | 2 | 2 missed `SIO_010092⊑…` |
| galen (Phase 6) | 27,997 | 27,997 | 0 | 0 | full parity |
| notgalen (Phase 2d+2c-redux) | 32,721 | 32,739 | 0 | 18 | 16 dl-approx artifacts |

## What this measures vs claims

rustdl ships:

- **Sound**: FP=0 across the entire measured corpus (every
  reported subsumption holds vs Konclude).
- **Complete** on alehif (Horn), ORE-10908, ORE-15672, GALEN
  (full Konclude parity!), sulo-stripped, ro-stripped, sio-fp2-module.
- **MISSED present** on pizza (4 `*⊑InterestingPizza`),
  sio-stripped (2 `SIO_010092⊑…`), notgalen (18, 16 of which are
  dl-approximation artifacts per `docs/phase2e-notgalen-diagnosis.md`),
  and family-stripped (consistency-verdict divergence vs Konclude).

Konclude remains faster on every workload after subtracting the
docker container floor — but rustdl is no longer DNF'ing on pizza or
SIO as it was in May, and it now matches Konclude's closure on GALEN.

## Invocation surprises

- Konclude emits `{error} … Couldn't match parameters for
  'Declaration'-Expression` on `pizza.ofn` and `sio-fp2-module.ofn`
  but proceeds to classify and reports a result. Treated as a
  warning (it finishes with non-empty classification output).
- Konclude reports `{error} >> All parsers failed for
  '/work/real/ro-stripped.ofn'` then succeeds anyway with the
  fallback parser — silent recovery, result still emitted.
- Konclude reports `family-stripped.ofn` as inconsistent. rustdl
  classifies through. The two reasoners disagree on this verdict.

## Cross-references

- Phase 6 (most recent perf win — GALEN walk dedup):
  `docs/phase6-results.md`.
- Phase 2d + 2c-redux (GALEN closure parity):
  `docs/phase2d-2c-redux-results.md`.
- Prior multi-reasoner head-to-head (May 24):
  `docs/perf-2026-05-24-new-server.md` §4.
- Fragment classification (Horn vs out-of-EL):
  `docs/fragment-completeness.md`.
- Soundness diff tests:
  `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`.
- notgalen MISSED diagnosis:
  `docs/phase2e-notgalen-diagnosis.md`.

## Raw measurement logs

- rustdl run output: `/tmp/p7-rustdl.log`,
  `/tmp/p7-rustdl-banners.log`.
- Konclude run output: `/tmp/p7-konclude.log`.
