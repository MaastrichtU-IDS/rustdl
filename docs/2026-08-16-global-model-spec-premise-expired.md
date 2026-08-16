# The global-model rewrite's premise has expired — and the real target moved

**Date:** 2026-08-16 · **Retires P0 and P1 of
`docs/superpowers/specs/2026-06-10-global-model-rewrite-design.md`. Re-aims P2.**

## The spec's premise

> "Today's classifier is **per-pair** … On alehif (247 classes) that is **16 048 wedge
> `decide` calls** … the inefficiency is **probe count × per-probe redundancy**."

Its P0 gate found the picture already split: alehif was *walk-overhead*-bound with
essentially free probes, while `ore-10908` was *probe*-bound — 645 medium-cost probes
driving ~19 s of a 23 s wall. It parked the project pending one measurement: **are those
645 probes refutable (`Sat`) or real subsumptions (`Unsat`)?**

## That question can no longer be asked

Re-run on today's binary (v0.4.19, unbounded per-pair and global budgets so nothing is
truncated):

| fixture | spec (2026-06-10) | **today** | tableau probes | label-cache prunes |
|---|---|---|---|---|
| `ore_ont_10908` | 23 s, 6,881 probes | **0.3 s** | **0** | 33,118 |
| `ore_ont_15672` | 140 s, 1,969 probes | **0.1 s** | **0** | 2,795 |
| `sio` | (24 s historically) | **0.7 s** | **0** | 121,413 |
| `wine` (defaults) | — | **3.4 s** | **0** | — |
| `ore_ont_11378` | — | 3.0 s | **0** | 1,217,499 |

**`tableau = 0` everywhere.** The 645 probes whose refute/confirm split was the go/no-go
datum do not exist any more. Neither do `ore-10908`'s 6,881 or `ore-15672`'s 1,969.

So:

* **P0 is unanswerable** — its subject is gone.
* **P1 (de-redundancy of probe dispatch) has nothing to de-duplicate** — probe count is
  already zero on every fixture the spec named.
* **P2 (pseudo-model merging) was specced to "shrink the residual further"** — and the
  residual is already empty on these ontologies.

`alehif.ofn` is no longer in the corpus, so the spec's headline case cannot be re-measured
at all.

## What I got wrong on the way here

Seeing `tableau=0` alongside a 1,312 ms `label_cache_build`, I inferred the cache was
overhead: it prunes 1.2 M pairs so that zero probes run, so surely the pruning bought
nothing. **Exactly backwards, and the test says so:**

| `ore_ont_11378`, label cache | wall | result |
|---|---|---|
| on | **3.0 s** | 8,732 rows, `tableau=0` |
| `RUSTDL_LABEL_HEURISTIC=0` | **DNF at 300 s** | nothing |

`tableau = 0` **is** the pruning working. Without it those 1.2 M pairs each become a probe.
The cache costs 1.3 s and saves >300 s. It is the most load-bearing component in the
classify path, not overhead.

## The target that is actually there

The failing ontologies are not probe-bound; they are bound by **building the label cache
itself**:

| ontology | `label_cache_build` | `tier_walk` | outcome |
|---|---|---|---|
| `ore_ont_11311` (8,022 classes) | **118,479 ms** | 6 ms | DNF |
| `ore_ont_9944` (8,008) | 118,324 ms | 5 ms | DNF |
| `ore_ont_7914` (17,680) | 114,042 ms | 11 ms | DNF |

The cache is `n` per-class wedge runs with no sharing: **≥14.7 ms per class marginal, against
0.14 ms per class for the global saturation fixpoint — ≥105×** (see
`docs/2026-08-16-inverse-trigger-analysis-insufficient.md` for the measurement, and note the
per-class figure is a lower bound because the phase never finished).

So the restated target is **not** "issue fewer probes". It is:

> **Obtain the same refutation power as `n` per-class label caches, without paying `n`
> independent wedge runs.**

That is P2's mechanism — one model refuting many pairs — but the justification is now
completeness-of-pruning at scale, not residual shrinkage, and P0/P1 are not prerequisites.

## Evidence that a global refuter would pay

* The label cache's product is almost entirely **refutation**: 1,217,499 prunes against 683
  pass-throughs on `ore_ont_11378` (99.94% refuted).
* Its confirmations are worth little: over 53 sampled ontologies the whole post-saturation
  phase changes **nothing on 46**, and where it does the gain is 2–234 closure pairs carried
  by **≤7 superclasses** (`docs/2026-08-16-post-saturation-phase-value.md`).
* Refutation from a model is the **sound** direction (`D ∉ L(C) ⟹ C ⋢ D`) and is already
  shipped in two places — the Phase-7 label heuristic and `RUSTDL_PSEUDO_MODEL` for realize.
  Confirmation from a model is the FP-unsound direction that killed the snapshot cache, and
  nothing here proposes it.

## Before writing code

**Re-run the spec's P0-style instrumentation on the ontologies that actually fail**
(`11311`, `9944`, `7914`) rather than on the ones it named in June. The question is no
longer "how many probes and are they refutable" but:

1. How many of the `n` per-class cache builds are individually expensive, versus a long thin
   tail? (Decides whether a global model must replace all of them or only the tail.)
2. Does a single global model retain the prune rate? A 99.94% prune rate is what makes
   `tableau=0` possible; a global model that pruned 90% would put 120,000 pairs on the
   tableau and be catastrophically worse.

Point 2 is the real risk and has no measurement yet. **The spec should not be resumed as
written**; it should be re-derived from these numbers.
