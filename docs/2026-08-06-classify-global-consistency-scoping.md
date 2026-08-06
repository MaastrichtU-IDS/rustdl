# Scoping: why `classify` and `consistent` disagree, and whether the known dead-end really blocks the fix

**Date:** 2026-08-06 · **Trigger:** `ore_ont_16372` reports `consistent: true` from
`classify --json` while `rustdl consistent` reports `inconsistent`, **in shipped
defaults, with no flag involved.**

## Diagnosis

`classify` runs an inconsistency **pre-check** (`classify_inconsistency_precheck`, wired at
`classify.rs:970` and `:2136`), which combines the saturator's
`globally_inconsistent() || top_is_unsat()` with `abox_saturation_inconsistent`. On
`ore_ont_16372` that pre-check returns **false**, unchanged by
`RUSTDL_CLASSIFY_INCONSISTENCY=1` or an unbounded `RUSTDL_CLASSIFY_INCONSISTENCY_MS=0`.

`is_consistent`, by contrast, reaches the verdict in **0.13–0.37 s**, and ablation shows no
single pre-check is responsible: `RUSTDL_ABOX_SATURATION=0`, `RUSTDL_ABOX_CHECK=0`,
`RUSTDL_WEDGE_CONSISTENCY=0` and the pairwise combination **all still report
`inconsistent`** (fastest, 0.13 s, with the wedge OFF). So the verdict comes from the
global tableau consistency check — which **`classify` never runs**.

So this is a second instance of the residual CLAUDE.md already records for
`family-mech4-ddmin-core.ofn`: an inconsistency reachable only by the global consistency
route, which neither classify pre-check reaches.

## The recorded dead-end is about a DIFFERENT mechanism than the promising one

CLAUDE.md says: *"a bounded global `decide(Top)` probe on the classify path is a measured
dead-end (hangs on consistent `alehif`/`pizza`)."*

That claim is about injecting a **`decide(Top)` probe**. The alternative — **reuse
`is_consistent`'s existing pipeline as a budgeted classify pre-check** — is not the same
mechanism, and it is not covered by that measurement. Relevant timings for the whole
`rustdl consistent` pipeline:

| ontology | wall |
|---|---:|
| pizza | **0.02 s** |
| sulo | 0.01 s |
| bibtex | 0.01 s |
| ro | 0.11 s |
| sio | 0.20 s |
| `ore_ont_16372` | 0.13–0.37 s |

**What this shows and does not show.** It shows the *pipeline* is cheap on these inputs,
including on the very fixture (`pizza`) the dead-end note names. It does **not** refute the
`decide(Top)` finding — `is_consistent` can answer via a cheap route without ever reaching
the probe that was measured to hang, so the two are not comparable. The claim is
**untested for the reuse proposal**, not overturned.

## What a real attempt would need

1. **Corpus-scale cost.** The proposal adds a bounded pre-check to *every* classify call, so
   the cost is (budget × ontologies that do not short-circuit), not the 6 numbers above.
   Cheap on curated fixtures is weak evidence; the ~1,750 completing ORE ontologies are the
   population that matters. `sio` at 0.20 s is already 10× `pizza`.
2. **A budget with a measured basis.** `ore_ont_16372` needs ~0.4 s. The
   `RUSTDL_CLASSIFY_INCONSISTENCY_MS` history is the cautionary precedent: an "obviously
   ample" few-hundred-ms default silently lost `family.ofn`, which needs ~2.6 s, and the
   flat 3 000 ms that replaced it had only ~13% headroom.
3. **Both gates.** A default-behaviour change on every classify call needs the FP=0 net
   *and* a 1,920-ontology two-arm sweep. Today's `RUSTDL_INVERSE_PAIR_FUNC` sweep is the
   argument: it found 4 ontologies going from 1–5 s to non-terminating, which no small
   sample would have caught.

## Status

**Not attempted.** This is a scoping note, not a fix. The value delivered here is: the
divergence is diagnosed to a specific missing step; the mechanism that would close it is
distinguished from the one previously measured out; and the cost question is stated in the
form that would actually settle it.

Two consequences meanwhile:

- **`ore_ont_16372`'s flag-ON classify DNF should be re-tested only after this lands**, since
  a working short-circuit would remove the expensive classification entirely rather than
  requiring it to be made fast.
- The `RUSTDL_INVERSE_PAIR_FUNC` divergence blocker is the same underlying gap, so one fix
  clears both.
