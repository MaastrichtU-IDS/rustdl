# Match-plan refusal census over all 1,920 ORE ontologies (2026-09-04)

Closes the frame question left open by #90's fix (`d51ac27`): the `eval_order` change
converts a whole-clause **refusal** into a **filter**, and `eval_order` runs for every
clause of every ontology. The commit's evidence was **0 of 150 sampled** ORE ontologies
plus a reasoning argument. A 0-of-150 sample bounds the rate at roughly <2%, not zero,
and the reasoning argument was wrong in a way described below — so this is the
population measurement.

**Answer: 0 of 1,920. Two independent instruments agree, and the residual is 2 named
ontologies where the change cannot fire by construction.**

## The frame argument in `d51ac27` was TOO NARROW

That commit says the addressable construct is a **negated** `Self` (or a `Self` disjunct
under `∀`), and that ORE has none. The conclusion holds; the reason given does not, and
it is contradicted by the commit's own evidence: **`ro.ofn` has 5 *unnegated*
`ObjectHasSelf` and produced 6 refusals.**

The generator is broader. `ro` carries two
`EquivalentClasses(C, ObjectHasSelf(r))`, and an equivalence gives BOTH directions — so
the `⊒` half puts `Self` in the **antecedent**, which after NNF is a **negative**
occurrence. The same applies to `Self` on the LHS of a `SubClassOf`.

So the trigger is *any occurrence that yields a negative one*, which includes an ordinary
`EquivalentClasses` — a far more common authoring shape than an explicit
`ObjectComplementOf(ObjectHasSelf(…))`. **The first ORE scan grepped for the explicit
complement and found zero, which is the right answer for the wrong reason.** Re-scanning
for the corrected pattern (complement OR equivalence-containing-Self OR Self-on-LHS)
still gives **0 of the 9** `Self`-bearing ORE ontologies — all of theirs is a positive
`SubClassOf` RHS, which `emit_head` lowers to a HEAD `Role(var,var)` and never a body
atom.

## Instrument 1 — engine probe (`RUSTDL_TRACE_BODY_VARS=1` on the BEFORE pin)

Reports by *gate* whether `build_clause_match_plan` returned `None`, i.e. a clause was
actually discarded. Smoke-tested first against a known positive (`ro` = 6 `NotTree`) and
a known negative (`pizza` = none).

| | count |
|---|---:|
| clean (no refusal) | 1,774 |
| refusals, **all `VarCap`** | 39 |
| **`NotTree` / `Disconnected`** | **0** |
| unmeasured at a 30 s cap | 107 → all retried at 300 s, **0 addressable** |

The 39 are the `MAX_BODY_VARS` cap — a separate, documented negative result, untouched by
this fix. The 107 were **not** folded into "clean"; they were re-measured.

## Instrument 2 — clausify probe (`examples/clause_stats_probe`, extended here)

The refusal is a pure function of the clause bodies, so parse + convert + clausify is
sufficient — no reasoning. That is the point: the engine probe **cannot report on an
ontology that stalls in reasoning**, which is exactly the 107.

| | count |
|---|---:|
| `FILTERS=0` and `DISCONNECTED=0` | **1,918** |
| `FILTERS>0` (what the fix changes) | **0** |
| `DISCONNECTED>0` | **0** |
| unmeasured (conversion cap) | 2 |

**It replicates `eval_order`'s walk, so it is a drift hazard — calibrate before use.**
Doing so caught a real discrepancy: it reports `FILTERS=2` on `ro` where the engine probe
reports 6. Both are right and they measure different things:

* the engine counts **indexing events**, and `ro` is re-indexed 3× during classify
  (base index + per-class label cache + consistency cache) → 2 clauses × 3 = 6;
* the clausify probe counts **distinct clause bodies** → 2, matching `ro`'s two
  `EquivalentClasses`-with-`Self` axioms exactly.

They also disagree on *attribution* for `∀p.(¬Self ⊔ ¬Z)` — engine says `Disconnected`,
probe says a filter — because they read **different clause sets**: the engine probe runs
the pre-fix binary (naming clause on `var`, hence disconnected) and the probe runs the
post-fix clausifier (anchored on `X`, hence a self-loop filter). As **booleans** the two
agree 4/4 on `ro` / `pizza` / `negself` / `negself_d`, which is the property the census
needs.

## The residual is 2 ontologies, and the change cannot fire on either

`ore_ont_10860` and `ore_ont_4572` do not finish conversion. Both have
**`ObjectHasSelf` = 0**, so the `Self` route is excluded by construction, and their
unmeasured status has documented causes unrelated to this fix: `10860` is the known
`horned-owl` SWRL grammar gap (17 `BuiltInAtom`/`Variable` lines — it does not parse),
and `4572` is the conversion-bound DKey member (98,867 `DataPropertyAssertion`, 54 MB).

Stated precisely: the `Self` route is impossible on those two; a *join* route (two role
atoms sharing a target) is **empirically absent from all 1,918 measured ontologies** but
not proven impossible for these two. That is the only gap left, and its direction of risk
is a false positive, which the FP=0 net gates on the curated fixtures.

## What this does and does not license

* The fix is **corpus-inert on ORE** — it can change no answer there. That is
  non-regression evidence, not correctness evidence, and it is why #90's real gate stays
  the 6 canaries plus the Konclude adjudication.
* `ro.ofn` remains the one place the change is **observable on real data**: 6 refusals →
  none, six clauses moving from silently-ignored to enforced, with the oracle-verified
  closure unchanged at 158=158.
* **`VarCap` is the live residual in this area, at 39 ontologies** — a clause discarded
  for exceeding `MAX_BODY_VARS`, silently, exactly as `NotTree` used to be. Raising that
  cap is a measured hard stop (`docs/2026-08-03-max-body-vars.md`: 8 → 16 recovers nothing
  and destroys three completers), so this is recorded, not queued.
