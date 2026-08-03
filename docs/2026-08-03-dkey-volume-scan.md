# DKey volume scan — `RUSTDL_DKEY_EMIT_ORDER` and `RUSTDL_DKEY_ONEOF_SEED` flipped ON

**Date:** 2026-08-03 · **Base:** v0.4.12 (`d567319`) · **Outcome:** both flags DEFAULT ON

Two `DKey` levers shipped on 2026-08-01 correct, gated, canaried and FP=0-verified,
but **default OFF solely because a volume measurement had never been run**. Each emits
*more* axioms, and axiom-volume re-inflation is the shape that caused the v0.3.29
conversion DNFs, so the flips were held pending a scan. This is that scan.

| flag | defect it fixes | risk direction |
|---|---|---|
| `RUSTDL_DKEY_EMIT_ORDER` | **live, non-monotonic**: `∀p.[0,5] ⊓ ∃p.{9}` is `⊥`, but adding an *unrelated* property `q` that merely mentions the same keys makes it satisfiable again. Adding an axiom must never remove an entailment. | emits MORE `DisjointClasses(DKey, DKey)` ⇒ **FALSE POSITIVE** |
| `RUSTDL_DKEY_ONEOF_SEED` | the **sixth D10-class bug**: the six numeric `DataOneOf` buckets were minted but never seeded, so `is_pure_el` certified the closure complete while the engine dropped the axiom. | emits MORE told edges + disjointness ⇒ FP, plus `O(k²)` volume |

## Method

`rustdl tbox-stats` over **all 1,920** ontologies in `/data/dumontier/ore-run/pool_sample/files`,
in **four arms** — neither / `EMIT_ORDER=1` / `ONEOF_SEED=1` / both — conversion-only,
serial, `RAYON_NUM_THREADS=1`, `ulimit -v 24 GB`, 60 s cap per run. 7,680 runs, ~4 h.

Raw data is committed alongside this doc as
`docs/data-2026-08-03-dkey-volume-scan.csv` (`stem,arm,cr,tse,tdp,ms`; 7,680 rows),
so the conclusions below can be re-derived or falsified without re-running the scan.
Three population measurements in this project's history have been retracted; keeping
the rows is cheap insurance against a fourth.

Four things a previous draft of this task got wrong, and how each is handled here:

1. **Prove the instrument fires.** Not assumed — established three ways.
   - `ore_ont_5368`, the DKey discriminator, reads exactly **18,620,251** `concept_rules`
     in the OFF arm. `ore_ont_9347` is reported but **never judged on**: it reads 113
     under both a correct gate and a build emitting *no* DKey disjointness at all, which
     is why the 2026-07-30 population scan was retracted.
   - Both flags are **behaviourally live in the pinned scan binary** (`sha256 ee559d4c…`),
     not merely present in its source: on the `EMIT_ORDER` fixture it reports
     `unsatisfiable=[]` at `=0` and `[Direct]` at `=1`; on the `ONEOF_SEED` ladder it
     reports one relation at `=0` and four at `=1`.
   - The two **new** counters register those same effects (`told_disjoint_pairs` 0 → 1;
     `told_super_edges` 7 → 10, `concept_rules` 4 → 7), so they are not dead fields.
2. **Record conversion WALL and told-edge counts, not just `concept_rules`.**
   `ONEOF_SEED` emits told `DKey ⊑ DKey` edges, which land in the told table and appear
   *nowhere* in the absorbed-`TBox` rule counts — and `told.rs` closes that table
   transitively at build, so linear seeding can grow it quadratically. The v0.3.27 fix
   was a DNF in exactly that table. `TBoxStats` therefore gained `told_super_edges` and
   `told_disjoint_pairs`, and `tbox-stats` now prints `convert_ms`. This was not
   theoretical: `ore_ont_14459` carries **13,962,063** told-super-edges against 847,755
   concept rules, and 1,032 of 1,913 ontologies have non-zero told-disjoint pairs — a
   `concept_rules`-only scan would have been blind across a third of the corpus.
3. **An ON-arm timeout is a BLOCKING result, not a dropped row.** The analysis never
   filters to "rows where every arm parsed" — that would silently discard an ontology
   that converts at baseline and times out only with a flag on, which is the v0.3.29
   signature it exists to detect. `NA` is counted **per arm**.
4. **Threshold is OR, not AND**: block if `concept_rules` grows **>2× OR by >100k**.
   The AND version passes 1 → 99,999.

## Results

Baseline shape (OFF arm, 1,913 ontologies that convert): `concept_rules` median 3,233 /
p95 196,070 / max 18,620,251; `told_super_edges` median 10,102 / p95 546,159 / max
15,790,194; `convert_ms` median 17 ms / p95 1,362 ms / max 52,623 ms.

### Per-arm `NA`

| arm | NA | new NA vs OFF |
|---|---|---|
| off | 7 | — |
| `eo` | 7 | **0** |
| `os` | 7 | **0** |
| both | 7 | **0** |

The `NA` set is **identical in all four arms** — `ore_ont_{10860, 10929, 15635, 2504,
4141, 4572, 8445}` — i.e. a baseline property of those ontologies, not flag-induced.
That is precisely the distinction per-arm counting exists to make.

### Growth

**Exactly one ontology in 1,920 moves at all, in any of the three metrics:**

| ontology | arm | `concept_rules` | `told_disjoint_pairs` |
|---|---|---|---|
| `ore_ont_9303` | `eo`, both | 8,886 → 8,887 (+1) | 6,669 → 6,670 (+1) |

