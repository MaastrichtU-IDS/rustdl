# `RUSTDL_INVERSE_FUNC_MAX`: the default-flip sweep says STAY OFF — but it is not inert

**Date:** 2026-08-18 · Gates the default of the flag shipped in `b24cd88`
(`docs/known-limitations/realize-drops-derived-individual-equality.md`).

**Verdict: STAYS OFF.** It recovers **17 oracle-confirmed entailments** on one ontology and
**zero** realize types across the corpus, while taking **three** ontologies from 2–7 s to
DNF-at-120 s. Not a dead end — a **performance-blocked completeness gain**.

## Method

* **Frame: 109 of 1,920** ORE ontologies carrying `InverseFunctionalObjectProperty`. The flag is
  inert by construction elsewhere — both touched code paths key on that axiom. The frame is a
  grep **superset** used for framing, not counting (`grep ≠ gate`, the Lever-1 lesson), so it
  cannot miss an affected ontology.
* **Binary pinned** to `/tmp/rustdl-ifp-sweep-ec7704b` immediately after building, and the pin
  **verified against a discriminating input** (the fixture the flag fixes) before use.
* `--pair-timeout-ms 1000`, single-thread, 60 s wall cap. The budget is **non-truncating on
  purpose**: under a truncating budget the hierarchy is not run-to-run deterministic, so A/B row
  identity would be meaningless.
* Realize arm on the 53 that also carry `ObjectPropertyAssertion` (the shape that can benefit),
  90 s cap.

## Classify arm — 109 ontologies

| verdict | n |
|---|---|
| IDENTICAL | 89 |
| both-arms DNF (uninformative, excluded) | 16 |
| **REGRESSED_BY_ON** | **3** |
| ROWS_DIFFER | 1 |

Wall over the 90 both-completed: off 85.3 s vs on 84.9 s — **flat**. There is no broad cost; the
cost is concentrated in three ontologies.

### The 3 regressions are real, large, and not a contention artifact

| ontology | OFF | ON |
|---|---|---|
| `ore_ont_9662` | 2.26 s, 1554 rows | **DNF at 120 s** |
| `ore_ont_7532` | 2.82 s, 1934 rows | **DNF at 120 s** |
| `ore_ont_9786` | 7.43 s, 1307 rows | **DNF at 120 s** |

Re-run **sequentially on an idle host at double the cap**. Two independent reasons this is not
the concurrency confound I flagged when launching: the OFF walls reproduce to within **0.04 s**
of the sweep values, and a 2.3 s → >120 s flip is orders of magnitude outside any contention
effect. They are also **not slowdowns** — still unfinished at 120 s.

**Mechanism: HYPOTHESIS, not measured.** The derived GCI puts an `at_most` on `r⁻` at every node
with an `r⁻`-successor, so it broadly triggers the predecessor-walking merge. `CLAUDE.md` records
that a whole-graph re-fire of *precisely this merge* made galen a 6.6-minute DNF until it was
made incremental in `horn_fixpoint` (2026-07-11). A broad `≤1 r⁻` plausibly re-creates that cost
class. **Unverified — do not cite as a finding.** Confirming it is the first step of any retry.

### The 1 row difference is a GENUINE COMPLETENESS GAIN

`ore_ont_13859`, same wall (48.1 s both arms): closure **6253 → 6270**, **+17 gained, 0 lost**.

**All 17 confirmed entailed by Konclude's transitive closure (17/17).** So the flag is not
merely safe here, it is *more complete*, and the risk direction that mattered (this change ADDS a
constraint, so the hazard is an FP) is clean on the one ontology where output moved.

## Realize arm — 53 ontologies

| verdict | n |
|---|---|
| IDENTICAL | 22 |
| no output in ≥1 arm (uninformative) | 31 |
| **GAINED** | **0** |
| lost types (red flag) | 0 |

**Zero realize gains** — the flag's original purpose is corpus-invisible, matching its functional
sibling (0 of 64). The synthetic fixture remains the only demonstrated realize benefit. The 22
usable pairs are a thin base, so this is evidence of **rarity, not proof of inertness**.

## Decision

**OFF.** Three ontologies from seconds to DNF is a hard stop, and the measured benefit is 17
entailments on one ontology plus zero realize gains. Flipping would trade a broad regression for
a narrow completeness win.

This is exactly the failure a 12-ontology benchmark hid in v0.4.8 (four ontologies, ~5 s → DNF),
and it was **pre-registered as the stop criterion** before the data came in.

### What would unblock a flip

1. **Confirm the mechanism** — is the cost the predecessor-merge volume, as hypothesised?
2. **If so, narrow the trigger.** The GCI is emitted for every inverse-functional role
   unconditionally; the three regressions suggest it should be conditional on something (role
   arity, ABox shape, or emitting only where a merge is *consumable* — the same reasoning that
   made `RUSTDL_DKEY_MERGING_GATE` a 311× RSS win).
3. **Re-run this sweep**, plus a ΔMISSED arm. The frame is only 109 ontologies, so a retry is
   cheap — under an hour.

Until then `RUSTDL_PSEUDO_MODEL`'s falsified soundness-by-construction argument **stays
falsified**: restoring it needs the witness to apply inverse-functional merges, which needs this
flag ON. The honest position is the weaker empirical one already recorded in
`docs/2026-08-18-pseudo-model-bakeoff.md`.

## Method notes that cost something

* **`direct` rows are NOT the closure, and this bit three times in one session.** Adding entailed
  subsumptions *re-parents* the Hasse diagram, so `direct` rows legitimately move without any
  entailment being lost. On the raw `direct` rows `ore_ont_13859` looked like it *lost* 3 rows;
  on the closure it gained 17 and lost 0. Konclude's output has the same trap — its 611 direct
  pairs made 3 of 5 sampled gains look unasserted, while its transitive closure confirms all 17.
  **Always close before comparing.**
* **`pgrep -f "<script>"` self-matches its own command line**, so an "are the sweeps still
  running?" check reported a false warning. Third instance this session. The wall agreement, not
  the process check, is what established the host was idle.
* My realize-arm bucket labelled `DNF_EITHER` conflates timeout with a non-zero exit (realize
  errors on an inconsistent KB). Uninformative either way for an A/B, but reported here as
  "no output in ≥1 arm" rather than as DNF.
