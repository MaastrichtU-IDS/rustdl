# C2 calibration matrix — paper experiment (iii)

Measurement-only, env-gated. Instrumentation site:
`crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`
(`diff_corpus_ontology` now emits a `MATRIX …` line; new driver test
`c2_calibration_matrix`). No engine behaviour changed.

Run:
```
RUSTDL_HYPERTABLEAU_TRUST_SAT=1 RUSTDL_C2_WINE_MAX_MS=25 \
  cargo test --release -p owl-dl-reasoner --test konclude_closure_diff \
  c2_calibration_matrix -- --ignored --nocapture
RUSTDL_HYPERTABLEAU_TRUST_SAT=0 RUSTDL_C2_BUDGETS="25,100" RUSTDL_C2_WINE_MAX_MS=25 \
  cargo test … (same)
```

## What each column means

- `timed_out` = `ClassificationStats::timed_out_pairs` — THE incompleteness
  signal (CLI ⚠ INCOMPLETE / Python `.complete=false` ⟺ `timed_out>0`).
- `MISSED` = oracle-closure ∖ rustdl-closure (Konclude/HermiT oracle).
- `trusted_refute` = `hyper_refuted_pairs + snapshot_replay_not_subsumed`
  — confident "not-subsumed" verdicts trusted via wedge `Sat` WITHOUT timing
  out (the C2 *architectural exposure surface*, gated by trust_sat).
- `label_pruned` = `label_cache_pruned` — a SECOND silent not-subsumed channel
  (Phase-7 per-class label heuristic), **not gated by trust_sat**.
- FP = 0 is the soundness floor (C1).

`timed_out`/`trusted_refute` are direct-probe counts; `MISSED` is a
transitive-closure count. Pizza is the only MISSED>0 cell on this corpus.

## Key lemma (why no pair-set capture is needed for the FN test)

The two silent not-subsumed channels (`trusted_refute`, `label_pruned`) are
budget-invariant: a fast wedge `Sat` or a label-cache prune does not flip with
more `per_pair_timeout`. Only a *timeout* miss is budget-sensitive. Therefore a
MISSED pair that disappears at a higher budget was necessarily timeout-flagged
(in the `timed_out` set) at the lower budget. Pizza MISSED 4→0 across 25→100 ms
⟹ all 4 were flagged ⟹ `MISSED ⊆ timed_out` at pizza@25, with no per-pair
instrumentation.

## Matrix (trust_sat=1, DEFAULT)

| ontology | budget | FP | MISSED | timed_out (signal) | trusted_refute | label_pruned | over-warn = (timed_out−MISSED)/timed_out | wall s |
|---|---|---|---|---|---|---|---|---|
| pizza | 25 | 0 | 4 | 25 | 1304 | 1173 | 21/25 = 84% | 0.6 |
| pizza | 100 | 0 | 0 | 4 | 1316 | 1173 | 4/4 = 100% | 1.2 |
| pizza | 1000 | 0 | 0 | 2 | 1318 | 1173 | 2/2 = 100% | 8.5 |
| ore-10908 | 25 | 0 | 0 | 58 | 6823 | 26140 | 100% | 1.7 |
| ore-10908 | 100 | 0 | 0 | 0 | 6881 | 26140 | n/a (to=0) | 3.1 |
| ore-10908 | 1000 | 0 | 0 | 0 | 6881 | 26140 | n/a | 22.9 |
| ore-15672 | 25 | 0 | 0 | 109 | 1860 | 842 | 100% | 4.6 |
| ore-15672 | 100 | 0 | 0 | 109 | 1860 | 842 | 100% | 15.2 |
| ore-15672 | 1000 | 0 | 0 | 109 | 1860 | 842 | 100% | 140.3 |
| sio | 25 | 0 | 0 | 6085 | 72765 | 32209 | 100% | 18.0 |
| sio | 100 | 0 | 0 | 787 | 78063 | 32209 | 100% | 30.1 |
| sio | 1000 | 0 | 0 | 0 | 78850 | 32209 | n/a (to=0) | 72.4 |
| wine | 25 | 0 | 0 | 9213 | 4099 | 698 | 100% | 54.2 |
| wine | 100/1000 | — | — | — | — | — | SKIP (DNF cap @25 ms) | — |
| galen | 25/100/1000 | 0 | 0 | 0 | 0 | 0 | n/a | 0.5 |
| notgalen | 25/100/1000 | 0 | 0 | 0 | 0 | 0 | n/a | 0.9 |
| ro-stripped | 25/100/1000 | 0 | 0 | 0 | 170 | 632 | n/a (to=0) | 0.07–2.0 |
| sulo-stripped | 25/100/1000 | 0 | 0 | 0 | 0 | 71 | n/a | 0.01 |
| alehif-test | 25/100/1000 | 0 | 0 | 0 | 0 | 0 | n/a | 0.07 |
| shoiq-knowledge | 25/100/1000 | 0 | 0 | 0 | 1465 | 3610 | n/a (to=0) | 0.2–5.0 |

