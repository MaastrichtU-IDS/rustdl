# Deep analysis of rustdl's reasoning misses on the full ORE corpus (2026-07-19)

Follow-up to the full-ORE 4-reasoner run (`2026-07-18-km-headtohead-and-rustdl-FP.md`).
That run found rustdl **sound corpus-wide (0 FP)** but with **149 `r<kon` onts** where
rustdl reports fewer subsumptions than Konclude. This doc dissects those misses.
Gold = Konclude (equals rustdl on every satisfiable class; DNF'd only 83/1917).

## Headline: most of the "miss" volume is a comparison artifact, not incompleteness

Raw: 148 onts, 27,416 "missed" subsumptions (1 more, `10860`, is a parse failure —
rustdl doesn't parse `DLSafeRule`/SWRL, returns empty; not a reasoning miss).

Split the 148 by rustdl's **own fragment verdict**:

| fragment | onts | "missed" subs | interpretation |
|---|---|---|---|
| pure-EL | 4 | 15,910 (58%) | rustdl is **provably complete** here → these are comparison artifacts |
| out-of-EL | 136 | 10,028 | the genuine reasoning-miss candidates |
| Horn | 3 | 1,041 | mixed |
| (classify timed out in probe) | 4 | 433 | large onts |

### The pure-EL "misses" are equivalent-IRI artifacts (verified)
rustdl's EL saturator is complete on EL by construction, so a pure-EL ont cannot
have a real reasoning miss. Confirmed on `ore_ont_2612` (3341 "misses"): after
canonicalizing every class to its `EquivalentClasses` representative (union-find
over the ontology's named 2-arg equivalences) and re-diffing on **full IRIs**, the
miss count drops **3341 → 0**. Cause: the ont (a DBpedia/FRED lexical ontology)
declares `EquivalentClasses(<dbpedia…/dev/null>, <fred#%2Fdev%2Fnull>)` — the same
class under two IRIs with different fragments. rustdl reports the subsumption under
one representative, Konclude under the other; the harness's fragment-keyed diff
counts it as a miss. **Not a rustdl miss.**

### CONFIRMED rustdl EL-COMPLETENESS BUG — `2658`/`3406` (1350 each = 2700 subs)
These are pure-EL OBO chemistry onts (ChEBI + GOCHE, **zero non-EL constructs**).
`GOCHE_37527 ≡ ∃GOCHEREL_0000004.CHEBI_37527 ⊓ CHEBI_24431` (pure-EL *defined*
class), Konclude+HermiT both derive ~675 `CHEBI_x ⊑ GOCHE_37527`, **rustdl derives 0**.
The bridging axiom is line 18620:
`SubClassOf(∃RO_0000087.CHEBI_37527, ∃GOCHEREL_0000004.CHEBI_37527)` — a **GCI with
a compound existential on BOTH sides** (EL-shaped; `GOCHEREL_0000004 ⊑ RO_0000087`).

**Minimally reproduced** (`docs/known-limitations/fixtures/el-existential-gci-defined-class-gap.ofn`):
```
s ⊑ r
GOCHE ≡ ∃s.C ⊓ D
∃r.C ⊑ ∃s.C          ← compound-existential GCI
X ⊑ ∃r.C ⊓ D
⟹ X ⊑ GOCHE          (EL-entailed: X⊑∃r.C →GCI→ ∃s.C; X⊑D)
```
rustdl outputs only `X ⊑ D` (misses `X ⊑ GOCHE`); `explain` says *"no — answered by
saturation (input is pure EL; closure is complete)"* — i.e. it **wrongly reports the
EL closure complete while missing the pair**. Konclude derives `X ⊑ GOCHE`; with the
GCI deleted rustdl correctly says no (clean control). **This VIOLATES the "complete on
EL" guarantee** — the saturator derives `X ⊑ ∃r.C` but never fires the GCI
`∃r.C ⊑ ∃s.C` to get `X ⊑ ∃s.C`, so it misses the defined-class recognition. Root
cause is the compound-∃ **antecedent** GCI not being propagated onto a class that has
the ∃r.C consequent (a Tseitin/absorb gap on `∃r.C ⊑ ∃s.D`-form axioms). Fixable;
this is the **#1 lever** (the only miss that touches a *proven* guarantee, cf. the two
EL gaps whelk-rs surfaced earlier: `⊤⊑C`, `∃R.⊤`). (3406 ≈ dup of 2658.)

## Attribution of the out-of-EL misses (all 136 onts, unsat-normalized)

Ran a decomposition (`decomp_miss.py`) over **all 136** out-of-EL miss onts,
splitting each ont's raw miss into (a) **missed-unsatisfiability** — Konclude found
classes unsatisfiable that rustdl didn't, so Konclude enumerates that Nothing-block's
pairs and rustdl has none (`unsat=0`); vs (b) **genuine satisfiable-class subsumption
misses** (after stripping both reasoners' unsat classes uniformly):

| category | onts | subs |
|---|---|---|
| missed-unsatisfiability (genuine=0) | 3 | 3,289 |
| genuine satisfiable-class subsumption miss | 133 | 6,739 |
| **out-of-EL total** | 136 | 10,028 |

- **Missed-unsat is concentrated in 3 onts** (`5014` 331, `4198` 1479, `16321` 1479
  — the latter two near-dups). rustdl reports `unsat=0` where Konclude found 19–40
  unsatisfiable classes; the raw counts are inflated by the Nothing-block
  enumeration, so the *real* gap here is "rustdl missed N **unsatisfiability
  detections**" (the ∀+⊔ / ≤n+disjoint clashes), a handful of detections, not
  thousands of subsumptions. A sound TBox unsat-detection pass would clear all 3.
- **The genuine subsumption misses (6,739 / 133 onts)** are the real completeness
  gap, dominated by defined-class / ∀ recognition (below).

## The genuine misses (133 onts) — pattern taxonomy

Diagnosed 10 representative onts (missing-pair samples + defining axioms):

1. **Defined-class sufficient-direction recognition (dominant).** `X ⊑ C` where
   `C ≡` a compound (∃/⊓, often + ∀/¬). rustdl's saturator/`trust_sat` fails to
   recognize `X` matches `C`'s definition.
   - `ore_ont_778` (Animals/BioTop): **59/59 missing pairs have a defined RHS**,
     e.g. `AfricanElephant ⊑ OmnivoreAnimal` where `OmnivoreAnimal ≡ ∃bearerOf.(…∀…)`.
     `rustdl explain` on this pair **times out >90 s** — the pair is genuinely hard
     (deep tableau), so classify's 1000 ms per-pair budget + adaptive early-cut
     skip it. **These are budget/tractability misses, not calculus gaps.**
   - `ore_ont_15167` (Mondial geo): 25/42 defined RHS, with **negation** —
     `EncompassedArea ≡ LargeArea ⊓ ¬Continent ⊓ ¬Sea`; needs disjointness/¬
     reasoning the saturator doesn't do.
   - `ore_ont_9534` (SWEET): 14/28 defined RHS (`Bag ≡ Multiset`, container lattice).

2. **Missed unsatisfiability → missed `⊑ everything`.** A class becomes
   unsatisfiable via a clash rustdl's `trust_sat` doesn't derive, so rustdl keeps it
   satisfiable and misses all its (vacuously entailed) subsumptions. Inflates counts
   because one missed-unsat class = dozens of missed pairs.
   - `ore_ont_5014` (e-tourism): `Accomodation ⊑ {Activity, ConferenceRoom, …}`
     (⊑ ~everything); `Accomodation ⊑ ∃hasRoom.Guestroom ⊓ ∀hasRoom.(Guestroom ⊔ …)`
     — an **∀ + ⊔** interaction. rustdl reports `unsat=0` (thinks it satisfiable).
   - `ore_ont_4198` (BioPAX): `bioSource ⊑ everything`; `bioSource` carries
     `ObjectExactCardinality(1 …) / ObjectMaxCardinality(1 …)` and the RHS classes
     are pairwise `DisjointClasses` — a **≤n + disjointness** clash rustdl misses.

3. **∀-heavy / defined-class propagation to upper ontology roots.**
   `ore_ont_13647`/`4911` (BFO-based, ∀=611/480, hundreds of defined classes,
   nominals, inverse): deep classes fail to reach `BFO_0000001/2/4` (entity/…).

## Interpretation

- **Soundness is unaffected** — every miss is `rustdl ⊆ Konclude` (fewer, never
  wrong). This analysis is purely about completeness.
- **The real completeness gap is far smaller than the raw 27,416.** Breakdown:
  - **~13,210 pure-EL equivalent-IRI artifacts** (`2612` verified 3341→0; `13224`
    9869 unverified but pure-EL + huge, almost certainly the same artifact — worth
    a canonicalized re-check).
  - **~3,289 missed-unsatisfiability** (3 onts; Nothing-block enumeration + a few
    real missed unsat detections).
  - **2,700 the confirmed EL compound-∃-GCI bug** (`2658`/`3406`).
  - **~6,739 genuine satisfiable-class subsumption misses** (133 out-of-EL onts) —
    defined-class / ∀ recognition, mostly tiny (corpus median miss = 24; 98/148
    onts ≤25), heavily clustered on near-duplicate ont families.
  So the genuine reasoning-completeness gap is ~**9.4k** (6.7k defined-class + 2.7k
  the EL bug), not 27k, spread thin — rustdl is ~90% exact-match with Konclude,
  bounded and sound. Matches the CLAUDE.md caveat: completeness is *proven* only on
  the curated corpus; ORE is empirical.
- **The misses are `trust_sat` behaving as designed:** on hard out-of-EL pairs
  (defined-class recognition, ∀/⊔/≤n clashes, negation) the wedge's `Sat` verdict +
  1000 ms per-pair budget + adaptive early-cut yield "not subsumed" rather than
  burning the deadline. Sound, near-complete, not complete.

> **UPDATE 2026-07-19 — lever #1 FIXED.** Fixed in `lower_sub_class_of` (Some-branch
> now emits a two-way equivalent marker for a compound-∃ consequent, mirroring the
> And-branch's Phase-2b.5). `2658`/`3406` recovered to exact Konclude match; 5 curated
> oracles byte-identical; broad ORE FP sweep TRUE-FP=0. Levers #2/#3 scoped and
> DOCUMENTED as the retired disjunctive / EL-ALC frontier (rustdl's own complete
> hypertableau doesn't derive #2's unsats — `5014` diverges >90 s). See
> `docs/superpowers/plans/2026-07-19-ore-completeness-fixes-plan.md`.

## Follow-up levers (ranked)
1. **[FIXED 2026-07-19] EL compound-existential GCI (`∃r.C ⊑ ∃s.D`) not propagated
   into defined-class recognition** — minimal repro in
   `docs/known-limitations/fixtures/el-existential-gci-defined-class-gap.ofn`,
   Konclude+HermiT derive `X⊑GOCHE`, rustdl misses (and mis-reports the EL closure
   complete). **Violates the "complete on EL" guarantee** — the #1 fix. Explains
   `2658`/`3406` (2,700 subs) and likely some out-of-EL onts whose defined classes
   sit behind such GCIs. Fix lives in the saturator's ∃-antecedent GCI handling
   (Tseitin/absorb of `∃r.C ⊑ …`).
2. **Missed unsatisfiability via ∀+⊔ / ≤n+disjoint** (`5014`, `4198`, `16321`) — a
   sound TBox unsat-detection pass (analog of the abox-saturation pre-check) would
   clear all 3 onts; the raw counts are Nothing-block enumeration, so the real cost
   is a handful of missed *detections*.
3. **Defined-class sufficient-direction recognition** (the 6,739 genuine misses:
   `13647`/`4911` ∀-heavy BFO, the SWEET 28-cluster, `778`, `15167`, `9534`) —
   the defined-SUB sweep misses compound/negation/∀ RHS; the hard ones (`778`,
   `explain` >90 s) are per-pair-budget bound.

Harnesses: `scratchpad/diag_miss.py`, `verify_eq.py`, `misses.txt`,
`frag148.txt`, `diag.out`.
