# SP1 ORE benefit sweep + increment-3 FP discovery — 2026-06-23

Follows `docs/sp2-coupling-gate-results-2026-06-23.md` (SP2 sound-seed NO-GO). The
user asked whether the SP1 saturator increments benefit *other* corpus ontologies
beyond wine. The answer surfaced a **soundness violation in increment-3** that the
tuned 12-fixture corpus missed.

## Setup

- **before** = `rustdl-mainbase` (commit 1110e02, pre-SP1).
- **after** = `rustdl-after` (commit d2fe403, all of SP1 increments 1+2+3).
- Probe: `classify --saturation-only` closure diff (deterministic, no tableau
  timeout; isolates the saturator, which is all SP1 changes). Sound-by-construction
  recoveries would show as *added* edges.
- Corpora: ORE pilot (233 onts, each with a HermiT∩Konclude oracle in-dir) and
  ORE pool_sample (1920 flat OFN files, no per-ont oracle).

## Pilot result (233 onts): SP1 is corpus-invisible on the default classifier

2 onts differ in **saturation-only** mode (`ore_ont_11149`, `ore_ont_7901` —
chem2bio2rdf variants), each +77 edges. But both were **already complete on the
default (hybrid) classifier**: the in-dir `diff.json` shows closure 380 = oracle,
**FP=0, MISSED=0** — the wedge found those subsumptions. Confirmed directly: mainbase
and after produce **byte-identical hybrid output** on `ore_ont_11149`. So SP1's gain
is only that the *saturator alone* derives more (a fast-path/`--saturation-only`
completeness gain), changing **no default-path verdict** and adding **no FP**. This is
consistent with the corpus-wide `tableau=0` finding: the saturator already answers
100% of positive subsumptions; the wedge does only refutation.

## Pool result (interim): increment-3 is FP-UNSOUND

`ore_ont_10621.owl` (an FMA extract, 41647 classes) exploded:

| build | saturation closure | classes flagged unsat |
|---|---|---|
| main-base (pre-SP1) | 480 723 | **0** |
| increment-2 (1f69b43) | 480 723 | **0** |
| increment-3 (d2fe403) | 72 366 | **33 272** |
| **Konclude (oracle)** | — | **~0** (consistent; 1.5s) |

Increment-3 makes **33 272 of 41 647 classes (80%) spuriously unsatisfiable** on a
**consistent** ontology — a catastrophic false-positive cascade (each unsat class
subsumes everything, exploding the reported closure to ~644k edges).

### Root cause (bisected)

Disabling **change-2** of increment-3 (`DifferentIndividuals` → pairwise disjoint
`NomKey`s) returns to the clean 480 723 / 0-unsat. The mechanism:

- The pre-existing **functional-role witness merge** (Phase 2a) pools the atom-sets of
  all `R_f`-witnesses of a class into one `merged_atom_sets[(sub, R_f)]` synthetic
  (an over-approximation that accumulates nominal witnesses `NomKey(a)`, `NomKey(b)`…
  from `ObjectHasValue` — this ont has **6455 `ObjectHasValue`**).
- That pooling was sound *only because NomKeys were never disjoint*. Increment-3's
  change-2 makes distinct-individual NomKeys disjoint, so the pooled
  `NomKey(a) ⊓ NomKey(b)` becomes ⊥ → the witness's source class becomes unsat →
  cascades up FMA's deep `is-a` hierarchy.
- Note: this ont has **0 `ObjectMaxCardinality`**, so increment-3's own merge guard /
  re-trigger (change-3) and the qualified-`≤1` machinery (increment-2) never fire here.
  The FP is change-2 alone interacting with the *pre-existing* functional merge.

The functional merge's flat atom-set is a sound over-approximation **iff every pooled
atom is co-satisfiable**. Adding nominal disjointness violates that invariant. Making
change-2 sound would require auditing/fixing the functional-merge's nominal pooling —
significant work for **zero default-classifier benefit** (SP1 is corpus-invisible).

## Decision

- **Increment-3 reverted** (branch `feat/saturator-forall-propagation` reset
  d2fe403 → 1f69b43). Its only goal (the nominal+`≤1` clash) requires change-2's
  disjointness, which is the FP source; and its corpus benefit is zero. Not worth
  fixing.
- **Increments 1+2 retained** pending an ORE re-sweep (main-base vs 1f69b43) to
  confirm they carry no similar latent FP. They were byte-identical on the tuned
  corpus; increment-2's merge only fires under an explicit `≤1 R.C` + *existing*
  `DisjointClasses` (sound coincidence), unlike change-2's *added* nominal
  disjointness.

## Lessons

1. **The tuned 12-fixture corpus is not a sufficient FP gate.** Increment-3 was
   "byte-identical, FP=0" on all 12 fixtures (incl. ore-15672 SHOIN) yet 80%-unsound
   on an ORE FMA extract. Any saturator change touching nominals/disjointness needs an
   ORE sweep before it can claim FP=0. The `--saturation-only` ORE diff is a cheap,
   deterministic, sound-by-construction probe that catches exactly this.
2. **Adding disjointness to an over-approximate merge is FP-delicate.** The
   functional-merge atom-set pooling silently assumed co-satisfiability of pooled
   atoms; nominal disjointness broke it. (Echoes `[[saturator-fp-disjunction-existential-marker]]`.)
3. The whole SP1 arc confirms: **extending saturator completeness yields no
   default-classifier benefit** (the wedge already saturates positives, `tableau=0`
   corpus-wide), so there is no payoff to justify the FP risk.