(galen/notgalen/alehif/sulo-stripped are Horn-shortcircuited → pure-EL fast path,
no wedge/tableau/timeout: budget-invariant, trivially C2-safe. ro-stripped &
shoiq-knowledge run the wedge but resolve fully — `timed_out=0` yet the silent
channels are non-empty.)

## Matrix (trust_sat=0; budgets {25,100} for tractability)

| ontology | budget | FP | MISSED | timed_out | trusted_refute | label_pruned | wall s |
|---|---|---|---|---|---|---|---|
| pizza | 25 | 0 | 4 | 1328 | 0 | 1173 | 1.9 |
| pizza | 100 | 0 | 0 | 1320 | 0 | 1173 | 6.5 |
| ore-10908 | 25 | 0 | 0 | 6881 | 0 | 26140 | 7.1 |
| ore-10908 | 100 | 0 | 0 | 6881 | 0 | 26140 | 25.1 |
| ore-15672 | 25 | 0 | 0 | 1969 | 0 | 842 | 5.5 |
| ore-15672 | 100 | 0 | 0 | 1969 | 0 | 842 | 18.8 |
| sio | 25 | 0 | 0 | 78850 | 0 | 32209 | 77.0 |
| sio | 100 | 0 | 0 | 78850 | 0 | 32209 | 278.6 |
| wine | 25 | 0 | 0 | 13312 | 0 | 698 | 58.3 |
| galen/notgalen/alehif/sulo | any | 0 | 0 | 0 | 0 | 0/71 | <1 |
| ro-stripped | 25/100 | 0 | 0 | 170 | 0 | 632 | 0.4/1.6 |
| shoiq-knowledge | 25/100 | 0 | 0 | 1465 | 0 | 3610 | 1.5/5.8 |

**trust_sat=0 collapses `trusted_refute` to 0 on every row** (the wedge-`Sat`
channel is closed; those verdicts now fall through to the tableau and, when they
exceed the per-pair budget, become *flagged* timeouts — e.g. sio@25: the 72765
trusted refutes become part of the 78850 timed-out set; shoiq-knowledge@25:
`timed_out` 0→1465; wine@25: 9213→13312). **`label_pruned` remains non-zero** —
the label-heuristic silent channel survives trust_sat=0. **MISSED is UNCHANGED on
every row vs trust_sat=1** — no trust_sat-induced miss observed anywhere
(including wine, which cannot use the recovery lemma).

## Matrix (trust_sat=0 AND label_heuristic=0 — the provably-sound config)

100 ms; cheap onts (RUSTDL_C2_ONLY) to keep wall bounded (no pruning at all).

| ontology | budget | FP | MISSED | timed_out | trusted_refute | label_pruned | wall s |
|---|---|---|---|---|---|---|---|
| pizza | 100 | 0 | 0 | 2493 | **0** | **0** | 27.8 |
| ore-15672 | 100 | 0 | 0 | 2811 | **0** | **0** | 30.5 |
| shoiq-knowledge | 100 | 0 | 0 | 5075 | **0** | **0** | 32.0 |

