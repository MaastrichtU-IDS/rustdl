# Diagnosis: wine's residual 31 MISSES (2026-06-07)

After the precise-card-deps lever (v0.3.5, wine 34→31), this is a root-cause
diagnosis of the remaining 31 — *not* a fix. Method: axiom inspection +
per-pair probes (`explain`, `subclass`, `subclass` under
`RUSTDL_HYPERTABLEAU_TRUST_SAT=0`). **Headline: the 31 are not one problem.
2 are a recoverable classify-path prune; 29 are genuine modeling gaps in the
under-approximated nominal/functional/cardinality semantics.**

## The 31, categorized

### Cluster A — covering / union-on-the-left (2) — RECOVERABLE
```
food#Fruit ⊑ food#ConsumableThing
food#Fruit ⊑ food#EdibleThing
```
`Fruit ≡ NonSweetFruit ⊔ SweetFruit` (line 564); both members are told
`⊑ EdibleThing` (671) and `⊑ ConsumableThing`. Pure disjunction case-split —
**no nominals, no cardinality.**

**Probe result — root-caused to a fresh-vs-`PreparedOntology` tableau
discrepancy.** `explain` and the per-pair `subclass` command return **yes in
0.01 s** (both trust-Sat ON and `=0`). Yet the bulk **classify** path misses it,
and the miss survives **every** optimization flag off
(`RUSTDL_HYPERTABLEAU_TRUST_SAT=0` + `RUSTDL_LABEL_HEURISTIC=0` +
`RUSTDL_SNAPSHOT_CAPTURE=0` + `RUSTDL_HORN_SHORTCIRCUIT=0`). So it is **not**
trust-Sat, the label heuristic, the snapshot cache, or the per-pair budget.

The cause is the **two different tableau entry points**:
- `is_subclass_of` (the `subclass`/`explain` path) falls through to a **fresh**
  `run_satisfiability(Fruit ⊓ ¬EdibleThing)` (`lib.rs:1695`) → **Unsat in
  0.01 s** → subsumed.
- the classify walk's `subsumes_via_tableau` (`classify.rs:1452`), on a wedge
  non-proof with trust-Sat off, falls through to **`prepared.decide`** — the
  `PreparedOntology` snapshot, which is **ABox/nominal-seeded once**
  (`from_internal`, per CLAUDE.md). On `Fruit ⊓ ¬EdibleThing` that path returns
  Sat (or stalls), so classify records "not subsumed".

So the `Fruit` covering subsumption is dropped by a **classify-snapshot
discrepancy**: the ABox-seeded `PreparedOntology` tableau disagrees with the
fresh tableau on a union-defined sub. Recoverable (the fresh tableau proves it
trivially); the fix lives in the classify/`PreparedOntology` path, not in
reasoning power.

**SETTLED (direct unit test `wine_fruit_prepared_vs_fresh_probe`, classify.rs):
it is a PERFORMANCE pathology, not a wrong verdict.** `is_subclass_of` (fresh
`run_satisfiability`) → `true` in **0.01 s**. `prepared.decide(Fruit ⊓
¬EdibleThing)` **does not terminate in 150 s** unbounded, and **still times out
at a 5 s deadline** — vs the fresh path's 0.01 s, a >10⁴× slowdown. So classify's
200 ms budget yields `NoVerdict` → missed. The cause: `PreparedOntology` is
**ABox/nominal-seeded once** (`from_internal`), so *every* per-pair `decide`
drags wine's 207-nominal ABox into the tableau — even for `food#Fruit`, which is
nominal-irrelevant. The fresh path does not carry that seed and is instant.

**Broader implication:** this ABox-seeding slowdown is almost certainly not
specific to `Fruit` — it inflates *every* per-pair tableau check on wine, so it
likely drives much of wine's 311 s classify wall AND contributes to the B/C/D
timeouts (they may be partly perf, not purely modeling — worth re-checking a B/C
pair via the fresh path once the seeding is addressed). **Fix directions:** (a)
don't ABox-seed the per-pair `decide` for pairs whose classes don't reach a
nominal/individual (food vs vin namespaces are a coarse proxy); (b) fall back to
the fresh, un-seeded tableau when `prepared.decide` stalls (sound iff the fresh
path models the ABox correctly for nominal-dependent pairs — verify); or (c)
root-cause why the seeded state fails to terminate (a blocking/termination
interaction with the seeded nominals) rather than papering over it with (a)/(b).

(Two earlier drafts mis-attributed this — first to the label heuristic, then to
trust-Sat. Both refuted by the flag-off runs; the real cause is the prepared-vs-
fresh tableau split.)

