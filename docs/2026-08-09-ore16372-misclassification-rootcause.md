# Why rustdl misclassified `ore_ont_16372`

2026-08-09. `ore_ont_16372` is **inconsistent** — Konclude (`Ontology … is inconsistent`,
0 ms query) and HermiT (`InconsistentOntologyException`) agree independently, as does
rustdl's own `is_consistent` (0.36 s). Only `rustdl classify` disagreed, reporting
`consistent = true`. This is the root cause.

## Two defects, one causing the other

**Defect A — the unsat probe trusts a wedge `Sat`.** `classify.rs`'s per-class unsat probe
reuses the label cache's wedge verdict rather than re-running the main tableau (the
2026-06-10 "unsat-probe de-redundancy" win, worth ~20× on alehif/ore-10908). The wedge is
Horn-incomplete, so a wrong `Sat` silently marks an unsatisfiable class satisfiable:

| `RUSTDL_UNSAT_VIA_LABELS` | unsat classes found | wall |
|---|---:|---:|
| 1 (default) | **3** of 744 | 3.2 s |
| 0 | **744** | **2.1 s** |

On this ontology the optimisation is **both less complete and slower**.

**Defect B — no inconsistency signal, even with every class unsat.** With all 744 classes
unsatisfiable, classify still reported `consistent = true`. That is correct *in general* —
`{A ⊑ ⊥, B ⊑ ⊥}` empties every named class yet has a model, so the valid test is that `⊤`
is unsat, and nothing here derives that.

**A causes B.** The sound signal available is `ClassAssertion(C, a)` with `C`
unsatisfiable ⟹ inconsistent. `ore_ont_16372` has 108 class assertions over **7 distinct
asserted types**, and *all 7* are in the 744-unsat set — but **none** of the 3 classes the
default configuration detects. So the ABox route could not fire while defect A hid the
rest. (`abox_check`'s P1 tests exactly this rule, but against the *saturator closure*,
which knows none of these are unsat.)

## Hypotheses tested and refuted on the way

* **`trust_sat`** — `RUSTDL_HYPERTABLEAU_TRUST_SAT=0` still gives 3 unsat. That flag
  governs the *subsumption* oracle, not the unsat probe's use of the label cache.
* **`RUSTDL_INVERSE_PAIR_FUNC`'s cost** — earlier framed as a 53× slowdown and
  two-thirds answer loss. Both arms produce **wrong** answers on an inconsistent KB, so
  the row counts being compared were meaningless.
* **A dedicated consistency probe** (`RUSTDL_CLASSIFY_CONSISTENCY_PROBE`) — does not fix
  `16372`, and not for want of budget (1,000 ms and 10,000 ms agree). Its wedge returns
  `Stalled`; detection lives in `is_consistent`'s bounded main-tableau fall-through.

## The fix, and its price

Two opt-in flags, both **default OFF**:

* `RUSTDL_UNSAT_VERIFY_ASSERTED` — distrust a wedge `Sat` **only** for a class with an
  asserted instance, falling through to the main tableau. Narrow by construction (4–7
  classes on the measured ontologies) and follows the concrete-domain `needs_verify`
  carve-out sitting in the same match arm.
* `RUSTDL_CLASSIFY_CONSISTENCY_PROBE` — adds the asserted-instance-of-unsatisfiable-class
  test (exact and engine-free, reading a set classify already computed) plus a gated
  wedge-consistency probe.

Result at **default** `UNSAT_VIA_LABELS`:

| | verdict | wall |
|---|---|---|
| `ore_ont_16372`, flags off | `consistent=true`, 3 unsat ✗ | 3.9 s |
| `ore_ont_16372`, flags on | **`consistent=false`, 744 unsat** ✓ | **3.1 s** |
| `ore_ont_7610`, flags on | **`consistent=false`**, 91/91 unsat ✓ | 0.12 s |

**The price is real and is why these stay OFF.** The carve-out costs `ore_ont_10908`
**0.13 s → 1.15 s (8.8×)** for byte-identical answers; `sio` +6%. Verifying even 4 classes
on the main tableau is exactly the cost `RUSTDL_UNSAT_VIA_LABELS` exists to avoid. So this
is a correctness-versus-wall trade — ~1 s on ABox-bearing ontologies to fix 2 wrong answers
in 1,920 — and it needs a full-corpus two-arm sweep before any default flip.

