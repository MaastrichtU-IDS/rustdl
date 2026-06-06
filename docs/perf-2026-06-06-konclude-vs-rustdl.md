# Head-to-head: rustdl vs Konclude (2026-06-06, post-0.3.5 precise-card-deps default-ON)

Run 2026-06-06 at HEAD `bdae558` (release 0.3.5). Companion to
`docs/perf-2026-06-04-konclude-vs-rustdl.md` (the prior anchor, HEAD `f5ee999`).

## What changed since 06-04

0.3.5 flipped `RUSTDL_PRECISE_CARD_DEPS` to **default ON**: the wedge's `≤n`
cardinality-clash pre-check reports a sound over-approximation of the clash's
dependency set instead of `DepSet::ALL`, unblocking dependency-directed
backjumping on cardinality clashes (see `docs/backjump-reconcile-2026-06-06.md`).

**The performance question this doc answers: does the default flip move walls?
Answer: no — it is perf-neutral. No regression anywhere; no measurable speedup.**
The value of 0.3.5 is a *completeness* gain (wine MISSED 34→31, sound), not speed.

## Method

Isolated A/B on the **same 0.3.5 binary**: wall with default (flag ON) vs
`RUSTDL_PRECISE_CARD_DEPS=0` (flag OFF), `classify --pair-timeout-ms 200`,
`/usr/bin/time -f %e`, min of 2 reps (single rep on the >150 s walls). Same host,
same conditions → the delta is purely the flag's effect, free of host-load /
HEAD-drift noise. Konclude walls/reasoning from the `konclude/konclude:latest`
docker image, `classification`, 3-rep min.

Only **out-of-EL** ontologies can exercise the wedge cardinality path; EL/Horn
ontologies (GALEN, notgalen, alehif) are Horn-shortcircuited and never enter it
(GALEN included below as a sanity check — it must be exactly flat).

## A/B — precise-card-deps OFF vs ON (same binary)

| Ontology | Fragment | OFF (`=0`) | ON (default) | Δ |
|---|---|---:|---:|---:|
| wine | out-of-EL | ~311 s | ~311 s | **neutral** (see below) |
| pizza | out-of-EL | 2.06 s | 2.06 s | 0.0% |
| ore-10908-sroiq | out-of-EL (Q) | 5.40 s | 5.39 s | −0.2% |
| ore-15672-shoin | out-of-EL (N) | 29.10 s | 29.10 s | 0.0% |
| sio-stripped | out-of-EL | 31.99 s | 32.23 s | +0.8% |
| np-module | out-of-EL | 1.32 s | 1.32 s | 0.0% |
| sio-fp2-module | out-of-EL | 0.44 s | 0.45 s | +2.3%¹ |
| GALEN (Horn sanity) | Horn | 0.58 s | 0.58 s | 0.0% ✓ |

¹ +2.3% on a 0.44 s workload = ~10 ms — run-to-run noise, not a real delta.

**Every ontology is flat within noise. GALEN (Horn) is exactly flat, as it must
be (shortcircuited).** The SROIQ fixtures that *do* carry cardinality
(ore-10908, ore-15672, pizza) are also flat: they don't hit the wine-style
cardinality-clash-under-disjunction pattern, so default-ON is **free** for them —
no cost, no benefit.

## wine — the wall is neutral; the −25% was noise (correction)

The 0.3.5 commit messages (`b5ab3e8`, `6d656dd`, `bdae558`) and the early drafts
of `docs/backjump-reconcile-2026-06-06.md` cited a **−25% wall on wine**. That
came from a *single* OFF-vs-ON pair measured during the closure-diff run
(OFF 311.20 s, ON 232.45 s). A clean 5-run re-measurement shows it does **not**
reproduce — the 232 s was a one-off light-load outlier:

```
wine OFF (=0):       311.13, 311.20                         → ~311.2 (tight)
wine ON (default):   232.45*, 311.25, 312.08, 312.10, 312.06 → ~311.7 (* outlier)
```

**The wine wall is neutral (~311 s both ways).** This is consistent with the
mechanism: the 3 pairs that backjumping newly resolves were capped at the 200 ms
per-pair budget under OFF anyway, so resolving them faster saves ≈0.6 s — lost in
noise on a 311 s wall. The −25% claim is **retracted**; the commit messages
predate this re-measurement and could not be rewritten (shared `main`).

**The completeness gain is unaffected and stands:** wine MISSED **34→31**, FP=0,
shown algorithmic (budget-independent) by the 2000 ms-budget control in
`docs/backjump-reconcile-2026-06-06.md` (OFF flat at 34 at both 200 ms and
2000 ms; ON at 31 at both). That control measured *verdicts*, not walls, and is
not affected by this wall correction.

## Konclude anchors (fresh spot-check) + ratios

Konclude reasoning times match the 06-04 doc (stable anchor confirmed); walls are
lower today purely because docker overhead is lighter on this host.

| Ontology | Konclude wall | Konclude reasoning | (06-04 reasoning) |
|---|---:|---:|---:|
| ore-10908 | 0.49 s | 46 ms | 50 ms ✓ |
| ore-15672 | 0.45 s | 7 ms | 12 ms ✓ |
| pizza | 0.52 s | 51 ms | 101 ms ≈ |
| wine | 0.63 s | 119 ms | (new) |

Because the rustdl walls did not move, **the Konclude ratios are unchanged from
06-04**: rustdl wins on tiny + Horn (GALEN/notgalen beat Konclude wall-to-wall),
ORE-10908 stays inside the ≤5× target (5.39 s; ~3× on the 06-04 anchor), and
ore-15672 / sio-stripped remain the SROIQ gap (16×, 13×).

**wine is rustdl's worst SROIQ ratio** (new this corpus): ~311 s vs Konclude's
0.63 s — a nominal + cardinality stressor where most of the 137²≈18.7 k pairs hit
the 200 ms tableau budget. 0.3.5 recovers 3 of its MISالسES but does not close the
perf gap; that gap is the open frontier (deeper-search conflict provenance / the
`solve_at_most` fallback site, and wedge incompleteness on the residual 31).

## Headline

0.3.5 is **perf-neutral** vs 0.3.4 across the corpus (A/B flat; GALEN exactly
flat) — the default-ON flip carries **no regression** and is free on
cardinality-bearing SROIQ that doesn't hit the backjump pattern. Its value is the
sound **completeness** gain on wine (34→31), not speed. The earlier −25% wall
figure was a single-run host-load artifact and is retracted here.