### Cluster B — sugar: `∀hasSugar.{…}` via functional role + nominal set (11)
```
DryRiesling ⊑ WhiteNonSweetWine     DryWhiteWine ⊑ WhiteNonSweetWine
Meursault ⊑ WhiteNonSweetWine       Muscadet ⊑ WhiteNonSweetWine
Muscadet ⊑ DryWhiteWine             Sancerre ⊑ WhiteNonSweetWine
Tours ⊑ WhiteNonSweetWine           WhiteBurgundy ⊑ WhiteNonSweetWine
WhiteTableWine ⊑ WhiteNonSweetWine  StEmilion ⊑ DryRedWine
StEmilion ⊑ DryWine
```
`WhiteNonSweetWine ≡ WhiteWine ⊓ ∀hasSugar.{Dry,OffDry}` (1173);
`DryWine ≡ Wine ⊓ ∃hasSugar.{Dry}` (941); `hasSugar` is **functional** (438).
The entailment needs: `∃hasSugar.{Dry}` + functional ⟹ `∀hasSugar.{Dry}`, then
nominal-set subsumption `{Dry} ⊆ {Dry,OffDry}` ⟹ `∀hasSugar.{Dry,OffDry}`.

### Cluster C — region-nominal → grape + `≤1 madeFromGrape` (11)
```
Beaujolais ⊑ Gamay        CotesDOr ⊑ PinotNoir      Margaux ⊑ Merlot
Meursault ⊑ Chardonnay    Muscadet ⊑ PinotBlanc     Pauillac ⊑ CabernetSauvignon
RedBurgundy ⊑ PinotNoir   Sancerre ⊑ SauvignonBlanc StEmilion ⊑ CabernetSauvignon
Tours ⊑ CheninBlanc       WhiteBurgundy ⊑ Chardonnay
```
`Beaujolais ≡ Wine ⊓ ∃locatedIn.{BeaujolaisRegion}` (852);
`Gamay ≡ Wine ⊓ ∃madeFromGrape.{GamayGrape} ⊓ ≤1 madeFromGrape` (959). Needs
region-nominal → grape propagation + the `≤1` cardinality + `∃R.{a}` nominal
identity.

### Cluster D — color / table-wine hierarchy via nominal value (7)
```
Muscadet ⊑ WhiteLoire   Muscadet ⊑ WhiteTableWine  Muscadet ⊑ WhiteWine
StEmilion ⊑ RedTableWine StEmilion ⊑ TableWine     Tours ⊑ WhiteLoire
Tours ⊑ WhiteWine
```
Same family as B/C: `∃hasColor.{White}` → `WhiteWine` etc. — nominal
value-restriction reasoning.

**Probe result for B/C/D:** representative pairs `DryWhiteWine ⊑
WhiteNonSweetWine` (B) and `Beaujolais ⊑ Gamay` (C) **time out at 40 s even
under `trust_sat=0`** on this tiny ontology. The full tableau cannot prove them —
rustdl's nominal lever maps nominals to opaque per-individual synthetic classes
and **deliberately under-models** singleton identity, `∀`-over-nominal-set, and
the functional/`≤1` → `∀` collapse (see `docs/nominal-lever-scoping-2026-06-06.md`).
So these build an open (under-approximated) model and never close. **Not
recoverable without real modeling work** — not a prune, not a timeout to tune.

## Verdict

| Cluster | Count | Cause | Recoverable? |
|---|---:|---|---|
| A covering (`Fruit`) | 2 | fresh `run_satisfiability` proves it in 0.01 s; classify's ABox-seeded `prepared.decide` returns Sat/stalls. NOT trust-Sat / label-heuristic / snapshot / budget | **Yes** — classify/`PreparedOntology` fix |
| B sugar `∀`+functional+nominal-set | 11 | `∃R.{a}`+functional→`∀R.{a}`, `{a}⊆{a,b}` un-modeled | No — modeling |
| C region→grape + `≤1` | 11 | nominal `∃R.{a}` identity + `≤1` un-modeled | No — modeling |
| D color/table via nominal value | 7 | nominal value-restriction un-modeled | No — modeling |

**The 31 are 2 + 29.** The 29 (B/C/D) are all the same root: rustdl's sound
under-approximation of nominals (`ObjectOneOf`/`ObjectHasValue`) + functional/
`≤1` cardinality. Closing them is the **nominal-completeness project**, not a
wedge tweak — and it is the dominant remaining wine gap.

## Recommended next steps (if pursued)

- **Cluster A (cheap, 2 pairs):** root-caused to the classify walk's
  `prepared.decide` (ABox-seeded `PreparedOntology`) disagreeing with the fresh
  `run_satisfiability` on `Fruit ⊓ ¬EdibleThing`. Settle timeout-vs-wrong-verdict
  (run in flight), then fix in the classify path — e.g. skip ABox-seeding for
  pairs in ABox-irrelevant namespaces (food), or fall back to the fresh tableau
  when `prepared.decide` stalls. Recovers wine 31→29. A classify bug, not a
  reasoning gap — the most actionable thread.
- **Clusters B/C/D (the real frontier, 29 pairs):** a scoped nominal-completeness
  increment — model `∃R.{a}` singleton identity well enough to derive
  `functional R + ∃R.{a} ⟹ ∀R.{a}` and `∀R.{a} ⊑ ∀R.{a,b}`, plus region→grape
  nominal propagation. This is the same lever the nominal-lever scoping doc
  deferred; the diagnosis confirms it owns 29 of the 31.