## The default flip, and the budget the sweep forced

Shipped **default ON**, budget **10 ms**. Both numbers are measured, not chosen.

**The first mechanism was wrong.** Verifying every asserted-instance class through
the main tableau is `k` UNBOUNDED probes — 58 on `wine`, whose tableau probes are
the documented hard frontier. It made the FP=0 net run **8h47m at 3162% CPU**
without finishing, against a normal ~4 minutes. Removed, not left as a dead
opt-in: a mechanism shown to be wrong invites a retry if the flag survives. (The
"8.8× on `ore_ont_10908`" figure reported for it is therefore **superseded** — the
shipped mechanism leaves `10908` at 0.14 s, unchanged.)

**The shipped mechanism is three layers, cheapest first:** (1) an asserted instance
of an unsatisfiable class — exact and engine-free, reading a set classify already
has; (2) the wedge consistency route; (3) ONE bounded `⊤` probe, mirroring
`is_consistent`'s fall-through after a wedge `Stalled`.

**A 1,920-ontology two-arm sweep at a 1000 ms budget rejected that budget:**

| | OFF | ON @1000 ms |
|---|---|---|
| outcomes | 1751 ok / 167 dnf / 2 rej | 1747 ok / **171 dnf** |
| `ok → dnf` | — | **4** (`14881`, `6108`, `7416`, `7803`) |
| `dnf → ok` | — | 0 |
| wall | 3572 s | 3609 s (+1.06%) |

plus `ore_ont_1966` at **7.30 → 58.20 s**.

**The cost is not proportional to the budget**, which is the diagnostic detail:
`1966` reads **66.08 s at 1000 ms, 73.00 s at 100 ms, and 5.17 s at 10 ms** against
a 5.06 s baseline. `decide_with_deadline` overshoots its deadline on the main
tableau — the same defect class found in `horn_fixpoint` the day before.

**No single budget satisfies both sides.** `ore_ont_16372` needs **≥200 ms**;
`ore_ont_1966` is already destroyed at 100 ms. At 10 ms the five harmed ontologies
are back within noise (+0.06 to +0.37 s).

## Outcome, stated without rounding up

| | at the shipped default |
|---|---|
| `ore_ont_7610` | **fixed** — `consistent=false`, 91/91 unsat |
| `ore_ont_16372` | **still wrong** — needs ≥200 ms, which costs 4 correct answers |

So this closes **1 of the 2** wrong answers at a measured cost of ~0 and **0
regressions**. The residual is not a mystery: raising the budget fixes `16372` and
breaks ontologies that currently answer correctly, so **the real blocker is the
`decide_with_deadline` overshoot**, not the consistency logic.

## Final state: both fixed, zero regressions

The unsat-FRACTION gate (see the commit) let the budget go back to 200 ms. Full
1,920-ontology two-arm sweep, cap 60 s, `--threads 1`:

| | OFF | ON |
|---|---|---|
| outcomes | 1749 ok / 169 dnf / 2 err_reject | **identical** |
| `ok → dnf` | — | **0** |
| `dnf → ok` | — | 0 |
| wall (1749 completers) | 3564 s | 3547 s (**−0.48%**) |

| | at the shipped default |
|---|---|
| `ore_ont_16372` | **fixed** — `consistent=false` |
| `ore_ont_7610` | **fixed** — `consistent=false`, 91/91 unsat |

Both now agree with Konclude and HermiT. Workspace 1,605 pass / 0 fail; FP=0 net
14 VERIFIED with no `FP>0`/`MISSED>0`; fmt and clippy clean.

## What is still latent, and why it is not this feature's problem

`decide_with_deadline` overshoots on the main tableau: at a 1000 ms budget with only
the `≥1 unsat` gate, `ore_ont_1966` spent **58–73 s**, and the cost was *not*
proportional to the budget (66 s at 1000 ms, 73 s at 100 ms, 5.2 s at 10 ms). So
there is a region of the main-tableau search that does not consult
`check_deadline` — which itself reads the clock exactly, with no stride, so this is
a missing call rather than granularity. It is the same defect class as
`horn_fixpoint` (2026-08-08).