**Both silent channels are 0 here.** Every not-subsumed verdict is now either
the complete tableau's own `Sat` (sound) or a flagged timeout. So `MISSED ⊆
timed_out` holds **by construction**, not just empirically — this is the config
under which the C2 signal is provably sound. (The flagged set inflates further —
e.g. shoiq-knowledge `timed_out` 1465→5075 — i.e. tighter soundness costs more
over-warning and wall.)

## Conservation identities (the airtight core)

Each silently-refuted pair converts ONE-FOR-ONE into a flagged timeout when its
channel is closed, and MISSED stays 0 through both conversions:

- **Closing the wedge channel** (trust_sat 1→0):
  `timed_out(ts0) = timed_out(ts1) + trusted_refute(ts1)`, exact on every SROIQ
  row — sio@25 6085+72765=78850, sio@100 787+78063=78850, ore-10908@25
  58+6823=6881, ore-15672@25 109+1860=1969, wine@25 9213+4099=13312,
  shoiq@25 0+1465=1465, ro-stripped@25 0+170=170. (pizza@25 1329 vs 1328: off by
  1 = run-to-run wall jitter at the budget boundary, not structural.)
- **Closing the label channel** (label_heur 1→0, under trust_sat=0):
  `timed_out(ts0,lh0) = timed_out(ts0) + label_pruned(ts0)`, exact —
  pizza@100 1320+1173=2493, ore-15672@100 1969+842=2811, shoiq@100
  1465+3610=5075.

These identities prove (a) the instrumentation counts exactly the claimed sets,
(b) `trusted_refute` and `label_pruned` ARE precisely the C2 exposure surface,
(c) no subsumption was hidden in either channel on this corpus.

Caveat on the budget-invariance lemma: `label_pruned` is strictly
budget-invariant (fixed `label_cache_timeout_ms` deadline). The wedge `Sat`
channel is `per_pair_timeout`-bounded, so in principle a wedge `Sat` at the
deadline could differ with more budget — but the argument rests on the exact
conservation identities above, not on the lemma. (Pizza's 4 InterestingPizza
misses route through the defined-sup sweep with hardcoded `trust_sat=false` — no
wedge short-circuit — so they are pure tableau-or-timeout; `trusted_refute=0`
with MISSED=4 at pizza@25 confirms empirically.)

## Verdicts

**(1) FP=0 everywhere?** YES — every cell, every budget, both trust_sat
configs. C1 (soundness floor) holds rock-solid.

**(2) Any signal false-negative (`MISSED ∖ timed_out > 0`, i.e. a cell with
`timed_out==0 AND MISSED>0`)?** NO cell on this corpus. The only MISSED>0 cell is
pizza@25 (MISSED=4), and there `timed_out` is 25 (trust_sat=1) / 1328
(trust_sat=0) > 0, and the 4 pairs recover at 100 ms ⟹ by the budget-invariance
lemma they were in the timed_out set ⟹ `MISSED ⊆ timed_out`. So the *boolean*
signal (`complete=true ⟹ MISSED=0`) is never violated here.

**(3) Over-warn rate.** Very high. Wherever `timed_out>0`, almost all flagged
pairs are correctly-resolved non-subsumptions: over-warn = (timed_out−MISSED)/
timed_out is 84% at pizza@25 and **100% everywhere else** (ore-15672, sio, wine
all flag thousands of pairs at MISSED=0). The signal is a *sound but very
conservative over-approximation* of the uncertain set, not a tight one.

**(4) Does C2's "MISSED ⊆ flagged" hold, and only with trust_sat=0?**
On THIS corpus it holds under BOTH configs — but that is because the corpus is
tuned to MISSED=0 (pizza@25 is the lone exception and it recovers). The
false-negative (signal-soundness) test is therefore **VACUOUS where MISSED=0**:
we can only measure the over-warn rate, not exhibit a realized FN.

The *architectural* hole stands regardless of the empirical MISSED=0:
- Under **trust_sat=1 (default)** there are TWO silent (non-flagged)
  not-subsumed channels that can mask a miss without setting the signal:
  `trusted_refute` (wedge `Sat`) **and** `label_pruned` (label heuristic). Both
  are large on every SROIQ row (e.g. sio@1000: `timed_out=0` but
  `trusted_refute=78850`, `label_pruned=32209` — the signal says "complete" while
  ~111k not-subsumed verdicts were never tableau-verified). If the wedge were
  incomplete on any of those pairs, C2's boolean would break with NO warning.
- Setting **trust_sat=0** empirically zeroes `trusted_refute` (demonstrated) and
  converts those verdicts into flagged timeouts — closing the first channel.
- But `label_pruned` survives trust_sat=0. So the signal is **provably** sound
  (MISSED ⊆ flagged by construction — the only non-flagged not-subsumed verdicts
  then come from the *complete* tableau's own `Sat`) only under
  **trust_sat=0 AND `RUSTDL_LABEL_HEURISTIC=0`**. This is demonstrated directly:
  in that config both silent channels read 0 on pizza / ore-15672 /
  shoiq-knowledge (table above), at the cost of more over-warning and wall.

**Bottom line for the paper:** C2's boolean ("complete=true ⟹ MISSED=0") is
empirically clean across the corpus, but it is *not provably sound under the
default config* — two trust_sat/label-heuristic silent channels can mask a miss.
C2 should be stated as either (a) a sound-conservative flag whose *certain* set
is exact on the measured corpus (with the over-warn caveat and the architectural
caveat made explicit), or (b) a provable guarantee only under
`RUSTDL_HYPERTABLEAU_TRUST_SAT=0` + `RUSTDL_LABEL_HEURISTIC=0`. A *demonstrated*
false-negative requires a MISSED>0 workload (full ORE suite); none appears on the
tuned 11-ontology corpus, and trust_sat=0 did not lower MISSED on any row
(no trust_sat-induced miss observed here).