Corpus-wide **total** deltas: `EMIT_ORDER` **+1** concept rule and **+1** told-disjoint
pair, summed over all 1,913 converting ontologies; `ONEOF_SEED` **exactly 0** on all
three metrics. `told_super_edges` is unchanged everywhere.

- Ontologies over the >2× OR >100k threshold: **0** in every arm.
- Ontologies >2× slower to convert: **0** in every arm (worst 1.38× on a 523 ms
  baseline, with counts identical — run-to-run noise, not work).
- `ore_ont_5368`: **18,620,251 / 55,054 / 18,608,050 in all four arms** — unmoved.
- `ore_ont_9347`: 113 / 18,309 / 0 in all four arms — reported, not judged on.

`ONEOF_SEED` moving nothing is a real finding, not a null: the numeric `DataOneOf`
pattern **does not occur anywhere in ORE**. So the corpus establishes that flipping it
is free; it cannot establish that it is right.

## FP adjudication

Mandatory for `EMIT_ORDER`, whose own doc says its failure mode is a false positive.

**On the sole mover, `ore_ont_9303`:** classify output is **byte-identical** ON vs OFF
(md5 `7912ce79…` both arms) — the extra pair is emitted but never consumed. Its verdict
is *inconsistent*, all 726 named classes unsatisfiable, and **both** oracles agree:

- **Konclude** — 727 of 728 declared classes `≡ owl:Nothing`;
- **HermiT** — `throwInconsistentOntologyExceptionIfNecessary` (exit 1, no taxonomy).

**FP = 0** on the mover, at both flag settings.

**On the fixtures where the levers actually fire** — the only place they *can* be
adjudicated, since ORE contains no such pattern:

| fixture | Konclude ∪ HermiT | rustdl OFF | rustdl ON |
|---|---|---|---|
| `EMIT_ORDER` non-monotonicity | `Direct ≡ owl:Nothing` (both, independently) | *nothing* (MISS) | `Direct` unsat — **exact** |
| `ONEOF_SEED` ladder | `C ≡ F`, `F ⊑ D`, `C ⊑ D`, `D ⊑ E` | `C ≡ F` only (3 MISSED) | all four — **exact, FP=0 MISSED=0** |

### Which evidence is which

- **Non-regression only:** the FP=0 soundness net (11 fixtures VERIFIED, all closures
  exact, manifest **identical** to the pre-flip baseline). The curated corpus **cannot**
  validate this area — `datatype_value_membership.rs` says so itself: *"the corpus has
  NO such clash, so these canaries are the ENTIRE safety net."* A green net here shows
  inertness. It is also how the v0.4.9 float/double FP survived for months.
- **Positive evidence:** the canaries, the two oracle adjudications above, and the
  `ore_ont_9303` byte-identity plus its Konclude+HermiT-confirmed verdict.

## Decision

Rule, fixed before the scan ran: flip only if **no** ontology exceeds the growth
threshold, **no** ontology gains an ON-arm conversion timeout, and `ore_ont_5368` is
unmoved.

| flag | growth blocks | new ON-arm NA | `5368` unmoved | verdict |
|---|---|---|---|---|
| `RUSTDL_DKEY_EMIT_ORDER` | 0 | 0 | yes | **FLIP ON** |
| `RUSTDL_DKEY_ONEOF_SEED` | 0 | 0 | yes | **FLIP ON** |

Both now use the house default-ON idiom `is_none_or(|v| v != "0")`, so **empty
enables** and only an explicit `=0` reverts.

## What is now guarded

`crates/owl-dl-reasoner/tests/dkey_flag_defaults.rs` — three tests, each pinning
**both halves** of a default (unset ⇒ ON, `""` ⇒ ON, `"0"` ⇒ OFF). The empty-string row
is the one a flip most easily gets wrong: the opt-in idiom these flags used to carry
makes `""` mean OFF, and `VAR=` in a shell wrapper is a common accident.

The third test guards the **instrument**, which nothing else in the tree reads. Without
it a refactor could make either counter read 0 and the next scan would report "no growth
anywhere" for the most boring possible reason — the exact failure that got the 2026-07-30
scan retracted.

**Sabotage: 5 run, 5 caught, 0 survivors.** Reverting either default to the opt-in idiom
fails that flag's test; hardwiring both to `true` fails all three (the `=0` escape hatch);
killing either told counter fails the instrument test. Independently, both default tests
**failed on the pre-flip tree** and pass after — so they are not vacuous.

## Threats to validity

- **Corpus scope.** ORE only. A BioPortal-scale corpus with numeric `DataOneOf`
  enumerations could exercise `ONEOF_SEED`'s `O(k²)` disjointness in a way ORE does not;
  this scan cannot speak to that, because ORE contains no instance of the pattern.
- **`ore_ont_5368` is slow, not capped.** It converts in ~43–47 s against a 60 s cap.
  It did not time out in any arm, and its counts are identical in all four, so the
  margin did not bite here — but a loaded host could turn it into a spurious `NA`. Any
  future re-run should give it headroom.
- **Conversion-only.** `tbox-stats` measures conversion + NNF + absorb + told build. It
  says nothing about classify wall. That is deliberate — the risk being measured is
  axiom volume at conversion — but a lever that inflated *search* rather than axioms
  would be invisible here. Neither of these does: their entire effect is which axioms
  `seed_disjoint_bucket` / `seed_bucket` emit.
- **The 7 baseline `NA` ontologies are unmeasured**, in every arm. They convert in
  neither configuration, so the flip cannot regress them, but neither can this scan
  confirm that.