The fraction gate **masks** it here — `1966` is now skipped outright — so it is no
longer this feature's blocker. It remains latent for every other budget, notably
`--pair-timeout-ms`.

**Method note:** an attempt to bisect the budget to localise the unguarded region
produced 5.29–5.57 s at every value from 10 to 100 ms and was therefore **vacuous**:
the fraction gate skips `1966` before the probe runs, so varying the probe's budget
could not exercise it. Localising this defect requires a diagnostic build that
bypasses the gate.


## 2026-08-10: the root cause of the *budget* problem, and the final sweep

The probe could not be given an honest budget: at 100 ms it spent 66–80 s on
`ore_ont_1966`. I attributed that to `decide_with_deadline` on the main tableau.
**That was wrong.** Stack-sampling the overshoot region showed
`owl_dl_tableau::hyper::*` — the **wedge**. It is `horn_fixpoint` failing to consult
the clock: exactly the defect fixed on 2026-08-08 and then shipped **default OFF**
as a corpus-neutral NO-GO.

| `RUSTDL_FIXPOINT_DEADLINE` | budget 100 ms | budget 1000 ms |
|---|---|---|
| 0 (was default) | 80.28 s | 66.34 s |
| **1** | **5.89 s** | **6.50 s** |
| baseline (probe off) | 5.48 s | — |

**What changed is not the measurement but the arrival of a caller.** The 08-08
NO-GO was correct on the evidence then: both gates passed, but nothing in the corpus
benefited, so there was no reason to pay for it. This probe is that reason. The flag
is now **default ON**, and the two mechanisms compose — the fraction gate bounds
*which* ontologies pay, the fixpoint deadline bounds *how much*.

### Final gate: 1,920-ontology two-arm sweep of the COMBINED defaults

| | OFF (probe off + deadline off) | ON (both) |
|---|---|---|
| outcomes | 1749 ok / 169 dnf / 2 rej | 1750 ok / 168 dnf / 2 rej |
| `ok → dnf` | — | **0** |
| `dnf → ok` | — | 1 (`ore_ont_15491`) |
| wall (1749 completers) | 3695 s | 3762 s (+1.81%) |
| peak RSS | 281.4 GB | 281.5 GB (+0.01%) |

**Every apparent difference is contention, verified individually** — the sweep runs
`JOBS=4`, and re-measuring on a quiet host collapses all of them:

| ontology | sweep | re-measured |
|---|---|---|
| `ore_ont_13071` | 33.89 → 52.73 s | **20.61 → 20.68 s** |
| `ore_ont_5852` | 0.17 → 2.66 s | **0.01 → 0.01 s** |
| `ore_ont_5792` | 0.33 → 3.04 s | **0.16 → 0.17 s** |
| `ore_ont_9257` | 0.43 → 2.28 s | **0.29 → 0.30 s** |
| `ore_ont_15491` ("recovery") | dnf → ok | **41.86 → 39.50 s, ok in BOTH** |

So the honest reading is **0 real regressions and 0 real recoveries**: the change is
outcome-neutral corpus-wide. The "+1 recovery" is a 60 s cap crossed under load in
one arm and not the other, and must not be quoted as a win.

## Summary of the arc

| | |
|---|---|
| wrong answers fixed | **2 of 2** (`ore_ont_16372`, `ore_ont_7610`), both agreeing with Konclude AND HermiT |
| corpus regressions | **0** over 1,920 ontologies |
| FP=0 net | clean, zero `FP>0` / `MISSED>0` |
| workspace | 1,605 pass / 0 fail |
| defaults flipped ON | consistency probe, unsat-fraction gate, `RUSTDL_FIXPOINT_DEADLINE` |

### Still latent

`solve_at_most` in the wedge has **no deadline check at all** — the same defect
class, noticed while mapping the consultation sites and not pursued. Nothing
measured so far reaches it. The `FIXPOINT_DEADLINE` history is the argument for
waiting: a bound with no caller that needs it measures as a NO-GO, so this should be
built when something provably reaches it, not before.
