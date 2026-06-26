# SP2 sat-seed gate — RESULTS + VERDICT (2026-06-26)

**VERDICT: soundness PROVEN at scale (FP=0/MISSED=0 byte-identical corpus-wide); wall
win NOT realized as-wired — it needs label-cache-build seeding, now a de-risked
increment.** The named-subsumer seed wired into the per-pair classify path is sound at
classify scale (the load-bearing proof the per-class probe couldn't give). It does not
move the wine classify wall under the production `--pair-timeout-ms 25` cap because the
per-pair path is cap-truncated; the win lives in the label-cache build.

## Soundness gate — FULL PASS (flag ON, 25 ms/pair, byte-identical)

`konclude_closure_diff`, `RUSTDL_SAT_SEED=1`, all 19 oracled runs pass:

| fixture | rustdl=konclude | FP | MISSED | unsat r/k |
|---|---|---|---|---|
| bibtex 16, sulo 51, galen 27997 (+5s), notgalen 32739, ro 158, ore-10908 6001 | = | 0 | 0 | 0/0 |
| alehif 247, sio 8904, ore-15672 142 | = | 0 | 0 | 0/0 |
| **wine 653** | **=** | **0** | **0** | **0/0** |
| pizza 499 (at its higher deadline) | = | 0 | 0 | 2/2 |

(pizza shows a transient MISSED=4 at the first low-deadline run, resolved 499=499 at the
higher deadline — identical to flag-off; the test's own two-deadline structure, not a seed
regression.) **Wine — the critical fixture — is 653=653, FP=0, MISSED=0, unsat:rustdl=0.**
This is the classify-scale soundness the probe (2 satisfiable classes) could not test: the
per-pair `¬sup` injection × seeded labels across 137² pairs is FP-free and MISSED-free.
Seeding `sub`'s entailed named subsumers is monotone, and the corpus oracle confirms it at
scale.

## Wall — flat as-wired (and why)

Wine classify wall (`--pair-timeout-ms 25`): flag OFF 49.33 s, flag ON 49.35 s — **no
change.** Diagnosis: the Task-1 seed is in `decide_with_stats` (the per-pair tier walk),
which is **cap-truncated** at 25 ms. A seeded hard pair takes ~2.6 s (per the probe) ≫
25 ms, so it times out at the cap exactly like the unseeded DNF. The per-pair path can't
show the win under the production cap.

The seed's DNF→2.6 s only pays off where the deadline lets the seeded `sat(C)` **complete**
— which is the **label-cache build** (`HyperCache::classify_labels`, per-class `sat(C)`),
the path Task 1 did NOT wire. The build-once probe (`docs/` build-once analysis) already
located wine's classify wall there: ~4638 of wine's pairs are label-cache **misses** caused
by `classify_labels` timing out on hard classes → those pairs fall through to per-pair
refutations. Seeding `classify_labels` makes those per-class sats terminate (the probe's
seeded `sat(Zinfandel)` = 2.6 s is exactly this call) → the label cache completes → the
misses vanish → the per-pair refutations are eliminated → wine classifies fast.

## Consequence — the de-risked next increment (SP2.1)

SP2 proved the **mechanism is sound at classify scale** (the de-risking that mattered). The
**wall win** is one more, now-low-risk wiring:

- **SP2.1: seed `classify_labels` (the label-cache build) + an adequate label-cache
  deadline** (≥ the seeded hard-class sat time, ~3 s; cf. the adaptive label-cache work).
  This completes the label cache on wine's hard classes (DNF→terminate), pruning the ~4638
  misses → the per-pair refutations that ARE the wine wall. Gate: same corpus FP=0/MISSED=0
  (already proven for the seed mechanism) + wine classify wall flag-on-vs-off (the real
  speedup number).

The SP1 saturation increments (∀/≤n/nominal — more named subsumers seeded = more collapse)
remain the longer arc toward Konclude's 1 ms, now with both a validated sound mechanism AND
a measured payoff path.

## SP2.1 (label-cache-build seeding) — measured outcome (2026-06-26)

Seeded `classify_labels` (the per-class `sat(c)` label-cache build) with `c`'s named
subsumers + **rebuilt the trigger index when seeded** (`HyperEngine::new`, not
`new_with_prebuilt` — the amortized index, built pre-seed, leaves appended seed clauses
inert; the first cut was a silent no-op, misses identical on/off). Now the seed fires.

**Sound at scale (re-gated with SP2.1):** flag-ON `konclude_closure_diff` FP=0/MISSED=0
byte-identical — wine 653=653 unsat:0, sio 8904, ore-15672 142, pizza 499, all EL fixtures.

**Wine wall — real but MODEST, not a collapse** (`--pair-timeout-ms 25`):
| label-cache deadline | misses | label_cache_build | tier_walk | wall |
|---|---|---|---|---|
| OFF baseline | 4105 | 6.9 s | 42.5 s | **49.4 s** |
| seed, 3.4 s (adaptive) | 3476 | 6.5 s | 39.2 s | **45.7 s (−7.5%)** |
| seed, 5 s | 3365 | 9.1 s | 36.9 s | 46.0 s |
| seed, 15 s | 2410 | 18.3 s | 31.9 s | 50.3 s |
| seed, 10 s, adaptive-off | 2919 | 14.1 s | 33.9 s | 48.0 s |

Raising the deadline cuts misses + tier_walk but grows build-cost ~evenly → **net flat**.
The per-class probe's 21× (Zinfandel) does NOT generalize to a classify collapse: only
~25–30 of 137 classes are the hard nominal ones, and the **current** saturator's named
subsumers collapse only ~5 more of them below a tractable deadline. The magnitude is gated
by **saturation richness**, not the wiring.

**Verdict: SP2+SP2.1 = a SOUND wine lever, validated FP=0/MISSED=0 corpus-wide, delivering
a modest ~7.5% wine-classify win.** The wine *collapse* requires the **SP1 saturation
increments** (∀ done, ≤1 done, nominal-interaction next): richer ∀/≤n/nominal closure →
more named subsumers seeded per class → more hard classes collapse below the deadline →
the per-class 21× generalizes. That is now the evidence-backed next step with a measured
per-class payoff target. Default OFF until the increments make the win substantial.

## Disposition

SP2 (per-pair seed) committed on `feat/sat-seed-sp2` (Task 1 `5b610f2`), `RUSTDL_SAT_SEED`
default OFF, `main` untouched. Soundness gate PASS recorded. SP2 is **not flipped default-ON**
(no wall win as-wired ⇒ nothing to ship yet); the value lands with SP2.1. Verdict: GO to
SP2.1 (the mechanism is sound at scale; wire the label-cache build path).
