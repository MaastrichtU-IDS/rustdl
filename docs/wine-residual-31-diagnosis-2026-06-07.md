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

**ROOT-CAUSED (code-grounded) to a classify RECOVERY-SWEEP coverage gap — the
wedge proves it; the orchestrator never asks.** `explain`/`subclass` return
**yes in 0.01 s**, and `prepared.hyper_decide(Fruit, EdibleThing, 200 ms)` =
**Subsumed in 0 ms** (the wedge proves it directly; the slow tableau is the
*fallback that is never reached* for this pair). Yet classify misses it even
all-flags-off **and single-threaded** (4671 s run; rules out trust-Sat,
label-heuristic, snapshot, per-pair budget, AND contention). The cause is in the
top-down classifier's structure (`classify_top_down_with_timeout` /
`find_direct_parents_top_down`):

1. The walk processes classes **tier-by-tier, ordered by EL-closure-subsumer
   count**, descending only through classes placed in *earlier* tiers
   (`classify.rs:1024–1071`). `Fruit ⊑ EdibleThing` is **not in the EL closure**
   (it needs the union case-split `Fruit ≡ NonSweetFruit ⊔ SweetFruit`, both told
   `⊑ EdibleThing`), so `EdibleThing` is not among `Fruit`'s closure-subsumers and
   the walk never positions one to test the other → walk misses it.
2. The recovery for walk-missed pairs is the **"defined-sup sweep"**
   (`classify.rs:1130`), which only tests pairs whose **sup** is a *defined*
   class (`EquivalentClasses(Name, ComplexExpr)`). `Fruit` IS defined (a union),
   but **`EdibleThing`/`ConsumableThing` are primitive** (`SubClassOf` only — 0
   `EquivalentClasses`). The missed subsumption is **defined-SUB ⊑ primitive-SUP**;
   the sweep's sup side never includes a primitive class → it never tests
   `Fruit ⊑ EdibleThing` either.

So neither the tier-walk nor the recovery sweep ever poses the question the wedge
would answer in 0 ms. **Not a reasoning gap, not the ABox/tableau path** (the
tableau is never reached here). **Fix:** extend the recovery to **defined SUBS**
— for a union-defined class `C ≡ D₁ ⊔ … ⊔ Dₙ`, test `C ⊑ X` against candidate
sups `X` (e.g. the common supersumers of all `Dᵢ`), or more simply add
union-defined classes to the sweep's sub side and let the wedge adjudicate.
Recovers wine 31→29. Cheap, sound (the wedge proves the recovered pairs).

(Earlier drafts mis-attributed this — label-heuristic, then trust-Sat, then the
ABox-seed tableau hang — each refuted by a flag-off / single-thread / direct-API
measurement. The ABox-seed tableau hang IS real but is a *separate* finding
about B/C/D and wine's wall, NOT cluster A: see below.)

### Separate finding — ABox-seeded `prepared.decide` is pathologically slow / non-terminating

`prepared.decide(Fruit ⊓ ¬EdibleThing)` does **not terminate in 150 s** unbounded
(times out even at 5 s) while the fresh `run_satisfiability` of the same query is
**0.01 s** (`wine_fruit_prepared_vs_fresh_probe`). `PreparedOntology` is
**ABox/nominal-seeded once** (`from_internal`), so every per-pair `decide` drags
wine's 207-nominal ABox into the tableau. This is **irrelevant to cluster A** (the
wedge proves `Fruit` before the tableau is reached), but it is a real
perf/termination issue: it is the fallback for B/C/D (whose wedge does *not*
prove them), so it likely contributes to their timeouts and to wine's ~311 s
wall. >150 s on a tiny ontology smells like a blocking/termination bug with
seeded nominals, not mere slowness. Worth a separate investigation; see the
scoping doc.

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
| A covering (`Fruit`) | 2 | classify recovery-sweep coverage gap: tier-walk can't see it (not in EL closure), and the defined-sup sweep only tests *defined sups* — but `Fruit ⊑ EdibleThing` is defined-SUB ⊑ primitive-SUP. Wedge proves it in 0 ms; orchestrator never asks | **Yes** — extend sweep to defined SUBS |
| B sugar `∀`+functional+nominal-set | 11 | `∃R.{a}`+functional→`∀R.{a}`, `{a}⊆{a,b}` un-modeled | No — modeling |
| C region→grape + `≤1` | 11 | nominal `∃R.{a}` identity + `≤1` un-modeled | No — modeling |
| D color/table via nominal value | 7 | nominal value-restriction un-modeled | No — modeling |

**The 31 are 2 + 29.** The 29 (B/C/D) are all the same root: rustdl's sound
under-approximation of nominals (`ObjectOneOf`/`ObjectHasValue`) + functional/
`≤1` cardinality. Closing them is the **nominal-completeness project**, not a
wedge tweak — and it is the dominant remaining wine gap.

## Recommended next steps (if pursued)

- **Cluster A (cheap, 2 pairs):** root-caused to the **defined-sup-sweep coverage
  gap** — the recovery sweep only tests *defined sups*, but `Fruit ⊑ EdibleThing`
  is *defined-SUB ⊑ primitive-SUP*, so it's never tested though the wedge proves
  it in 0 ms. Fix: extend the sweep to defined SUBS (union/covering classes
  `C ≡ D₁ ⊔ …`), testing `C` against candidate sups (the common supersumers of the
  `Dᵢ`). Recovers wine 31→29. Sound (wedge adjudicates), cheap, classify-only.
- **Clusters B/C/D (the real frontier, 29 pairs):** a scoped nominal-completeness
  increment — model `∃R.{a}` singleton identity well enough to derive
  `functional R + ∃R.{a} ⟹ ∀R.{a}` and `∀R.{a} ⊑ ∀R.{a,b}`, plus region→grape
  nominal propagation. This is the same lever the nominal-lever scoping doc
  deferred; the diagnosis confirms it owns 29 of the 31.
