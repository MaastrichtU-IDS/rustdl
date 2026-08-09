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

Workspace 1,605 pass / 0 fail; fmt and clippy clean; FP=0 net all closures exact.
